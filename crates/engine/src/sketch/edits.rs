//! One-time native curve edits (trim, extend, break) and the report every sketch edit returns.
use super::{ConstraintInfo, EntityRef};
use crate::{Error, Id, Num, Result, Session};
use qymcad_core::geom::Point2;
use qymcad_core::model::curve::Curve;
use qymcad_core::model::{entity_ends, entity_points, point_on, Constraint, EndKept, ExtendAsk, ExtendSides, Sketch, SketchEntity};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A unique curve and a one-time location on the part to edit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CurvePickSpec {
    /// Exactly one curve in the sketch.
    pub curve: EntityRef,
    /// One-time pick in sketch coordinates; it does not replay after parameter edits.
    pub at: [Num; 2],
}

/// Extend an open end with the native target and end-selection rules.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtendSpec {
    /// Exactly one open curve.
    pub curve: EntityRef,
    /// One-time point choosing the end to extend, in sketch coordinates.
    pub at: [Num; 2],
    /// The boundary to reach; otherwise the nearest boundary along the end.
    #[serde(default)]
    pub target: Option<EntityRef>,
    /// Extend both open ends instead of the end nearest `at`.
    #[serde(default)]
    pub both: bool,
}

/// Split a curve at a picked location or at all its intersections.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BreakSpec {
    /// Exactly one curve.
    pub curve: EntityRef,
    /// One-time split point; required unless `intersections` is true.
    #[serde(default)]
    pub at: Option<[Num; 2]>,
    /// Split at every intersection with other curves, construction ones included; excludes `at`.
    #[serde(default)]
    pub intersections: bool,
}

/// What a sketch edit changed, and the sketch's freedom afterwards.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct EditedSketch {
    /// Entity ids allocated by the edit.
    pub created_entities: Vec<Id>,
    /// Entity ids removed by the edit.
    pub removed_entities: Vec<Id>,
    /// Kept entity ids whose geometry changed.
    pub changed_entities: Vec<Id>,
    /// Point ids allocated by the edit.
    pub created_points: Vec<Id>,
    /// Point ids removed by the edit.
    pub removed_points: Vec<Id>,
    /// Indices of the driving dimensions that hold the sizes asked for (offset distance, corner sizes), in
    /// the order of the request; empty for trim, extend, break and mirror.
    pub dimensions: Vec<usize>,
    /// Formula dimensions that the edit removed, as they were before it (their old indices).
    pub dropped_formulas: Vec<ConstraintInfo>,
    /// Native offset associations released by the edit.
    pub released_offsets: Vec<Id>,
    /// Remaining degrees of freedom and redundant constraints; freedom alone is never a refusal.
    pub dof: (i32, i32),
    /// Point ids that can still move.
    pub free_points: Vec<Id>,
}

