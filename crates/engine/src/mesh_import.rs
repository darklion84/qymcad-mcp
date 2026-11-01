use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use qymcad_core::model::{FileGroup, Id, ImportNode};
use qymcad_io::NamedMesh;
use serde::Serialize;
use std::path::Path;

pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_ZIP_UNCOMPRESSED: u64 = 64 * 1024 * 1024;
pub const MAX_TRIANGLES: usize = 200_000;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MeshFormat {
    Stl,
    Obj,
    Ply,
    Gltf,
    Glb,
    ThreeMf,
    Amf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MeshUnit {
    Mm,
    Cm,
    M,
    In,
    Ft,
}

impl MeshUnit {
    fn factor(self) -> f64 {
        match self {
            Self::Mm => 1.0,
            Self::Cm => 10.0,
            Self::M => 1000.0,
            Self::In => 25.4,
            Self::Ft => 304.8,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MeshPieceResult {
    pub body: Id,
    pub name: String,
    pub triangles: usize,
    pub vertices: usize,
    pub unit: MeshUnit,
    pub scale: f64,
    #[serde(serialize_with = "crate::session::serialize_coordinates")]
    pub bbox: [f64; 6],
}

#[derive(Clone, Debug, Serialize)]
pub struct MeshImportResult {
    pub source: Id,
    pub feature: Id,
    pub pieces: Vec<MeshPieceResult>,
    pub unit: MeshUnit,
    pub scale: f64,
    pub warnings: Vec<crate::session::NodeIssue>,
    pub rebuild: Rebuild,
}

fn extension(path: &Path) -> Result<MeshFormat> {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("stl") => Ok(MeshFormat::Stl),
        Some("obj") => Ok(MeshFormat::Obj),
        Some("ply") => Ok(MeshFormat::Ply),
        Some("gltf") => Ok(MeshFormat::Gltf),
        Some("glb") => Ok(MeshFormat::Glb),
        Some("3mf") => Ok(MeshFormat::ThreeMf),
        Some("amf") => Ok(MeshFormat::Amf),
        _ => Err(Error::Invalid("mesh import path must end in .stl, .obj, .ply, .gltf, .glb, .3mf or .amf".into())),
    }
}

fn format_name(format: MeshFormat) -> &'static str {
    match format {
        MeshFormat::Stl => "stl",
        MeshFormat::Obj => "obj",
        MeshFormat::Ply => "ply",
        MeshFormat::Gltf => "gltf",
        MeshFormat::Glb => "glb",
        MeshFormat::ThreeMf => "3mf",
        MeshFormat::Amf => "amf",
    }
}

fn preflight(path: &Path, format: MeshFormat) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path).map_err(|e| Error::Io(format!("cannot read mesh source {}: {e}", path.display())))?;
    if !metadata.is_file() {
        return Err(Error::Invalid(format!("mesh input is not a file: {}", path.display())));
    }
    if metadata.len() > MAX_BYTES {
        return Err(Error::Invalid(format!("mesh input byte limit is {MAX_BYTES}, measured {}", metadata.len())));
    }
    let bytes = std::fs::read(path).map_err(|e| Error::Io(format!("cannot read mesh source {}: {e}", path.display())))?;
    if bytes.is_empty() {
        return Err(Error::Invalid("mesh input is empty".into()));
    }
    if matches!(format, MeshFormat::Stl) && bytes.len() >= 84 {
        let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        let ascii = bytes.trim_ascii_start().starts_with(b"solid");
        if !ascii && n > MAX_TRIANGLES {
            return Err(Error::Invalid(format!("mesh triangle limit is {MAX_TRIANGLES}, measured {n}")));
        }
        if n.checked_mul(50).and_then(|v| v.checked_add(84)).is_some_and(|end| end == bytes.len()) {
            return Ok(bytes);
        }
    }
    // A ZIP container (3MF, zipped AMF) is inflated here, counted, before the native reader touches it: declared
    // sizes are only a quick first refusal, a lying central directory is caught by the real inflated count.
    let zipped = match format {
        MeshFormat::ThreeMf => true,
        MeshFormat::Amf => bytes.starts_with(b"PK"),
        _ => false,
    };
    if zipped {
        zip_within_cap(&bytes, format)?;
    }
    // Secondary files the native readers open by a path written inside the mesh (OBJ `mtllib`, glTF buffer `uri`)
    // must lie beside it: a plain relative path, a regular file, inside the mesh's folder; they count to MAX_BYTES.
    let mut total = metadata.len();
    match format {
        MeshFormat::Obj => {
            for library in obj_material_libraries(&String::from_utf8_lossy(&bytes)) {
                secondary_file(path, &library, true, &mut total)?;
            }
        }
        MeshFormat::Gltf | MeshFormat::Glb => {
            for uri in gltf_buffer_uris(&bytes)? {
                if !uri.starts_with("data:") {
                    secondary_file(path, &uri.replace("%20", " "), false, &mut total)?;
                }
            }
        }
        _ => {}
    }
    Ok(bytes)
}

