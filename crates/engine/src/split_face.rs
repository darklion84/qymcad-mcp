//! Split faces: the app's `SplitFace` node, which cuts every face a plane crosses and leaves the body in one piece (ADR 0057).
//! The native result is silently wrong in five ways (F-187..F-190); each is refused at creation and again on every
//! rebuild of the node: a creation, a parameter or dimension edit, an edit upstream, a reorder.
use crate::session::{AdvisoryWarning, NodeIssue};
use crate::{Error, Id, Num, PlaneRef, Rebuild, Result, Session};
use qymcad_core::{
    errors::{CoreError, Op},
    feature::{FeatureKind, FeatureNode},
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
    /// The source has no face of this id and area: this node cut it (or made it). Not told apart reliably means cut.
    cut: bool,
}

/// What a Split faces node last cut that was accepted: its faces with local bounds and the point its cutting plane passed through.
/// `pieces` is `None` when they could not be measured (a file whose topology cannot be read, a face without triangles): an edit that
/// rebuilds the node is then refused, since nothing proves its ids stay on their pieces.
#[derive(Clone)]
pub(crate) struct Witness {
    pieces: Option<Vec<Piece>>,
    origin: Option<[f64; 3]>,
}

pub(crate) type Witnesses = HashMap<Id, Witness>;

fn moved(bbox: &[f64; 6], by: [f64; 3]) -> [f64; 6] {
    std::array::from_fn(|i| bbox[i] + by[i % 3])
}

const EXIT: &str = "To go on, feature_suppress or feature_delete this node.";