impl Session {
    /// Run a sketch edit atomically and report what it changed. `edit` returns the indices of the dimensions
    /// that hold the requested sizes; on error the sketch is restored.
    pub(crate) fn sketch_edit_report(&mut self, sketch: Id, edit: impl FnOnce(&mut Self) -> Result<Vec<usize>>) -> Result<EditedSketch> {
        let si = self.sketch_si(sketch)?;
        self.transaction(|s| {
            let before = s.p.sketches[si].clone();
            let formulas: Vec<ConstraintInfo> = s.sketch_detail(sketch)?.constraints.into_iter().filter(has_formula).collect();
            let dimensions = s.preserving_points(si, edit)?;
            let after = &s.p.sketches[si];
            // A formula dimension survives when one afterwards has the same kind, points and text; failing that
            // (an edit that moved it to another point), the same kind and text. What is left was dropped.
            let after_formulas: Vec<ConstraintInfo> = s.sketch_detail(sketch)?.constraints.into_iter().filter(has_formula).collect();
            let key = |c: &ConstraintInfo, with_points: bool| {
                let mut points = if with_points { c.points.clone() } else { Vec::new() };
                points.sort_unstable();
                (c.kind.clone(), c.expr.clone().unwrap_or_default(), points)
            };
            // Each row afterwards accounts for one row before, exact matches first.
            let mut unmatched_after = after_formulas;
            let mut dropped_formulas = formulas;
            for with_points in [true, false] {
                dropped_formulas.retain(|c| match unmatched_after.iter().position(|a| key(a, with_points) == key(c, with_points)) {
                    Some(i) => {
                        unmatched_after.swap_remove(i);
                        false
                    }
                    None => true,
                });
            }
            let old_entities: HashSet<Id> = before.entities.iter().map(|e| e.id).collect();
            let new_entities: HashSet<Id> = after.entities.iter().map(|e| e.id).collect();
            let old_points: HashSet<Id> = before.points.iter().map(|p| p.id).collect();
            let new_points: HashSet<Id> = after.points.iter().map(|p| p.id).collect();
            Ok(EditedSketch {
                created_entities: after.entities.iter().filter(|e| !old_entities.contains(&e.id)).map(|e| e.id).collect(),
                removed_entities: before.entities.iter().filter(|e| !new_entities.contains(&e.id)).map(|e| e.id).collect(),
                changed_entities: after
                    .entities
                    .iter()
                    .filter(|e| old_entities.contains(&e.id) && !unchanged_entity(&before, after, e))
                    .map(|e| e.id)
                    .collect(),
                created_points: after.points.iter().filter(|p| !old_points.contains(&p.id)).map(|p| p.id).collect(),
                removed_points: before.points.iter().filter(|p| !new_points.contains(&p.id)).map(|p| p.id).collect(),
                dimensions,
                dropped_formulas,
                released_offsets: before.offsets.iter().filter(|o| !after.offsets.iter().any(|n| n.id == o.id)).map(|o| o.id).collect(),
                dof: s.p.sketch_dof(si),
                free_points: after.points.iter().zip(s.p.sketch_free_points(si)).filter_map(|(p, free)| free.then_some(p.id)).collect(),
            })
        })
    }

    /// Remove the part of a curve between the intersections around a pick, as the app's trim does. A curve with
    /// no intersection is removed whole.
    pub fn sketch_trim(&mut self, sketch: Id, spec: &CurvePickSpec) -> Result<EditedSketch> {
        let si = self.sketch_si(sketch)?;
        let id = self.sketch_curve_id(sketch, &spec.curve)?;
        let at = self.edit_pick(&spec.at)?;
        self.sketch_edit_report(sketch, |s| {
            let before = s.p.sketches[si].clone();
            if !s.p.trim_curve(si, id, at.x, at.y) {
                return Err(Error::Invalid("trim cannot apply to the selected curve at this point".into()));
            }
            s.tie_edited_ends(si, id, &before);
            s.finish_sketch_edit(si)?;
            Ok(Vec::new())
        })
    }

    /// Extend an open end to a boundary, as the app's extend does. An end that is joined or held by a driving
    /// dimension stays, and is refused here with the reason.
    pub fn sketch_extend(&mut self, sketch: Id, spec: &ExtendSpec) -> Result<EditedSketch> {
        let si = self.sketch_si(sketch)?;
        let id = self.sketch_curve_id(sketch, &spec.curve)?;
        let at = self.edit_pick(&spec.at)?;
        let over = spec.target.as_ref().map(|t| self.sketch_curve_id(sketch, t)).transpose()?;
        if over == Some(id) {
            return Err(Error::Invalid("the extension boundary must be a different curve".into()));
        }
        let ask = ExtendAsk { pointer: at, over, sides: if spec.both { ExtendSides::Both } else { ExtendSides::Nearer } };
        let ext = self.p.curve_extension(si, id, &ask).ok_or_else(|| Error::Invalid("extend needs an open curve".into()))?;
        if ext.goes.is_empty() {
            return Err(Error::Invalid(if ext.kept == EndKept::Joined {
                "this end is joined to other geometry or held by a driving dimension and stays where it is; remove that constraint first"
                    .into()
            } else {
                "no boundary lies along this end".into()
            }));
        }
        self.sketch_edit_report(sketch, |s| {
            let before = s.p.sketches[si].clone();
            if !s.p.extend_curve_by(si, id, &ext) {
                return Err(Error::Invalid("the extension did not apply".into()));
            }
            s.tie_edited_ends(si, id, &before);
            s.finish_sketch_edit(si)?;
            Ok(Vec::new())
        })
    }

