//! `Session`: one open QymCAD document plus its live B-rep shapes, and the regenerate pipeline.

use crate::error::{Error, Result};
use qymcad_core::model::{Id, Project};
use qymcad_kernel::Shape;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A problem attached to a timeline node after a rebuild.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct NodeIssue {
    pub node: Id,
    pub name: String,
    pub message: String,
}

/// Source availability cannot be repaired by retessellating an import's live shape, nor can geometry QymCAD
/// dropped when it read the file; both last for the session.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AdvisoryWarning {
    Geometry(NodeIssue),
    ImportSource(NodeIssue),
    LostOnLoad(NodeIssue),
}

impl AdvisoryWarning {
    pub(crate) fn issue(&self) -> &NodeIssue {
        match self {
            Self::Geometry(issue) | Self::ImportSource(issue) | Self::LostOnLoad(issue) => issue,
        }
    }
}

/// A body that is a current result of the model (not consumed by a later feature).
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct BodyInfo {
    pub id: Id,
    pub name: String,
    /// mm³, serialized as `volume_mm3` at native floating-point precision.
    #[serde(rename = "volume_mm3")]
    pub volume: f64,
    /// `[xmin, ymin, zmin, xmax, ymax, zmax]` in mm, from fresh nominal-deflection tessellation
    /// (.005 mm, scaled to 1e-5 of diagonals over 500 mm) plus f32 coordinate rounding; native bounds are a fallback if meshing fails (F-066).
    /// JSON coordinates use four decimals, rounded half away from zero, with negative zero normalized.
    #[serde(serialize_with = "serialize_coordinates")]
    pub bbox: [f64; 6],
}

/// Four-decimal presentation only; internal model values retain full precision.
fn round_coordinate(value: f64) -> f64 {
    let scaled = value * 10_000.0;
    let rounded = if scaled.is_finite() { scaled.round() / 10_000.0 } else { value };
    if rounded == 0.0 {
        0.0
    } else {
        rounded
    }
}

pub(crate) fn serialize_coordinates<const N: usize, S: serde::Serializer>(
    values: &[f64; N],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    values.map(round_coordinate).serialize(serializer)
}

pub(crate) fn serialize_coordinate<S: serde::Serializer>(value: &f64, serializer: S) -> std::result::Result<S::Ok, S::Error> {
    round_coordinate(*value).serialize(serializer)
}

/// What a rebuild produced.
#[derive(Clone, Debug, Serialize, Default, PartialEq)]
pub struct Rebuild {
    pub errors: Vec<NodeIssue>,
    pub warnings: Vec<NodeIssue>,
    /// Information about this call's resolved selection; not a persistent document warning.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    pub bodies: Vec<BodyInfo>,
}

struct BodyMetrics {
    volume: f64,
    bbox: Option<[f64; 6]>,
    shells: u32,
    solids: u32,
    faces: Option<u32>,
}

impl BodyMetrics {
    fn of(shape: &Shape) -> Self {
        Self {
            volume: shape.volume(),
            bbox: reporting_bbox(shape),
            shells: shape.shell_count(),
            solids: shape.solid_count(),
            faces: shape.face_kinds().map(|kinds| kinds.into_iter().sum()),
        }
    }
}

/// Restore saved bodies through the public door; this function acquires the kernel gate.
pub(crate) fn restore_shapes(saved: Vec<(Id, Vec<u8>)>) -> HashMap<Id, Shape> {
    qymcad_kernel::imports::restore(saved.into_iter().map(|(body, bytes)| qymcad_kernel::imports::SavedBody { body, bytes }).collect())
        .into_iter()
        .map(|b| (b.body, b.shape))
        .collect()
}

/// Independent copies for rollback. Serialization holds the gate, restoration takes it itself.
/// Refuse a missing copy rather than silently losing a rollback body.
pub(crate) fn copy_shapes<'a>(shapes: impl IntoIterator<Item = (Id, &'a Shape)>) -> std::result::Result<HashMap<Id, Shape>, Id> {
    let saved: Vec<(Id, Vec<u8>)> = {
        let _gate = qymcad_kernel::kernel_gate();
        shapes.into_iter().map(|(id, sh)| sh.to_brep_bytes().map(|b| (id, b)).ok_or(id)).collect::<std::result::Result<_, _>>()?
    };
    let ids: Vec<Id> = saved.iter().map(|(id, _)| *id).collect();
    let copies = restore_shapes(saved);
    if let Some(id) = ids.into_iter().find(|id| !copies.contains_key(id)) {
        return Err(id);
    }
    Ok(copies)
}

/// Per sketch index, how many splines a QymCAD 0.1.0 document stores in the old `Sketch.splines` records. QymCAD
/// 0.1.1 has no such field and drops them on load without a word (F-075). Unreadable files report nothing: the
/// native loader has already accepted this one.
fn legacy_splines(path: &Path) -> Vec<(usize, usize)> {
    #[derive(serde::Deserialize)]
    struct Doc {
        #[serde(default)]
        sketches: Vec<Sketch>,
    }
    #[derive(serde::Deserialize)]
    struct Sketch {
        #[serde(default)]
        splines: Vec<ron::Value>,
    }
    let read = || -> Option<Doc> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("document.ron").ok()?, &mut text).ok()?;
        ron::Options::default().without_recursion_limit().from_str(&text).ok()
    };
    read()
        .map(|doc| doc.sketches.into_iter().enumerate().filter(|(_, s)| !s.splines.is_empty()).map(|(i, s)| (i, s.splines.len())).collect())
        .unwrap_or_default()
}

/// The public adjacency API owns its shapes. Use a named B-rep copy while Session retains its live handle.
/// A failed copy is an error, never an empty adjacency answer. Must be called without the gate held.
pub(crate) fn edge_face_pairs(shape: &Shape) -> Result<Vec<(u32, u32, u32)>> {
    use qymcad_core::feature::Kernel;
    let kernel = qymcad_kernel::OcctKernel::default();
    *kernel.shapes.borrow_mut() = copy_shapes([(0, shape)]).map_err(|_| Error::Io("cannot copy body for adjacency".into()))?;
    let _gate = qymcad_kernel::kernel_gate();
    Ok(kernel.edge_face_pairs(0).into_iter().map(|p| (p.edge, p.faces[0], p.faces[1])).collect())
}

/// The edges of `shape` as the kernel lists them for a body, with the native names of their end vertices
/// (`MeshEdge::ends`). Same ownership rule as `edge_face_pairs`. Must be called without the gate held.
pub(crate) fn edges_with_ends(shape: &Shape) -> Result<Vec<qymcad_core::geom::MeshEdge>> {
    use qymcad_core::feature::Kernel;
    let kernel = qymcad_kernel::OcctKernel::default();
    *kernel.shapes.borrow_mut() = copy_shapes([(0, shape)]).map_err(|_| Error::Io("cannot copy body for edges".into()))?;
    let _gate = qymcad_kernel::kernel_gate();
    Ok(kernel.edges(0))
}

/// Reporting bounds must not depend on the live shape's triangulation state (F-066).
/// Tessellate an independent copy: the native tessellator cleans/replaces triangulations even via &Shape.
/// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()`.
/// This function always acquires that non-reentrant mutex (F-069).
fn reporting_bbox(shape: &Shape) -> Option<[f64; 6]> {
    let measured = || {
        let copy = copy_shapes([(0, shape)]).ok()?.remove(&0)?;
        // Keep .005 mm accuracy for ordinary parts, but avoid unbounded mesh growth on huge imports.
        // F-066: upstream also scales deflection with size to bound curved-body mesh costs.
        let deflection = {
            let _gate = qymcad_kernel::kernel_gate();
            0.005_f64.max(copy.bbox_diag() * 1e-5)
        };
        let qymcad_core::geom::Built { mesh, faces, .. } = qymcad_kernel::ask::export_mesh(&copy, deflection)?;
        let _gate = qymcad_kernel::kernel_gate();
        // A partial mesh cannot establish the body's extrema. Retain conservative native bounds instead.
        let face_count: u32 = copy.face_kinds()?.into_iter().sum();
        if faces.len() != face_count as usize || mesh.verts.is_empty() {
            return None;
        }
        let mut bbox = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for v in &mesh.verts {
            for (i, coordinate) in [v.x, v.y, v.z].into_iter().enumerate() {
                if !coordinate.is_finite() {
                    return None;
                }
                bbox[i] = bbox[i].min(coordinate);
                bbox[i + 3] = bbox[i + 3].max(coordinate);
            }
        }
        Some(bbox)
    };
    measured().or_else(|| {
        let _gate = qymcad_kernel::kernel_gate();
        shape.bbox()
    })
}

#[derive(Clone)]
pub(crate) struct DetachedProjection {
    pub id: Id,
    pub body: Id,
    pub source: qymcad_core::model::ProjSource,
}

