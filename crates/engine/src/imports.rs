//! Restore recipe-less imports whose saved B-rep is absent, using their embedded originals.
use crate::session::{AdvisoryWarning, NodeIssue};
use qymcad_core::feature::{FeatureKind, Kernel as _};
use qymcad_core::model::{Id, Project, SourceFile};
use qymcad_kernel::Shape;
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

/// Restore only absent bodies; validate sources separately so usable stored B-reps stay authoritative.
/// Restored shapes are named the way the app names them (`restore_names`).
pub(crate) fn restore_missing(project: &Project, shapes: &mut HashMap<Id, Shape>) -> Vec<AdvisoryWarning> {
    let mut by_source: HashMap<Id, Vec<(Id, u32, f64)>> = HashMap::new();
    for n in &project.timeline {
        if let FeatureKind::Import { body, source, solid, scale } = n.kind {
            by_source.entry(source).or_default().push((body, solid, scale));
        }
    }
    let mut warnings = Vec::new();
    let mut restored_bodies: Vec<(Id, Shape)> = Vec::new();
    for (source, bodies) in by_source {
        let restored = project.sources.iter().find(|sf| sf.id == source).ok_or_else(|| "source is missing".into()).and_then(read_source);
        let (mut solids, error) = match restored {
            Ok(solids) => (solids.into_iter().map(Some).collect::<Vec<_>>(), None),
            Err(error) => (Vec::new(), Some(error)),
        };
        for (body, solid, scale) in bodies {
            if shapes.contains_key(&body) && error.is_none() {
                continue;
            }
            let restored = solids
                .get_mut(solid as usize)
                .and_then(Option::take)
                .ok_or_else(|| error.clone().unwrap_or_else(|| format!("solid index {solid} is absent")))
                .and_then(|shape| {
                    if (scale - 1.0).abs() <= 1e-12 {
                        return Ok(shape);
                    }
                    qymcad_kernel::imports::scaled(&shape, scale).ok_or_else(|| format!("cannot apply import scale {scale}"))
                });
            match restored {
                Ok(shape) => {
                    restored_bodies.push((body, shape));
                }
                Err(error) => {
                    let name =
                        project.timeline.iter().find(|n| n.id == body).map(|n| crate::localization::name(&n.name)).unwrap_or_default();
                    let geometry =
                        if shapes.contains_key(&body) { "stored exact geometry is retained" } else { "exact geometry is unavailable" };
                    warnings.push(AdvisoryWarning::ImportSource(NodeIssue {
                        node: body,
                        message: format!("import body {body} `{name}` has an unavailable embedded source {source}: {error}; {geometry}; document data is retained"),
                        name,
                    }));
                }
            }
        }
    }
    if !restored_bodies.is_empty() {
        for (body, shape, unknown) in restore_names(project, restored_bodies) {
            if unknown {
                let name = project.timeline.iter().find(|n| n.id == body).map(|n| crate::localization::name(&n.name)).unwrap_or_default();
                warnings.push(AdvisoryWarning::ImportSource(NodeIssue {
                    node: body,
                    message: format!(
                        "import body {body} `{name}` was restored with positional face and edge ids: the document's name table does not hold the names it came in with; saved selections on it may not resolve"
                    ),
                    name,
                }));
            }
            shapes.insert(body, shape);
        }
    }
    warnings.sort_by_key(|w| w.issue().node);
    warnings
}

/// Upstream `named_as_they_came_in` (qymcad-kernel/src/imports.rs:90-107) without `raise_imports`, which stages
/// the source under a predictable name and parses it outside the gate. Our parse above stages safely under the
/// gate; naming then runs under one gate of its own. Upstream also sends names the document does not hold back to
/// positional numbers (`unminted`, private); without that step such a body keeps its parsed positional ids and
/// the caller warns when the document knew some of its names. Returns (body, shape, warn).
fn restore_names(project: &Project, bodies: Vec<(Id, Shape)>) -> Vec<(Id, Shape, bool)> {
    let kernel = qymcad_kernel::OcctKernel::default();
    // Copies before naming, for the fallback; `scaled` takes the gate itself, so this runs before ours.
    let mut positional: HashMap<Id, Shape> =
        bodies.iter().filter_map(|(body, shape)| qymcad_kernel::imports::scaled(shape, 1.0).map(|copy| (*body, copy))).collect();
    let ids: Vec<Id> = bodies.iter().map(|(body, _)| *body).collect();
    *kernel.shapes.borrow_mut() = bodies.into_iter().collect();
    let mut names = project.names.clone();
    let mut unknown: HashMap<Id, bool> = HashMap::new();
    {
        let _gate = qymcad_kernel::kernel_gate();
        for &body in &ids {
            let Some(feature) =
                project.timeline.iter().find(|n| matches!(n.kind, FeatureKind::Import { body: b, .. } if b == body)).map(|n| n.id)
            else {
                continue;
            };
            let scale = project.import_scale(body).filter(|scale| *scale > 0.0).unwrap_or(1.0);
            kernel.name_silent(qymcad_core::feature::SilentNaming { body, feature, listed: None, scale, names: &mut names });
            // Every face of a solid body is bounded by an edge, so the edge-face pairs list all of them.
            let named = kernel
                .edge_face_pairs(body)
                .into_iter()
                .flat_map(|pair| [pair.edge, pair.faces[0], pair.faces[1]])
                .filter(|d| qymcad_core::names::NameTable::is_named(*d));
            let (known, missing) = named.fold((0, 0), |(k, m), d| if project.names.knows(d) { (k + 1, m) } else { (k, m + 1) });
            if missing > 0 {
                // None known: the import was never named in the document, nothing saved can refer to it, and upstream
                // leaves it positional too. Some known: upstream renumbers only the unknown ones; warn.
                unknown.insert(body, known > 0);
            }
        }
    }
    kernel
        .shapes
        .into_inner()
        .into_iter()
        .map(|(body, shape)| {
            if let Some(&warn) = unknown.get(&body) {
                (body, positional.remove(&body).unwrap_or(shape), warn)
            } else {
                (body, shape, false)
            }
        })
        .collect()
}

fn read_source(source: &SourceFile) -> Result<Vec<Shape>, String> {
    use std::os::unix::fs::OpenOptionsExt;
    if source.data.is_empty() {
        return Err("source is empty".into());
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let format = qymcad_kernel::ExactFormat::of_extension(&source.ext);
    let ext = match format {
        qymcad_kernel::ExactFormat::Step => "step",
        qymcad_kernel::ExactFormat::Iges => "igs",
    };
    for _ in 0..64 {
        let path =
            std::env::temp_dir().join(format!("qymcad-mcp-import-{}-{}.{ext}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let mut file = match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("cannot stage source: {error}")),
        };
        let result = (|| {
            file.write_all(&source.data).map_err(|e| format!("cannot stage source: {e}"))?;
            let path = path.to_str().ok_or("temporary path is not UTF-8")?;
            let _gate = qymcad_kernel::kernel_gate();
            qymcad_kernel::exact_solids(format, path).map_err(|e| format!("cannot read source: {e}"))
        })();
        drop(file);
        let _ = std::fs::remove_file(&path);
        return result;
    }
    Err("cannot allocate a temporary source file".into())
}
