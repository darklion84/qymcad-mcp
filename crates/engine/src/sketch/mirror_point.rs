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
            // The source of a copied point is the point of the copied set standing at its mirror image.
            let sources: HashSet<Id> = curves.iter().filter_map(|id| before.entity(*id)).flat_map(entity_points).collect();
            let source_of = |x: f64, y: f64| {
                before
                    .points
                    .iter()
                    .filter(|q| sources.contains(&q.id))
                    .find(|q| (q.x - (2.0 * px - x)).hypot(q.y - (2.0 * py - y)) < 1e-9)
                    .map(|q| q.id)
            };
            // Each tie is a list of alternatives, strongest first. A midpoint holds both coordinates; when the
            // copy's own dimensions already hold one of them (the end of an ellipse axis at its carried
            // length), the midpoint would be redundant, and the point is kept on the line through its source
            // and the centre instead, which still holds the turn.
            let mut ties: Vec<Vec<Constraint>> = Vec::new();
            for p in after.points.iter().filter(|p| before.point(p.id).is_none()) {
                if let Some(a) = source_of(p.x, p.y) {
                    let mut tie = vec![Constraint::Midpoint { p: centre, a, b: p.id }];
                    if a != centre {
                        tie.push(Constraint::PointOnLine { p: p.id, a, b: centre });
                    }
                    ties.push(tie);
                }
            }
            for copy in &copies {
                if let Some(c) = copy.kind.as_circle() {
                    let at = after.point(c.center).map(|q| (q.x, q.y));
                    if let Some(source) = at.and_then(|(x, y)| source_of(x, y)) {
                        ties.push(vec![Constraint::EqualRadius { c1: source, c2: c.center }]);
                    }
                }
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