pub struct Session {
    pub(crate) p: Project,
    pub(crate) shapes: HashMap<Id, Shape>,
    path: Option<PathBuf>,
    // Saved model's first body-producing node. History restores recipes, never this association.
    saved_lineage: Option<(Id, std::mem::Discriminant<qymcad_core::feature::FeatureKind>)>,
    pub(crate) advisory_warnings: Vec<AdvisoryWarning>,
    pub(crate) undo: std::collections::VecDeque<crate::history::Snapshot>,
    pub(crate) undo_limit_reached: bool,
    pub(crate) tool_edit_active: bool,
    // Originals removed by native sketch deletion, retained once while undo still references them.
    pub(crate) retired_sources: HashMap<Id, qymcad_core::model::SourceFile>,
    pub(crate) detached_projections: HashMap<(Id, Id), DetachedProjection>,
}

fn first_body_feature(project: &Project) -> Option<(Id, std::mem::Discriminant<qymcad_core::feature::FeatureKind>)> {
    project.timeline.iter().find(|n| !n.kind.bodies().is_empty()).map(|n| (n.id, std::mem::discriminant(&n.kind)))
}

impl Session {
    /// A new document with one empty part, ready to model in.
    pub fn new_part() -> Session {
        qymcad_core::model::set_producer(&format!("qymcad-mcp {}", env!("CARGO_PKG_VERSION")));
        let mut p = Project::default();
        p.new_document();
        Session {
            p,
            shapes: HashMap::new(),
            path: None,
            saved_lineage: None,
            advisory_warnings: Vec::new(),
            undo: Default::default(),
            undo_limit_reached: false,
            tool_edit_active: false,
            retired_sources: HashMap::new(),
            detached_projections: HashMap::new(),
        }
    }

    /// Open a `.qcad`, restoring live bodies from the file as the app does (FINDINGS F-007), and rebuilding
    /// whatever has no stored geometry. Imports without stored B-reps are restored from embedded originals
    /// first; unavailable originals produce persistent warnings while retaining the document data.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()` (F-069).
    pub fn open(path: &Path) -> Result<(Session, Rebuild)> {
        qymcad_core::model::set_producer(&format!("qymcad-mcp {}", env!("CARGO_PKG_VERSION")));
        let s = path.to_str().ok_or_else(|| Error::Invalid(format!("path is not UTF-8: {}", path.display())))?;
        let qymcad_io::LoadedProject { mut project, breps } =
            qymcad_io::load_project_with_brep(s).map_err(|e| Error::Io(format!("cannot open {s}: {e}")))?;
        project.ensure_document();
        restore_faces(&mut project);
        let mut shapes = restore_shapes(breps);
        let import_warnings = crate::imports::restore_missing(&project, &mut shapes);
        let missing = project.timeline.iter().flat_map(|n| n.kind.bodies()).any(|b| !shapes.contains_key(&b));
        let saved_lineage = first_body_feature(&project);
        let mut import_warnings = import_warnings;
        for (si, count) in legacy_splines(path) {
            let Some(sketch) = project.sketches.get(si) else { continue };
            import_warnings.push(AdvisoryWarning::LostOnLoad(NodeIssue {
                node: sketch.id,
                name: crate::localization::name(&sketch.name),
                message: format!(
                    "{count} {} saved by QymCAD 0.1.0 {} lost: QymCAD 0.1.1 does not read the old spline records and keeps only \
                     their points (F-075). Save to a new path to keep the original file",
                    if count == 1 { "spline" } else { "splines" },
                    if count == 1 { "is" } else { "are" },
                ),
            }));
        }
        let mut sess = Session {
            p: project,
            shapes,
            path: Some(path.to_path_buf()),
            saved_lineage,
            advisory_warnings: import_warnings,
            undo: Default::default(),
            undo_limit_reached: false,
            tool_edit_active: false,
            retired_sources: HashMap::new(),
            detached_projections: HashMap::new(),
        };
        // Files may be externally edited or reordered. Reject any stored projection with a forward source reference.
        for sketch in &sess.p.sketches {
            for projection in &sketch.projections {
                sess.check_projection_order(sketch.id, projection.body)?;
            }
        }
        // Clean documents remain usable even without named edges, but rebuilding with an empty edge pool
        // can silently round every edge of a stored query (F-024).
        if let Err(e) = sess.restore_edges() {
            if missing || sess.p.timeline.iter().any(|n| n.dirty) {
                return Err(Error::Invalid(format!("cannot open safely: {e}; the document requires a rebuild")));
            }
        }
        let before_evaluation = sess.p.clone_without_source_data();
        sess.p.eval_parameters();
        sess.check_fixed_text_geometry(&before_evaluation)?;
        sess.check_thread_dimensions().map_err(|(id, error)| Error::Invalid(format!("{} ({id}): {error}", sess.node_name(id))))?;
        let sketches: Vec<Id> = sess.p.sketches.iter().map(|s| s.id).collect();
        for sid in sketches {
            sess.track_datum_dependencies(sid);
        }
        if missing {
            sess.p.mark_all_dirty();
        }
        let stored: HashMap<Id, BodyMetrics> = sess.shapes.iter().map(|(&id, sh)| (id, BodyMetrics::of(sh))).collect();
        // Planned nodes are checked by the rebuild itself; the rest are checked below (F-074).
        let planned: HashSet<Id> = sess.p.regen_plan().nodes.into_iter().collect();
        let mut r = sess.rebuild();
        for (&id, old) in &stored {
            let Some(sh) = sess.shapes.get(&id) else { continue };
            let new = BodyMetrics::of(sh);
            let (volume, rebuilt) = (old.volume, new.volume);
            // The 0.05 mm comparison allowance applies to approximate bounds, not volume (F-066). Boolean volume
            // comparisons use numerical roundoff only, so interior changes with identical bboxes warn.
            let volume_tol = (volume.abs().max(rebuilt.abs()) * 1e-9).max(1e-6);
            let bounds = old.bbox.zip(new.bbox);
            let bbox_changed = bounds.is_some_and(|(old, new)| old.iter().zip(new).any(|(a, b)| (a - b).abs() > 0.05));
            let faces = old.faces.zip(new.faces);
            let faces_changed = faces.is_some_and(|(old, new)| old != new);
            let sealed = old.shells > old.solids;
            let changed = (volume - rebuilt).abs() > volume_tol
                || bbox_changed
                || old.shells != new.shells
                || old.solids != new.solids
                || faces_changed;
            let message = if changed {
                let mut message = format!("stored geometry differs from the rebuild: volume {volume:.4} → {rebuilt:.4} mm³");
                if old.shells != new.shells || sealed {
                    let void = if sealed { " (sealed void)" } else { "" };
                    message.push_str(&format!("; stored body had {} shells{void}, the rebuild has {}", old.shells, new.shells));
                }
                if old.solids != new.solids {
                    message.push_str(&format!("; solids {} → {}", old.solids, new.solids));
                }
                if let Some((old, new)) = faces.filter(|_| faces_changed) {
                    message.push_str(&format!("; faces {old} → {new}"));
                }
                if let Some((old, new)) = bounds.filter(|_| bbox_changed) {
                    let format_bounds =
                        |values: [f64; 6]| format!("[{}]", values.iter().map(|v| format!("{v:.4}")).collect::<Vec<_>>().join(", "));
                    message.push_str(&format!("; bbox {} → {} mm", format_bounds(old), format_bounds(new)));
                }
                message.push_str("; the rebuilt geometry is now used; save to update the file");
                Some(message)
            } else if sealed {
                Some(format!("stored body has {} shells and {} solids; may contain a sealed internal void", old.shells, old.solids))
            } else {
                None
            };
            if let Some(message) = message {
                let warning = sess.issue(id, message);
                sess.advisory_warnings.push(AdvisoryWarning::Geometry(warning.clone()));
                r.warnings.push(warning);
            }
        }
        // Fillets kept from the file were not rebuilt, so the rebuild did not check them: a file saved by the app
        // can hold overlapping blends (F-074).
        let checked: HashSet<Id> = r.errors.iter().map(|e| e.node).chain(sess.p.regen_errors.keys().copied()).chain(planned).collect();
        for (node, message) in sess.fillet_refusals(|n| !checked.contains(&n)) {
            sess.p.regen_errors.insert(node, qymcad_core::errors::CoreError::Kernel(message.clone()));
            r.errors.push(sess.issue(node, message));
        }
        Ok((sess, r))
    }

    /// Save to `path`, or to the path the document was opened from / last saved to.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()` (F-069).
    pub fn save(&mut self, path: Option<&Path>) -> Result<PathBuf> {
        self.save_with_options(path, false)
    }

    /// Save, explicitly allowing replacement of a body-containing file by an empty document when requested.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()` (F-069).
    pub fn save_with_options(&mut self, path: Option<&Path>, allow_empty: bool) -> Result<PathBuf> {
        self.save_with_overwrite(path, allow_empty, false)
    }

