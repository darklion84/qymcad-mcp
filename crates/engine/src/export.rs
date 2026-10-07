//! Export of result bodies to exchange files: STEP (exact B-rep) and meshes (STL, 3MF, GLB, OBJ).
//!
//! Mirrors QymCAD.app's File > Export (`crates/qymcad/src/gui/io_jobs.rs` `write_exact_to` / `write_mesh_to`):
//! every body goes out with its world transform (`body_world_transform`); meshes are re-tessellated from the live
//! B-rep at the chosen deflection. Unlike the app we always write flat (one object per body, no component tree,
//! no colours): see FINDINGS F-3C-2.

use crate::error::{Error, Result};
use crate::session::Session;
use qymcad_core::geom::Mesh;
use qymcad_core::model::Id;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// An export file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// STEP AP214, exact B-rep, millimetres (`.step`, `.stp`).
    Step,
    /// Binary STL, millimetres (`.stl`).
    Stl,
    /// 3MF package, millimetres, one object per body (`.3mf`).
    #[serde(rename = "3mf")]
    ThreeMf,
    /// Binary glTF, metres, +Y up (`.glb`).
    Glb,
    /// Wavefront OBJ, millimetres (`.obj`).
    Obj,
}

impl ExportFormat {
    /// File extensions a path for this format may have (lowercase, without the dot).
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            ExportFormat::Step => &["step", "stp"],
            ExportFormat::Stl => &["stl"],
            ExportFormat::ThreeMf => &["3mf"],
            ExportFormat::Glb => &["glb"],
            ExportFormat::Obj => &["obj"],
        }
    }

    pub fn is_mesh(self) -> bool {
        self != ExportFormat::Step
    }
}

/// Mesh detail: the chordal deflection used to tessellate curved faces. The presets of QymCAD.app's mesh export
/// dialog (`crates/qymcad/src/gui/panels_windows.rs` `mesh_quality_dialog`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    /// 0.2 mm
    Draft,
    /// 0.05 mm (default)
    #[default]
    Standard,
    /// 0.02 mm
    High,
    /// 0.005 mm
    Max,
}

impl Quality {
    /// The deflection in mm.
    pub fn deflection(self) -> f64 {
        match self {
            Quality::Draft => 0.2,
            Quality::Standard => 0.05,
            Quality::High => 0.02,
            Quality::Max => 0.005,
        }
    }
}

/// What an export wrote.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ExportReport {
    pub format: ExportFormat,
    pub path: String,
    /// The bodies written, in file order.
    pub bodies: Vec<ExportedBody>,
    /// Mesh formats: the deflection used, mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deflection_mm: Option<f64>,
    /// Mesh formats: triangles written in total.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triangles: Option<usize>,
    /// Size of the written file.
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ExportedBody {
    pub id: Id,
    pub name: String,
    /// Mesh formats: volume of the written mesh (sum of tetrahedra, absolute), mm³. Compare with the B-rep volume to see
    /// the tessellation error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_volume: Option<f64>,
}

impl Session {
    /// The bodies an export or render works on: `bodies` (each must be a current result body), or all result
    /// bodies. Errors if there is nothing to write.
    pub(crate) fn output_bodies(&self, bodies: Option<&[Id]>) -> Result<Vec<Id>> {
        // A failed feature passes its source body through unchanged (FINDINGS F-008): writing or showing the
        // result would silently drop that feature, so refuse until the document rebuilds cleanly.
        if !self.p.regen_errors.is_empty() {
            let mut lines: Vec<String> = self.p.regen_errors.iter().map(|(id, e)| format!("{} ({id}): {e}", self.node_name(*id))).collect();
            lines.sort();
            return Err(Error::Invalid(format!(
                "the document has features that did not build, so the bodies do not show the full recipe: {}. Fix or \
                 remove them first (doc_info lists them)",
                lines.join("; ")
            )));
        }
        let results: Vec<Id> = self.result_bodies().iter().map(|b| b.id).collect();
        let Some(asked) = bodies else {
            if results.is_empty() {
                return Err(Error::Invalid("the document has no bodies yet; build something first".into()));
            }
            return Ok(results);
        };
        if asked.is_empty() {
            return Err(Error::Invalid("`bodies` is empty; omit it to use every result body".into()));
        }
        let mut out = Vec::new();
        for &id in asked {
            if !results.contains(&id) {
                return Err(Error::Invalid(format!(
                    "{id} (`{}`) is not a current result body; result bodies are {results:?} (a feature's id is its body's id, \
                     and a body consumed by a later feature is not a result)",
                    self.node_name(id)
                )));
            }
            if !out.contains(&id) {
                out.push(id);
            }
        }
        Ok(out)
    }

    /// Write `bodies` (default: every result body) to `path` in `format`. The caller has validated the path; the
    /// extension is checked again here. `quality` applies to mesh formats only.
    pub fn export(&self, format: ExportFormat, path: &Path, quality: Quality, bodies: Option<&[Id]>) -> Result<ExportReport> {
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
        if !format.extensions().contains(&ext.as_str()) {
            return Err(Error::Invalid(format!("a {format:?} file must end in .{}", format.extensions().join(" or ."))));
        }
        let final_path = path.to_str().ok_or_else(|| Error::Invalid(format!("path is not UTF-8: {}", path.display())))?;
        let ids = self.output_bodies(bodies)?;
        // Write inside a private directory created fresh next to the target (mode 0700; `create_dir` never follows
        // or reuses an existing entry, a planted symlink included: that name is skipped), then rename the file over the target. QymCAD's writers open
        // their path by name, so the file must live where no other user can swap it; writing the target directly
        // would truncate whatever inode it is (a hard link, a late symlink). A rename only replaces the directory
        // entry (docs/SECURITY.md).
        let stage = private_dir_next_to(path)
            .map_err(|e| Error::Io(format!("cannot create a private staging directory next to {final_path}: {e}")))?;
        let tmp = stage.join(format!("export.{ext}"));
        let result = self.export_to(format, &tmp, quality, &ids, final_path).and_then(|mut report| {
            std::fs::rename(&tmp, path).map_err(|e| Error::Io(format!("cannot move the export into place at {final_path}: {e}")))?;
            report.bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            Ok(report)
        });
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_dir(&stage);
        result
    }