    /// Split a curve at a pick (the app's break) or at all its intersections.
    pub fn sketch_break(&mut self, sketch: Id, spec: &BreakSpec) -> Result<EditedSketch> {
        let si = self.sketch_si(sketch)?;
        let id = self.sketch_curve_id(sketch, &spec.curve)?;
        if spec.intersections == spec.at.is_some() {
            return Err(Error::Invalid("break needs exactly one of `at` or `intersections: true`".into()));
        }
        let at = spec.at.as_ref().map(|p| self.edit_pick(p)).transpose()?;
        let places = if spec.intersections {
            let s = &self.p.sketches[si];
            let curve = s.entity(id).and_then(|e| Curve::of(s, e)).ok_or_else(|| Error::Invalid(format!("curve {id} cannot be read")))?;
            let mut places: Vec<f64> =
                self.p.entity_intersections(si, id).into_iter().map(|(x, y)| curve.nearest(Point2::new(x, y)).t).collect();
            places.sort_by(f64::total_cmp);
            places.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
            if places.is_empty() {
                return Err(Error::Invalid("the selected curve has no intersections to split at".into()));
            }
            places
        } else {
            Vec::new()
        };
        let redundant = self.p.sketch_dof(si).1;
        self.sketch_edit_report(sketch, |s| {
            let before = s.p.sketches[si].clone();
            let applied = match at {
                Some(at) => s.p.break_curve(si, id, at.x, at.y),
                None => !s.p.cut_curve(si, id, &places).is_empty(),
            };
            if !applied {
                return Err(Error::Invalid("the selected curve cannot be split at this location".into()));
            }
            s.tie_edited_ends(si, id, &before);
            // The app holds the halves of a broken line on it only through a horizontal or vertical of its ends,
            // and only for a picked break (`break_line_at`; `cut_curve` keeps the row on the far ends); any other
            // line would bend at its split points, so each is put on the line's ends. A split point that was already
            // there (the pieces are welded to it) and is held otherwise can take no such tie; unless it is tied to
            // the line already, the break is refused.
            if let Some((entity, line)) = before.entity(id).and_then(|e| e.kind.as_line().map(|l| (e, l))) {
                let after = &s.p.sketches[si];
                let mut split: Vec<Id> = after
                    .entities
                    .iter()
                    .filter(|e| e.id == id || before.entity(e.id).is_none())
                    .filter_map(|e| e.kind.as_line())
                    .flat_map(|l| [l.a, l.b])
                    .filter(|p| *p != line.a && *p != line.b)
                    .collect();
                split.sort_unstable();
                split.dedup();
                // The halves the app holds by a horizontal or vertical each keep the split point on the line; a point
                // that is also held otherwise then over-holds the sketch, which the redundancy check below refuses.
                let halved = |p: Id| {
                    let row = |x: Id, y: Id| {
                        after.constraints.iter().any(|c| {
                            matches!(*c, Constraint::Horizontal { a, b } | Constraint::Vertical { a, b } if (a, b) == (x, y) || (b, a) == (x, y))
                        })
                    };
                    row(line.a, p) && row(p, line.b)
                };
                let halved: Vec<Id> = split.iter().copied().filter(|p| halved(*p)).collect();
                // A point the halves hold takes no tie: the near-solved check of a dependent row could still add it.
                for p in split.into_iter().filter(|p| !halved.contains(p)) {
                    let tie = Constraint::PointOnLine { p, a: line.a, b: line.b };
                    let tied = before.constraints.iter().any(|c| same_membership(c, &tie) || same_membership(c, &point_on(entity, p)));
                    if !s.p.add_constraint_if_independent(si, tie) && !tied && before.point(p).is_some() {
                        return Err(Error::Invalid(format!(
                            "point {p} is held by other dimensions, so the line would bend at it when they change; tie it to the line first (point_on_curve) or break elsewhere"
                        )));
                    }
                }
            }
            s.finish_sketch_edit(si)?;
            // A picked break of a horizontal line at a point held otherwise splits its row into two that over-hold the
            // point. (Breaking a circle or an ellipse leaves the app's own redundant rows, accepted as they are.)
            let dof = s.p.sketch_dof(si);
            if before.entity(id).is_some_and(|e| e.kind.as_line().is_some()) && dof.1 > redundant {
                return Err(Error::Invalid(format!("the break would leave redundant constraints in the sketch: DOF {dof:?}")));
            }
            Ok(Vec::new())
        })
    }