    /// Save with explicit permission to reuse the associated path for a different model.
    /// `allow_empty` independently controls replacing stored bodies with an empty document.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()` (F-069).
    pub fn save_with_overwrite(&mut self, path: Option<&Path>, allow_empty: bool, overwrite: bool) -> Result<PathBuf> {
        let target = match path {
            Some(p) => p.to_path_buf(),
            None => self.path.clone().ok_or_else(|| Error::Invalid("the document has no file yet; give a path".into()))?,
        };
        let s = target.to_str().ok_or_else(|| Error::Invalid(format!("path is not UTF-8: {}", target.display())))?;
        if !allow_empty
            && self.result_bodies().is_empty()
            && target.try_exists().map_err(|e| Error::Io(format!("cannot inspect {s}: {e}")))?
        {
            let stored = qymcad_io::load_project(s).map_err(|e| Error::Io(format!("cannot inspect existing document {s}: {e}")))?;
            if !stored.bodies.is_empty() {
                return Err(Error::Invalid(format!(
                    "document has no bodies; refusing to overwrite {s}, which has bodies; set allow_empty=true to replace it"
                )));
            }
        }
        // Keep the existing explicit empty-save permission: it intentionally replaces the model.
        if path.is_none() && !overwrite && !(allow_empty && self.result_bodies().is_empty()) {
            if let Some((id, kind)) = self.saved_lineage {
                if !self.p.timeline.iter().any(|n| n.id == id && std::mem::discriminant(&n.kind) == kind) {
                    return Err(Error::Invalid(format!(
                        "this is a different model from the one last loaded or saved at {s}; give a path or set overwrite=true to replace it"
                    )));
                }
            }
        }
        let breps: Vec<(Id, Vec<u8>)> = {
            let _gate = qymcad_kernel::kernel_gate();
            self.shapes.iter().filter_map(|(id, sh)| sh.to_brep_bytes().map(|b| (*id, b))).collect()
        };
        qymcad_io::save_project_guarded_with_brep(&self.p, s, &breps).map_err(|e| Error::Io(format!("cannot save {s}: {e}")))?;
        self.path = Some(target.clone());
        self.saved_lineage = first_body_feature(&self.p);
        Ok(target)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Read access to the underlying QymCAD document (tests, diagnostics).
    pub fn project(&self) -> &Project {
        &self.p
    }

    /// The live B-rep of a body, if built.
    pub fn shape(&self, body: Id) -> Option<&Shape> {
        self.shapes.get(&body)
    }

    /// Rebuild what is dirty. Errors are retried once with the whole timeline dirty: a datum plane created in
    /// the same edit is resolved only during regenerate (FINDINGS F-005), and `retryable()` errors need a pass
    /// after their source exists.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()` (F-069).
    pub fn rebuild(&mut self) -> Rebuild {
        self.rebuild_retrying(None)
    }

    /// Native projection resolution can release a whole record or only part of a face outline.
    pub(crate) fn projection_provenance(&self) -> HashMap<(Id, Id), DetachedProjection> {
        self.p
            .sketches
            .iter()
            .flat_map(|s| {
                s.projections.iter().flat_map(move |p| {
                    p.entities.iter().map(move |entity| ((s.id, *entity), DetachedProjection { id: p.id, body: p.body, source: p.src }))
                })
            })
            .collect()
    }

    pub(crate) fn remember_detached_projections(&mut self, projections_before: HashMap<(Id, Id), DetachedProjection>) {
        for (key, projection) in projections_before {
            if let Some(si) = self.p.sketch_index(key.0) {
                let sketch = &self.p.sketches[si];
                if sketch.entities.iter().any(|e| e.id == key.1) && !sketch.projections.iter().any(|p| p.entities.contains(&key.1)) {
                    self.detached_projections.insert(key, projection);
                }
            }
        }
        self.detached_projections.retain(|(sid, entity), _| {
            self.p.sketch_index(*sid).is_some_and(|si| {
                let sketch = &self.p.sketches[si];
                sketch.entities.iter().any(|e| e.id == *entity) && !sketch.projections.iter().any(|p| p.entities.contains(entity))
            })
        });
    }

    /// `rebuild`, with the retry pass limited to the nodes in `only` when given. An edit retries only the nodes
    /// it created: rebuilding the whole timeline would replace every old body's shape with a fresh rebuild, which
    /// need not be bit-identical — a refused edit must retain the exact old representation (F-034).
    /// F-005 still holds: a datum created by the edit is one of its nodes.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()` (F-069).
    pub(crate) fn rebuild_retrying(&mut self, only: Option<&HashSet<Id>>) -> Rebuild {
        if let Err((id, e)) = self.check_thread_dimensions() {
            return Rebuild { errors: vec![self.issue(id, e.to_string())], bodies: self.result_bodies(), ..Default::default() };
        }
        let blocked = self.edge_query_rebuild_errors(&self.p);
        if !blocked.is_empty() {
            return Rebuild { errors: blocked, bodies: self.result_bodies(), ..Default::default() };
        }
        // A safe first pass may fail on an unrelated node, expanding the retry into a blocked query.
        // Preserve original handles only in that case, so refusing the retry also undoes the first pass.
        let saved = if only.is_none() && !self.p.regen_plan().nodes.is_empty() {
            let mut retry = self.p.clone();
            retry.mark_all_dirty();
            if self.edge_query_rebuild_errors(&retry).is_empty() {
                None
            } else {
                let copies = copy_shapes(self.shapes.iter().map(|(&id, sh)| (id, sh)));
                match copies {
                    Ok(copies) => Some((self.p.clone(), std::mem::replace(&mut self.shapes, copies))),
                    Err(body) => {
                        return Rebuild {
                            errors: vec![self.issue(body, format!("cannot snapshot body {body} before a guarded rebuild"))],
                            bodies: self.result_bodies(),
                            ..Default::default()
                        }
                    }
                }
            }
        } else {
            None
        };
        let projections_before = self.projection_provenance();
        let split_identities = self.split_identities();
        let boolean_identities = self.boolean_identities();
        let before_counts: HashMap<Id, (u32, u32)> =
            self.shapes.iter().map(|(&id, sh)| (id, (sh.shell_count(), sh.solid_count()))).collect();
        let shapes = std::mem::take(&mut self.shapes);
        let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut self.p, shapes);
        self.shapes = shapes;

        let mut built = report.built;
        let mut errors: Vec<NodeIssue> = report.errors.iter().map(|(id, e)| self.issue(*id, crate::localization::error(e))).collect();
        if !errors.is_empty() {
            // A full retry can reach queries absent from the first plan. Check that plan before changing
            // dirty flags or handing shapes to the kernel; callers retain their existing rollback paths.
            let mut retry = self.p.clone();
            match only {
                None => retry.mark_all_dirty(),
                Some(nodes) => retry.timeline.iter_mut().filter(|n| nodes.contains(&n.id)).for_each(|n| n.dirty = true),
            }
            let blocked = self.edge_query_rebuild_errors(&retry);
            if blocked.is_empty() {
                self.p = retry;
                let shapes = std::mem::take(&mut self.shapes);
                let (again, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut self.p, shapes);
                self.shapes = shapes;
                built.extend(again.built);
                errors = again.errors.iter().map(|(id, e)| self.issue(*id, crate::localization::error(e))).collect();
            } else {
                errors.extend(blocked);
                if let Some((project, shapes)) = saved {
                    self.p = project;
                    self.shapes = shapes;
                    return Rebuild { errors, bodies: self.result_bodies(), ..Default::default() };
                }
            }
        }
        // The GUI copies faces into bodies; a headless regenerate does not (FINDINGS F-007).
        let rebuilt: HashSet<Id> = built.iter().map(|(id, _)| *id).collect();
        for (id, faces) in built {
            self.p.set_body_faces(id, faces);
        }
        // Import source warnings outlive retessellation: a live B-rep cannot repair an unavailable original.
        // Geometry lost on load is a fact of the document while its sketch exists, timeline node or not.
        // Other diagnostics last until their node rebuilds or disappears; inherited inner shells do not warn again.
        let live: HashSet<Id> = self.p.timeline.iter().map(|n| n.id).collect();
        let sketches: HashSet<Id> = self.p.sketches.iter().map(|s| s.id).collect();
        self.advisory_warnings.retain(|w| {
            matches!(w, AdvisoryWarning::LostOnLoad(issue) if sketches.contains(&issue.node))
                || live.contains(&w.issue().node) && (!rebuilt.contains(&w.issue().node) || matches!(w, AdvisoryWarning::ImportSource(_)))
        });
        for node in self.p.timeline.iter().filter(|n| rebuilt.contains(&n.id)) {
            if errors.iter().any(|e| e.node == node.id) {
                continue;
            }
            for body in node.kind.bodies() {
                let Some(shape) = self.shapes.get(&body) else { continue };
                let shells = shape.shell_count();
                let solids = shape.solid_count();
                let source =
                    node.kind.consumed_body().and_then(|id| self.shapes.get(&id)).map_or((1, 1), |sh| (sh.shell_count(), sh.solid_count()));
                let old = before_counts.get(&body).copied().unwrap_or(source);
                // Each disconnected solid contributes one outer shell; only excess shells indicate cavities.
                let excess = shells.saturating_sub(solids);
                if excess > old.0.saturating_sub(old.1) || excess > source.0.saturating_sub(source.1) {
                    self.advisory_warnings.push(AdvisoryWarning::Geometry(self.issue(
                        node.id,
                        format!("result body has {shells} shells and {solids} solids; may contain a sealed internal void"),
                    )));
                }
            }
        }
        let refused = self.fillet_refusals(|n| rebuilt.contains(&n) && !errors.iter().any(|e| e.node == n));
        for (node, message) in refused {
            // Kept like a native failure (F-074), in the check's own words: doc_info and export see it, and the
            // node rebuilds next time.
            self.p.regen_errors.insert(node, qymcad_core::errors::CoreError::Kernel(message.clone()));
            errors.push(self.issue(node, message));
        }
        for (node, message) in self.split_identity_refusals(&split_identities, &rebuilt) {
            self.p.regen_errors.insert(node, qymcad_core::errors::CoreError::Kernel(message.clone()));
            errors.push(self.issue(node, message));
        }
        for (node, message) in self.boolean_identity_refusals(&boolean_identities, &rebuilt) {
            self.p.regen_errors.insert(node, qymcad_core::errors::CoreError::Kernel(message.clone()));
            errors.push(self.issue(node, message));
        }
        for issue in self.check_thread_stock() {
            if !errors.iter().any(|e| e.node == issue.node) {
                self.p.regen_errors.insert(issue.node, qymcad_core::errors::CoreError::Kernel(issue.message.clone()));
                errors.push(issue);
            }
        }
        self.remember_detached_projections(projections_before);
        let mut warnings: Vec<NodeIssue> =
            self.p.regen_warnings.iter().map(|(id, e)| self.issue(*id, crate::localization::error(e))).collect();
        warnings.extend(self.advisory_warnings.iter().map(|w| w.issue().clone()));
        warnings.extend(self.cross_part_projection_warnings());
        let bodies = self.result_bodies();
        warnings.extend(bodies.iter().filter_map(|b| self.ambiguous_face_warning(b.id).map(|message| self.issue(b.id, message))));
        Rebuild { errors, warnings, bodies, ..Default::default() }
    }