    /// The actual write, into `tmp` (a fresh file); the report names `final_path`.
    fn export_to(&self, format: ExportFormat, tmp: &Path, quality: Quality, ids: &[Id], final_path: &str) -> Result<ExportReport> {
        let s = tmp.to_str().ok_or_else(|| Error::Invalid(format!("path is not UTF-8: {}", tmp.display())))?;
        let mut report =
            ExportReport { format, path: final_path.to_string(), bodies: Vec::new(), deflection_mm: None, triangles: None, bytes: 0 };
        let _gate = qymcad_kernel::kernel_gate();
        if format.is_mesh() {
            let deflection = quality.deflection();
            let mut meshes: Vec<Mesh> = Vec::with_capacity(ids.len());
            for &id in ids {
                let shape = self.shapes.get(&id).ok_or_else(|| Error::NotFound(format!("body {id} has no geometry")))?;
                let (mut mesh, _faces) = shape
                    .tessellate_merged(deflection)
                    .ok_or_else(|| Error::Invalid(format!("body {id} (`{}`) did not tessellate", self.node_name(id))))?;
                mesh.transform(&self.p.body_world_transform(id));
                report.bodies.push(ExportedBody { id, name: self.node_name(id), mesh_volume: Some(mesh.volume()) });
                meshes.push(mesh);
            }
            let written = match format {
                ExportFormat::Stl => qymcad_io::export_stl(&meshes, s),
                ExportFormat::ThreeMf => qymcad_io::export_3mf(&meshes, s),
                ExportFormat::Glb => qymcad_io::export_glb(&meshes, s),
                ExportFormat::Obj => qymcad_io::export_obj(&meshes, s),
                ExportFormat::Step => unreachable!("STEP is not a mesh format"),
            };
            written.map_err(|e| Error::Io(format!("cannot write {final_path}: {e}")))?;
            report.deflection_mm = Some(deflection);
            report.triangles = Some(meshes.iter().map(|m| m.tris.len()).sum());
        } else {
            let mut pairs = Vec::with_capacity(ids.len());
            for &id in ids {
                let shape = self.shapes.get(&id).ok_or_else(|| Error::NotFound(format!("body {id} has no geometry")))?;
                pairs.push((shape, self.p.body_world_transform(id)));
                report.bodies.push(ExportedBody { id, name: self.node_name(id), mesh_volume: None });
            }
            qymcad_kernel::write_step(&pairs, s).map_err(|e| Error::Io(format!("cannot write {final_path}: {e}")))?;
        }
        Ok(report)
    }
}

static STAGE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Create a fresh directory `.{stem}.qymcad-mcp-{pid}-{n}.d` (mode 0700) next to `path`, like `mkdtemp`: a name that
/// already exists, whatever it is, is skipped rather than used.
fn private_dir_next_to(path: &Path) -> std::io::Result<std::path::PathBuf> {
    private_dir_next_to_with_seq(path, &STAGE_SEQ)
}

fn private_dir_next_to_with_seq(path: &Path, seq: &std::sync::atomic::AtomicU64) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("export");
    let mut last = None;
    for _ in 0..64 {
        let dir = path.with_file_name(format!(
            ".{stem}.qymcad-mcp-{}-{}.d",
            std::process::id(),
            seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free name")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    /// Entries planted at the next staging names (symlinks to another directory) are skipped, never followed or
    /// reused; the directory made is private (review round 2, Codex).
    #[test]
    fn staging_skips_planted_entries() {
        let base = std::env::temp_dir().join(format!("qymcad-mcp-stage-{}", std::process::id()));
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let target = base.join("part.stl");
        let n = 0;
        let seq = AtomicU64::new(n);
        let name = |k: u64| base.join(format!(".part.qymcad-mcp-{}-{k}.d", std::process::id()));
        for k in n..n + 3 {
            std::os::unix::fs::symlink(&elsewhere, name(k)).unwrap();
        }
        // Another staging user may allocate after the test has planted its names. Advance past all three names
        // deterministically, as four intervening exports would, without depending on the test scheduler.
        for _ in 0..4 {
            let other = private_dir_next_to(&base.join("other.stl")).unwrap();
            std::fs::remove_dir(other).unwrap();
        }
        let dir = private_dir_next_to_with_seq(&target, &seq).unwrap();
        // Starting at n and refusing three occupied names must allocate n + 3 regardless of unrelated users.
        assert_eq!(dir, name(n + 3), "the three planted names are skipped");
        let meta = std::fs::symlink_metadata(&dir).unwrap();
        assert!(meta.is_dir() && !meta.file_type().is_symlink());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(meta.permissions().mode() & 0o777, 0o700, "private");
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0, "nothing created through a link");
        std::fs::remove_dir_all(&base).unwrap();
    }
}
