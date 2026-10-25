//! One-time native curve edits (trim, extend, break) and the report every sketch edit returns.
use super::{ConstraintInfo, EntityRef};
use crate::{Error, Id, Num, Result, Session};
use qymcad_core::geom::Point2;
use qymcad_core::model::curve::Curve;
use qymcad_core::model::{entity_ends, entity_points, point_on, Constraint, EndKept, ExtendAsk, ExtendSides, Sketch, SketchEntity};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
            let dimensions = edit(s)?;
            let after = &s.p.sketches[si];
            // A formula is dropped when fewer constraints carry its text afterwards than before.
            let mut kept: HashMap<String, usize> = HashMap::new();
            for c in s.sketch_detail(sketch)?.constraints.into_iter().filter(has_formula) {
                *kept.entry(c.expr.unwrap_or_default()).or_default() += 1;
            }
            let dropped_formulas = formulas
                .into_iter()
                .filter(|c| match kept.get_mut(c.expr.as_deref().unwrap_or_default()) {
                    Some(n) if *n > 0 => {
                        *n -= 1;
                        false
                    }
                    _ => true,
                })
                .collect();
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
            s.finish_sketch_edit(si)?;
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
