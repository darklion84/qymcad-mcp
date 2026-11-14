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

/// One face of a Split faces result, as the rebuild left it.
#[derive(Clone)]
struct Piece {
    id: u32,
    centroid: [f64; 3],
    normal: [f64; 3],
    /// The source has no face of this id and area: this node cut it (or made it). Not told apart reliably means cut.
    cut: bool,
}

/// What a Split faces node last cut that was accepted: its faces and the cutting plane (a point and the unit normal).
/// `pieces` is `None` when nothing could be measured (a file whose topology cannot be read, a node that cannot be built to measure):
/// an edit that rebuilds the node is then refused, since nothing proves its ids stay on their pieces.
#[derive(Clone)]
pub(crate) struct Witness {
    pieces: Option<Vec<Piece>>,
    plane: Option<([f64; 3], [f64; 3])>,
}

/// The last accepted state of the nodes whose ids another tool can hold: the pieces of every Split faces node, and the bounds of the
/// outputs of every Split body (F-100). Kept in the undo snapshot and rolled back with a refused edit.
#[derive(Clone, Default)]
pub(crate) struct Witnesses {
    pub(crate) faces: HashMap<Id, Witness>,
    pub(crate) splits: HashMap<Id, Vec<(Id, [f64; 6])>>,
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}

type Plane = ([f64; 3], [f64; 3]);

/// Which side of the plane a point lies on: 1, -1, or 0 when it is on it (the pieces of a cut face lie on both sides).
fn side(plane: Option<Plane>, at: [f64; 3]) -> i8 {
    plane.map_or(0, |(p, n)| {
        let d = dot(std::array::from_fn(|i| at[i] - p[i]), n);
        if d.abs() <= 1e-6 {
            0
        } else {
            d.signum() as i8
        }
    })
}

/// The ids among `pairs` (old, new, one id each) that moved. A piece stays on its side of the cutting plane (the plane as it was and as it
/// is now); among the pieces on that side with a similar normal, the new one must lie clearly nearer to its own old one than to any other
/// one, once the old ones are moved by `shift`. A piece alone on its side with its normal has nothing to swap with, so a body that is
/// resized far past its pieces' size keeps its ids.
fn displaced(pairs: &[(&Piece, &Piece)], old_cut: &[&Piece], shift: [f64; 3], planes: (Option<Plane>, Option<Plane>)) -> Vec<u32> {
    pairs
        .iter()
        .filter(|(o, n)| {
            let on = side(planes.0, o.centroid);
            if on != side(planes.1, n.centroid) {
                return true;
            }
            let own = dist(add(o.centroid, shift), n.centroid);
            let other = old_cut
                .iter()
                .filter(|q| q.id != o.id && dot(q.normal, n.normal) > 0.99 && side(planes.0, q.centroid) == on)
                .map(|q| dist(add(q.centroid, shift), n.centroid))
                .fold(f64::INFINITY, f64::min);
            // Clearly nearer, with a margin: a tie is a piece that may stand on the other one.
            !(own <= 1e-6 || own < 0.9 * other)
        })
        .map(|(o, _)| o.id)
        .collect()
}

/// The cut ids that moved to another piece (see `displaced`). Where the old piece was is judged in the frame of the plane's movement
/// (the part of it along the normal: a plane that follows a face of a body that grew takes its cut along) or, failing that, in the frame
/// of the mean movement of the cut pieces (the whole body was moved by a parameter, the plane was not).
fn moved_ids(old: &[Piece], new: &[Piece], plane_shift: [f64; 3], planes: (Option<Plane>, Option<Plane>)) -> Vec<u32> {
    let pairs: Vec<(&Piece, &Piece)> =
        old.iter().filter_map(|o| new.iter().find(|n| n.id == o.id).filter(|n| o.cut || n.cut).map(|n| (o, n))).collect();
    if pairs.is_empty() {
        return Vec::new();
    }
    let old_cut: Vec<&Piece> = old.iter().filter(|o| o.cut).collect();
    let mean: [f64; 3] =
        std::array::from_fn(|i| pairs.iter().map(|(o, n)| n.centroid[i] - o.centroid[i]).sum::<f64>() / pairs.len() as f64);
    let first = displaced(&pairs, &old_cut, plane_shift, planes);
    if first.is_empty() || displaced(&pairs, &old_cut, mean, planes).is_empty() {
        return Vec::new();
    }
    first
}