impl Session {
    /// Add Split faces as the app's one action: the node, the plane reference, the offset formula and the name; refused with the
    /// document unchanged when the plane divides nothing, the result is not a clean split, or a general precondition fails.
    /// Precondition: the caller must not hold the kernel gate (`ask::splits_faces` acquires it).
    pub fn split_face(&mut self, options: &SplitFaceOptions) -> Result<SplitFaceResult> {
        self.ensure_topology()?;
        let made = std::cell::Cell::new(None);
        let outcome = self.atomic(|s| {
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
            if s.p.bodies.iter().any(|b| b.id == src && b.sheet) {
                return Err(Error::Invalid(format!(
                    "body {src} is a sheet (a surface without volume): Split faces works on the faces of a solid"
                )));
            }
            // A clone of a Part is read-only; say so before anything about the plane, whatever frame the clone stands in.
            if let Some(source) = crate::instances::instance_links(&s.p).get(&s.p.body_owner(src).unwrap_or(0)) {
                return Err(Error::Assembly(format!("part {} is an instance of {source}; edit the source part", s.p.body_owner(src).unwrap_or(0))));
            }
            let offset = options.offset.eval(&s.p.param_map())?;
            if !offset.is_finite() {
                return Err(Error::Invalid(format!("split_face offset must be a finite number, got {offset}")));
            }
            let op = s.resolve_op_plane(&options.plane)?;
            let shape = s.shapes.get(&src).ok_or_else(|| Error::NotFound(format!("body {src} has no built shape")))?;
            // The app asks the kernel on a copy of the live shape before it lays the node; it does so in the world frame while the
            // rebuild cuts in the body's local frame (F-194). The server asks in the frame that is built.
            if !qymcad_kernel::ask::splits_faces(shape, op.at(offset)) {
                let toward = if matches!(options.plane, PlaneRef::Face { .. }) {
                    " For a face the offset runs along its OUTWARD normal: a negative offset goes into the body."
                } else {
                    ""
                };
                return Err(Error::Invalid(format!(
                    "split plane splits nothing: it misses the body or lies on a face. The plane and the offset are in the local frame of the body's Part; read the body's bounds with doc_info.{toward}"
                )));
            }
            let feature = s.in_component(s.p.body_owner(src), |p| p.add_split_face(src, op.code, op.datum, offset));
            made.set(Some(feature));
            op.attach(&mut s.p, feature);
            // The offset text stays on the node as for `split`: a formula follows its parameters, a number is what `dimension_set` shows.
            let text = options.offset.expr().unwrap_or_else(|| offset.to_string());
            s.p.write_feat_formula(&FeatNumber { node: feature, key: "offset" }, &text, PlainNumber::Stands)
                .map_err(|e| Error::Expr(format!("split_face offset: {e:?}")))?;
            s.set_node_name(feature, options.name.as_deref())?;
            Ok((feature, src, offset))
        });
        let ((feature, src, offset), mut rebuild) = match outcome {
            Ok(done) => done,
            // The node of a refused creation is rolled back: its id means nothing to the caller.
            Err(Error::Rebuild(lines)) => {
                let id = made.get().map(|f| format!(" ({f})")).unwrap_or_default();
                return Err(Error::Rebuild(lines.into_iter().map(|l| if id.is_empty() { l } else { l.replacen(&id, "", 1) }).collect()));
            }
            Err(e) => return Err(e),
        };
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
    fn face_boxes(&self, body: Id) -> Option<Vec<[f64; 6]>> {
        let faces = self.p.regen_faces.get(&body)?;
        let mesh = &self.p.bodies[self.p.mesh_index(body)?].mesh;
        let mut boxes = Vec::with_capacity(faces.len());
        for face in faces {
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
            boxes.push(b);
        }
        Some(boxes)
    }

    /// The faces of a Split faces result as they stand, measured the same way before and after an edit; `None` when a face cannot be.
    fn split_face_pieces(&self, src: Id, body: Id) -> Option<Vec<Piece>> {
        let source = self.p.regen_faces.get(&src)?;
        let faces = self.p.regen_faces.get(&body)?;
        let boxes = self.face_boxes(body)?;
        Some(
            faces
                .iter()
                .zip(boxes)
                .map(|(f, bbox)| Piece {
                    id: f.id,
                    bbox,
                    // A face that keeps its id and its area to 1e-6 is the source's; any doubt is a cut face, compared more strictly.
                    cut: !source.iter().any(|s| s.id == f.id && (s.area - f.area).abs() <= 1e-6 * s.area.abs().max(1e-9)),
                })
                .collect(),
        )
    }

    /// The point the node's cutting plane passes through, in the body's local frame.
    fn split_face_origin(&self, node: &FeatureNode) -> Option<[f64; 3]> {
        let FeatureKind::SplitFace { plane, datum, offset, face, .. } = &node.kind else { return None };
        let offset = self.p.feat_dim_value(node.id, "offset", *offset, &self.p.param_map());
        let (origin, normal) = if let Some((body, key)) = face {
            self.p.regen_faces.get(body)?;
            self.p.resolve_face(*body, key)
        } else if *datum != 0 {
            let p = self.p.planes.iter().find(|p| p.id == *datum)?;
            (p.origin, p.normal)
        } else {
            ([0.0; 3], [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]][(*plane as usize).min(2)])
        };
        let m = normal.iter().map(|v| v * v).sum::<f64>().sqrt();
        (m > 1e-12 && origin.iter().all(|v| v.is_finite())).then(|| std::array::from_fn(|i| origin[i] + offset * normal[i] / m))
    }

    /// After a rebuild: remember the pieces of every Split faces node that was built clean and really split something, so that a later
    /// rebuild is judged against them also when the node is not built in between (suppressed, past the rollback bar). A node that
    /// failed or was refused keeps its earlier witness.
    pub(crate) fn accept_split_faces(&mut self, rebuilt: &HashSet<Id>, failed: &HashSet<Id>) {
        let buildable = Self::geometry_check_nodes(&self.p);
        let mut accepted = Vec::new();
        for node in &self.p.timeline {
            let FeatureKind::SplitFace { src, body, .. } = &node.kind else { continue };
            if !buildable.contains(&node.id)
                || !rebuilt.contains(&node.id)
                || failed.contains(&node.id)
                || self.p.regen_errors.contains_key(&node.id)
            {
                continue;
            }
            let (Some(source), Some(result)) = (self.p.regen_faces.get(src), self.p.regen_faces.get(body)) else { continue };
            if result.len() > source.len() {
                accepted.push((node.id, Witness { pieces: self.split_face_pieces(*src, *body), origin: self.split_face_origin(node) }));
            }
        }
        self.split_face_witness.extend(accepted);
        let live: HashSet<Id> = self.p.timeline.iter().map(|n| n.id).collect();
        self.split_face_witness.retain(|id, _| live.contains(id));
    }

