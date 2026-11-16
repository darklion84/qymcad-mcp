//! Surface tools: the app's FaceCopy and OffsetSurface nodes (ADR 0060), SurfaceReplace and Stitch (ADR 0061).
//! FaceCopy and OffsetSurface take faces of a body (solid or sheet) and make a NEW sheet body beside it; the source is read,
//! not consumed. Several faces make ONE sheet body. SurfaceReplace swaps faces of a solid for a sheet that closes them and
//! Stitch joins sheets into one body; both consume their inputs. Faces are stored as `Ref::picks` of the resolved ids, the
//! way the app's own edit mode stores them (`apply/update.rs`).
use crate::error::{Error, Result};
use crate::localization;
use crate::modifiers::dim;
use crate::session::{Rebuild, Session};
use crate::topology::{Element, Sel};
use crate::value::Num;
use qymcad_core::errors::CoreError;
use qymcad_core::feature::FeatureKind;
use qymcad_core::model::Id;
use qymcad_core::refs::Ref;

/// The app's Offset surface field range (`start_face_copy_cmd`: `CmdParam::new("f-distance", "dist", 5.0, -10000.0, 10000.0)`); the kernel has
/// no limit of its own. Zero is allowed: a copy in place.
pub(crate) const OFFSET_LIMIT: f64 = 10_000.0;

/// The app's Stitch tolerance field (`start_stitch_cmd`: `CmdParam::new("f-stitch-tol", "tol", 0.01, 1e-6, 10.0)`): the default and
/// the range. The kernel has no limit of its own.
pub const STITCH_TOLERANCE_DEFAULT: f64 = 0.01;
const STITCH_TOLERANCE_RANGE: (f64, f64) = (1e-6, 10.0);

fn stitch_tolerance(v: f64) -> Result<f64> {
    let (lo, hi) = STITCH_TOLERANCE_RANGE;
    if !(v.is_finite() && (lo..=hi).contains(&v)) {
        return Err(Error::Invalid(format!("stitch tolerance must be finite and within 1e-6..10 mm (the app's range), got {v}")));
    }
    Ok(v)
}

fn offset_distance(v: f64) -> Result<f64> {
    if !(v.is_finite() && v.abs() <= OFFSET_LIMIT) {
        return Err(Error::Invalid(format!(
            "offset distance must be finite and within -{OFFSET_LIMIT}..{OFFSET_LIMIT} mm (the app's range), got {v}"
        )));
    }
    Ok(v)
}

impl Session {
    /// Recheck evaluated stitch tolerances, as `check_offset_values` does for distances.
    pub(crate) fn check_stitch_values(&self) -> std::result::Result<(), (Id, Error)> {
        let vars = self.p.param_map();
        for node in &self.p.timeline {
            if let FeatureKind::Stitch { tol, .. } = node.kind {
                let v = self.p.feat_dim(node.id, "tol").filter(|e| !e.is_empty()).map_or(Ok(tol), |e| Num::from(e).eval(&vars));
                v.and_then(stitch_tolerance).map_err(|e| (node.id, e))?;
            }
        }
        Ok(())
    }

    /// Recheck evaluated offset distances: native regeneration bounds none (a parameter or dimension edit can push `dist` past
    /// the app's range, or to a non-finite value).
    pub(crate) fn check_offset_values(&self) -> std::result::Result<(), (Id, Error)> {
        let vars = self.p.param_map();
        for node in &self.p.timeline {
            if let FeatureKind::OffsetSurface { dist, .. } = node.kind {
                let v = self.p.feat_dim(node.id, "dist").filter(|e| !e.is_empty()).map_or(Ok(dist), |e| Num::from(e).eval(&vars));
                v.and_then(offset_distance).map_err(|e| (node.id, e))?;
            }
        }
        Ok(())
    }

