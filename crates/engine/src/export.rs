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
    /// Mesh formats: volume of the written mesh (signed tetrahedra), mm³. Compare with the B-rep volume to see
    /// the tessellation error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_volume: Option<f64>,
}

impl Session {
    /// The bodies an export or render works on: `bodies` (each must be a current result body), or all result
    /// bodies. Errors if there is nothing to write.
    pub(crate) fn output_bodies(&self, bodies: Option<&[Id]>) -> Result<Vec<Id>> {
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
        let s = path.to_str().ok_or_else(|| Error::Invalid(format!("path is not UTF-8: {}", path.display())))?;
        let ids = self.output_bodies(bodies)?;
        let mut report = ExportReport { format, path: s.to_string(), bodies: Vec::new(), deflection_mm: None, triangles: None, bytes: 0 };
        let _gate = qymcad_kernel::kernel_gate();
        if format.is_mesh() {
            let deflection = quality.deflection();
            let mut meshes: Vec<Mesh> = Vec::with_capacity(ids.len());
            for &id in &ids {
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
            written.map_err(|e| Error::Io(format!("cannot write {s}: {e}")))?;
            report.deflection_mm = Some(deflection);
            report.triangles = Some(meshes.iter().map(|m| m.tris.len()).sum());
        } else {
            let mut pairs = Vec::with_capacity(ids.len());
            for &id in &ids {
                let shape = self.shapes.get(&id).ok_or_else(|| Error::NotFound(format!("body {id} has no geometry")))?;
                pairs.push((shape, self.p.body_world_transform(id)));
                report.bodies.push(ExportedBody { id, name: self.node_name(id), mesh_volume: None });
            }
            qymcad_kernel::write_step(&pairs, s).map_err(|e| Error::Io(format!("cannot write {s}: {e}")))?;
        }
        report.bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Ok(report)
    }
}