    /// `part_copy` makes new nodes whose faces have ids of their own, so the copy's witness is not the source's renamed: the rebuild of
    /// the copy has already recorded its pieces. A copied node that was not built (suppressed) has none: it is unverifiable until deleted.
    pub(crate) fn copy_split_face_witness(&mut self, nodes: &[(Id, Id)]) {
        for (from, to) in nodes {
            if self.split_face_witness.contains_key(from) && !self.split_face_witness.contains_key(to) {
                self.split_face_witness.insert(*to, Witness { pieces: None, origin: None });
            }
        }
    }

    /// Every Split faces node that was rebuilt must still be a clean split of its source. The app builds the node green in all
    /// of these cases (F-187..F-190): no face gained, duplicate face ids, the plane's own face gone, another set of faces cut by an edit.
    /// A node the app itself failed is left to its own error.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()`.
    pub(crate) fn split_face_refusals(&self, before: &Witnesses, rebuilt: &HashSet<Id>) -> Vec<(Id, String)> {
        let buildable = Self::geometry_check_nodes(&self.p);
        let mut refused = Vec::new();
        for node in self.p.timeline.iter().filter(|n| buildable.contains(&n.id) && rebuilt.contains(&n.id)) {
            let FeatureKind::SplitFace { src, body, .. } = &node.kind else { continue };
            // Our own refusals are `Kernel` errors; any other mark is the app's own failure, with the source passed through.
            if self.p.regen_errors.get(&node.id).is_some_and(|e| !matches!(e, CoreError::Kernel(_))) {
                continue;
            }
            let (Some(source), Some(result)) = (self.p.regen_faces.get(src), self.p.regen_faces.get(body)) else { continue };
            // A node that was split before can be got past by suppressing or deleting it; a node being created is rolled back instead.
            let exit = if before.contains_key(&node.id) { format!(" {EXIT}") } else { String::new() };
            if let Some(message) = self.split_face_fault(node, source.len(), result) {
                refused.push((node.id, format!("{message}{exit}")));
                continue;
            }
            let Some(old) = before.get(&node.id) else { continue };
            if old.pieces.is_none() && self.establishing_topology {
                continue;
            }
            if let Some(message) = self.changed_pieces(node, old, *src, *body) {
                refused.push((node.id, format!("{message}{exit}")));
            }
        }
        refused
    }

    /// F-190, F-189, F-187 on one node's result, in that order: the text of the first that holds.
    fn split_face_fault(&self, node: &FeatureNode, source_faces: usize, result: &[qymcad_core::geom::MeshFace]) -> Option<String> {
        let FeatureKind::SplitFace { face, .. } = &node.kind else { return None };
        if let Some((plane_body, key)) = face {
            if self.p.regen_faces.get(plane_body).is_some_and(|fs| !fs.iter().any(|f| f.id == key.id)) {
                return Some(format!(
                    "the face {} of body {plane_body} that gives the cutting plane of this split is gone, and the app would keep cutting where the plane was made without a word. Recreate the split with a base plane, a datum plane or another face (F-190).",
                    key.id
                ));
            }
        }
        let distinct: HashSet<u32> = result.iter().map(|f| f.id).collect();
        if distinct.len() < result.len() {
            return Some(format!(
                "the split leaves {} faces with only {} distinct ids, so one face id would name two faces and a later tool, selection or export could take either. The app does this when a split crosses an earlier split of the same body, and for parallel splits after the first two or three (fewer in some directions); the next split must be a separate body: use `split` for that (F-189).",
                result.len(),
                distinct.len()
            ));
        }
        if result.len() <= source_faces {
            return Some(format!(
                "the split divided no face: the body has {source_faces} faces before and {} after. A plane within about 1e-5 mm of a face, or through the axis of a torus, passes the app's own check and reports success while it splits nothing; near a face it also shrinks the body. Move the plane at least 1e-4 mm off the face or choose another plane (F-187).",
                result.len()
            ));
        }
        None
    }