const EXIT_FAULT: &str = "To go on, feature_suppress or feature_delete this node.";
const EXIT_EDIT: &str = "To go on, put the edit back, or recreate the split; feature_delete removes the node.";

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

    /// The faces of a Split faces result as they stand: centre, normal and whether this node cut them.
    fn split_face_pieces(&self, src: Id, body: Id) -> Option<Vec<Piece>> {
        let source = self.p.regen_faces.get(&src)?;
        let faces = self.p.regen_faces.get(&body)?;
        Some(
            faces
                .iter()
                .map(|f| Piece {
                    id: f.id,
                    centroid: [f.centroid.x, f.centroid.y, f.centroid.z],
                    normal: f.normal,
                    // A face that keeps its id and its area to 1e-6 is the source's; any doubt is a cut face, compared more strictly.
                    cut: !source.iter().any(|s| s.id == f.id && (s.area - f.area).abs() <= 1e-6 * s.area.abs().max(1e-9)),
                })
                .collect(),
        )
    }

    /// The node's cutting plane in the body's local frame: a point on it (the offset applied) and its unit normal.
    fn split_face_plane(&self, node: &FeatureNode) -> Option<([f64; 3], [f64; 3])> {
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
        (m > 1e-12 && origin.iter().all(|v| v.is_finite()))
            .then(|| (std::array::from_fn(|i| origin[i] + offset * normal[i] / m), normal.map(|v| v / m)))
    }

    /// After a rebuild: remember the pieces of every Split faces node that was built clean and really split something, and the bounds
    /// of the outputs of every Split body that was built clean, so that a later rebuild is judged against them also when the node is
    /// not built in between (suppressed, past the rollback bar). A node that failed or was refused keeps its earlier witness.
    pub(crate) fn accept_split_faces(&mut self, rebuilt: &HashSet<Id>, failed: &HashSet<Id>) {
        let buildable = Self::geometry_check_nodes(&self.p);
        let mut accepted = Vec::new();
        let mut splits = Vec::new();
        for node in &self.p.timeline {
            if !buildable.contains(&node.id)
                || !rebuilt.contains(&node.id)
                || failed.contains(&node.id)
                || self.p.regen_errors.contains_key(&node.id)
            {
                continue;
            }
            match &node.kind {
                FeatureKind::SplitFace { src, body, .. } => {
                    let (Some(source), Some(result)) = (self.p.regen_faces.get(src), self.p.regen_faces.get(body)) else { continue };
                    if result.len() > source.len() {
                        accepted
                            .push((node.id, Witness { pieces: self.split_face_pieces(*src, *body), plane: self.split_face_plane(node) }));
                    }
                }
                FeatureKind::SplitBody { .. } => {
                    if let Some(pieces) = self.split_piece_bounds(node) {
                        splits.push((node.id, pieces));
                    }
                }
                _ => {}
            }
        }
        self.split_face_witness.faces.extend(accepted);
        self.split_face_witness.splits.extend(splits);
        let live: HashSet<Id> = self.p.timeline.iter().map(|n| n.id).collect();
        self.split_face_witness.faces.retain(|id, _| live.contains(id));
        self.split_face_witness.splits.retain(|id, _| live.contains(id));
    }

    /// `part_copy` makes new nodes whose faces have ids of their own, so the copy's witness is not the source's renamed: the rebuild of
    /// the copy has already recorded its pieces. A copied node that was not built (suppressed, past the bar) is measured on a copy of
    /// the document with the node active.
    pub(crate) fn copy_split_face_witness(&mut self, nodes: &[(Id, Id)]) {
        for (from, to) in nodes {
            if self.split_face_witness.faces.contains_key(from) && !self.split_face_witness.faces.contains_key(to) {
                let measured = self.measure_unbuilt_split_face(*to);
                self.split_face_witness.faces.insert(*to, measured);
            }
        }
    }

    /// What `read` finds in the document when `node` is built: measured on a copy where the node is active (unsuppressed, the rollback
    /// bar at the end); the document is restored whatever happens. `None` when it cannot be built there or `read` finds nothing.
    fn measured_when_active<T>(&mut self, node: Id, read: impl FnOnce(&Session) -> Option<T>) -> Option<T> {
        let found = std::cell::Cell::new(None);
        let nodes: HashSet<Id> = self.p.timeline.iter().map(|n| n.id).collect();
        let before = self.p.clone();
        self.establishing_topology = true;
        let _ = self.with_rebuild_copies(before, &nodes, "measuring a node that is not built", |s| {
            if let Some(ti) = s.p.timeline.iter().position(|n| n.id == node) {
                s.p.set_feature_suppressed(ti, false);
                s.p.set_rollback(None);
            }
            if s.rebuild_for_edit(Some(&nodes))?.errors.is_empty() {
                found.set(read(s));
            }
            // Never keep the measuring: the closure fails on purpose and the document goes back as it was.
            Err::<(), _>(Error::Invalid("measured".into()))
        });
        self.establishing_topology = false;
        found.take()
    }

    /// The pieces of a Split faces node that is not built now, measured when it is active. A node that cannot be built there is faulty
    /// when it is active, and gets a witness with no pieces.
    fn measure_unbuilt_split_face(&mut self, node: Id) -> Witness {
        let read = |s: &Session| {
            let n = s.p.timeline.iter().find(|n| n.id == node)?;
            let FeatureKind::SplitFace { src, body, .. } = &n.kind else { return None };
            let (source, faces) = (s.p.regen_faces.get(src)?.len(), s.p.regen_faces.get(body)?.len());
            (faces > source).then(|| Witness { pieces: s.split_face_pieces(*src, *body), plane: s.split_face_plane(n) })
        };
        self.measured_when_active(node, read).unwrap_or(Witness { pieces: None, plane: None })
    }

    /// The outputs of a Split body that is not built now (past the rollback bar), measured when it is active.
    fn measure_unbuilt_split_body(&mut self, node: Id) -> Option<Vec<(Id, [f64; 6])>> {
        self.measured_when_active(node, |s| s.split_piece_bounds(s.p.timeline.iter().find(|n| n.id == node)?))
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
            let existing = before.faces.contains_key(&node.id);
            if let Some(message) = self.split_face_fault(node, source.len(), result) {
                refused.push((node.id, if existing { format!("{message} {EXIT_FAULT}") } else { message }));
                continue;
            }
            let Some(old) = before.faces.get(&node.id) else { continue };
            if old.pieces.is_none() && self.establishing_topology {
                continue;
            }
            if let Some(message) = self.changed_pieces(node, old, *src, *body) {
                refused.push((node.id, format!("{message} {EXIT_EDIT}")));
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
                "the split leaves {} faces with only {} distinct ids, so one face id would name two faces and a later tool, selection or export could take either. The app does this when a split crosses an earlier split of the same body, and after a few parallel splits (on a block between 1 and 4 in a row, by axis and direction); the next split must be a separate body: use `split` for that (F-189).",
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
    /// piece (see `moved_ids`). The app lets a new offset cut another set of faces; the old ids then point at other pieces without a
    /// word (F-188). Faces the node did not cut are the business of the nodes above it.
    fn changed_pieces(&self, node: &FeatureNode, old: &Witness, src: Id, body: Id) -> Option<String> {
        let (Some(old_pieces), Some(new)) = (old.pieces.as_ref(), self.split_face_pieces(src, body)) else {
            return Some(
                "the pieces this split cut are not known, so nothing proves that its face ids stay on their pieces; edit refused (F-188)."
                    .into(),
            );
        };
        let was: HashSet<u32> = old_pieces.iter().filter(|p| p.cut).map(|p| p.id).collect();
        let now: HashSet<u32> = new.iter().filter(|p| p.cut).map(|p| p.id).collect();
        let mut gone: Vec<u32> = was.difference(&now).copied().collect();
        let mut fresh: Vec<u32> = now.difference(&was).copied().collect();
        gone.sort_unstable();
        fresh.sort_unstable();
        // The plane's own movement along its normal only: a movement inside the plane is the body's, not the cut's.
        let shift = old.plane.zip(self.split_face_plane(node)).map_or([0.0; 3], |((a, _), (b, n))| {
            let along = dot(std::array::from_fn(|i| b[i] - a[i]), n);
            n.map(|v| v * along)
        });
        let mut moved = moved_ids(old_pieces, &new, shift, (old.plane, self.split_face_plane(node)));
        moved.sort_unstable();
        if gone.is_empty() && fresh.is_empty() && moved.is_empty() {
            return None;
        }
        let mut what = Vec::new();
        if !gone.is_empty() {
            what.push(format!("face ids {gone:?} are no longer cut"));
        }
        if !fresh.is_empty() {
            what.push(format!("face ids {fresh:?} are cut now"));
        }
        if !moved.is_empty() {
            what.push(format!("face ids {moved:?} move to another piece"));
        }
        Some(format!(
            "this edit changes the faces the split cuts ({}): the ids kept would silently name other pieces for every tool that uses them, and the app says nothing. The edit was not applied; the document is unchanged (F-188).",
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
                    .split_face_plane(node)
                    .map(|(o, _)| {
                        format!(
                            " After the edit the plane would pass through ({:.4}, {:.4}, {:.4}) in the Part's local frame.",
                            o[0], o[1], o[2]
                        )
                    })
                    .unwrap_or_default();
                issue.message.push_str(&format!(
                    ". The cutting plane would no longer meet the body, or would lie on one of its faces.{at} The edit was not applied and the document is unchanged; choose a value that keeps the plane inside the body (doc_info shows its bounds)."
                ));
            }
        }
    }

    /// A file the app made can hold a Split faces node that is already wrong (F-187, F-189, F-190). Its stored geometry is judged here, also
    /// for nodes the open did not rebuild; the file is not changed. Nodes that are wrong, or whose topology cannot be read, get a witness
    /// with no pieces that refuses every edit rebuilding them: the way out is to suppress or delete the node. A node that is not built
    /// (suppressed, past the rollback bar) is measured by building it on a copy where it is active. The outputs of every Split body are
    /// recorded as well (F-100).
    pub(crate) fn split_face_open_check(&mut self, checked: &HashSet<Id>) -> Vec<NodeIssue> {
        let mut warnings = Vec::new();
        let nodes: Vec<FeatureNode> = self
            .p
            .timeline
            .iter()
            .filter(|n| matches!(n.kind, FeatureKind::SplitFace { .. } | FeatureKind::SplitBody { .. }))
            .cloned()
            .collect();
        if nodes.is_empty() {
            return warnings;
        }
        let topology = self.ensure_topology();
        let buildable = Self::geometry_check_nodes(&self.p);
        for node in nodes {
            if let FeatureKind::SplitBody { .. } = &node.kind {
                if buildable.contains(&node.id) {
                    if let Some(pieces) = self.split_piece_bounds(&node) {
                        self.split_face_witness.splits.insert(node.id, pieces);
                    }
                } else if let Some(pieces) = self.measure_unbuilt_split_body(node.id) {
                    self.split_face_witness.splits.insert(node.id, pieces);
                }
                continue;
            }
            let FeatureKind::SplitFace { src, body, .. } = &node.kind else { continue };
            let unverifiable = || Witness { pieces: None, plane: None };
            if !buildable.contains(&node.id) {
                let measured = self.measure_unbuilt_split_face(node.id);
                self.split_face_witness.faces.entry(node.id).or_insert(measured);
                continue;
            }
            if let Err(e) = &topology {
                // One word for the whole file, on its first Split faces node.
                if warnings.is_empty() {
                    let cause = match e {
                        Error::Rebuild(lines) => lines.iter().skip(1).cloned().collect::<Vec<_>>().join("; "),
                        other => other.to_string(),
                    };
                    warnings.push(self.issue_for(
                        &node,
                        format!("the faces of the Split faces nodes cannot be read from the file, so what they cut cannot be checked: {}. Edits that rebuild them are refused. {EXIT_FAULT}", cause.trim_end_matches('.')),
                    ));
                }
                self.split_face_witness.faces.insert(node.id, unverifiable());
                continue;
            }
            let (Some(source), Some(result)) = (self.p.regen_faces.get(src), self.p.regen_faces.get(body)) else {
                self.split_face_witness.faces.insert(node.id, unverifiable());
                continue;
            };
            // A node the app failed is judged as a creation once an edit makes it build: there is nothing to compare it with.
            if self.p.regen_errors.contains_key(&node.id) || checked.contains(&node.id) {
                continue;
            }
            if let Some(message) = self.split_face_fault(&node, source.len(), result) {
                warnings.push(self.issue_for(&node, format!("this split is already wrong in the file: {message} {EXIT_FAULT}")));
                self.split_face_witness.faces.insert(node.id, unverifiable());
                continue;
            }
            let witness = Witness { pieces: self.split_face_pieces(*src, *body), plane: self.split_face_plane(&node) };
            self.split_face_witness.faces.insert(node.id, witness);
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
    use super::{moved_ids, Piece, SplitFaceOptions};
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
    fn a_cut_id_stays_when_its_piece_is_clearly_nearest_after_the_planes_movement() {
        let piece = |id, z: f64| Piece { id, centroid: [0.0, 0.0, z], normal: [1.0, 0.0, 0.0], cut: true };
        let old = [piece(1, 2.5), piece(2, 7.5)];
        let flat = (Some(([0.0, 0.0, 5.0], [0.0, 0.0, 1.0])), Some(([0.0, 0.0, 5.0], [0.0, 0.0, 1.0])));
        // The lower piece grew and the upper moved: each is still nearest to itself.
        assert!(moved_ids(&old, &[piece(1, 4.0), piece(2, 9.0)], [0.0; 3], flat).is_empty());
        // The ids swapped places.
        assert_eq!(moved_ids(&old, &[piece(1, 7.5), piece(2, 2.5)], [0.0; 3], flat).len(), 2);
        // The plane went up by 4 and took its cut with it: the same pieces, 4 higher.
        assert!(moved_ids(&old, &[piece(1, 6.5), piece(2, 11.5)], [0.0, 0.0, 4.0], (flat.0, Some(([0.0, 0.0, 9.0], [0.0, 0.0, 1.0]))))
            .is_empty());
        // The whole body went 100 away and the plane did not: the frame of the mean movement finds the pieces again.
        let sideways = |id, z: f64| Piece { centroid: [100.0, 0.0, z], ..piece(id, z) };
        assert!(moved_ids(&old, &[sideways(1, 2.5), sideways(2, 7.5)], [0.0; 3], flat).is_empty());
        // A tie is no proof: the new piece is as near to the other one.
        assert!(!moved_ids(&old, &[piece(1, 5.0), piece(2, 5.0)], [0.0; 3], (None, None)).is_empty());
    }
}
