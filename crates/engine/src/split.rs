//! Fixed-count native body splitting and native output-body names.
use crate::{BaseName, BodyInfo, Element, Error, Id, Num, PlaneRef, Rebuild, Result, Sel, Session};
use qymcad_core::{
    feature::{FaceKey, PlaneAt},
    model::{FeatNumber, PlainNumber, Project},
};
use serde::Serialize;
use std::collections::HashSet;

/// Arguments for a fixed-count split of one current body.
#[derive(Clone, Debug)]
pub struct SplitOptions {
    /// Stock body; defaults to the current tip. Use an explicit body after splitting.
    pub body: Option<Id>,
    /// Base plane, datum plane or planar body face.
    pub plane: PlaneRef,
    /// Signed offset, mm: XY +Z, XZ +Y, YZ +X; datum/face along its own normal.
    pub offset: Num,
    /// Timeline feature name.
    pub name: Option<String>,
    /// One distinct native body name per measured piece; defaults to feature name plus the next unused positive numbers.
    pub piece_names: Option<Vec<String>>,
}

/// Split feature, its independently addressable pieces and rebuild diagnostics.
#[derive(Debug, Serialize)]
pub struct SplitResult {
    /// First output body; also the feature id for existing creation/history clients.
    pub body: Id,
    /// Timeline feature id.
    pub feature: Id,
    /// Feature name.
    pub name: String,
    /// Output bodies in the native plane-normal ordering.
    pub pieces: Vec<BodyInfo>,
    /// Evaluated signed plane offset, mm.
    pub offset: f64,
    /// Current document bodies and rebuild diagnostics.
    pub rebuild: Rebuild,
}

/// What an operation that cuts by a plane stores about it: the same three references as the app's Split body and Split faces
/// (a base plane code, a datum plane, or a face), and the plane they stand for in the body's local frame.
pub(crate) struct OpPlane {
    /// Base plane code of the node: 0 XY, 1 XZ, 2 YZ (0 as well for a datum or a face).
    pub(crate) code: u8,
    /// Datum plane id, 0 when the plane is a base plane or a face.
    pub(crate) datum: Id,
    /// The face the plane is taken from, with the witness the node stores.
    pub(crate) face: Option<(Id, FaceKey)>,
    origin: [f64; 3],
    /// Unit normal; the offset runs along it.
    normal: [f64; 3],
}

impl OpPlane {
    /// The plane shifted by `offset` mm along its normal.
    pub(crate) fn at(&self, offset: f64) -> PlaneAt {
        PlaneAt { origin: std::array::from_fn(|i| self.origin[i] + offset * self.normal[i]), normal: self.normal }
    }

    /// Store the face reference on a node the app's adder made (the datum is cleared by the app's own setter).
    pub(crate) fn attach(&self, p: &mut Project, node: Id) {
        if let Some((body, key)) = &self.face {
            p.set_op_face(node, *body, *key);
        }
    }
}

impl Session {
    /// The one place that turns a plane reference into what the nodes store and the plane they mean: base plane (XZ offsets
    /// along +Y, as `BasePlane::offset_direction`), datum plane, or exactly one planar face. A curved face is refused: the app
    /// would build its plane from the face's centre and an averaged normal (F-191).
    pub(crate) fn resolve_op_plane(&self, plane: &PlaneRef) -> Result<OpPlane> {
        let (code, datum, face, origin, normal) = match plane {
            PlaneRef::Base(base) => {
                let (code, normal) = match base {
                    BaseName::XY => (0, [0.0, 0.0, 1.0]),
                    BaseName::XZ => (1, [0.0, 1.0, 0.0]),
                    BaseName::YZ => (2, [1.0, 0.0, 0.0]),
                };
                (code, 0, None, [0.0; 3], normal)
            }
            PlaneRef::Plane(id) => {
                let p = self.p.planes.iter().find(|p| p.id == *id).ok_or_else(|| Error::NotFound(format!("datum plane {id}")))?;
                (0, *id, None, p.origin, p.normal)
            }
            PlaneRef::Face { body, face } => {
                self.resolve_sel(*body, Element::Faces, &Sel::Ids(vec![*face]))?;
                if !self.face_is_planar(*body, *face) {
                    return Err(Error::Invalid(format!(
                        "plane face {face} of body {body} is not planar: the app would cut by a plane built from the curved face's centre and an averaged normal, which is not the surface (F-191); use a base plane, a datum plane or a planar face"
                    )));
                }
                let f = self
                    .p
                    .regen_faces
                    .get(body)
                    .into_iter()
                    .flatten()
                    .find(|f| f.id == *face)
                    .ok_or_else(|| Error::NotFound(format!("face {face} of body {body}")))?;
                let key = crate::topology::face_key(f);
                let (origin, normal) = self.p.resolve_face(*body, &key);
                (0, 0, Some((*body, key)), origin, normal)
            }
        };
        let magnitude = normal.iter().map(|v| v * v).sum::<f64>().sqrt();
        if !magnitude.is_finite() || magnitude < 1e-12 || origin.iter().any(|v| !v.is_finite()) {
            return Err(Error::Invalid("split plane needs a finite origin and nonzero finite normal".into()));
        }
        Ok(OpPlane { code, datum, face, origin, normal: normal.map(|v| v / magnitude) })
    }

