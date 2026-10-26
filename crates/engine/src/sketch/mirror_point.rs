//! The app's point mirror, with the copy tied to its source through the mirror centre.
use super::{EditedSketch, PointSel, SketchCurveSel};
use crate::{Error, Id, Result, Session};
use qymcad_core::model::{entity_points, Constraint};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Copy sketch curves through a half turn about a sketch point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MirrorPointSpec {
    /// Curves to copy; a whole native figure (rectangle, polygon, slot) carries its auxiliary geometry.
    pub curves: SketchCurveSel,
    /// The mirror centre: an existing sketch point, the origin, or a point of a curve.
    pub about: PointSel,
}

impl Session {
    /// Mirror curves about a point through the app's own copy (`mirror_about_point`), which keeps the copy's
    /// own size dimensions but leaves where it stands free. Each copied point is then tied to its source point
    /// with the centre as their midpoint (and each copied circle to its source's radius), so the copy follows
    /// the source's position, the centre and any shared formula when parameters change; a size dimension the copy
    /// carried stays its own. Only independent ties are added.
    pub fn sketch_mirror_point(&mut self, sketch: Id, spec: &MirrorPointSpec) -> Result<EditedSketch> {
        let si = self.sketch_si(sketch)?;
        let curves = self.sketch_curve_ids(sketch, &spec.curves)?;
        let centre = self.sketch_point_id(sketch, &spec.about)?;
        let (px, py) = self.p.point_xy(si, centre).ok_or_else(|| Error::NotFound(format!("mirror centre point {centre}")))?;
        self.sketch_edit_report(sketch, |s| {
            let before = s.p.sketches[si].clone();
            s.p.mirror_about_point(si, &curves, px, py);
            let after = &s.p.sketches[si];
            let copies: Vec<_> = after.entities.iter().filter(|e| before.entity(e.id).is_none()).collect();
            if copies.is_empty() {
                return Err(Error::Invalid("the mirror made no copy of the selected curves".into()));
            }
            // The native copy keeps the sketch order of the copied curves (figures carry their auxiliary
            // curves), so each copy pairs with the next earlier curve of its kind whose points stand at the
            // mirror images of its own. Coincident points of different curves (concentric centres) stay apart.
            let image = |id: Id, of: Id| {
                after.point(id).zip(before.point(of)).is_some_and(|(q, p)| (q.x - (2.0 * px - p.x)).hypot(q.y - (2.0 * py - p.y)) < 1e-9)
            };
            let mut pairs: Vec<(Id, Id)> = Vec::new();
            let mut circles: Vec<(Id, Id)> = Vec::new();
            let mut next = 0;
            for copy in &copies {
                let copy_points = entity_points(copy);
                let found = before.entities[next..].iter().position(|src| {
                    let src_points = entity_points(src);
                    std::mem::discriminant(&src.kind) == std::mem::discriminant(&copy.kind)
                        && src_points.len() == copy_points.len()
                        && src_points.iter().zip(&copy_points).all(|(p, q)| image(*q, *p))
                });
                let Some(at) = found else {
                    return Err(Error::Invalid(format!("cannot tell which curve copy {} was made from", copy.id)));
                };
                let src = &before.entities[next + at];
                next += at + 1;
                pairs.extend(entity_points(src).into_iter().zip(copy_points));
                if let (Some(a), Some(b)) = (src.kind.as_circle(), copy.kind.as_circle()) {
                    circles.push((a.center, b.center));
                }
            }
            // Each tie is a list of alternatives, strongest first. A midpoint holds both coordinates; when the
            // copy's own dimensions already hold one of them (the end of an ellipse axis at its carried
            // length), the midpoint would be redundant, and the point is kept on the line through its source
            // and the centre instead, which still holds the turn.
            let mut ties: Vec<Vec<Constraint>> = Vec::new();
            let mut tied = HashSet::new();
            for (a, b) in pairs {
                if !tied.insert(b) {
                    continue;
                }
                let mut tie = vec![Constraint::Midpoint { p: centre, a, b }];
                if a != centre {
                    tie.push(Constraint::PointOnLine { p: b, a, b: centre });
                }
                ties.push(tie);
            }
            for (c1, c2) in circles {
                ties.push(vec![Constraint::EqualRadius { c1, c2 }]);
            }
            for tie in ties {
                for c in tie {
                    let (free, redundant) = s.p.sketch_dof(si);
                    let n = s.p.sketches[si].constraints.len();
                    if !s.p.add_constraint_if_independent(si, c) {
                        continue;
                    }
                    let (now_free, now_redundant) = s.p.sketch_dof(si);
                    if now_redundant == redundant && now_free < free {
                        break;
                    }
                    s.p.sketches[si].constraints.truncate(n);
                }
            }
            s.finish_sketch_edit(si)?;
            Ok(Vec::new())
        })
    }
}