    /// The faces of `src` that `sel` resolves to, as ids and as the app's stored picks. An empty set is refused (the app's
    /// `msg-faces-not-found`).
    fn surface_faces(&self, src: Id, sel: &Sel, what: &str) -> Result<(Vec<u32>, Ref)> {
        let ids = self.resolve_sel(src, Element::Faces, sel)?;
        if ids.is_empty() {
            return Err(Error::Invalid(format!("the {what} face selection matched no face of body {src}")));
        }
        let picks = Ref::picks(&ids);
        Ok((ids, picks))
    }

    /// Copy faces into a new sheet body (the app's Copy faces). Returns the node (also the sheet's id), the face ids and the report.
    pub fn face_copy(&mut self, body: Option<Id>, faces: &Sel, name: Option<&str>) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let (ids, picks) = s.surface_faces(src, faces, "copy")?;
            let id = s.in_component(s.p.body_owner(src), |p| p.add_face_copy(src, picks));
            s.set_node_name(id, name)?;
            Ok((id, ids))
        })
        .map(|((id, ids), rebuild)| (id, ids, rebuild))
    }

    /// Replace the faces of an existing FaceCopy node, rebuilding what depends on it.
    pub fn repick_face_copy(&mut self, feature: Id, faces: &Sel) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        let (src, body) = self.feature_as(feature, "face_copy", |kind| match kind {
            FeatureKind::FaceCopy { src, body, .. } => Some((*src, *body)),
            _ => None,
        })?;
        let (ids, picks) = self.surface_faces(src, faces, "copy")?;
        let bodies = self.repick_rebuild(feature, "face_copy", |kind, _| {
            let FeatureKind::FaceCopy { faces: stored, .. } = kind else {
                return None;
            };
            *stored = picks;
            Some(())
        })?;
        Ok((body, ids, self.current_rebuild_after_edit(bodies)))
    }

    /// Move faces along their normals into a new sheet body (the app's Offset surface): outward of a solid is positive, zero is
    /// a copy in place. `dist` is a feature dimension, editable with `dimension_set`.
    pub fn offset_surface(&mut self, body: Option<Id>, faces: &Sel, dist: &Num, name: Option<&str>) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.source_body(body)?;
            // The range is judged by the rebuild (`check_offset_values`), creation included.
            let d = dist.eval(&s.p.param_map())?;
            let (ids, picks) = s.surface_faces(src, faces, "offset")?;
            let id = s.in_component(s.p.body_owner(src), |p| p.add_offset_surface(src, picks, d));
            dim(s, id, "dist", dist);
            s.set_node_name(id, name)?;
            Ok((id, ids))
        })
        .map(|((id, ids), rebuild)| (id, ids, rebuild))
    }

    /// Replace the faces of an existing OffsetSurface node; the distance formula stays.
    pub fn repick_offset_surface(&mut self, feature: Id, faces: &Sel) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        let (src, body) = self.feature_as(feature, "offset_surface", |kind| match kind {
            FeatureKind::OffsetSurface { src, body, .. } => Some((*src, *body)),
            _ => None,
        })?;
        let (ids, picks) = self.surface_faces(src, faces, "offset")?;
        let bodies = self.repick_rebuild(feature, "offset_surface", |kind, _| {
            let FeatureKind::OffsetSurface { faces: stored, .. } = kind else {
                return None;
            };
            *stored = picks;
            Some(())
        })?;
        Ok((body, ids, self.current_rebuild_after_edit(bodies)))
    }

    /// A sheet body that SurfaceReplace or Stitch takes (`what` names the role in refusals). `node` is the node being re-picked:
    /// the bodies it consumes now are still free to it, and a body built after it cannot feed it.
    fn sheet_input(&self, body: Id, node: Option<Id>, what: &str) -> Result<Id> {
        if !self.is_active_body(body) {
            return Err(Error::NotFound(format!("body {body} is not built in the active document")));
        }
        let mut consumed = self.p.consumed_bodies();
        if let Some(n) = self.p.timeline.iter().find(|n| Some(n.id) == node) {
            for b in n.kind.consumed() {
                consumed.remove(&b);
            }
        }
        if consumed.contains(&body) {
            return Err(Error::Invalid(format!("body {body} was consumed by a later feature")));
        }
        if self.p.mesh_born(body) {
            return Err(Error::Invalid(crate::topology::mesh_body_refusal(body)));
        }
        if !self.is_sheet(body) {
            return Err(Error::Invalid(format!(
                "body {body} is a solid; {what} must be a sheet (a surface: face_copy, offset_surface, loft surface:true)"
            )));
        }
        if let Some(node) = node {
            let built_at = |id: Id| self.p.timeline.iter().position(|n| n.kind.bodies().contains(&id));
            if built_at(body) >= built_at(node) {
                return Err(Error::Invalid(format!("body {body} is built after feature {node}, which cannot take it")));
            }
        }
        Ok(body)
    }

    /// The solid whose faces SurfaceReplace drops: the usual source body, which must not be a sheet.
    fn solid_to_replace(&self, body: Option<Id>) -> Result<Id> {
        let src = self.source_body(body)?;
        if self.is_sheet(src) {
            return Err(Error::Invalid(format!(
                "body {src} is a sheet; `body` is the solid whose faces are replaced, the sheet goes in `surface`"
            )));
        }
        Ok(src)
    }

    /// The app's pre-check before it lays a SurfaceReplace node (`apply_surface_replace_cmd`): the sheet must close exactly what
    /// the dropped faces leave open. Asked of the live shapes; with either missing the rebuild judges, as in the app.
    fn sheet_must_close(&self, src: Id, faces: &[u32], surface: Id) -> Result<()> {
        let (Some(base), Some(sheet)) = (self.shapes.get(&src), self.shapes.get(&surface)) else {
            return Ok(());
        };
        qymcad_kernel::ask::sheet_closes(base, faces, sheet).map_err(|free| {
            Error::Invalid(format!(
                "the sheet does not close what it replaces: {}",
                localization::error(&CoreError::SurfaceDoesNotClose { free })
            ))
        })
    }

    /// Replace faces of a solid with a sheet (the app's Replace face): `src` and `surface` are consumed, the node's body is a
    /// new solid. Returns the node (also the body's id), the face ids and the report.
    pub fn surface_replace(&mut self, body: Option<Id>, faces: &Sel, surface: Id, name: Option<&str>) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.solid_to_replace(body)?;
            let surface = s.sheet_input(surface, None, "`surface`")?;
            let (ids, picks) = s.surface_faces(src, faces, "replace")?;
            s.sheet_must_close(src, &ids, surface)?;
            let id = s.in_component(s.p.body_owner(src), |p| p.add_surface_replace(src, picks, surface));
            s.set_node_name(id, name)?;
            Ok((id, ids))
        })
        .map(|((id, ids), rebuild)| (id, ids, rebuild))
    }

    /// Re-pick the faces and/or the sheet of an existing SurfaceReplace node, rebuilding what depends on it (the app's edit mode
    /// changes either). `ids` are empty when only the sheet changes.
    pub fn repick_surface_replace(&mut self, feature: Id, faces: Option<&Sel>, surface: Option<Id>) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        let (src, body) = self.feature_as(feature, "surface_replace", |kind| match kind {
            FeatureKind::SurfaceReplace { src, body, .. } => Some((*src, *body)),
            _ => None,
        })?;
        let (ids, picks) = match faces {
            Some(sel) => {
                let (ids, picks) = self.surface_faces(src, sel, "replace")?;
                (ids, Some(picks))
            }
            None => (Vec::new(), None),
        };
        let surface = surface.map(|b| self.sheet_input(b, Some(feature), "`surface`")).transpose()?;
        let bodies = self.repick_rebuild(feature, "surface_replace", |kind, _| {
            let FeatureKind::SurfaceReplace { faces: stored, surface: sheet, .. } = kind else {
                return None;
            };
            if let Some(picks) = picks {
                *stored = picks;
            }
            if let Some(s) = surface {
                *sheet = s;
            }
            Some(())
        })?;
        Ok((body, ids, self.current_rebuild_after_edit(bodies)))
    }

    /// The sheets a Stitch takes, in the order given (the first is the lineage): at least two, each once, none a solid.
    fn stitch_parts(&self, parts: &[Id], node: Option<Id>) -> Result<Vec<Id>> {
        if parts.len() < 2 {
            return Err(Error::Invalid("stitch needs at least two sheets in `parts`".into()));
        }
        let mut seen = Vec::new();
        for &part in parts {
            if seen.contains(&part) {
                return Err(Error::Invalid(format!("body {part} is listed twice in `parts`")));
            }
            seen.push(self.sheet_input(part, node, "every part of a stitch")?);
        }
        Ok(seen)
    }

    /// Stitch sheets into one body (the app's Stitch): every part is consumed; the result is a SOLID when the sheets close a
    /// shell, otherwise a sheet. `tol` is a feature dimension (`tol`), 1e-6..10 mm. Returns the node (also the body's id).
    pub fn stitch(&mut self, parts: &[Id], tol: &Num, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let parts = s.stitch_parts(parts, None)?;
            let t = tol.eval(&s.p.param_map())?;
            let owner = s.p.body_owner(parts[0]);
            let id = s.in_component(owner, |p| p.add_stitch(parts, t));
            dim(s, id, "tol", tol);
            s.set_node_name(id, name)?;
            Ok(id)
        })
    }

    /// Replace the sheets of an existing Stitch node; the tolerance formula stays.
    pub fn repick_stitch(&mut self, feature: Id, parts: &[Id]) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        let body = self.feature_as(feature, "stitch", |kind| match kind {
            FeatureKind::Stitch { body, .. } => Some(*body),
            _ => None,
        })?;
        let new = self.stitch_parts(parts, Some(feature))?;
        let bodies = self.repick_rebuild(feature, "stitch", |kind, _| {
            let FeatureKind::Stitch { parts: stored, .. } = kind else {
                return None;
            };
            *stored = new;
            Some(())
        })?;
        Ok((body, self.current_rebuild_after_edit(bodies)))
    }

    /// FINDINGS F-198: Stitch nodes picked by `check` whose SOLID result is not one. When the tolerance is wider than the gap
    /// between the edges of two sheets, the sewing glues them into a closed shell that encloses nothing (two parallel plates 5 apart
    /// at tol 5.1: kernel volume 4000, mesh 0), and the node is green. A solid's kernel volume and its mesh agree; here they do not.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()`.
    pub(crate) fn stitch_refusals(&self, check: impl Fn(Id) -> bool) -> Vec<(Id, String)> {
        self.p
            .timeline
            .iter()
            .filter(|n| check(n.id))
            .filter_map(|n| {
                let FeatureKind::Stitch { body, .. } = n.kind else { return None };
                let shape = self.shapes.get(&body)?;
                let (sheet, kernel) = {
                    let _gate = qymcad_kernel::kernel_gate();
                    (shape.is_sheet(), shape.volume())
                };
                let mesh = self.p.mesh_index(body).map(|i| self.p.bodies[i].mesh.volume())?;
                (!sheet && (kernel - mesh).abs() > 0.05 * kernel.abs().max(mesh.abs()) + 1e-6).then(|| {
                    (
                        n.id,
                        format!(
                            "the stitch gave a solid that is not one (kernel volume {kernel:.1} mm3, its mesh encloses {mesh:.1} mm3): the tolerance is wider than the gap between the sheets' edges, so it joined sheets that do not meet; use a smaller `tolerance`"
                        ),
                    )
                })
            })
            .collect()
    }
}
