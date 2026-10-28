use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use qymcad_core::model::{Id, ImportNode};
use qymcad_kernel::{ExactFormat, Shape};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug, Serialize)]
pub struct ImportedBody {
    pub body: Id,
    pub name: String,
    pub solid: u32,
    pub scale: f64,
    #[serde(rename = "volume_mm3")]
    pub volume: f64,
    #[serde(serialize_with = "crate::session::serialize_coordinates")]
    pub bbox: [f64; 6],
}

#[derive(Clone, Debug, Serialize)]
pub struct ImportResult {
    pub source: Id,
    pub feature: Id,
    pub bodies: Vec<ImportedBody>,
    pub warnings: Vec<crate::session::NodeIssue>,
    pub rebuild: Rebuild,
}

struct ParsedImport {
    data: Vec<u8>,
    name: String,
    bodies: Vec<qymcad_core::geom::Built>,
    shapes: Vec<Shape>,
    nodes: Vec<qymcad_kernel::ImportNode>,
}

fn extension(path: &Path) -> Result<(ExactFormat, &'static str)> {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("step") | Some("stp") => Ok((ExactFormat::Step, "step")),
        Some("iges") | Some("igs") => Ok((ExactFormat::Iges, "igs")),
        _ => Err(Error::Invalid("import path must end in .step, .stp, .iges or .igs".into())),
    }
}

