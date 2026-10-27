//! Sketches: planar profiles. Every entity added here is fully dimensioned (size and position from the sketch
//! origin), with the given expressions as the driving values, so the sketch stays fully defined and editable
//! in the QymCAD GUI.

mod constrain;
mod corner_edit;
mod detail;
mod edits;
mod entities;
mod figures;
mod mirror_point;
mod offset_edit;
mod selectors;
mod splines;
mod text;

pub use corner_edit::{CornerSize, CornerSpec};
pub use edits::{BreakSpec, CurvePickSpec, EditedSketch, ExtendSpec};
pub use mirror_point::MirrorPointSpec;
pub use offset_edit::SketchOffsetSpec;

pub use constrain::{ConstrainSpec, Constrained, ConstraintKind, DistAxis, FrameRef, SketchRef};
pub use detail::{ConstraintInfo, EntityInfo, PointInfo, SketchDetail};
pub use entities::{Added, ArcSpec, LineSpec, PolygonSpec, PolylineSpec, SlotSpec, Xy};
pub use figures::{EllipseArcSpec, EllipseSpec, RectAnchor, RectSpec};
pub use selectors::{
    CurveQuery, EntityRef, OriginRef, PointRole, PointRoleSel, PointSel, SketchCurveKind, SketchCurveSel, TangentCircleSpec,
};
pub use splines::SplineSpec;
pub use text::{TextFont, TextInfo, TextSpec};

use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::value::Num;
use qymcad_core::feature::{BasePlane, FaceKey, Purpose, SketchPlane};
use qymcad_core::model::{entity_points, Constraint, EntityKind, Id, PlaneDef};
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
pub struct SketchWorldFrame {
    /// World origin in mm (including component placement and any GUI origin shift).
    #[serde(serialize_with = "crate::session::serialize_coordinates")]
    pub origin: [f64; 3],
    /// World unit direction of positive sketch x.
    #[serde(serialize_with = "crate::session::serialize_coordinates")]
    pub x_axis: [f64; 3],
    /// World unit direction of positive sketch y.
    #[serde(serialize_with = "crate::session::serialize_coordinates")]
    pub y_axis: [f64; 3],
    /// World unit normal, x cross y.
    #[serde(serialize_with = "crate::session::serialize_coordinates")]
    pub normal: [f64; 3],
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SketchInfo {
    pub id: Id,
    pub name: String,
    pub plane: String,
    /// Resolved world frame, or null when the sketch's host cannot be resolved.
    pub world_frame: Option<SketchWorldFrame>,
    /// Remaining degrees of freedom (0 = fully defined) and redundant constraints.
    pub dof: (i32, i32),
    pub contours: Vec<ContourInfo>,
    /// Native texts, including construction texts and their baked glyph contour counts.
    pub texts: Vec<TextInfo>,
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
        self.track_datum_dependencies(sid);
        Ok(sid)
    }

    /// Native feature dimensions make a datum sketch dirty before parallel preparation (F-017).
    /// The pinned scheduler otherwise misses the datum dependency and can capture an old sketch frame.
    /// Persist one expression per ancestor: the GUI's parameter marking then provides the same barrier.
    pub(crate) fn track_datum_dependencies(&mut self, sid: Id) {
        let Some(si) = self.p.sketch_index(sid) else { return };
        let mut desired = HashMap::new();
        if let SketchPlane::Datum(mut plane) = self.p.sketches[si].plane {
            // Bound traversal even for malformed cycles in externally produced documents.
            for _ in 0..self.p.planes.len() {
                if let Some(expr) = self.p.feat_dim(plane, "dist") {
                    desired.insert(format!("datum_dist_{plane}"), expr.to_string());
                }
                match self.p.planes.iter().find(|p| p.id == plane).map(|p| &p.def) {
                    Some(PlaneDef::OffsetPlane { plane: parent, .. }) => plane = *parent,
                    _ => break,
                }
            }
        }
        // A GUI edit may change the datum expression or reattach the sketch. Drop only obsolete guards.
        let obsolete: Vec<String> = self
            .p
            .feat_dims
            .get(&sid)
            .into_iter()
            .flat_map(|m| m.keys())
            .filter(|k| k.starts_with("datum_dist_") && !desired.contains_key(*k))
            .cloned()
            .collect();
        for key in obsolete {
            self.p.set_feat_dim(sid, &key, String::new());
        }
        for (key, expr) in desired {
            if self.p.feat_dim(sid, &key) != Some(expr.as_str()) {
                self.p.set_feat_dim(sid, &key, expr);
            }
        }
    }