    /// The faces this node cut, before the rebuild against after: the set of cut ids must be the same and every cut id must stay on its
    /// piece, also when the whole cut moved with its plane. The app lets a new offset cut another set of faces; the old ids then point at
    /// other pieces without a word (F-188). Faces the node did not cut are the business of the nodes above it.
    fn changed_pieces(&self, node: &FeatureNode, old: &Witness, src: Id, body: Id) -> Option<String> {
        let (Some(old_pieces), Some(new)) = (old.pieces.as_ref(), self.split_face_pieces(src, body)) else {
            return Some("the pieces this split cut cannot be measured, so nothing proves that its face ids stay on their pieces; edit refused (F-188).".into());
        };
        let was: HashSet<u32> = old_pieces.iter().filter(|p| p.cut).map(|p| p.id).collect();
        let now: HashSet<u32> = new.iter().filter(|p| p.cut).map(|p| p.id).collect();
        let mut gone: Vec<u32> = was.difference(&now).copied().collect();
        let mut fresh: Vec<u32> = now.difference(&was).copied().collect();
        gone.sort_unstable();
        fresh.sort_unstable();
        // The cut moved with its plane (a plane that follows a face of a body that grew): compare also after moving the old boxes by it.
        let shift = old.origin.zip(self.split_face_origin(node)).map(|(a, b)| std::array::from_fn(|i| b[i] - a[i]));
        let mut moved_ids: Vec<u32> = old_pieces
            .iter()
            .filter_map(|o| {
                let n = new.iter().find(|n| n.id == o.id)?;
                let same = !(o.cut || n.cut)
                    || same_piece(&o.bbox, &n.bbox)
                    || shift.is_some_and(|by: [f64; 3]| same_piece(&moved(&o.bbox, by), &n.bbox));
                (!same).then_some(o.id)
            })
            .collect();
        moved_ids.sort_unstable();
        if gone.is_empty() && fresh.is_empty() && moved_ids.is_empty() {
            return None;
        }
        let mut what = Vec::new();
        if !gone.is_empty() {
            what.push(format!("face ids {gone:?} are no longer cut"));
        }
        if !fresh.is_empty() {
            what.push(format!("face ids {fresh:?} are cut now"));
        }
        if !moved_ids.is_empty() {
            what.push(format!("face ids {moved_ids:?} move to another piece"));
        }
        Some(format!(
            "this edit changes the faces the split cuts ({}): the ids kept would silently name other pieces for every tool that uses them, and the app says nothing. Edit refused; recreate the split for this change and read topology again (F-188).",
            what.join("; ")
        ))
    }

    /// The app's own failure of a split plane that no longer meets the body is a bare `operation failed`: say what happened and what to do.
    pub(crate) fn split_face_hints(&self, errors: &mut [NodeIssue]) {
        for issue in errors.iter_mut() {
            let Some(node) = self.p.timeline.iter().find(|n| n.id == issue.node && matches!(n.kind, FeatureKind::SplitFace { .. })) else {
                continue;
            };
            if matches!(self.p.regen_errors.get(&node.id), Some(CoreError::OpFailed(Op::SplitFaces))) {
                let at = self
                    .split_face_origin(node)
                    .map(|o| format!(" The plane now passes through ({:.4}, {:.4}, {:.4}) in the Part's local frame.", o[0], o[1], o[2]))
                    .unwrap_or_default();
                issue.message.push_str(&format!(
                    ". The cutting plane no longer meets the body, or lies on one of its faces.{at} Undo this edit, or give the offset a value inside the body's extent (doc_info shows its bounds)."
                ));
            }
        }
    }