fn zip_within_cap(bytes: &[u8], format: MeshFormat) -> Result<()> {
    use std::io::Read;
    let label = format_name(format);
    let bad = |e: String| Error::Invalid(format!("cannot inspect {label} archive: {e}"));
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| bad(e.to_string()))?;
    let mut declared = 0u64;
    for i in 0..archive.len() {
        declared = declared.saturating_add(archive.by_index_raw(i).map_err(|e| bad(e.to_string()))?.size());
        if declared > MAX_ZIP_UNCOMPRESSED {
            return Err(Error::Invalid(format!(
                "{label} declared uncompressed limit is {MAX_ZIP_UNCOMPRESSED}, measured at least {declared}"
            )));
        }
    }
    let mut inflated = 0u64;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| bad(e.to_string()))?;
        let room = MAX_ZIP_UNCOMPRESSED - inflated;
        inflated += std::io::copy(&mut entry.take(room + 1), &mut std::io::sink()).map_err(|e| bad(e.to_string()))?;
        if inflated > MAX_ZIP_UNCOMPRESSED {
            return Err(Error::Invalid(format!("{label} uncompressed limit is {MAX_ZIP_UNCOMPRESSED}, inflated at least {inflated}")));
        }
    }
    Ok(())
}

/// The `mtllib` references exactly as `qymcad_io::obj::parse` reads them: backslash-continued lines joined, `#`
/// comments cut, the rest of the line's words joined by one space. Any other reading would check a different path
/// than the one the reader opens.
fn obj_material_libraries(text: &str) -> Vec<String> {
    let joined = text.replace("\\\r\n", " ").replace("\\\n", " ");
    joined
        .lines()
        .filter_map(|raw| {
            let mut words = raw.split('#').next().unwrap_or("").split_whitespace();
            (words.next() == Some("mtllib")).then(|| words.collect::<Vec<_>>().join(" "))
        })
        .collect()
}

/// The `uri` of every glTF buffer, from a `.gltf` JSON or the JSON chunk of a `.glb`.
fn gltf_buffer_uris(bytes: &[u8]) -> Result<Vec<String>> {
    let not_gltf = || Error::Invalid("mesh input is not a readable glTF".into());
    let json = if bytes.starts_with(b"glTF") {
        let len = bytes.get(12..16).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize).ok_or_else(not_gltf)?;
        bytes.get(20..20usize.checked_add(len).ok_or_else(not_gltf)?).ok_or_else(not_gltf)?
    } else {
        bytes
    };
    let doc: serde_json::Value = serde_json::from_slice(json).map_err(|_| not_gltf())?;
    Ok(doc["buffers"]
        .as_array()
        .map(|buffers| buffers.iter().filter_map(|b| b["uri"].as_str().map(str::to_string)).collect())
        .unwrap_or_default())
}

/// A file a mesh names inside itself. `optional`: the native reader skips a missing one (OBJ materials).
fn secondary_file(main: &Path, reference: &str, optional: bool, total: &mut u64) -> Result<()> {
    use std::path::Component;
    let refuse = |why: &str| {
        Error::Invalid(format!(
            "mesh input refers to `{reference}`: {why}; a secondary file must be a plain relative path to a regular file in the mesh's folder"
        ))
    };
    let relative = Path::new(reference);
    if reference.is_empty() || !relative.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(refuse("not a plain relative path"));
    }
    let dir = main.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let joined = dir.join(relative);
    let meta = match std::fs::symlink_metadata(&joined) {
        Ok(meta) => meta,
        Err(e) if optional && e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::Io(format!("cannot read secondary mesh file `{reference}`: {e}"))),
    };
    if !meta.file_type().is_file() {
        return Err(refuse("not a regular file"));
    }
    let inside = match (dir.canonicalize(), joined.canonicalize()) {
        (Ok(d), Ok(f)) => f.starts_with(d),
        _ => false,
    };
    if !inside {
        return Err(refuse("outside the mesh's folder"));
    }
    *total = total.saturating_add(meta.len());
    if *total > MAX_BYTES {
        return Err(Error::Invalid(format!("mesh input byte limit (with its secondary files) is {MAX_BYTES}, measured {total}")));
    }
    Ok(())
}