    /// Add a rectangle centred at (`cx`, `cy`) of size `w` × `h` in sketch coordinates. Returns its four line
    /// ids (bottom, right, top, left).
    pub fn sketch_rect(&mut self, sketch: Id, cx: &Num, cy: &Num, w: &Num, h: &Num, construction: bool) -> Result<Vec<Id>> {
        self.sketch_rectangle(
            sketch,
            &RectSpec {
                cx: cx.clone(),
                cy: cy.clone(),
                w: w.clone(),
                h: h.clone(),
                turn: Num::Value(0.0),
                anchor: RectAnchor::Centre,
                construction,
            },
        )
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
        let world_frame = self.p.sketch_frame(si).map(|f| {
            let f = self.p.sketch_owner(sketch).map(|owner| f.transformed(&self.p.world_transform(owner))).unwrap_or(f);
            SketchWorldFrame { origin: f.origin, x_axis: f.x, y_axis: f.y, normal: f.normal() }
        });
        Ok(SketchInfo {
            id: s.id,
            name: crate::localization::name(&s.name),
            plane: plane_desc(self, &s.plane),
            world_frame,
            dof: self.p.sketch_dof(si),
            contours,
            texts: s
                .texts
                .iter()
                .map(|t| TextInfo {
                    id: t.id,
                    text: t.text.clone(),
                    font: TextFont { path: t.font.path.clone(), index: t.font.index },
                    family: t.font.family.clone(),
                    height: t.height,
                    at: [t.x, t.y],
                    angle: t.angle,
                    construction: t.construction,
                    contours: t.glyphs.len(),
                })
                .collect(),
        })
    }

    pub fn sketches(&self) -> Vec<SketchInfo> {
        self.p.sketches.iter().filter_map(|s| self.sketch_info(s.id).ok()).collect()
    }

    /// Contours that are not inside another: extruding them gives each region minus its holes.
    pub(crate) fn root_contours(&self, sketch: Id) -> Result<Vec<Id>> {
        let si = self.sketch_si(sketch)?;
        Ok(self.p.sketches[si].contour_ids.iter().copied().filter(|&c| self.p.contours.parent_of(c).is_none_or(|x| x == 0)).collect())
    }