fn stage_and_parse(path: &Path) -> Result<ParsedImport> {
    use std::os::unix::fs::OpenOptionsExt;
    let (format, ext) = extension(path)?;
    let data = std::fs::read(path).map_err(|e| Error::Io(format!("cannot read import source {}: {e}", path.display())))?;
    if data.is_empty() {
        return Err(Error::Invalid("import source is empty".into()));
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..64 {
        let staged =
            std::env::temp_dir().join(format!("qymcad-mcp-import-{}-{}.{ext}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let mut file = match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&staged) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(Error::Io(format!("cannot stage import source: {error}"))),
        };
        let parsed = (|| {
            file.write_all(&data).map_err(|e| Error::Io(format!("cannot stage import source: {e}")))?;
            let staged_path = staged.to_str().ok_or_else(|| Error::Invalid("temporary import path is not UTF-8".into()))?;
            let _gate = qymcad_kernel::kernel_gate();
            let tree = qymcad_kernel::read_exact_tree(format, staged_path, 0.5)
                .map_err(|e| Error::Invalid(format!("cannot read import source: {e}")))?;
            if tree.shapes.is_empty() || tree.bodies.is_empty() {
                return Err(Error::Invalid("import contains no exact solids".into()));
            }
            Ok((tree.bodies, tree.shapes, tree.nodes))
        })();
        drop(file);
        let _ = std::fs::remove_file(&staged);
        let (bodies, shapes, nodes) = parsed?;
        return Ok(ParsedImport {
            data,
            name: path.file_name().and_then(|n| n.to_str()).unwrap_or("import").to_string(),
            bodies,
            shapes,
            nodes,
        });
    }
    Err(Error::Io("cannot allocate a temporary import file".into()))
}

fn shape_snapshot(shape: &Shape, scale: f64, index: u32) -> Result<Shape> {
    let base =
        qymcad_kernel::imports::scaled(shape, 1.0).ok_or_else(|| Error::Invalid(format!("cannot snapshot imported solid {index}")))?;
    if (scale - 1.0).abs() <= 1e-12 {
        Ok(base)
    } else {
        qymcad_kernel::imports::scaled(&base, scale).ok_or_else(|| Error::Invalid(format!("cannot snapshot imported solid {index}")))
    }
}

fn document_nodes(parsed: &ParsedImport, ids: &[Id]) -> Vec<ImportNode> {
    let stem = Path::new(&parsed.name).file_stem().and_then(|n| n.to_str()).filter(|n| !n.is_empty()).unwrap_or("import");
    qymcad_kernel::document_tree(&parsed.nodes, ids, stem)
}

fn selected_solids(parsed: &ParsedImport, solids: Option<&[u32]>) -> Result<Vec<u32>> {
    let available: HashSet<u32> =
        parsed.nodes.iter().filter_map(|node| node.solid).filter(|i| *i < parsed.shapes.len()).map(|i| i as u32).collect();
    let mut indexes: Vec<u32> = match solids {
        Some([]) => return Err(Error::Invalid("solids must contain at least one source index".into())),
        Some(values) => {
            let mut seen = HashSet::new();
            values.iter().copied().filter(|index| seen.insert(*index)).collect()
        }
        None => (0..parsed.shapes.len() as u32).collect(),
    };
    indexes.sort_unstable();
    if let Some(bad) = indexes.iter().find(|i| !available.contains(i)) {
        return Err(Error::Invalid(format!("solid index {bad} does not exist in the imported tree")));
    }
    Ok(indexes)
}

fn prune_nodes(nodes: &mut Vec<ImportNode>, allowed: &HashSet<u32>) -> bool {
    for node in nodes.iter_mut() {
        if (node.body.is_some() && !allowed.contains(&node.solid)) || node.repeat_of.is_some_and(|index| !allowed.contains(&index)) {
            node.body = None;
            node.repeat_of = None;
        }
        prune_nodes(&mut node.children, allowed);
    }
    nodes.retain(|node| node.carries_something());
    !nodes.is_empty()
}

impl Session {
    pub fn import(&mut self, path: &Path, scale: f64, solids: Option<&[u32]>, name: Option<&str>) -> Result<ImportResult> {
        let name = name.filter(|n| !n.trim().is_empty());
        let parsed = stage_and_parse(path)?;
        if !(scale.is_finite() && scale > 0.0) {
            return Err(Error::Invalid(format!("scale must be finite and positive, got {scale}")));
        }
        if let Some(name) = name {
            Self::check_object_name(name)?;
        }
        let selected = selected_solids(&parsed, solids)?;
        let selected_set: HashSet<u32> = selected.iter().copied().collect();
        let mut built_by_solid = HashMap::new();
        let mut result_shapes = HashMap::new();
        let body_indexes = selected.clone();
        let result = self.atomic(|session| {
            let source = session.p.add_source(parsed.name.clone(), parsed.data.clone());
            for index in &body_indexes {
                let built = parsed
                    .bodies
                    .get(*index as usize)
                    .ok_or_else(|| Error::Invalid(format!("solid index {index} has no tessellation")))?
                    .clone();
                let original =
                    parsed.shapes.get(*index as usize).ok_or_else(|| Error::Invalid(format!("solid index {index} has no exact shape")))?;
                let shape = shape_snapshot(original, scale, *index)
                    .map_err(|_| Error::Invalid(format!("cannot apply import scale {scale} to solid {index}")))?;
                let index = *index;
                let mut mesh = built.mesh;
                let faces = if (scale - 1.0).abs() > 1e-12 {
                    mesh.scale(0.0, 0.0, 0.0, scale);
                    built.faces.iter().map(|face| face.scaled(scale)).collect()
                } else {
                    built.faces
                };
                let body = session.p.add_mesh(mesh);
                session.p.set_body_faces(body, faces.clone());
                session.p.regen_faces.insert(body, faces);
                session.shapes.insert(body, shape);
                result_shapes.insert(body, shape_snapshot(original, scale, index)?);
                built_by_solid.insert(index, (body, built_by_solid.len()));
            }
            let ids_by_index: Vec<Id> =
                (0..parsed.shapes.len()).map(|i| built_by_solid.get(&(i as u32)).map_or(0, |(body, _)| *body)).collect();
            let mut nodes = document_nodes(&parsed, &ids_by_index);
            if !prune_nodes(&mut nodes, &selected_set) {
                return Err(Error::Invalid("selected solids have no importable tree nodes".into()));
            }
            let stem = name.unwrap_or_else(|| Path::new(&parsed.name).file_stem().and_then(|n| n.to_str()).unwrap_or("import"));
            let feature = session
                .p
                .import_tree_as_parts(nodes, source, stem)
                .ok_or_else(|| Error::Invalid("import tree contains no solids".into()))?;
            if let Some(name) = name {
                let descendants: HashSet<Id> = std::iter::once(feature).chain(session.p.descendants(feature)).collect();
                if let Some(component) =
                    session.p.components.iter_mut().find(|component| descendants.contains(&component.id) && component.name == stem)
                {
                    component.name = name.to_string();
                } else if let Some(component) = session.p.components.iter_mut().find(|component| descendants.contains(&component.id)) {
                    component.name = name.to_string();
                } else {
                    session.set_node_name(feature, Some(name))?;
                }
            }
            let components: Vec<Id> = std::iter::once(feature).chain(session.p.descendants(feature)).collect();
            if (scale - 1.0).abs() > 1e-12 {
                for component in &components {
                    let mut transform = session.p.component_transform(*component);
                    for axis in [3, 7, 11] {
                        transform[axis] *= scale;
                    }
                    session.p.set_component_transform(*component, transform);
                }
            }
            session.p.name_apart(&components, &crate::localization::name);
            // File-derived part/tree names become resolver aliases too. Validate after native naming,
            // inside the transaction so a bad source name restores the recipe and original shapes.
            for component in session.p.components.iter().filter(|c| components.contains(&c.id)) {
                Self::check_object_name(&crate::localization::name(&component.name))?;
            }
            for index in &body_indexes {
                if let Some((body, _)) = built_by_solid.get(index) {
                    session.p.set_import_scale(*body, scale);
                }
            }
            Ok((source, feature))
        })?;
        let (source, feature, rebuild) = (result.0 .0, result.0 .1, result.1);
        let mut imported = Vec::new();
        for index in selected {
            let body = built_by_solid[&index].0;
            let Some(shape) = result_shapes.remove(&body) else { continue };
            let name = self.body_name(body);
            let (volume, source_bbox) = {
                let _gate = qymcad_kernel::kernel_gate();
                (shape.volume(), shape.bbox().unwrap_or([0.0; 6]))
            };
            let mut bbox = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
            let world_transform = self.p.body_world_transform(body);
            for corner in 0..8 {
                let local = [
                    if corner & 1 == 0 { source_bbox[0] } else { source_bbox[3] },
                    if corner & 2 == 0 { source_bbox[1] } else { source_bbox[4] },
                    if corner & 4 == 0 { source_bbox[2] } else { source_bbox[5] },
                ];
                for axis in 0..3 {
                    let world = world_transform[axis * 4] * local[0]
                        + world_transform[axis * 4 + 1] * local[1]
                        + world_transform[axis * 4 + 2] * local[2]
                        + world_transform[axis * 4 + 3];
                    if corner == 0 {
                        bbox[axis] = world;
                        bbox[axis + 3] = world;
                    } else {
                        bbox[axis] = bbox[axis].min(world);
                        bbox[axis + 3] = bbox[axis + 3].max(world);
                    }
                }
            }
            imported.push(ImportedBody { body, name, solid: index, scale, volume, bbox });
        }
        let warnings = rebuild.warnings.clone();
        Ok(ImportResult { source, feature, bodies: imported, warnings, rebuild })
    }
}