    /// Fillet nodes picked by `check` whose built result fails `modifiers::fillet_acceptance` (F-074), with the reason.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()`.
    fn fillet_refusals(&self, check: impl Fn(Id) -> bool) -> Vec<(Id, String)> {
        self.p
            .timeline
            .iter()
            .filter(|n| !n.suppressed && check(n.id))
            .filter_map(|n| match n.kind {
                qymcad_core::feature::FeatureKind::Fillet { src, radius, body, .. } => {
                    // The node keeps the number typed last; a formula's value is what was built.
                    let radius = self.p.feat_dim_value(n.id, "radius", radius, &self.p.param_map());
                    let result = self.shapes.get(&body)?;
                    let message = match self.shapes.get(&src) {
                        Some(source) => crate::modifiers::fillet_acceptance(&self.p, n.id, source, result, radius)?,
                        None => format!("fillet R{radius:.2}: its source body is not built, so the result cannot be checked"),
                    };
                    Some((n.id, message))
                }
                _ => None,
            })
            .collect()
    }

    /// Reject only planned stored edge queries, including transitive dependents of an unrestorable body.
    /// Pick lists use live kernel edges; queries require the restored pool (F-023/F-024).
    /// The engine never creates Patch features. If Patch is added, include its boundary edge queries in
    /// this guard before exposing the feature; checking only Fillet/Chamfer would leave them unrestorable.
    fn edge_query_rebuild_errors(&self, p: &Project) -> Vec<NodeIssue> {
        use qymcad_core::feature::FeatureKind;
        let planned = crate::dependencies::rebuild_nodes(p);
        let queries: Vec<Id> = p
            .timeline
            .iter()
            .take(p.rollback.unwrap_or(usize::MAX))
            .filter(|n| !n.suppressed && planned.contains(&n.id))
            .filter(|n| matches!(&n.kind, FeatureKind::Fillet { edges, .. } | FeatureKind::Chamfer { edges, .. } if !edges.query.is_pick_list()))
            .map(|n| n.id)
            .collect();
        if queries.is_empty() {
            return Vec::new();
        }
        let _gate = qymcad_kernel::kernel_gate();
        let mut errors = Vec::new();
        for (&body, shape) in &self.shapes {
            if p.regen_edges.get(&body).is_some_and(|edges| !edges.is_empty()) {
                continue;
            }
            let edges = shape.edges_info();
            // Match Kernel::edges' usable-name filter exactly (kernel.rs:386-399).
            if edges.is_empty() || edges.iter().any(|e| e.id != 0 && e.poly.len() >= 2) {
                continue;
            }
            let dependents = crate::dependencies::closure(p, HashSet::from([body]));
            for &node in queries.iter().filter(|node| dependents.contains(node)) {
                errors.push(self.issue(
                    node,
                    format!(
                        "body {body} `{}` has live edges but no usable named edges; refusing a stored edge query rebuild (F-024)",
                        self.node_name(body)
                    ),
                ));
            }
        }
        errors
    }

    /// Create nodes in their input's part, as the app does after entering that part. Restore the user's
    /// active context before returning; explicit imported-body edits must not redirect later implicit edits.
    pub(crate) fn in_component<T>(&mut self, owner: Option<Id>, edit: impl FnOnce(&mut Project) -> T) -> T {
        let saved = self.p.active_component;
        self.p.set_active_component(owner);
        let value = edit(&mut self.p);
        self.p.set_active_component(saved);
        value
    }

    /// Run `edit`, rebuild, and keep the result only if the nodes it created built cleanly. Otherwise restore
    /// the document as it was and return the errors. Pending edits are rebuilt before the snapshot so the
    /// project and its live shapes describe the same baseline (F-034). Returns the edit's value and report.
    pub(crate) fn atomic<T>(&mut self, edit: impl FnOnce(&mut Session) -> Result<T>) -> Result<(T, Rebuild)> {
        self.atomic_with_post(edit, |_, value, _| Ok(value))
    }

    pub(crate) fn atomic_with_post<T, U>(
        &mut self,
        edit: impl FnOnce(&mut Session) -> Result<T>,
        post: impl FnOnce(&mut Session, T, &mut Rebuild) -> Result<U>,
    ) -> Result<(U, Rebuild)> {
        if self.p.timeline.iter().any(|n| n.dirty) {
            let baseline = self.rebuild();
            if !baseline.errors.is_empty() {
                return Err(Error::Rebuild(
                    baseline.errors.iter().map(|i| format!("baseline: {} ({}): {}", i.name, i.node, i.message)).collect(),
                ));
            }
        }
        let before = self.p.clone();
        let before_warnings = self.advisory_warnings.clone();
        let old_nodes: HashSet<Id> = self.p.timeline.iter().map(|n| n.id).collect();
        let value = match edit(self) {
            Ok(v) => v,
            Err(e) => {
                self.p = before;
                return Err(e);
            }
        };
        let new_nodes: HashSet<Id> = self.p.timeline.iter().map(|n| n.id).filter(|id| !old_nodes.contains(id)).collect();
        let mut rebuild = self.rebuild_retrying(Some(&new_nodes));
        let new_errors: Vec<String> = rebuild
            .errors
            .iter()
            .filter(|i| !old_nodes.contains(&i.node))
            .map(|i| format!("{} ({}): {}", i.name, i.node, i.message))
            .collect();
        if !new_errors.is_empty() {
            self.p = before;
            self.advisory_warnings = before_warnings;
            let live: HashSet<Id> = self.p.timeline.iter().flat_map(|n| n.kind.bodies()).collect();
            self.shapes.retain(|id, _| live.contains(id));
            return Err(Error::Rebuild(new_errors));
        }
        match post(self, value, &mut rebuild) {
            Ok(value) => Ok((value, rebuild)),
            Err(error) => {
                self.p = before;
                self.advisory_warnings = before_warnings;
                let live: HashSet<Id> = self.p.timeline.iter().flat_map(|n| n.kind.bodies()).collect();
                self.shapes.retain(|id, _| live.contains(id));
                Err(error)
            }
        }
    }