    /// The contours picked by points in the sketch plane, as a click in the app picks them: for each point,
    /// the innermost closed contour that contains it (its region, minus the contours inside it, is what a
    /// feature takes). Points are evaluated once; the ids are what the feature stores. A point outside every
    /// contour, or within 1e-6 mm of a contour's edge, is refused. Repeated contours are listed once.
    pub fn contours_at(&self, sketch: Id, points: &[[Num; 2]]) -> Result<Vec<Id>> {
        let si = self.sketch_si(sketch)?;
        let vars = self.p.param_map();
        let loops: Vec<(Id, Vec<[f64; 2]>)> = self.p.sketches[si]
            .contour_ids
            .iter()
            .filter_map(|&c| self.p.contour_profile_xy(c).map(|xy| (c, xy.as_chunks::<2>().0.to_vec())))
            .collect();
        let mut picked = Vec::new();
        for [x, y] in points {
            let at = [x.eval(&vars)?, y.eval(&vars)?];
            if loops.iter().any(|(_, poly)| polygon_edge_distance(poly, at) <= 1e-6) {
                return Err(Error::Invalid(format!("point {at:?} lies on a contour's edge; pick a point inside the region")));
            }
            let inner = loops
                .iter()
                .filter(|(_, poly)| polygon_contains(poly, at))
                .min_by(|a, b| polygon_area(&a.1).total_cmp(&polygon_area(&b.1)))
                .map(|(c, _)| *c)
                .ok_or_else(|| Error::Invalid(format!("point {at:?} lies in no closed contour of sketch {sketch}")))?;
            if !picked.contains(&inner) {
                picked.push(inner);
            }
        }
        if picked.is_empty() {
            return Err(Error::Invalid("give at least one point".into()));
        }
        Ok(picked)
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
        // regen_plan includes transitive sketch dependents and any already dirty nodes.
        let mut planned = before.clone();
        planned.mark_sketch_dirty(sketch);
        let mut nodes: HashSet<Id> = planned.regen_plan().nodes.into_iter().collect();
        // regen_plan omits dirty sketch outputs; regenerate still rebuilds their consumers (F-023).
        nodes.extend(before.dependents(sketch));
        let had: HashSet<Id> = before.regen_errors.keys().copied().collect();
        let copies = if before.dependents_of(sketch).is_empty() { HashSet::new() } else { nodes.clone() };
        self.with_rebuild_copies(before, &copies, "sketch edit", |s| {
            let value = edit(s)?;
            if s.p.dependents_of(sketch).is_empty() {
                return Ok((value, None));
            }
            let r = s.rebuild_retrying(Some(&nodes));
            let broken: Vec<String> =
                r.errors.iter().filter(|i| !had.contains(&i.node)).map(|i| format!("{} ({}): {}", i.name, i.node, i.message)).collect();
            if !broken.is_empty() {
                return Err(Error::Rebuild(broken));
            }
            Ok((value, Some(r)))
        })
    }

    /// Solve sketch `si` and return the residual. QymCAD 0.1.1 settles angle-dimension arms in one solve (c899ea7;
    /// the repeated calls of F-040 are gone). Its own "solved" bar is 1e-4 while edits here are refused above 1e-6,
    /// so a solve that stops in between gets one more before a refusal. A wrong branch is checked below.
    pub(crate) fn solve_settled(&mut self, si: usize) -> f64 {
        let mut residual = self.p.solve_sketch(si);
        if residual > 1e-6 {
            residual = self.p.solve_sketch(si);
        }
        // Horizontal + PointOnCircle also admit a reference on the -x side. That alternate branch
        // satisfies ArcLength while rotating every direction by 180°, so reject it through the same
        // residual/rollback path as a failed solve.
        let sketch = &self.p.sketches[si];
        if sketch.constraints.iter().any(|constraint| match *constraint {
            Constraint::ArcLength { c, a, .. } if !sketch.entities.iter().any(|e| entity_points(e).contains(&a)) => {
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

fn plane_desc(session: &Session, p: &SketchPlane) -> String {
    match p {
        SketchPlane::World(b) => format!("{b:?}"),
        SketchPlane::Datum(id) => {
            let name = session.node_name(*id);
            if name.is_empty() {
                format!("plane {id}")
            } else {
                format!("{name} (plane {id})")
            }
        }
        SketchPlane::Face(body, k) => format!("face {} of body {body}", k.id),
    }
}

/// Even-odd test of a point against a closed polygon.
fn polygon_contains(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    for (i, a) in poly.iter().enumerate() {
        let b = poly[(i + 1) % poly.len()];
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]) {
            inside = !inside;
        }
    }
    inside
}

/// Absolute area of a closed polygon (shoelace).
fn polygon_area(poly: &[[f64; 2]]) -> f64 {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

/// Distance from a point to the nearest edge of a closed polygon.
fn polygon_edge_distance(poly: &[[f64; 2]], p: [f64; 2]) -> f64 {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
        })
        .fold(f64::INFINITY, f64::min)
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
