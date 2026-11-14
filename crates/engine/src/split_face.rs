//! Split faces: the app's `SplitFace` node, which cuts every face a plane crosses and leaves the body in one piece (ADR 0057).
//! The native result is silently wrong in five ways (F-187..F-190); each is refused at creation and again on every
//! rebuild of the node: a creation, a parameter or dimension edit, an edit upstream, a reorder.
use crate::{Error, Id, Num, PlaneRef, Rebuild, Result, Session};
use qymcad_core::{
    errors::CoreError,
    feature::FeatureKind,
    model::{FeatNumber, PlainNumber},
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Arguments of one Split faces action.
#[derive(Clone, Debug)]
pub struct SplitFaceOptions {
    /// Body to cut; defaults to the current body of the active Part.
    pub body: Option<Id>,
    /// Base plane, datum plane or one planar face.
    pub plane: PlaneRef,
    /// Signed offset, mm, along the plane's normal: XY +Z, XZ +Y, YZ +X; a datum or a face along its own normal.
    pub offset: Num,
    /// Timeline feature name.
    pub name: Option<String>,
}

/// How many faces the body had before and has after.
#[derive(Debug, Serialize)]
pub struct FaceCounts {
    pub before: usize,
    pub after: usize,
}

/// The Split faces node and what it did to the faces.
#[derive(Debug, Serialize)]
pub struct SplitFaceResult {
    /// Timeline feature id, which is also the id of the body it makes.
    pub feature: Id,
    /// The body after the split (the same id as `feature`).
    pub body: Id,
    /// Feature name.
    pub name: String,
    /// Evaluated signed plane offset, mm.
    pub offset: f64,
    /// Face counts of the source body and of the result.
    pub faces: FaceCounts,
    /// Ids of the faces the result has and the source did not: the new pieces, in ascending order.
    pub added_faces: Vec<u32>,
    /// Current document bodies and rebuild diagnostics.
    pub rebuild: Rebuild,
}

/// Two boxes (min xyz, max xyz) lie on the same piece: positive overlap along every axis in which either is extended. An axis
/// in which both are flat (a planar face's normal axis) is not compared: a face moves with its body when the body is resized.
fn same_piece(before: &[f64; 6], after: &[f64; 6]) -> bool {
    let scale = before.iter().chain(after).fold(1.0_f64, |m, v| m.max(v.abs()));
    // The mesh holds f32 coordinates (F-063): the tolerance grows with the coordinates, 1e-4 mm up to a body of 100 m.
    let tol = (1e-6 * scale).max(1e-4);
    (0..3).all(|i| {
        let (extent_before, extent_after) = (before[i + 3] - before[i], after[i + 3] - after[i]);
        if extent_before <= tol && extent_after <= tol {
            return true;
        }
        before[i + 3].min(after[i + 3]) - before[i].max(after[i]) > tol
    })
}

/// One face of a Split faces result, as the rebuild left it.
#[derive(Clone)]
struct Piece {
    id: u32,
    bbox: [f64; 6],
    /// The source has no face of this id and area: this node cut it (or made it).
    cut: bool,
}

/// What a Split faces node's result looked like before a rebuild: its faces with local bounds.
pub(crate) struct SplitFaceBefore {
    node: Id,
    pieces: Vec<Piece>,
}

impl Session {
    /// Add Split faces as the app's one action: the node, the plane reference, the offset formula and the name; refused with the
    /// document unchanged when the plane divides nothing, the result is not a clean split, or a general precondition fails.
    /// Precondition: the caller must not hold the kernel gate (`ask::splits_faces` acquires it).
    pub fn split_face(&mut self, options: &SplitFaceOptions) -> Result<SplitFaceResult> {
        self.ensure_topology()?;
        let ((feature, src, offset), mut rebuild) = self.atomic(|s| {
            // The app's command works on the selected body or the active Part's body. With an assembly (root or nested) active and no
            // body named there is no Part to take it from: the server does not pick the only body of another Part for it.
            if options.body.is_none() && !s.p.ctx_holds_bodies(s.p.active_component.unwrap_or(s.p.root)) {
                return Err(Error::Invalid(
                    "the active context is an assembly, not a Part, and no body was named: part_select a Part first or pass `body`".into(),
                ));
            }
            // The class of F-185: a body that a suppressed feature (or a suppressed input of its chain) leaves unbuilt has no faces to cut.
            if let Some(body) = options.body.filter(|b| crate::session::suppression_unbuilt(&s.p, None).contains(b)) {
                return Err(Error::Invalid(format!(
                    "body {body} is not built because it or a feature it depends on is suppressed: unsuppress it before splitting its faces"
                )));
            }
            let src = s.source_body(options.body)?;
            let offset = options.offset.eval(&s.p.param_map())?;
            if !offset.is_finite() {
                return Err(Error::Invalid(format!("split_face offset must be a finite number, got {offset}")));
            }
            let op = s.resolve_op_plane(&options.plane)?;
            let shape = s.shapes.get(&src).ok_or_else(|| Error::NotFound(format!("body {src} has no built shape")))?;
            // The app asks the kernel on a copy of the live shape before it lays the node; it does so in the world frame while the
            // rebuild cuts in the body's local frame (F-194). The server asks in the frame that is built.
            if !qymcad_kernel::ask::splits_faces(shape, op.at(offset)) {
                return Err(Error::Invalid(
                    "split plane splits nothing: it misses the body or lies on a face. The plane and the offset are in the local frame of the body's Part; read the body's bounds with doc_info".into(),
                ));
            }
            let feature = s.in_component(s.p.body_owner(src), |p| p.add_split_face(src, op.code, op.datum, offset));
            op.attach(&mut s.p, feature);
            // The offset text stays on the node as for `split`: a formula follows its parameters, a number is what `dimension_set` shows.
            let text = options.offset.expr().unwrap_or_else(|| offset.to_string());
            s.p.write_feat_formula(&FeatNumber { node: feature, key: "offset" }, &text, PlainNumber::Stands)
                .map_err(|e| Error::Expr(format!("split_face offset: {e:?}")))?;
            s.set_node_name(feature, options.name.as_deref())?;
            Ok((feature, src, offset))
        })?;
        let ids = |s: &Session, body: Id| -> Vec<u32> {
            s.p.regen_faces.get(&body).map(|fs| fs.iter().map(|f| f.id).collect()).unwrap_or_default()
        };
        let (before, after) = (ids(self, src), ids(self, feature));
        let known: HashSet<u32> = before.iter().copied().collect();
        let mut added_faces: Vec<u32> = after.iter().copied().filter(|id| !known.contains(id)).collect();
        added_faces.sort_unstable();
        rebuild.bodies = self.result_bodies();
        Ok(SplitFaceResult {
            feature,
            body: feature,
            name: self.node_name(feature),
            offset,
            faces: FaceCounts { before: before.len(), after: after.len() },
            added_faces,
            rebuild,
        })
    }

    /// Local bounds of each face of `body`, from its tessellation; `None` when a face has no triangles to measure.
    fn face_boxes(&self, body: Id) -> Option<HashMap<usize, [f64; 6]>> {
        let faces = self.p.regen_faces.get(&body)?;
        let mesh = &self.p.bodies[self.p.mesh_index(body)?].mesh;
        let mut boxes = HashMap::new();
        for (i, face) in faces.iter().enumerate() {
            let mut b = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
            for &ti in &face.triangles {
                for &vi in mesh.tris.get(ti as usize)? {
                    let v = mesh.verts.get(vi as usize)?;
                    for (axis, c) in [v.x, v.y, v.z].into_iter().enumerate() {
                        b[axis] = b[axis].min(c);
                        b[axis + 3] = b[axis + 3].max(c);
                    }
                }
            }
            if b.iter().any(|v| !v.is_finite()) {
                return None;
            }
            boxes.insert(i, b);
        }
        Some(boxes)
    }

    /// The faces of a Split faces result and of its source as they stand, or `None` when either has no faces to read.
    fn split_face_pieces(&self, src: Id, body: Id) -> Option<Vec<Piece>> {
        let source = self.p.regen_faces.get(&src)?;
        let faces = self.p.regen_faces.get(&body)?;
        let boxes = self.face_boxes(body)?;
        Some(
            faces
                .iter()
                .enumerate()
                .map(|(i, f)| Piece {
                    id: f.id,
                    bbox: boxes[&i],
                    cut: !source.iter().any(|s| s.id == f.id && (s.area - f.area).abs() <= 1e-4 * s.area.abs().max(1.0)),
                })
                .collect(),
        )
    }

    /// Before a rebuild: every live Split faces node's result that really split something. A result that is a pass-through of its
    /// source (the node was suppressed, or never built) has no pieces to keep and is not recorded.
    pub(crate) fn split_face_snapshots(&self) -> Vec<SplitFaceBefore> {
        self.p
            .timeline
            .iter()
            .filter(|n| !n.suppressed)
            .filter_map(|n| {
                let FeatureKind::SplitFace { src, body, .. } = n.kind else { return None };
                let pieces = self.split_face_pieces(src, body)?;
                (pieces.len() > self.p.regen_faces.get(&src)?.len()).then_some(SplitFaceBefore { node: n.id, pieces })
            })
            .collect()
    }

    /// Every Split faces node that was rebuilt must still be a clean split of its source. The app builds the node green in all
    /// of these cases (F-187..F-190): no face gained, duplicate face ids, the plane's own face gone, another set of faces cut by an edit.
    /// A node the app itself failed is left to its own error.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()`.
    pub(crate) fn split_face_refusals(&self, before: &[SplitFaceBefore], rebuilt: &HashSet<Id>) -> Vec<(Id, String)> {
        let buildable = Self::geometry_check_nodes(&self.p);
        let mut refused = Vec::new();
        for node in self.p.timeline.iter().filter(|n| buildable.contains(&n.id) && rebuilt.contains(&n.id)) {
            let FeatureKind::SplitFace { src, body, face, .. } = &node.kind else { continue };
            // Our own refusals are `Kernel` errors; any other mark is the app's own failure, with the source passed through.
            if self.p.regen_errors.get(&node.id).is_some_and(|e| !matches!(e, CoreError::Kernel(_))) {
                continue;
            }
            let (Some(source), Some(result)) = (self.p.regen_faces.get(src), self.p.regen_faces.get(body)) else { continue };
            if let Some((plane_body, key)) = face {
                if self.p.regen_faces.get(plane_body).is_some_and(|fs| !fs.iter().any(|f| f.id == key.id)) {
                    refused.push((node.id, format!(
                        "the face {} of body {plane_body} that gives the cutting plane of this split no longer exists after this edit: the app would keep cutting at the plane's old place without a word (F-190); edit refused, recreate the split with a base plane, a datum plane or the face's new id",
                        key.id
                    )));
                    continue;
                }
            }
            let distinct: HashSet<u32> = result.iter().map(|f| f.id).collect();
            if distinct.len() < result.len() {
                refused.push((node.id, format!(
                    "the split leaves {} faces with only {} distinct ids: a face id would name two faces, and a later feature, selection or export would take either (F-189, the class of upstream #173). The app does this for a plane that crosses an earlier split of the same body and for a parallel plane on the wrong side of it (below it along +Z or +Y, above it along +X); a cut that crosses an earlier one cannot be made, and parallel cuts must be made in the order that keeps the ids",
                    result.len(),
                    distinct.len()
                )));
                continue;
            }
            if result.len() <= source.len() {
                refused.push((node.id, format!(
                    "the split divided no face: the body has {} faces before and {} after. A plane within about 1e-5 mm of a face, or through the axis of a torus, passes the app's own check and reports success while it splits nothing; near the face it also shrinks the body (F-187). Move the plane at least 1e-4 mm off the face or choose another plane",
                    source.len(),
                    result.len()
                )));
                continue;
            }
            let Some(old) = before.iter().find(|b| b.node == node.id) else { continue };
            if let Some(message) = self.changed_pieces(old, *src, *body) {
                refused.push((node.id, message));
            }
        }
        refused
    }

    /// The faces this node cut before the rebuild against those after: the set of ids must be the same and every cut id must stay on
    /// its piece. The app lets a new offset cut another set of faces; the old ids then point at other pieces without a word (F-188).
    fn changed_pieces(&self, old: &SplitFaceBefore, src: Id, body: Id) -> Option<String> {
        let Some(new) = self.split_face_pieces(src, body) else {
            return Some("its faces cannot be measured after the edit, so the pieces cannot be checked; edit refused".into());
        };
        let was: HashSet<u32> = old.pieces.iter().map(|p| p.id).collect();
        let now: HashSet<u32> = new.iter().map(|p| p.id).collect();
        let mut gone: Vec<u32> = was.difference(&now).copied().collect();
        let mut fresh: Vec<u32> = now.difference(&was).copied().collect();
        gone.sort_unstable();
        fresh.sort_unstable();
        let moved: Vec<u32> = old
            .pieces
            .iter()
            .filter_map(|o| new.iter().find(|n| n.id == o.id).filter(|n| (o.cut || n.cut) && !same_piece(&o.bbox, &n.bbox)).map(|_| o.id))
            .collect();
        if gone.is_empty() && fresh.is_empty() && moved.is_empty() {
            return None;
        }
        let mut what = Vec::new();
        if !gone.is_empty() {
            what.push(format!("face ids {gone:?} disappear"));
        }
        if !fresh.is_empty() {
            what.push(format!("face ids {fresh:?} appear"));
        }
        if !moved.is_empty() {
            what.push(format!("face ids {moved:?} move to another piece"));
        }
        Some(format!(
            "this edit changes the faces the split cuts ({}): the ids kept would silently name other pieces for every feature that uses them, with no error from the app (F-188). Edit refused atomically; recreate the split for this change and read topology again",
            what.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::same_piece;

    #[test]
    fn pieces_need_positive_overlap_where_they_are_extended() {
        let lower = [-20.0, -15.0, 0.0, 20.0, -15.0, 5.0]; // a wall at y = -15
        let upper = [-20.0, -15.0, 5.0, 20.0, -15.0, 10.0];
        assert!(same_piece(&lower, &lower));
        assert!(!same_piece(&lower, &upper), "touching along the cut is another piece");
        let taller = [-20.0, -15.0, 0.0, 20.0, -15.0, 8.0];
        assert!(same_piece(&lower, &taller));
        let wider = [-20.0, -17.0, 0.0, 20.0, -17.0, 5.0];
        assert!(same_piece(&lower, &wider), "a wall moves with its body: the flat axis is not compared");
        let near = [-20.0, -15.0, 4.99995, 20.0, -15.0, 9.0];
        assert!(!same_piece(&lower, &near), "an overlap inside the mesh tolerance is none");
    }
}