    /// Rebuild edits on independent copies of planned live shapes, restoring the original project, handles
    /// and diagnostics on failure (F-034). Callers retain their own retry plan and existing-error policy.
    /// This deliberately strengthens the old inline paths: any edit error restores advisory warnings
    /// (the former early sketch error did not), and drops shapes whose bodies are absent from the restored
    /// recipe (the former parameter failure and early sketch error retained them). Neither caller normally
    /// creates bodies before failing, but a closure must not leak orphan geometry or diagnostics. A planned
    /// body with no pre-edit shape must also remain unbuilt after rollback.
    pub(crate) fn with_rebuild_copies<T>(
        &mut self,
        before: Project,
        nodes: &HashSet<Id>,
        context: &str,
        edit: impl FnOnce(&mut Session) -> Result<T>,
    ) -> Result<T> {
        if let Err(error) = self.check_fixed_text_geometry(&before) {
            self.p = before;
            return Err(error);
        }
        let split_identities = self.split_identities();
        let boolean_identities = self.boolean_identities();
        let before_warnings = self.advisory_warnings.clone();
        let before_detached = self.detached_projections.clone();
        let before_shape_ids: HashSet<Id> = self.shapes.keys().copied().collect();
        let copies = copy_shapes(
            self.p
                .timeline
                .iter()
                .filter(|n| nodes.contains(&n.id))
                .flat_map(|n| n.kind.bodies())
                .filter_map(|id| self.shapes.get(&id).map(|sh| (id, sh))),
        )
        .map_err(|id| Error::Io(format!("cannot snapshot body {id} before {context}")));
        let mut saved = match copies {
            Ok(copies) => copies,
            Err(e) => {
                self.p = before;
                return Err(e);
            }
        };
        for (id, copy) in &mut saved {
            if let Some(original) = self.shapes.get_mut(id) {
                std::mem::swap(original, copy);
            }
        }
        let result = edit(self).and_then(|value| {
            self.check_fixed_text_geometry(&before)?;
            // Callers allow unrelated native errors that existed before an edit. That exemption must
            // never hide a new safety refusal on the same node (e.g. a previously overlong thread).
            if let Err((id, error)) = self.check_thread_dimensions() {
                return Err(Error::Rebuild(vec![format!("{} ({id}): {error}", self.node_name(id))]));
            }
            let mut refused = self.check_thread_stock();
            refused.extend(
                self.split_identity_refusals(&split_identities, nodes).into_iter().map(|(node, message)| self.issue(node, message)),
            );
            refused.extend(
                self.boolean_identity_refusals(&boolean_identities, nodes).into_iter().map(|(node, message)| self.issue(node, message)),
            );
            if !refused.is_empty() {
                return Err(Error::Rebuild(refused.iter().map(|i| format!("{} ({}): {}", i.name, i.node, i.message)).collect()));
            }
            Ok(value)
        });
        if result.is_err() {
            self.p = before;
            self.advisory_warnings = before_warnings;
            self.detached_projections = before_detached;
            let live: HashSet<Id> = self.p.timeline.iter().flat_map(|n| n.kind.bodies()).collect();
            self.shapes.retain(|id, _| live.contains(id) && before_shape_ids.contains(id));
            self.shapes.extend(saved);
        }
        result
    }

    /// Run edits that do not need a rebuild (sketch geometry) as one unit: on error the document is restored.
    pub fn transaction<T>(&mut self, edit: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
        let before = self.p.clone();
        let r = edit(self).and_then(|value| {
            self.check_fixed_text_geometry(&before)?;
            Ok(value)
        });
        if r.is_err() {
            self.p = before;
        }
        r
    }

    /// Bodies that are current results: built, and not consumed by a later feature.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()`;
    /// bbox reporting acquires that non-reentrant mutex.
    pub fn result_bodies(&self) -> Vec<BodyInfo> {
        let consumed = self.p.consumed_bodies();
        self.p
            .timeline
            .iter()
            .filter(|n| !n.suppressed)
            .flat_map(|n| n.kind.bodies())
            .filter(|b| !consumed.contains(b))
            .filter_map(|b| {
                let sh = self.shapes.get(&b)?;
                Some(BodyInfo { id: b, name: self.body_name(b), volume: sh.volume(), bbox: reporting_bbox(sh).unwrap_or([0.0; 6]) })
            })
            .collect()
    }

    /// The single current body of the active part, which features modify by default.
    pub(crate) fn tip_body(&self) -> Option<Id> {
        let consumed = self.p.consumed_bodies();
        let part = self.p.active_component;
        let tips: Vec<Id> = self
            .p
            .timeline
            .iter()
            .filter(|n| !n.suppressed)
            .flat_map(|n| n.kind.bodies())
            .filter(|b| !consumed.contains(b) && (part.is_none() || self.p.body_owner(*b) == part))
            .collect();
        tips.last().copied()
    }

    /// The body a tool uses when none is given: the active part's current body, else the document's only current body
    /// (an import lives in its own part). Several bodies elsewhere are listed so the caller can pick one.
    pub(crate) fn default_body(&self, empty: &str) -> Result<Id> {
        if let Some(b) = self.tip_body() {
            return Ok(b);
        }
        let consumed = self.p.consumed_bodies();
        let mut current: Vec<Id> =
            self.p.timeline.iter().filter(|n| !n.suppressed).flat_map(|n| n.kind.bodies()).filter(|b| !consumed.contains(b)).collect();
        current.dedup();
        match current.as_slice() {
            [] => Err(Error::Invalid(empty.into())),
            [only] => Ok(*only),
            many => {
                let names: Vec<String> = many.iter().map(|b| format!("{} ({b})", self.body_name(*b))).collect();
                Err(Error::Invalid(format!("the active part has no body; pass body: one of {}", names.join(", "))))
            }
        }
    }

    /// Localized timeline node name for creation results and diagnostics.
    pub fn node_name(&self, id: Id) -> String {
        self.p.timeline.iter().find(|n| n.id == id).map(|n| crate::localization::name(&n.name)).unwrap_or_default()
    }

    /// Current bodies and regeneration diagnostics, including live topology naming warnings, without
    /// another rebuild. The current thread must not hold the native kernel gate (metrics acquire it).
    pub fn current_rebuild(&self) -> Rebuild {
        let errors = self.p.regen_errors.iter().map(|(id, e)| self.issue(*id, crate::localization::error(e))).collect();
        let mut warnings: Vec<_> = self.p.regen_warnings.iter().map(|(id, e)| self.issue(*id, crate::localization::error(e))).collect();
        warnings.extend(self.advisory_warnings.iter().map(|w| w.issue().clone()));
        warnings.extend(self.cross_part_projection_warnings());
        let bodies = self.result_bodies();
        warnings.extend(bodies.iter().filter_map(|b| self.ambiguous_face_warning(b.id).map(|message| self.issue(b.id, message))));
        Rebuild { errors, warnings, bodies, ..Default::default() }
    }

    /// Empty optional object names request the native/default name, including Unicode whitespace.
    pub(crate) fn optional_object_name(name: Option<&str>) -> Option<&str> {
        name.filter(|name| !name.trim().is_empty())
    }

    /// Keep object aliases addressable by `resolve`, which trims requests and reserves numeric ids.
    pub(crate) fn check_object_name(name: &str) -> Result<()> {
        if name.trim().parse::<Id>().is_ok() {
            return Err(Error::Invalid(format!("object name `{name}` is interpreted as an id; choose a nonnumeric name")));
        }
        // Optional names pass through optional_object_name before storage.
        if !name.trim().is_empty() && name != name.trim() {
            return Err(Error::Invalid(format!("object name `{name}` must not have leading or trailing whitespace")));
        }
        Ok(())
    }

    /// Derived from current records so rebuilds retain the warning and deletion cannot leave stale diagnostics.
    pub(crate) fn cross_part_projection_warnings(&self) -> Vec<NodeIssue> {
        let part = |owner: Option<Id>| match owner {
            Some(id) => {
                let name = self.p.components.iter().find(|c| c.id == id).map(|c| crate::localization::name(&c.name)).unwrap_or_default();
                format!("{name} ({id})")
            }
            None => "unowned part (none)".into(),
        };
        let mut warnings = Vec::new();
        for sketch in &self.p.sketches {
            for p in sketch.projections.iter().filter(|p| self.projection_cross_part(sketch.id, p.body)) {
                let name = crate::localization::name(&sketch.name);
                warnings.push(NodeIssue {
                    node: sketch.id,
                    name: name.clone(),
                    message: format!(
                        "projection {}: sketch {name} ({}) in part {}, body {} ({}) in part {} is projected in the source part's local coordinates; its placement is ignored, as in the QymCAD app",
                        p.id, sketch.id, part(self.p.sketch_owner(sketch.id)), self.body_name(p.body), p.body, part(self.p.body_owner(p.body)),
                    ),
                });
            }
        }
        warnings
    }

    pub(crate) fn set_node_name(&mut self, id: Id, name: Option<&str>) -> Result<()> {
        if let Some(name) = Self::optional_object_name(name) {
            Self::check_object_name(name)?;
            if let Some(n) = self.p.timeline.iter_mut().find(|n| n.id == id) {
                n.name = name.to_string();
            }
        }
        Ok(())
    }

    /// Reserve explicit piece aliases first so blank entries can choose unused default suffixes.
    pub(crate) fn piece_names(&self, feature: Id, count: usize, requested: Option<&[String]>) -> Result<Vec<String>> {
        if requested.is_some_and(|names| names.len() != count) {
            return Err(Error::Invalid(format!("piece_names needs exactly {count} names for the result")));
        }
        let mut reserved = HashSet::new();
        for name in requested.into_iter().flatten().filter_map(|name| Self::optional_object_name(Some(name))) {
            Self::check_object_name(name)?;
            if !reserved.insert(name.to_string()) {
                return Err(Error::Invalid("piece_names must be distinct names without surrounding whitespace".into()));
            }
            if !matches!(self.resolve(name), Err(Error::NotFound(_))) {
                return Err(Error::Invalid(format!("piece name `{name}` already names an object; choose distinct piece_names")));
            }
        }
        let prefix = self.node_name(feature);
        let mut next = 1;
        (0..count)
            .map(|index| {
                if let Some(name) = Self::optional_object_name(requested.map(|names| names[index].as_str())) {
                    return Ok(name.to_string());
                }
                loop {
                    let name = format!("{prefix} {next}");
                    next += 1;
                    // Ambiguous aliases and explicit names later in this list are occupied too.
                    if !reserved.contains(&name) && matches!(self.resolve(&name), Err(Error::NotFound(_))) {
                        Self::check_object_name(&name)?;
                        reserved.insert(name.clone());
                        break Ok(name);
                    }
                }
            })
            .collect()
    }