fn mesh_reader(format: MeshFormat, path: &Path) -> Result<Vec<NamedMesh>> {
    let path = path.to_str().ok_or_else(|| Error::Invalid("mesh path is not UTF-8".into()))?;
    let result = match format {
        MeshFormat::Stl => qymcad_io::import_stl_named(path),
        MeshFormat::Obj => qymcad_io::import_obj(path),
        MeshFormat::Ply => qymcad_io::import_ply_coloured(path).map(|mesh| vec![mesh]),
        MeshFormat::Gltf | MeshFormat::Glb => qymcad_io::import_gltf(path),
        MeshFormat::ThreeMf => qymcad_io::import_3mf(path),
        MeshFormat::Amf => qymcad_io::import_amf(path),
    };
    result.map_err(|e| Error::Invalid(format!("cannot read {} mesh: {e}", format_name(format))))
}

fn insert_group_path(level: &mut Vec<ImportNode>, groups: &[FileGroup], leaf: ImportNode) {
    let Some(group) = groups.first() else {
        level.push(leaf);
        return;
    };
    let position = level.iter().position(|node| node.body.is_none() && node.solid == group.index as u32 && node.name == group.name);
    let node = match position {
        Some(position) => &mut level[position],
        None => {
            level.push(ImportNode { name: group.name.clone(), place: group.place, solid: group.index as u32, ..Default::default() });
            level.last_mut().expect("group was just inserted")
        }
    };
    insert_group_path(&mut node.children, &groups[1..], leaf);
}

fn nodes(pieces: &[(usize, NamedMesh, Id)], stem: &str) -> Vec<ImportNode> {
    let mut top = Vec::new();
    for (index, piece, body) in pieces {
        let name = if piece.name.trim().is_empty() {
            if pieces.len() == 1 {
                stem.to_string()
            } else {
                format!("{stem} {}", index + 1)
            }
        } else {
            piece.name.clone()
        };
        let leaf = ImportNode {
            name,
            place: piece.place,
            body: Some(*body),
            solid: *index as u32,
            color: piece.color,
            tri_colors: piece.tri_colors.clone(),
            mesh: true,
            ..Default::default()
        };
        insert_group_path(&mut top, &piece.within, leaf);
    }
    top
}

fn bbox(mesh: &qymcad_core::geom::Mesh, transform: [f64; 12]) -> [f64; 6] {
    let mut out = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in &mesh.verts {
        let v = [
            transform[0] * p.x + transform[1] * p.y + transform[2] * p.z + transform[3],
            transform[4] * p.x + transform[5] * p.y + transform[6] * p.z + transform[7],
            transform[8] * p.x + transform[9] * p.y + transform[10] * p.z + transform[11],
        ];
        for i in 0..3 {
            out[i] = out[i].min(v[i]);
            out[i + 3] = out[i + 3].max(v[i]);
        }
    }
    out
}