    /// Split a current body by a plane, fixing the measured output count and native piece ids at creation.
    /// Later count-changing edits fail through the normal atomic rebuild path; recreate the split to change count.
    /// Precondition: the caller must not hold the kernel gate (`ask::split_pieces` acquires it).
    pub fn split(&mut self, options: &SplitOptions) -> Result<SplitResult> {
        self.ensure_topology()?;
        let ((ids, names, offset), mut rebuild) = self.atomic(|s| {
            let src = s.source_body(options.body)?;
            let offset = options.offset.eval(&s.p.param_map())?;
            let op = s.resolve_op_plane(&options.plane)?;
            let at = op.at(offset);
            let shape = s.shapes.get(&src).ok_or_else(|| Error::NotFound(format!("body {src} has no built shape")))?;
            // This public question copies the source and owns its gate. Never wrap it in an outer gate.
            let count = qymcad_kernel::ask::split_pieces(shape, at)
                .filter(|n| *n >= 2)
                .ok_or_else(|| Error::Invalid("split plane cuts nothing: it must divide the body into at least two pieces".into()))?;
            let ids = s.in_component(s.p.body_owner(src), |p| p.add_split_body(src, op.code, op.datum, offset, count));
            let feature = ids[0]; // count>=2 checked before the native adder's assertion.
            op.attach(&mut s.p, feature);
            let text = options.offset.expr().unwrap_or_else(|| offset.to_string());
            s.p.write_feat_formula(&FeatNumber { node: feature, key: "offset" }, &text, PlainNumber::Stands)
                .map_err(|e| Error::Expr(format!("split offset: {e:?}")))?;
            s.set_node_name(feature, options.name.as_deref())?;
            let names = s.piece_names(feature, count, options.piece_names.as_deref())?;
            Ok((ids, names, offset))
        })?;
        // Rebuild has now created every native mesh. Names live on Body, not on the feature node.
        for (&id, name) in ids.iter().zip(names) {
            if let Some(mi) = self.p.mesh_index(id) {
                self.p.set_mesh_name(mi, name);
            }
        }
        rebuild.bodies = self.result_bodies();
        let pieces = rebuild.bodies.iter().filter(|b| ids.contains(&b.id)).cloned().collect();
        let feature = ids[0];
        Ok(SplitResult { body: feature, feature, name: self.node_name(feature), pieces, offset, rebuild })
    }
}

/// Native sorting does not preserve identities of disconnected pieces on the same side (F-100).
/// Keep conservative local B-rep boxes, including consumed outputs, before any regeneration.
pub(crate) struct SplitIdentity {
    node: Id,
    pieces: Vec<(Id, [f64; 6])>,
}

fn overlap(a: &[f64; 6], b: &[f64; 6]) -> f64 {
    (0..3).map(|i| (a[i + 3].min(b[i + 3]) - a[i].max(b[i])).max(0.0)).product()
}

impl Session {
    pub(crate) fn split_identities(&self) -> Vec<SplitIdentity> {
        let _gate = qymcad_kernel::kernel_gate();
        self.p
            .timeline
            .iter()
            .filter(|n| !n.suppressed)
            .filter_map(|n| {
                if !matches!(n.kind, qymcad_core::feature::FeatureKind::SplitBody { .. }) {
                    return None;
                }
                // Creation and missing-B-rep recovery have no complete previous geometry to match.
                let pieces = n.kind.bodies().into_iter().map(|id| Some((id, self.shapes.get(&id)?.bbox()?))).collect::<Option<Vec<_>>>()?;
                Some(SplitIdentity { node: n.id, pieces })
            })
            .collect()
    }

