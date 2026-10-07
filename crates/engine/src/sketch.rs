//! Sketches: planar profiles. Every entity added here is fully dimensioned (size and position from the sketch
//! origin), with the given expressions as the driving values, so the sketch stays fully defined and editable
//! in the QymCAD GUI.

mod constrain;
mod detail;
mod entities;

pub use constrain::{ConstrainSpec, Constrained, ConstraintKind, DistAxis, FrameRef, SketchRef};
pub use detail::{ConstraintInfo, EntityInfo, PointInfo, SketchDetail};
pub use entities::{Added, ArcSpec, LineSpec, PolygonSpec, PolylineSpec, SlotSpec, Xy};

use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::value::Num;
use qymcad_core::feature::{BasePlane, FaceKey, Purpose, SketchPlane};
use qymcad_core::model::{Constraint, EntityKind, Id, SketchPoint};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Where a sketch (or an offset plane) sits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaneRef {
    /// A base plane: `XY` (normal +Z), `XZ` (normal −Y), `YZ` (normal +X).
    Base(BaseName),
    /// A datum plane by id (from `plane_offset`).
    Plane(Id),
    /// A planar face of a body (face id from `topology`).
    Face { body: Id, face: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum BaseName {
    XY,
    XZ,
    YZ,
}

impl From<BaseName> for BasePlane {
    fn from(b: BaseName) -> Self {
        match b {
            BaseName::XY => BasePlane::XY,
            BaseName::XZ => BasePlane::XZ,
            BaseName::YZ => BasePlane::YZ,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ContourInfo {
    pub id: Id,
    /// The enclosing contour, if any. Extruding a contour extrudes it minus its direct children (holes).
    pub parent: Option<Id>,
    /// mm²
    pub area: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SketchInfo {
    pub id: Id,
    pub name: String,
    pub plane: String,
    /// Remaining degrees of freedom (0 = fully defined) and redundant constraints.
    pub dof: (i32, i32),
    pub contours: Vec<ContourInfo>,
}

impl Session {
    /// Create an empty sketch on `plane`. Returns the sketch id (also its timeline node id).
    pub fn sketch_create(&mut self, plane: &PlaneRef, name: Option<&str>) -> Result<Id> {
        let sp = self.sketch_plane(plane)?;
        let n = self.p.sketches.len() + 1;
        let name = name.map(str::to_string).unwrap_or_else(|| format!("Sketch {n}"));
        let sid = self.p.add_sketch(name.clone(), vec![], None);
        self.p.add_sketch_node(sid, name);
        let si = self.sketch_si(sid)?;
        self.p.sketches[si].plane = sp;
        Ok(sid)
    }

    /// Add a rectangle centred at (`cx`, `cy`) of size `w` × `h` in sketch coordinates. Returns its four line
    /// ids (bottom, right, top, left).
    pub fn sketch_rect(&mut self, sketch: Id, cx: &Num, cy: &Num, w: &Num, h: &Num, construction: bool) -> Result<Vec<Id>> {
        self.transaction(|s| s.sketch_rect_inner(sketch, cx, cy, w, h, construction))
    }

    fn sketch_rect_inner(&mut self, sketch: Id, cx: &Num, cy: &Num, w: &Num, h: &Num, construction: bool) -> Result<Vec<Id>> {
        let si = self.sketch_si(sketch)?;
        let vars = self.p.param_map();
        let (vx, vy, vw, vh) = (cx.eval(&vars)?, cy.eval(&vars)?, w.eval(&vars)?, h.eval(&vars)?);
        if vw <= 0.0 || vh <= 0.0 {
            return Err(Error::Invalid(format!("rectangle size must be positive, got {vw} × {vh}")));
        }
        let lines = self.p.add_rect_entity(si, vx - vw / 2.0, vy - vh / 2.0, vx + vw / 2.0, vy + vh / 2.0, purpose(construction));
        let [bl, br, tr, tl] = self.rect_corners(si, &lines)?;
        // Corners may be shared with earlier geometry (QymCAD merges points within 1e-6): only independent
        // dimensions are added, as for every other entity.
        self.add_independent(si, [dist(br, bl, 1, vw, w.magnitude_expr(vw)?), dist(tl, bl, 2, vh, h.magnitude_expr(vh)?)]);
        let c = self.p.alloc_id();
        let s = &mut self.p.sketches[si];
        s.points.push(SketchPoint { id: c, x: vx, y: vy });
        s.constraints.push(Constraint::Midpoint { p: c, a: bl, b: tr });
        self.pin_point(si, c, cx, vx, cy, vy)?;
        self.finish_sketch_edit(si)?;
        Ok(lines)
    }

    /// Add a circle centred at (`cx`, `cy`) with diameter `d`. Returns the circle entity id.
    pub fn sketch_circle(&mut self, sketch: Id, cx: &Num, cy: &Num, d: &Num, construction: bool) -> Result<Id> {
        self.transaction(|s| s.sketch_circle_inner(sketch, cx, cy, d, construction))
    }

    fn sketch_circle_inner(&mut self, sketch: Id, cx: &Num, cy: &Num, d: &Num, construction: bool) -> Result<Id> {
        let si = self.sketch_si(sketch)?;
        let vars = self.p.param_map();
        let (vx, vy, vd) = (cx.eval(&vars)?, cy.eval(&vars)?, d.eval(&vars)?);
        if vd <= 0.0 {
            return Err(Error::Invalid(format!("circle diameter must be positive, got {vd}")));
        }
        let eid = self.p.add_circle_entity(si, vx, vy, vd / 2.0, purpose(construction));
        let center = self.p.sketches[si]
            .entities
            .iter()
            .find_map(|e| match e.kind {
                EntityKind::Circle { center, .. } if e.id == eid => Some(center),
                _ => None,
            })
            .ok_or_else(|| Error::Invalid("circle was not created".into()))?;
        let idx = self.p.ensure_diameter(si, center, true).ok_or_else(|| Error::Invalid("cannot dimension the circle".into()))?;
        if let Constraint::Diameter { expr, d: dd, .. } = &mut self.p.sketches[si].constraints[idx] {
            *expr = d.magnitude_expr(vd)?;
            *dd = vd;
        }
        self.pin_point(si, center, cx, vx, cy, vy)?;
        self.finish_sketch_edit(si)?;
        Ok(eid)
    }

    pub fn sketch_info(&self, sketch: Id) -> Result<SketchInfo> {
        let si = self.sketch_si(sketch)?;
        let s = &self.p.sketches[si];
        let contours = s
            .contour_ids
            .iter()
            .map(|&cid| ContourInfo {
                id: cid,
                parent: self.p.contours.parent_of(cid).filter(|&x| x != 0),
                area: self.p.contours.index_of(cid).map(|i| self.p.contours[i].signed_area().abs()).unwrap_or(0.0),
            })
            .collect();
        Ok(SketchInfo { id: s.id, name: s.name.clone(), plane: plane_desc(&s.plane), dof: self.p.sketch_dof(si), contours })
    }

    pub fn sketches(&self) -> Vec<SketchInfo> {
        self.p.sketches.iter().filter_map(|s| self.sketch_info(s.id).ok()).collect()
    }

    /// Contours that are not inside another: extruding them gives each region minus its holes.
    pub(crate) fn root_contours(&self, sketch: Id) -> Result<Vec<Id>> {
        let si = self.sketch_si(sketch)?;
        Ok(self.p.sketches[si].contour_ids.iter().copied().filter(|&c| self.p.contours.parent_of(c).is_none_or(|x| x == 0)).collect())
    }

    pub(crate) fn sketch_si(&self, sketch: Id) -> Result<usize> {
        self.p.sketch_index(sketch).ok_or_else(|| Error::NotFound(format!("sketch {sketch}")))
    }

    fn sketch_plane(&self, plane: &PlaneRef) -> Result<SketchPlane> {
        Ok(match plane {
            PlaneRef::Base(b) => SketchPlane::World((*b).into()),
            PlaneRef::Plane(id) => {
                if !self.p.planes.iter().any(|w| w.id == *id) {
                    return Err(Error::NotFound(format!("datum plane {id}")));
                }
                SketchPlane::Datum(*id)
            }
            PlaneRef::Face { body, face } => {
                let f = self
                    .p
                    .regen_faces
                    .get(body)
                    .and_then(|fs| fs.iter().find(|f| f.id == *face))
                    .ok_or_else(|| Error::NotFound(format!("face {face} of body {body} (re-read topology after each rebuild)")))?;
                SketchPlane::Face(
                    *body,
                    FaceKey { index: 0, centroid: [f.centroid.x, f.centroid.y, f.centroid.z], normal: f.normal, id: f.id },
                )
            }
        })
    }

    /// The four distinct corner points of a rectangle, classified by position: [bl, br, tr, tl].
    fn rect_corners(&self, si: usize, lines: &[Id]) -> Result<[Id; 4]> {
        let s = &self.p.sketches[si];
        let mut ids: Vec<Id> = Vec::new();
        for e in s.entities.iter().filter(|e| lines.contains(&e.id)) {
            if let EntityKind::Line { a, b } = e.kind {
                for p in [a, b] {
                    if !ids.contains(&p) {
                        ids.push(p);
                    }
                }
            }
        }
        let pts: Vec<(Id, f64, f64)> =
            ids.iter().filter_map(|id| s.points.iter().find(|p| p.id == *id).map(|p| (p.id, p.x, p.y))).collect();
        if pts.len() != 4 {
            return Err(Error::Invalid(format!("rectangle has {} corners", pts.len())));
        }
        let (mx, my) = (pts.iter().map(|p| p.1).sum::<f64>() / 4.0, pts.iter().map(|p| p.2).sum::<f64>() / 4.0);
        let pick = |left: bool, low: bool| pts.iter().find(|p| (p.1 < mx) == left && (p.2 < my) == low).map(|p| p.0);
        match (pick(true, true), pick(false, true), pick(false, false), pick(true, false)) {
            (Some(bl), Some(br), Some(tr), Some(tl)) => Ok([bl, br, tr, tl]),
            _ => Err(Error::Invalid("cannot classify rectangle corners".into())),
        }
    }

    /// Fix point `p` at (`x`, `y`) from the sketch origin with driving dimensions. A plain zero puts the point
    /// on the axis instead (a zero distance has no side). Distances are magnitudes `|Δ|`: the side comes from
    /// the initial geometry and the expression is negated when its value is negative (see `Num::magnitude_expr`).
    /// Only the independent dimensions are added: the point may be shared with earlier, already dimensioned
    /// geometry (a circle centred on a polyline vertex takes that vertex as its centre, FINDINGS F-042).
    fn pin_point(&mut self, si: usize, p: Id, x: &Num, vx: f64, y: &Num, vy: f64) -> Result<()> {
        let dims = self.pin_dims(si, p, x, vx, y, vy)?;
        self.add_independent(si, dims);
        Ok(())
    }

    /// The two dimensions of `pin_point` ([x, y]), not yet added.
    pub(crate) fn pin_dims(&mut self, si: usize, p: Id, x: &Num, vx: f64, y: &Num, vy: f64) -> Result<[Constraint; 2]> {
        let origin = self.p.ensure_origin(si);
        let x_on_axis = x.expr().is_none() && vx == 0.0;
        let y_on_axis = y.expr().is_none() && vy == 0.0;
        let yaxis = if x_on_axis { Some(self.p.ensure_axis(si, 1)) } else { None };
        let xaxis = if y_on_axis { Some(self.p.ensure_axis(si, 0)) } else { None };
        let cx = match yaxis {
            Some((a, b)) => Constraint::PointOnLine { p, a, b },
            None => dist(p, origin, 1, vx.abs(), x.magnitude_expr(vx)?),
        };
        let cy = match xaxis {
            Some((a, b)) => Constraint::PointOnLine { p, a, b },
            None => dist(p, origin, 2, vy.abs(), y.magnitude_expr(vy)?),
        };
        Ok([cx, cy])
    }

    /// Add each candidate dimension only if it removes a degree of freedom (`add_constraint_if_independent`).
    /// A point that is already determined (shared with earlier geometry, or fixed by the entity's own
    /// constraints) is not dimensioned twice, so the sketch never becomes over-constrained.
    pub(crate) fn add_independent(&mut self, si: usize, dims: impl IntoIterator<Item = Constraint>) {
        for c in dims {
            self.p.add_constraint_if_independent(si, c);
        }
    }

    /// Apply a sketch edit as one unit (restored on error). When features are built from the sketch they are
    /// rebuilt; if a feature that built before fails now, the edit is rolled back and the errors returned.
    pub fn sketch_edit<T>(&mut self, sketch: Id, edit: impl FnOnce(&mut Session) -> Result<T>) -> Result<(T, Option<Rebuild>)> {
        let before = self.p.clone();
        // regen_plan includes transitive sketch dependents and any already dirty nodes. Keep their original
        // handles, rebuilding on independent B-rep copies: restoring a recipe by rebuilding drifts (F-034).
        let has_dependents = !before.dependents_of(sketch).is_empty();
        let mut planned = before.clone();
        planned.mark_sketch_dirty(sketch);
        let nodes: HashSet<Id> = planned.regen_plan().nodes.into_iter().collect();
        let mut saved: HashMap<Id, qymcad_kernel::Shape> = if has_dependents {
            let _gate = qymcad_kernel::kernel_gate();
            before
                .timeline
                .iter()
                .filter(|n| nodes.contains(&n.id))
                .flat_map(|n| n.kind.bodies())
                .filter_map(|id| {
                    self.shapes.get(&id).map(|sh| {
                        sh.to_brep_bytes()
                            .and_then(|b| qymcad_kernel::Shape::from_brep_bytes(&b))
                            .map(|copy| (id, copy))
                            .ok_or_else(|| Error::Io(format!("cannot snapshot body {id} before sketch edit")))
                    })
                })
                .collect::<Result<_>>()?
        } else {
            HashMap::new()
        };
        for (id, copy) in &mut saved {
            if let Some(original) = self.shapes.get_mut(id) {
                std::mem::swap(original, copy);
            }
        }
        let value = match edit(self) {
            Ok(v) => v,
            Err(e) => {
                self.p = before;
                self.shapes.extend(saved);
                return Err(e);
            }
        };
        if self.p.dependents_of(sketch).is_empty() {
            return Ok((value, None));
        }
        let had: HashSet<Id> = before.regen_errors.keys().copied().collect();
        let r = self.rebuild_retrying(Some(&nodes));
        let broken: Vec<String> =
            r.errors.iter().filter(|i| !had.contains(&i.node)).map(|i| format!("{} ({}): {}", i.name, i.node, i.message)).collect();
        if !broken.is_empty() {
            self.p = before;
            let live: HashSet<Id> = self.p.timeline.iter().flat_map(|n| n.kind.bodies()).collect();
            self.shapes.retain(|id, _| live.contains(id));
            self.shapes.extend(saved);
            return Err(Error::Rebuild(broken));
        }
        Ok((value, Some(r)))
    }

    /// Solve sketch `si` until it settles. One `solve_sketch` call can stop at a compromise: QymCAD holds the arms
    /// of angle dimensions softly at their pre-solve lengths, so an edit that must change an arm's length gains
    /// only a fraction per call (FINDINGS F-040). Repeats while the residual keeps dropping (at most 200
    /// calls); returns the final residual.
    pub(crate) fn solve_settled(&mut self, si: usize) -> f64 {
        let mut residual = self.p.solve_sketch(si);
        for _ in 0..199 {
            if residual < 1e-10 {
                break;
            }
            let next = self.p.solve_sketch(si);
            let stalled = next > residual * 0.99;
            residual = next;
            if stalled {
                break;
            }
        }
        // Horizontal + PointOnCircle also admit a reference on the -x side. That alternate branch
        // satisfies ArcLength while rotating every direction by 180°, so reject it through the same
        // residual/rollback path as a failed solve.
        let sketch = &self.p.sketches[si];
        if sketch.constraints.iter().any(|constraint| match *constraint {
            Constraint::ArcLength { c, a, .. }
                if !sketch.entities.iter().any(|e| match e.kind {
                    EntityKind::Line { a: p, b } => p == a || b == a,
                    EntityKind::Arc { center, a: p, b, .. } => center == a || p == a || b == a,
                    EntityKind::Circle { center, .. } => center == a,
                    EntityKind::Ellipse { c, ma, mi } => c == a || ma == a || mi == a,
                }) =>
            {
                let center = sketch.points.iter().find(|p| p.id == c);
                let reference = sketch.points.iter().find(|p| p.id == a);
                center.zip(reference).is_some_and(|(c, a)| a.x < c.x)
            }
            _ => false,
        }) {
            return f64::INFINITY;
        }
        residual
    }

    pub(crate) fn finish_sketch_edit(&mut self, si: usize) -> Result<()> {
        self.p.eval_parameters();
        let residual = self.solve_settled(si);
        let sid = self.p.sketches[si].id;
        self.p.mark_sketch_dirty(sid);
        if residual > 1e-6 {
            let conflicts = self.p.sketch_conflicts(si);
            return Err(Error::Invalid(format!(
                "sketch {sid} does not solve (residual {residual:.3e}): conflicting dimensions{}",
                if conflicts.is_empty() { String::new() } else { format!(" (constraints {conflicts:?} disagree)") }
            )));
        }
        Ok(())
    }
}

pub(crate) fn dist(a: Id, b: Id, axis: u8, d: f64, expr: String) -> Constraint {
    Constraint::Distance { a, b, d, off: 0.0, expr, driven: false, axis, at: None }
}

pub(crate) fn purpose(construction: bool) -> Purpose {
    if construction {
        Purpose::Construction
    } else {
        Purpose::Real
    }
}

fn plane_desc(p: &SketchPlane) -> String {
    match p {
        SketchPlane::World(b) => format!("{b:?}"),
        SketchPlane::Datum(id) => format!("plane {id}"),
        SketchPlane::Face(body, k) => format!("face {} of body {body}", k.id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flipped_angle_reference_is_refused_and_rolled_back() {
        let mut s = Session::new_part();
        s.param_set("rot", &Num::Value(30.0)).unwrap();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let added = s
            .sketch_polygon(
                sk,
                &PolygonSpec {
                    cx: Num::Value(20.0),
                    cy: Num::Value(30.0),
                    sides: 3,
                    r: Some(Num::Value(10.0)),
                    angle: Some(Num::Expr("rot".into())),
                    vertex: None,
                    construction: false,
                    dimensioned: true,
                },
            )
            .unwrap();
        let before = s.sketch_detail(sk).unwrap();
        let reference = before.points.iter().find(|p| p.role == Some("angle_reference")).unwrap().id;
        let si = s.sketch_si(sk).unwrap();
        let err = s
            .transaction(|s| {
                // A 180° rotation about C preserves radius, equal sides, Horizontal, PointOnCircle and
                // directed ArcLength when the reference rotates too: this is the alternate satisfied branch.
                for p in &mut s.p.sketches[si].points {
                    if added.points[1..].contains(&p.id) || p.id == reference {
                        p.x = 2.0 * 20.0 - p.x;
                        p.y = 2.0 * 30.0 - p.y;
                    }
                }
                assert!(s.p.sketch_residuals(si).iter().all(|r| r.abs() < 1e-6));
                s.finish_sketch_edit(si)
            })
            .unwrap_err();
        assert!(err.to_string().contains("does not solve"), "{err}");
        assert_eq!(s.sketch_detail(sk).unwrap(), before, "the flipped branch must roll back");
    }
}