impl Session {
    pub fn import_mesh(
        &mut self,
        path: &Path,
        format: Option<&str>,
        unit: Option<&str>,
        scale: f64,
        name: Option<&str>,
    ) -> Result<MeshImportResult> {
        let detected = extension(path)?;
        let requested = format.map(|s| s.to_ascii_lowercase());
        if let Some(requested) = requested.as_deref() {
            if requested == "auto" {
            } else if requested != format_name(detected) {
                return Err(Error::Invalid(format!("mesh format `{requested}` does not agree with .{} extension", format_name(detected))));
            }
        }
        if !(scale.is_finite() && scale > 0.0) {
            return Err(Error::Invalid(format!("scale must be finite and positive, got {scale}")));
        }
        let embedded = matches!(detected, MeshFormat::Gltf | MeshFormat::Glb | MeshFormat::ThreeMf | MeshFormat::Amf);
        let chosen_unit = match unit.unwrap_or("mm") {
            "mm" => MeshUnit::Mm,
            "cm" => MeshUnit::Cm,
            "m" => MeshUnit::M,
            "in" => MeshUnit::In,
            "ft" => MeshUnit::Ft,
            other => return Err(Error::Invalid(format!("unknown mesh unit `{other}`"))),
        };
        if embedded && unit.is_some() {
            return Err(Error::Invalid(format!("explicit unit for {} is refused; the file declares its own unit", format_name(detected))));
        }
        let bytes = preflight(path, detected)?;
        let parsed = mesh_reader(detected, path)?;
        if parsed.is_empty() {
            return Err(Error::Invalid("mesh contains no pieces".into()));
        }
        let total_triangles: usize = parsed.iter().map(|p| p.mesh.tris.len()).sum();
        if total_triangles == 0 {
            return Err(Error::Invalid("mesh contains no triangles".into()));
        }
        if total_triangles > MAX_TRIANGLES {
            return Err(Error::Invalid(format!("mesh triangle limit is {MAX_TRIANGLES}, measured {total_triangles}")));
        }
        let unit_factor = if embedded { 1.0 } else { chosen_unit.factor() };
        let stem = path.file_stem().and_then(|s| s.to_str()).filter(|s| !s.is_empty()).unwrap_or("mesh").to_string();
        let mut prepared = Vec::with_capacity(parsed.len());
        for (index, mut piece) in parsed.into_iter().enumerate() {
            if unit_factor != 1.0 {
                piece.mesh.scale(0.0, 0.0, 0.0, unit_factor);
            }
            if !piece.mesh.verts.iter().all(|p| [p.x, p.y, p.z].into_iter().all(f64::is_finite)) {
                return Err(Error::Invalid("mesh contains a non-finite vertex".into()));
            }
            let faces = piece.mesh.detect_faces(1.0);
            if faces.is_empty() {
                return Err(Error::Invalid(format!("mesh piece {index} has no faces")));
            }
            prepared.push((index, piece, faces));
        }
        if let Some(name) = name {
            Self::check_object_name(name)?;
        }
        let result = self.atomic(|session| {
            let source = session.p.add_source(path.file_name().and_then(|s| s.to_str()).unwrap_or("mesh").to_string(), bytes.clone());
            let mut created = Vec::new();
            for (index, piece, faces) in &prepared {
                let body = session.p.add_mesh(piece.mesh.clone());
                session.p.set_body_faces(body, faces.clone());
                session.p.regen_faces.insert(body, faces.clone());
                created.push((
                    *index,
                    NamedMesh {
                        name: piece.name.clone(),
                        mesh: piece.mesh.clone(),
                        color: piece.color,
                        place: piece.place,
                        tri_colors: piece.tri_colors.clone(),
                        within: piece.within.clone(),
                    },
                    body,
                ));
            }
            let import_name = name.unwrap_or(&stem);
            let feature = session
                .p
                .import_tree_as_parts(nodes(&created, &stem), source, import_name)
                .ok_or_else(|| Error::Invalid("mesh import produced no tree".into()))?;
            let components: Vec<Id> = std::iter::once(feature).chain(session.p.descendants(feature)).collect();
            if scale != 1.0 {
                for component in &components {
                    let mut transform = session.p.component_transform(*component);
                    for axis in [3, 7, 11] {
                        transform[axis] *= scale;
                    }
                    session.p.set_component_transform(*component, transform);
                }
                for (_, _, body) in &created {
                    session.p.set_import_scale(*body, scale);
                }
            }
            session.p.name_apart(&components, &crate::localization::name);
            Ok((source, feature, created))
        })?;
        let ((source, feature, created), rebuild) = result;
        let pieces = created
            .into_iter()
            .map(|(_index, piece, body)| {
                let mut mesh = piece.mesh;
                if scale != 1.0 {
                    mesh.scale(0.0, 0.0, 0.0, scale);
                }
                let transform = self.p.body_world_transform(body);
                MeshPieceResult {
                    body,
                    name: self.body_name(body),
                    triangles: mesh.tris.len(),
                    vertices: mesh.verts.len(),
                    unit: if embedded { MeshUnit::Mm } else { chosen_unit },
                    scale,
                    bbox: bbox(&mesh, transform),
                }
            })
            .collect();
        Ok(MeshImportResult {
            source,
            feature,
            pieces,
            unit: if embedded { MeshUnit::Mm } else { chosen_unit },
            scale,
            warnings: rebuild.warnings.clone(),
            rebuild,
        })
    }
}
