//! Surface makers: the app's FaceCopy and OffsetSurface nodes (ADR 0060). Each takes faces of a body (solid or sheet) and
//! makes a NEW sheet body beside it; the source is read, not consumed. Several faces make ONE sheet body. The faces are
//! stored as `Ref::picks` of the resolved ids, the way the app's own edit mode stores them (`apply/update.rs`).
use crate::error::{Error, Result};
use crate::modifiers::dim;
use crate::session::{Rebuild, Session};
use crate::topology::{Element, Sel};
use crate::value::Num;
use qymcad_core::feature::FeatureKind;
use qymcad_core::model::Id;
use qymcad_core::refs::Ref;

/// The app's Offset surface field range (`start_face_copy_cmd`: `CmdParam::new("f-distance", "dist", 5.0, -10000.0, 10000.0)`); the kernel has
/// no limit of its own. Zero is allowed: a copy in place.
pub(crate) const OFFSET_LIMIT: f64 = 10_000.0;

fn offset_distance(v: f64) -> Result<f64> {
    if !(v.is_finite() && v.abs() <= OFFSET_LIMIT) {
        return Err(Error::Invalid(format!(
            "offset distance must be finite and within -{OFFSET_LIMIT}..{OFFSET_LIMIT} mm (the app's range), got {v}"
        )));
    }
    Ok(v)
}

impl Session {
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
}