    /// Display name of a body. Split and all boolean outputs use native piece names; ordinary features keep their names.
    pub fn body_name(&self, id: Id) -> String {
        let native_component_name =
            self.p
                .body_owner(id)
                .and_then(|owner| self.p.components.iter().find(|component| component.id == owner))
                .filter(|_| {
                    self.p.timeline.iter().any(|node| {
                        matches!(&node.kind, qymcad_core::feature::FeatureKind::Import { .. }) && node.kind.bodies().contains(&id)
                    })
                })
                .map(|component| component.name.clone());
        if let Some(name) = native_component_name {
            return crate::localization::name(&name);
        }
        let named_outputs = self.p.timeline.iter().any(|n| {
            matches!(n.kind, qymcad_core::feature::FeatureKind::SplitBody { .. } | qymcad_core::feature::FeatureKind::BodyBoolean { .. })
                && n.kind.bodies().contains(&id)
        });
        if named_outputs {
            if let Some(mi) = self.p.mesh_index(id) {
                return self.p.mesh_name(mi);
            }
        }
        self.node_name(id)
    }

    /// Resolve a timeline/sketch id or name, or any output-body id/native/imported-part name. Current body aliases take precedence
    /// over inherited historical body names; different live objects sharing an alias require an explicit id.
    pub fn resolve(&self, r: &str) -> Result<Id> {
        let r = r.trim();
        let bodies: HashSet<Id> = self.p.timeline.iter().flat_map(|n| n.kind.bodies()).collect();
        if let Ok(id) = r.parse::<Id>() {
            if self.p.timeline.iter().any(|n| n.id == id) || self.p.sketches.iter().any(|s| s.id == id) || bodies.contains(&id) {
                return Ok(id);
            }
            return Err(Error::NotFound(format!("no object with id {id}")));
        }
        let consumed = self.p.consumed_bodies();
        let mut native: Vec<Id> = self.p.bodies.iter().filter(|b| bodies.contains(&b.id) && b.name == r).map(|b| b.id).collect();
        let imported_aliases: Vec<Id> = self
            .p
            .timeline
            .iter()
            .filter(|node| matches!(node.kind, qymcad_core::feature::FeatureKind::Import { .. }))
            .flat_map(|node| node.kind.bodies())
            .filter(|body| self.body_name(*body) == r)
            .collect();
        // Import part aliases remain below native current-body names and ahead of historical aliases.
        native.extend(imported_aliases);
        let current: Vec<Id> = native.iter().copied().filter(|id| !consumed.contains(id)).collect();
        let mut hits: Vec<Id> =
            self.p.timeline.iter().filter(|n| n.name == r || crate::localization::name(&n.name) == r).map(|n| n.id).collect();
        hits.extend(if current.is_empty() { native } else { current });
        hits.sort_unstable();
        hits.dedup();
        match hits.as_slice() {
            [id] => Ok(*id),
            [] => Err(Error::NotFound(format!("no object named `{r}`"))),
            _ => Err(Error::Invalid(format!("several objects are named `{r}` ({hits:?}); use the id"))),
        }
    }

    fn issue(&self, node: Id, message: String) -> NodeIssue {
        NodeIssue { node, name: self.node_name(node), message }
    }
}