    /// A file the app made can hold a Split faces node that is already wrong (F-187, F-189, F-190). Its stored geometry is judged here, also
    /// for nodes the open did not rebuild; the file is not changed. Nodes whose topology cannot be read, or that are wrong, get a witness that
    /// refuses every edit rebuilding them: the way out is to suppress or delete the node.
    pub(crate) fn split_face_open_check(&mut self, checked: &HashSet<Id>) -> Vec<NodeIssue> {
        let mut warnings = Vec::new();
        let nodes: Vec<FeatureNode> = self.p.timeline.iter().filter(|n| matches!(n.kind, FeatureKind::SplitFace { .. })).cloned().collect();
        if nodes.is_empty() {
            return warnings;
        }
        let topology = self.ensure_topology();
        let buildable = Self::geometry_check_nodes(&self.p);
        for node in nodes {
            let FeatureKind::SplitFace { src, body, .. } = &node.kind else { continue };
            let unverifiable = Witness { pieces: None, origin: None };
            if !buildable.contains(&node.id) {
                // Suppressed or past the bar: nothing to read now; the first edit that builds it has nothing to compare with.
                self.split_face_witness.entry(node.id).or_insert(unverifiable);
                continue;
            }
            if let Err(e) = &topology {
                // One word for the whole file, on its first Split faces node.
                if warnings.is_empty() {
                    warnings.push(self.issue_for(&node, format!("the faces of the Split faces nodes cannot be read from the file, so what they cut cannot be checked: {e}. Edits that rebuild them are refused. {EXIT}")));
                }
                self.split_face_witness.insert(node.id, unverifiable);
                continue;
            }
            let (Some(source), Some(result)) = (self.p.regen_faces.get(src), self.p.regen_faces.get(body)) else {
                self.split_face_witness.insert(node.id, unverifiable);
                continue;
            };
            let native_error = self.p.regen_errors.contains_key(&node.id) || checked.contains(&node.id);
            if native_error {
                continue;
            }
            if let Some(message) = self.split_face_fault(&node, source.len(), result) {
                warnings.push(self.issue_for(&node, format!("this split is already wrong in the file: {message} {EXIT}")));
                self.split_face_witness.insert(node.id, unverifiable);
                continue;
            }
            let (pieces, origin) = (self.split_face_pieces(*src, *body), self.split_face_origin(&node));
            self.split_face_witness.insert(node.id, Witness { pieces, origin });
        }
        for w in &warnings {
            self.advisory_warnings.push(AdvisoryWarning::Geometry(w.clone()));
        }
        warnings
    }

    fn issue_for(&self, node: &FeatureNode, message: String) -> NodeIssue {
        NodeIssue { node: node.id, name: self.node_name(node.id), message }
    }
}

#[cfg(test)]
mod tests {
    use super::{same_piece, SplitFaceOptions};
    use crate::{BaseName, Num, Placement, PlaneRef, PrimitiveSpec, Session};

    #[test]
    fn a_sheet_body_is_refused_with_the_document_unchanged() {
        let mut s = Session::new_part();
        let spec = PrimitiveSpec::Box { dx: Num::Value(10.0), dy: Num::Value(10.0), dz: Num::Value(10.0) };
        let body = s.primitive(&spec, &Placement::Origin, None).unwrap().0.body;
        s.p.bodies.iter_mut().filter(|b| b.id == body).for_each(|b| b.sheet = true);
        let before = ron::to_string(&s.p).unwrap();
        let options = SplitFaceOptions { body: Some(body), plane: PlaneRef::Base(BaseName::XY), offset: Num::Value(5.0), name: None };
        let e = s.split_face(&options).unwrap_err().to_string();
        assert!(e.contains("is a sheet"), "{e}");
        assert!(ron::to_string(&s.p).unwrap() == before);
    }

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