    pub(crate) fn split_identity_refusals(&self, before: &[SplitIdentity], rebuilt: &HashSet<Id>) -> Vec<(Id, String)> {
        let mut errors = Vec::new();
        for split in before.iter().filter(|s| s.pieces.iter().any(|(id, _)| rebuilt.contains(id))) {
            let after = {
                let _gate = qymcad_kernel::kernel_gate();
                split.pieces.iter().map(|(id, _)| self.shapes.get(id).and_then(|sh| sh.bbox())).collect::<Option<Vec<_>>>()
            };
            let Some(after) = after else {
                errors.push((split.node, "split piece identity cannot be verified: a rebuilt piece has no bounds; edit refused".into()));
                continue;
            };
            let scores: Vec<Vec<f64>> = split.pieces.iter().map(|(_, old)| after.iter().map(|new| overlap(old, new)).collect()).collect();
            let tolerances: Vec<f64> = split
                .pieces
                .iter()
                .zip(&after)
                .map(|((_, old), new)| (overlap(old, old).max(overlap(new, new)) * 1e-8).max(1e-9))
                .collect();
            let changed = uncertain_piece_assignment(&scores, &tolerances);
            if !changed.is_empty() {
                let names = changed
                    .iter()
                    .map(|&i| {
                        let id = split.pieces[i].0;
                        format!("`{}` ({id})", self.body_name(id))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                errors.push((split.node, format!("split piece identity would change or is ambiguous for {names}: native ids do not give the unique maximum bounding-box overlap; edit refused atomically; recreate the split for this change")));
            }
        }
        errors
    }
}

/// Certify the native assignment instead of greedily choosing a match per row. Every alternative
/// permutation decomposes into cycles: gain(i,j)=overlap(i,j)-overlap(i,i). A nonnegative cycle
/// therefore means a better match or an ambiguous one. Max-plus Floyd-Warshall finds all such cycles.
/// Each row contributes its own piece's tolerance, so a cycle's allowance sums only its pieces.
pub(crate) fn uncertain_piece_assignment(scores: &[Vec<f64>], tolerances: &[f64]) -> Vec<usize> {
    let count = scores.len();
    let mut gain: Vec<Vec<f64>> = scores
        .iter()
        .enumerate()
        .map(|(i, row)| {
            row.iter().enumerate().map(|(j, score)| if i == j { f64::NEG_INFINITY } else { score - row[i] + tolerances[i] }).collect()
        })
        .collect();
    for k in 0..count {
        for i in 0..count {
            for j in 0..count {
                gain[i][j] = gain[i][j].max(gain[i][k] + gain[k][j]);
            }
        }
    }
    (0..count).filter(|&i| scores[i][i] <= tolerances[i] || gain[i][i] >= 0.0).collect()
}

#[cfg(test)]
mod tests {
    use super::uncertain_piece_assignment;

    #[test]
    fn overlap_matching_is_global_and_refuses_ties() {
        let assignment = |scores: &[Vec<f64>]| uncertain_piece_assignment(scores, &vec![1e-8; scores.len()]);
        // Offset10->15 in a box: the old upper overlaps both new halves equally;
        // assigning the old lower first proves that native ids still maximize total overlap.
        assert!(assignment(&[vec![4000., 0.], vec![2000., 2000.]]).is_empty());
        // A greedy row-best would keep the wrong assignment here: 10 < 9+9.
        assert_eq!(assignment(&[vec![10., 9.], vec![9., 0.]]), vec![0, 1]);
        assert_eq!(assignment(&[vec![1., 1.], vec![1., 1.]]), vec![0, 1]);
        // Three-way swaps must be detected even when no pair alone improves the score.
        assert_eq!(assignment(&[vec![2., 3., 0.], vec![0., 2., 3.], vec![3., 0., 2.]]), vec![0, 1, 2]);
    }

    #[test]
    fn overlap_tolerance_is_local_to_each_piece_and_competing_cycle() {
        let large = vec![62_500_000., 0., 0.];
        // An unchanged 1mm³ piece survives beside a 500mm stock and another small piece.
        assert!(uncertain_piece_assignment(&[large.clone(), vec![0., 1., 0.], vec![0., 0., 1.]], &[0.625, 1e-8, 1e-8]).is_empty());
        // Large unrelated stock must not turn a clear small-piece match into an ambiguous cycle.
        assert!(uncertain_piece_assignment(&[large.clone(), vec![0., 1., 0.99], vec![0., 0.99, 1.]], &[0.625, 1e-8, 1e-8]).is_empty());
        // A near tie between the small pieces still refuses both, using their own uncertainties.
        assert_eq!(
            uncertain_piece_assignment(&[large, vec![0., 1., 1. - 5e-9], vec![0., 1. - 5e-9, 1.]], &[0.625, 1e-8, 1e-8]),
            vec![1, 2]
        );
    }
}