/// Like QymCAD.app's `finish_project_load`: the B-rep faces stored in the bodies go back into `regen_faces`, so
/// face references resolve by id without a rebuild. Edges are NOT restored by the app either (F-023).
impl Session {
    /// Fill `regen_edges` for live bodies that lack them, from their B-reps, through the same `Kernel::edges` the
    /// regenerate post pass uses (F-023). Without it a stored edge query resolves against an empty pool and rounds
    /// every edge (F-024), on the first rebuild after opening.
    pub(crate) fn restore_edges(&mut self) -> Result<()> {
        use qymcad_core::feature::Kernel;
        let need: Vec<Id> = self.shapes.keys().copied().filter(|b| !self.p.regen_edges.contains_key(b)).collect();
        if need.is_empty() {
            return Ok(());
        }
        let _gate = qymcad_kernel::kernel_gate();
        let kernel = qymcad_kernel::OcctKernel { shapes: std::cell::RefCell::new(std::mem::take(&mut self.shapes)), ..Default::default() };
        let mut error = None;
        for b in need {
            let edges = kernel.edges(b);
            if edges.is_empty() && kernel.shapes.borrow().get(&b).is_some_and(|sh| !sh.edges_info().is_empty()) {
                error = Some(Error::Invalid(format!(
                    "body {b} has live edges but no usable named edges; refusing a full topology rebuild (F-024)"
                )));
                // Keep restoring other bodies: an unrelated named query must still receive its real pool.
                continue;
            }
            if !edges.is_empty() {
                self.p.regen_edges.insert(b, edges);
            }
        }
        self.shapes = kernel.shapes.into_inner();
        match error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

pub(crate) fn restore_faces(p: &mut Project) {
    for i in 0..p.bodies.len() {
        if let (Some(body), false) = (p.mesh_id(i), p.bodies[i].faces.is_empty()) {
            let faces = p.bodies[i].faces.clone();
            p.regen_faces.insert(body, faces);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support as native;

    #[test]
    fn missing_live_projection_edge_is_reported_detached_in_session_detail() {
        use crate::{BaseName, Direction, Extrude, Op, PlaneRef};
        use qymcad_core::model::ProjSource;

        let mut session = Session::new_part();
        let profile = session.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        session.sketch_rect(profile, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
        let hole = session.sketch_circle(profile, &0.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
        let (body, _) = session
            .extrude(&Extrude {
                sketch: profile,
                profiles: None,
                height: 5.0.into(),
                op: Op::NewBody,
                direction: Direction::Normal,
                through: false,
                target: None,
                name: None,
            })
            .unwrap();
        let sketch = session.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let edge = session.topology(Some(body), false).unwrap().edges.iter().find(|e| e.kind == crate::EdgeKind::Circle).unwrap().id;
        let (projection, _) = session.sketch_project(sketch, body, &[ProjSource::Edge(edge)]).unwrap();
        assert_eq!(projection.entities.len(), 1);
        let before = session.sketch_detail(sketch).unwrap();
        // A failed edit after native detachment must restore the inspection metadata too.
        let before_project = serde_json::to_vec(&session.p).unwrap();
        let before_bytes = {
            let _gate = qymcad_kernel::kernel_gate();
            session.shapes[&body].to_brep_bytes().unwrap()
        };
        let result: Result<()> = session.with_rebuild_copies(session.p.clone(), &HashSet::from([body]), "test", |s| {
            s.sketch_edit(profile, |s| s.sketch_remove_entity(profile, hole))?;
            assert_eq!(s.sketch_detail(sketch)?.entities[0].projection_state, Some("detached"));
            Err(Error::Invalid("forced failure after detachment".into()))
        });
        assert!(result.is_err());
        assert_eq!(serde_json::to_vec(&session.p).unwrap(), before_project);
        {
            let _gate = qymcad_kernel::kernel_gate();
            assert_eq!(session.shapes[&body].to_brep_bytes().unwrap(), before_bytes);
        }
        assert_eq!(session.sketch_detail(sketch).unwrap(), before);
        assert!(session.detached_projections.is_empty());
        let mut snapshot = session.begin_tool_edit().unwrap();
        snapshot.record_call("sketch_remove", serde_json::json!({"entity":hole}));
        session.sketch_edit(profile, |s| s.sketch_remove_entity(profile, hole)).unwrap();
        session.finish_tool_edit(snapshot, true);
        assert!(!session.p.regen_edges[&body].iter().any(|e| e.id == edge), "hole edge really disappeared");
        assert!(session.p.sketches[session.sketch_si(sketch).unwrap()].projections.is_empty());
        let detail = session.sketch_detail(sketch).unwrap();
        assert_eq!(detail.entities.len(), 1);
        let entity = &detail.entities[0];
        assert_eq!(entity.id, projection.entities[0]);
        assert!(!entity.driven);
        assert_eq!(entity.projection_state, Some("detached"));
        assert_eq!(entity.projection, Some(projection.projections[0]));
        assert_eq!(entity.projection_source, Some(format!("edge:{edge}")));
        assert_eq!(entity.geometry, before.entities[0].geometry);
        assert_eq!(detail.constraints, before.constraints);
        session.rebuild();
        assert_eq!(session.sketch_detail(sketch).unwrap().entities, detail.entities);
        // Undo must also restore provenance that was pruned after deleting detached geometry.
        let mut removed = session.begin_tool_edit().unwrap();
        removed.record_call("sketch_remove", serde_json::json!({"entity":entity.id}));
        session.sketch_remove_entity(sketch, entity.id).unwrap();
        assert!(session.rebuild().errors.is_empty());
        session.finish_tool_edit(removed, true);
        assert!(session.detached_projections.is_empty());
        session.undo().unwrap();
        assert_eq!(session.sketch_detail(sketch).unwrap().entities, detail.entities);
        session.undo().unwrap();
        assert_eq!(session.sketch_detail(sketch).unwrap(), before);
    }

    #[test]
    fn unbuilt_projection_source_is_unknown_without_marking_ordinary_geometry() {
        use crate::{BaseName, Direction, Extrude, Op, PlaneRef};
        use qymcad_core::model::ProjSource;
        let mut session = Session::new_part();
        let profile = session.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        session.sketch_rect(profile, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
        let (body, _) = session
            .extrude(&Extrude {
                sketch: profile,
                profiles: None,
                height: 5.0.into(),
                op: Op::NewBody,
                direction: Direction::Normal,
                through: false,
                target: None,
                name: None,
            })
            .unwrap();
        let sketch = session.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let edge = session.p.regen_edges[&body].iter().find(|e| e.a[2] == e.b[2]).unwrap().id;
        let (projection, _) = session.sketch_project(sketch, body, &[ProjSource::Edge(edge)]).unwrap();
        let ordinary = session.sketch_circle(sketch, &50.0.into(), &0.0.into(), &2.0.into(), true).unwrap();
        session.shapes.remove(&body);
        // Clean source with no live B-rep: native projection resolver retains its association.
        let report = session.rebuild();
        assert!(report.errors.is_empty(), "{report:?}");
        let detail = session.sketch_detail(sketch).unwrap();
        let entities: Vec<_> = detail.entities.iter().filter(|e| projection.entities.contains(&e.id)).collect();
        assert_eq!(entities.len(), 1);
        assert!(entities.iter().all(|e| e.driven && e.projection_state == Some("unknown")));
        let ordinary = detail.entities.iter().find(|e| e.id == ordinary).unwrap();
        assert!(!ordinary.driven);
        assert_eq!(ordinary.projection_state, None);
        assert_eq!(ordinary.projection_body, None);
        assert_eq!(ordinary.geometry, None);
    }

    fn projection_fixture() -> (Session, Id, Id, u32) {
        use crate::{BaseName, Direction, Extrude, Op, PlaneRef};
        let mut s = Session::new_part();
        let profile = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(profile, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
        let (body, _) = s
            .extrude(&Extrude {
                sketch: profile,
                profiles: None,
                height: 10.0.into(),
                op: Op::NewBody,
                direction: Direction::Normal,
                through: false,
                target: None,
                name: None,
            })
            .unwrap();
        let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let edge = s.p.regen_edges[&body].iter().find(|e| e.a[2] == e.b[2]).unwrap().id;
        (s, body, sketch, edge)
    }

    fn assert_projection_refused(s: &mut Session, sketch: Id, body: Id, sources: &[qymcad_core::model::ProjSource], message: &str) {
        let before = serde_json::to_vec(&s.p).unwrap();
        let shapes = {
            let _gate = qymcad_kernel::kernel_gate();
            s.shapes.iter().map(|(id, shape)| (*id, shape.to_brep_bytes().unwrap())).collect::<HashMap<_, _>>()
        };
        let detail: Vec<_> = s.p.sketches.iter().map(|sk| s.sketch_detail(sk.id).unwrap()).collect();
        let error = s.sketch_project(sketch, body, sources).unwrap_err();
        assert!(error.to_string().contains(message), "{message}: {error}");
        assert_eq!(serde_json::to_vec(&s.p).unwrap(), before, "refusal changed Project");
        {
            let _gate = qymcad_kernel::kernel_gate();
            assert_eq!(s.shapes.len(), shapes.len());
            for (id, bytes) in shapes {
                assert_eq!(s.shapes[&id].to_brep_bytes().unwrap(), bytes, "body {id}");
            }
        }
        assert_eq!(s.p.sketches.iter().map(|sk| s.sketch_detail(sk.id).unwrap()).collect::<Vec<_>>(), detail);
        assert!(s.undo().is_err(), "refusal created a history entry");
    }

    #[test]
    fn projection_validation_empty_geometry_and_solve_refuse_atomically() {
        use qymcad_core::model::{Constraint, ProjSource};
        let (mut s, body, sketch, edge) = projection_fixture();
        assert_projection_refused(&mut s, u64::MAX, body, &[ProjSource::Edge(edge)], "sketch");
        assert_projection_refused(&mut s, sketch, u64::MAX, &[ProjSource::Edge(edge)], "body");
        assert_projection_refused(&mut s, sketch, body, &[], "nothing");
        assert_projection_refused(&mut s, sketch, body, &[ProjSource::Edge(edge), ProjSource::Edge(u32::MAX)], "edge");
        let duplicate = s.p.regen_edges[&body].iter().find(|e| e.id == edge).unwrap().clone();
        s.p.regen_edges.get_mut(&body).unwrap().push(duplicate);
        assert_projection_refused(&mut s, sketch, body, &[ProjSource::Edge(edge)], "edge");
        s.p.regen_edges.get_mut(&body).unwrap().pop();
        // Cache claims an edge exists, but the actual named B-rep does not contain it.
        let mut fake = s.p.regen_edges[&body][0].clone();
        fake.id = u32::MAX;
        s.p.regen_edges.get_mut(&body).unwrap().push(fake);
        assert_projection_refused(&mut s, sketch, body, &[ProjSource::Edge(edge), ProjSource::Edge(u32::MAX)], "geometry");
        s.p.regen_edges.get_mut(&body).unwrap().pop();
        let shape = s.shapes.remove(&body).unwrap();
        assert_projection_refused(&mut s, sketch, body, &[ProjSource::Edge(edge)], "built");
        s.shapes.insert(body, shape);
        let circle = s.sketch_circle(sketch, &50.0.into(), &0.0.into(), &2.0.into(), true).unwrap();
        let si = s.sketch_si(sketch).unwrap();
        let center = s.p.sketches[si].entities.iter().find(|e| e.id == circle).unwrap().kind.as_circle().unwrap().center;
        s.p.sketches[si].constraints.push(Constraint::Diameter {
            c: center,
            d: 7.0,
            off: 0.0,
            expr: String::new(),
            driven: false,
            diam: true,
            at: None,
        });
        assert_projection_refused(&mut s, sketch, body, &[ProjSource::Edge(edge)], "solve");
    }

    #[test]
    fn face_projection_requires_unique_host_face_and_uses_live_host_body() {
        use crate::{PlaneRef, Sel};
        use qymcad_core::model::ProjSource;
        let (mut s, body, sketch, _) = projection_fixture();
        let top = s.p.regen_faces[&body].iter().find(|f| f.normal[2] > 0.99).unwrap().id;
        assert_projection_refused(&mut s, sketch, body, &[ProjSource::Face(top)], "hosted");
        let hosted = s.sketch_create(&PlaneRef::Face { body, face: top }, None).unwrap();
        let duplicate = s.p.regen_faces[&body].iter().find(|f| f.id == top).unwrap().clone();
        s.p.regen_faces.get_mut(&body).unwrap().push(duplicate);
        assert_projection_refused(&mut s, hosted, body, &[ProjSource::Face(top)], "ambiguous");
        s.p.regen_faces.get_mut(&body).unwrap().pop();
        assert_projection_refused(&mut s, hosted, body, &[ProjSource::Face(u32::MAX)], "hosted");
        let (live, _) = s.push_face(Some(body), &Sel::Ids(vec![top]), &1.0.into(), None).unwrap();
        // Put the consumer above the sketch, as when editing an earlier timeline feature in the app.
        let node = s.p.timeline.pop().unwrap();
        let at = s.p.timeline.iter().position(|n| n.id == hosted).unwrap();
        s.p.timeline.insert(at, node);
        let top = s.p.regen_faces[&live].iter().find(|f| f.normal[2] > 0.99).unwrap().id;
        let si = s.sketch_si(hosted).unwrap();
        if let qymcad_core::feature::SketchPlane::Face(_, mut key) = s.p.sketches[si].plane {
            key.id = top;
            s.p.sketches[si].plane = qymcad_core::feature::SketchPlane::Face(body, key);
        } else {
            panic!("face host");
        }
        assert_projection_refused(&mut s, hosted, body, &[ProjSource::Face(top)], "consumed");
        let (projected, _) = s.sketch_project(hosted, live, &[ProjSource::Face(top)]).unwrap();
        assert_eq!(projected.entities.len(), 4);
        assert!(s.sketch_detail(hosted).unwrap().entities.iter().all(|e| e.driven && e.projection_state == Some("attached")));
    }

    #[test]
    fn projection_mixed_duplicates_return_selected_stable_ids() {
        use qymcad_core::model::ProjSource;
        let (mut s, body, sketch, edge) = projection_fixture();
        let other = s.p.regen_edges[&body].iter().find(|e| e.id != edge && e.a[2] == e.b[2]).unwrap().id;
        let first = s.sketch_project(sketch, body, &[ProjSource::Edge(edge), ProjSource::Edge(edge)]).unwrap().0;
        assert_eq!(first.projections.len(), 1);
        assert_eq!(first.entities.len(), 1);
        let before = serde_json::to_vec(&s.p).unwrap();
        assert_eq!(s.sketch_project(sketch, body, &[ProjSource::Edge(edge)]).unwrap().0, first);
        assert_eq!(serde_json::to_vec(&s.p).unwrap(), before);
        let mixed = s.sketch_project(sketch, body, &[ProjSource::Edge(edge), ProjSource::Edge(other), ProjSource::Edge(edge)]).unwrap().0;
        assert_eq!(mixed.projections.len(), 2);
        assert_eq!(mixed.projections[0], first.projections[0]);
        assert_eq!(mixed.entities.len(), 2);
        assert!(mixed.entities.contains(&first.entities[0]));
        assert!(first.points.iter().all(|p| mixed.points.contains(p)));
        assert_eq!(s.sketch_detail(sketch).unwrap().entities.len(), 2);
    }

    #[test]
    fn projection_add_captures_detachment_before_rebuild() {
        use qymcad_core::model::ProjSource;
        let (mut s, body, sketch, edge) = projection_fixture();
        let (old, _) = s.sketch_project(sketch, body, &[ProjSource::Edge(edge)]).unwrap();
        let si = s.sketch_si(sketch).unwrap();
        s.p.sketches[si].projections[0].src = ProjSource::Edge(u32::MAX);
        let other = s.p.regen_edges[&body].iter().find(|e| e.id != edge && e.a[2] == e.b[2]).unwrap().id;
        s.sketch_project(sketch, body, &[ProjSource::Edge(other)]).unwrap();
        let detail = s.sketch_detail(sketch).unwrap();
        let old = detail.entities.iter().find(|e| e.id == old.entities[0]).unwrap();
        assert!(!old.driven);
        assert_eq!(old.projection_state, Some("detached"));
    }

    #[test]
    fn import_retessellation_replaces_void_advisories_but_retains_source_warning() {
        use qymcad_core::model::ImportNode;

        let mut s = Session::new_part();
        let solid = native::fixture("cube-20");
        let hollow = native::fixture("hollow");
        let mesh = qymcad_kernel::ask::export_mesh(&hollow, 0.5).unwrap().mesh;
        {
            let _gate = qymcad_kernel::kernel_gate();
            assert_eq!((hollow.shell_count(), hollow.solid_count()), (2, 1));
            // Fully enclosed rectangular cavity: 20³ - 4*6*3 mm³.
            assert!((hollow.volume() - (8000.0 - 4.0 * 6.0 * 3.0)).abs() < 1e-8);
        }
        let source = s.p.add_source("sealed.step", Vec::new());
        let body = s.p.add_mesh(mesh);
        s.p.import_tree_as_parts(
            vec![ImportNode { name: "sealed import".into(), body: Some(body), ..Default::default() }],
            source,
            "sealed",
        )
        .unwrap();
        s.shapes.insert(body, hollow);
        s.advisory_warnings = crate::imports::restore_missing(&s.p, &mut s.shapes);

        for rebuild in 1..=2 {
            s.p.mark_all_dirty();
            let report = s.rebuild();
            assert!(report.errors.is_empty(), "rebuild {rebuild}: {:?}", report.errors);
            assert_eq!(
                report.warnings.iter().filter(|w| w.node == body && w.message.contains("sealed internal void")).count(),
                1,
                "rebuild {rebuild} must replace the prior void advisory: {:?}",
                report.warnings
            );
            assert_eq!(
                report.warnings.iter().filter(|w| w.node == body && w.message.contains("unavailable embedded source")).count(),
                1,
                "rebuild {rebuild} must preserve the source warning: {:?}",
                report.warnings
            );
        }
        // Retessellating repaired geometry clears its void advisory, while the original is still unavailable.
        s.shapes.insert(body, solid);
        s.p.mark_all_dirty();
        let report = s.rebuild();
        assert!(report.errors.is_empty(), "repaired rebuild: {:?}", report.errors);
        assert_eq!(report.warnings.len(), 1, "only the source warning should remain: {:?}", report.warnings);
        assert!(report.warnings[0].message.contains("unavailable embedded source"));
        s.p.timeline.retain(|n| n.id != body);
        s.shapes.remove(&body);
        assert!(s.rebuild().warnings.is_empty(), "deleting the import also retires its source warning");
    }

    #[test]
    fn reporting_sphere_bounds_follow_nominal_accuracy_at_part_and_large_import_scales() {
        for radius in [2.0, 1000.0] {
            let shape = native::sphere_named(radius, [1, 2, 3]);
            // A sphere has bounds ±r and diagonal 2*r*sqrt(3). Use nominal deflection
            // max(.005, diagonal*1e-5) plus f32 coordinate rounding, rather than native padding.
            let tolerance = 0.005_f64.max(2.0 * radius * 3.0_f64.sqrt() * 1e-5) + f32::EPSILON as f64 * radius;
            let bbox = reporting_bbox(&shape).unwrap();
            for (actual, expected) in bbox.into_iter().zip([-radius, -radius, -radius, radius, radius, radius]) {
                assert!((actual - expected).abs() <= tolerance, "sphere bbox: {actual} vs {expected} ± {tolerance}");
            }
        }
    }

    #[test]
    fn live_unnamed_edges_refuse_topology_restoration() {
        let mut s = Session::new_part();
        let sh = {
            let mut bytes = native::bytes("cylinder-2-3");
            native::rename_names(&mut bytes, &[], true);
            let sh = native::restore(bytes);
            let _gate = qymcad_kernel::kernel_gate();
            assert!(!sh.edges_info().is_empty());
            sh
        };
        s.shapes.insert(42, sh);
        let e = s.ensure_topology().unwrap_err();
        assert!(e.to_string().contains("body 42 has live edges but no usable named edges"), "{e}");
        assert!(s.p.regen_edges.is_empty());
        assert!(s.shapes.contains_key(&42), "error preserves original live shape");
        // A cylinder r=2, h=3 has volume pi*r²*h = 12*pi.
        let _gate = qymcad_kernel::kernel_gate();
        assert!((s.shapes[&42].volume() - 12.0 * std::f64::consts::PI).abs() < 1e-8);
    }
    #[test]
    fn failed_partial_rebuild_drops_shapes_of_previously_unbuilt_planned_bodies() {
        use crate::{BaseName, PlaneRef};

        let mut s = Session::new_part();
        let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &4.0.into(), &6.0.into(), false).unwrap();
        // Native construction represents an opened recipe with pending, never-built bodies.
        let body = s.p.add_extrude(sketch, 3.0);
        let failed_body = s.p.add_extrude(Id::MAX, 3.0);
        let before = s.p.clone();
        let recipe = serde_json::to_value(&before).unwrap();
        let nodes = s.p.timeline.iter().filter(|n| n.dirty).map(|n| n.id).collect();
        assert!(s.shapes.is_empty());
        let result: Result<()> = s.with_rebuild_copies(before, &nodes, "partial rebuild", |s| {
            s.p.set_feat_dim(body, "height", "5".into());
            let report = s.rebuild_retrying(Some(&nodes));
            assert!(report.errors.iter().any(|e| e.node == failed_body), "the second extrusion must fail: {report:?}");
            // The first extrusion did build changed geometry: V = width * length * edited height.
            assert!((s.shapes[&body].volume() - 4.0 * 6.0 * 5.0).abs() < 1e-8);
            Err(Error::Rebuild(report.errors.iter().map(|e| e.message.clone()).collect()))
        });
        assert!(result.is_err());
        assert_eq!(serde_json::to_value(&s.p).unwrap(), recipe, "the pending native recipe is restored exactly");
        assert!(s.shapes.is_empty(), "the failed partial rebuild leaked geometry for a previously unbuilt body");
    }

    #[test]
    fn failed_rebuild_closure_restores_diagnostics_and_drops_orphan_shapes() {
        let mut s = Session::new_part();
        let baseline = NodeIssue { node: 1, name: "baseline".into(), message: "retained warning".into() };
        s.advisory_warnings.push(AdvisoryWarning::Geometry(baseline.clone()));
        let before = s.p.clone();
        let result: Result<()> = s.with_rebuild_copies(before, &HashSet::new(), "failed closure", |s| {
            let shape = native::sphere_named(2.0, [1, 2, 3]);
            s.shapes.insert(99, shape); // No recipe node owns this newly introduced body.
            s.advisory_warnings.push(AdvisoryWarning::Geometry(NodeIssue {
                node: 99,
                name: "orphan".into(),
                message: "must disappear".into(),
            }));
            Err(Error::Invalid("stop before regeneration".into()))
        });
        assert!(result.is_err());
        assert_eq!(s.advisory_warnings, vec![AdvisoryWarning::Geometry(baseline)]);
        assert!(s.shapes.is_empty(), "the failed closure leaked a body absent from the restored recipe");
    }
}