    pub(crate) fn edit_pick(&self, p: &[Num; 2]) -> Result<Point2> {
        let vars = self.p.param_map();
        Ok(Point2::new(p[0].eval(&vars)?, p[1].eval(&vars)?))
    }

    /// Tie each new or moved end of the edited curve's pieces to the other curves it now lies on, with the
    /// membership constraint the app itself uses for that curve kind (`point_on`). Rows already present (the
    /// native line and circle trims write them) are kept; dependent rows are not added.
    fn tie_edited_ends(&mut self, si: usize, id: Id, before: &Sketch) {
        let s = &self.p.sketches[si];
        let old_ends: Vec<(Id, Point2)> = before
            .entity(id)
            .and_then(entity_ends)
            .into_iter()
            .flatten()
            .filter_map(|p| before.point(p).map(|q| (p, Point2::new(q.x, q.y))))
            .collect();
        let ends: HashSet<Id> = s.entities.iter().filter(|e| !unchanged_entity(before, s, e)).filter_map(entity_ends).flatten().collect();
        let mut ties = Vec::new();
        for p in ends {
            let Some(q) = s.point(p) else { continue };
            let at = Point2::new(q.x, q.y);
            if old_ends.iter().any(|(old, was)| *old == p && was.dist(at) < 1e-7) {
                continue;
            }
            for boundary in &before.entities {
                let Some(e) = s.entity(boundary.id).filter(|e| e.id != id && !entity_points(e).contains(&p)) else { continue };
                if Curve::of(s, e).is_some_and(|c| c.nearest(at).off < 1e-6) {
                    ties.push(point_on(e, p));
                }
            }
        }
        for tie in ties {
            if !self.p.sketches[si].constraints.iter().any(|c| same_membership(c, &tie)) {
                self.p.add_constraint_if_independent(si, tie);
            }
        }
    }
}

fn has_formula(c: &ConstraintInfo) -> bool {
    c.expr.as_deref().is_some_and(|e| !e.is_empty())
}

fn same_membership(a: &Constraint, b: &Constraint) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b) && {
        let (mut x, mut y) = (a.points(), b.points());
        x.sort_unstable();
        y.sort_unstable();
        x == y
    }
}

/// Native records do not implement PartialEq; compare their serialized geometry, spline handles included.
fn same_entity(a: &SketchEntity, b: &SketchEntity) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}

fn unchanged_entity(before: &Sketch, after: &Sketch, e: &SketchEntity) -> bool {
    before.entity(e.id).is_some_and(|b| same_entity(b, e))
        && entity_points(e).into_iter().all(|id| before.point(id).zip(after.point(id)).is_some_and(|(a, b)| a.x == b.x && a.y == b.y))
}
