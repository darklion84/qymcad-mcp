//! Sketch entities beyond rectangles and circles: lines, polylines, arcs, regular polygons and slots.
//!
//! Each is drawn with QymCAD's own entity adders (so it carries the same intrinsic constraints as one drawn in
//! the GUI) and then, unless `dimensioned` is false, fixed by driving dimensions built from the given values and
//! expressions. Candidate dimensions are added only when they remove a degree of freedom
//! (`Session::add_independent`): a point shared with earlier geometry, or determined by the entity's own
//! constraints, is not dimensioned twice. The result lists the new entity and point ids; the sketch's remaining
//! degrees of freedom come from `sketch_info` / `sketch_dof`.

use super::purpose;
use crate::error::{Error, Result};
use crate::session::Session;
use crate::value::{check_expr, Num};
use qymcad_core::feature::Winding;
use qymcad_core::geom::Point2;
use qymcad_core::model::{Constraint, EntityKind, Id};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn zero() -> Num {
    Num::Value(0.0)
}

fn yes() -> bool {
    true
}

/// A point `[x, y]` in sketch coordinates (mm); each coordinate is a number or an expression.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Xy(pub Num, pub Num);

/// A straight segment from (x1, y1) to (x2, y2). Both endpoints are dimensioned from the sketch origin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineSpec {
    /// Start x.
    pub x1: Num,
    /// Start y.
    pub y1: Num,
    /// End x.
    pub x2: Num,
    /// End y.
    pub y2: Num,
    /// Construction geometry: not part of any profile.
    #[serde(default)]
    pub construction: bool,
    /// Add driving dimensions that fix the entity (default true). With false only the entity's own shape
    /// constraints are added, and you dimension it yourself with `sketch_constrain`.
    #[serde(default = "yes")]
    pub dimensioned: bool,
}

/// Connected straight segments through `points` (consecutive segments share their endpoint). Each vertex is
/// dimensioned from the sketch origin, so expressions like `["w", "t"]` keep the profile parametric.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolylineSpec {
    /// Vertices `[[x, y], ...]`, at least 2 (3 when closed). Do not repeat the first point to close it; set
    /// `closed`.
    pub points: Vec<Xy>,
    /// Join the last point back to the first, forming a closed contour that can be extruded.
    #[serde(default)]
    pub closed: bool,
    /// Construction geometry: not part of any profile.
    #[serde(default)]
    pub construction: bool,
    /// Add driving dimensions that fix the entity (default true). With false only the entity's own shape
    /// constraints are added, and you dimension it yourself with `sketch_constrain`.
    #[serde(default = "yes")]
    pub dimensioned: bool,
}

/// A circular arc around (cx, cy). Give either `r` + `start_angle` + `end_angle`, or `start` + `end` points.
/// An endpoint that coincides with an existing point (e.g. a polyline end) is joined to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArcSpec {
    /// Centre x.
    #[serde(default = "zero")]
    pub cx: Num,
    /// Centre y.
    #[serde(default = "zero")]
    pub cy: Num,
    /// Radius (with `start_angle` and `end_angle`).
    #[serde(default)]
    pub r: Option<Num>,
    /// Start angle, degrees counter-clockwise from +x (with `r`).
    #[serde(default)]
    pub start_angle: Option<Num>,
    /// End angle, degrees counter-clockwise from +x (with `r`).
    #[serde(default)]
    pub end_angle: Option<Num>,
    /// Start point `[x, y]` (instead of `r` and the angles; the radius is |start − centre|).
    #[serde(default)]
    pub start: Option<Xy>,
    /// End point `[x, y]`; must lie on the circle through `start`.
    #[serde(default)]
    pub end: Option<Xy>,
    /// Direction from start to end: counter-clockwise (default true) or clockwise.
    #[serde(default = "yes")]
    pub ccw: bool,
    /// Construction geometry: not part of any profile.
    #[serde(default)]
    pub construction: bool,
    /// Add driving dimensions that fix the entity (default true). With false only the entity's own shape
    /// constraints are added, and you dimension it yourself with `sketch_constrain`.
    #[serde(default = "yes")]
    pub dimensioned: bool,
}

/// A regular polygon around (cx, cy): `sides` equal sides with the vertices on a construction circle. Give the
/// circumradius `r` (and optionally `angle`) or one `vertex`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolygonSpec {
    /// Centre x.
    #[serde(default = "zero")]
    pub cx: Num,
    /// Centre y.
    #[serde(default = "zero")]
    pub cy: Num,
    /// Number of sides, 3..=64.
    pub sides: u32,
    /// Circumradius: centre to vertex (a hexagon of circumradius R is 2R across corners, √3·R across flats).
    #[serde(default)]
    pub r: Option<Num>,
    /// Direction of the first vertex, degrees counter-clockwise from +x (default 0: a hexagon then has
    /// horizontal top and bottom sides). Only with `r`.
    #[serde(default)]
    pub angle: Option<Num>,
    /// One vertex `[x, y]` instead of `r` and `angle`.
    #[serde(default)]
    pub vertex: Option<Xy>,
    /// Construction geometry: not part of any profile.
    #[serde(default)]
    pub construction: bool,
    /// Add driving dimensions that fix the entity (default true). With false only the entity's own shape
    /// constraints are added (vertices on the circle, equal sides, the radius), and you dimension the rest
    /// yourself with `sketch_constrain`.
    #[serde(default = "yes")]
    pub dimensioned: bool,
}

/// A slot (stadium): two half-circle ends of diameter `width` around the centres (x1, y1) and (x2, y2),
/// joined by tangent lines. Area = π·(width/2)² + width·|c2 − c1|.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SlotSpec {
    /// First end centre x.
    pub x1: Num,
    /// First end centre y.
    pub y1: Num,
    /// Second end centre x.
    pub x2: Num,
    /// Second end centre y.
    pub y2: Num,
    /// Slot width (= diameter of the round ends).
    pub width: Num,
    /// Construction geometry: not part of any profile.
    #[serde(default)]
    pub construction: bool,
    /// Add driving dimensions that fix the entity (default true). With false only the entity's own shape
    /// constraints are added (tangent sides, equal ends), and you dimension it yourself with `sketch_constrain`.
    #[serde(default = "yes")]
    pub dimensioned: bool,
}

/// What an entity call created.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Added {
    /// New entity ids in creation order (a polygon's construction circle comes first).
    pub entities: Vec<Id>,
    /// Point ids. line: `[start, end]`; polyline: the vertices in order; arc: `[centre, start, end]`; polygon:
    /// `[centre, vertices...]`; slot: `[centre 1, centre 2]`.
    pub points: Vec<Id>,
}

impl Session {
    /// Add a straight segment.
    pub fn sketch_line(&mut self, sketch: Id, l: &LineSpec) -> Result<Added> {
        let pl = PolylineSpec {
            points: vec![Xy(l.x1.clone(), l.y1.clone()), Xy(l.x2.clone(), l.y2.clone())],
            closed: false,
            construction: l.construction,
            dimensioned: l.dimensioned,
        };
        self.sketch_polyline(sketch, &pl)
    }

    /// Add connected segments through the given points.
    pub fn sketch_polyline(&mut self, sketch: Id, pl: &PolylineSpec) -> Result<Added> {
        self.transaction(|s| s.sketch_polyline_inner(sketch, pl))
    }

    fn sketch_polyline_inner(&mut self, sketch: Id, pl: &PolylineSpec) -> Result<Added> {
        let si = self.sketch_si(sketch)?;
        let vars = self.p.param_map();
        let v = eval_points(&pl.points, &vars)?;
        let n = v.len();
        if n < 2 || (pl.closed && n < 3) {
            return Err(Error::Invalid(format!(
                "a {} polyline needs at least {} points, got {n}",
                open_closed(pl.closed),
                2 + pl.closed as usize
            )));
        }
        if pl.closed && same(v[0], v[n - 1]) {
            return Err(Error::Invalid("the last point repeats the first; drop it and keep `closed: true`".into()));
        }
        let segs: Vec<(usize, usize)> = (0..n - 1).map(|i| (i, i + 1)).chain(pl.closed.then_some((n - 1, 0))).collect();
        if let Some((i, j)) = segs.iter().find(|(i, j)| same(v[*i], v[*j])) {
            return Err(Error::Invalid(format!("points {i} and {j} coincide (zero-length segment)")));
        }
        let mut entities = Vec::with_capacity(segs.len());
        for &(i, j) in &segs {
            entities.push(self.p.add_line_entity(si, v[i].0, v[i].1, v[j].0, v[j].1, purpose(pl.construction)));
        }
        let ends: Vec<(Id, Id)> = entities.iter().map(|e| line_ends(&self.p.sketches[si].entities, *e)).collect::<Result<_>>()?;
        let mut points: Vec<Id> = ends.iter().map(|(a, _)| *a).collect();
        if !pl.closed {
            points.push(ends[ends.len() - 1].1);
        }
        if pl.dimensioned {
            let mut dims = Vec::new();
            for (k, &p) in points.iter().enumerate() {
                let Xy(x, y) = &pl.points[k];
                dims.extend(self.pin_dims(si, p, x, v[k].0, y, v[k].1)?);
            }
            self.add_independent(si, dims);
        }
        self.finish_sketch_edit(si)?;
        Ok(Added { entities, points })
    }

    /// Add a circular arc.
    pub fn sketch_arc(&mut self, sketch: Id, a: &ArcSpec) -> Result<Added> {
        self.transaction(|s| s.sketch_arc_inner(sketch, a))
    }

    fn sketch_arc_inner(&mut self, sketch: Id, a: &ArcSpec) -> Result<Added> {
        let si = self.sketch_si(sketch)?;
        let vars = self.p.param_map();
        let c = (a.cx.eval(&vars)?, a.cy.eval(&vars)?);
        enum Form<'a> {
            Angles { r: &'a Num, vr: f64, a0: &'a Num, va0: f64, a1: &'a Num, va1: f64 },
            Points { s: &'a Xy, e: &'a Xy },
        }
        let (form, sp, ep) = match (&a.r, &a.start_angle, &a.end_angle, &a.start, &a.end) {
            (Some(r), Some(a0), Some(a1), None, None) => {
                let (vr, va0, va1) = (r.eval(&vars)?, a0.eval(&vars)?, a1.eval(&vars)?);
                if vr <= 0.0 {
                    return Err(Error::Invalid(format!("arc radius must be positive, got {vr}")));
                }
                if (va1 - va0).rem_euclid(360.0).min((va0 - va1).rem_euclid(360.0)) < 1e-6 {
                    return Err(Error::Invalid("start and end angles coincide; use a circle for a full circle".into()));
                }
                let at = |deg: f64| (c.0 + vr * deg.to_radians().cos(), c.1 + vr * deg.to_radians().sin());
                (Form::Angles { r, vr, a0, va0, a1, va1 }, at(va0), at(va1))
            }
            (None, None, None, Some(s), Some(e)) => {
                let sp = (s.0.eval(&vars)?, s.1.eval(&vars)?);
                let ep = (e.0.eval(&vars)?, e.1.eval(&vars)?);
                let (rs, re) = (len(sp, c), len(ep, c));
                if rs < 1e-6 {
                    return Err(Error::Invalid("the arc start coincides with its centre".into()));
                }
                if (rs - re).abs() > 1e-6 * rs.max(1.0) {
                    return Err(Error::Invalid(format!(
                        "the end point is not on the arc's circle: |start − centre| = {rs}, |end − centre| = {re}"
                    )));
                }
                if same(sp, ep) {
                    return Err(Error::Invalid("start and end coincide; use a circle for a full circle".into()));
                }
                (Form::Points { s, e }, sp, ep)
            }
            _ => return Err(Error::Invalid("give either `r` + `start_angle` + `end_angle`, or `start` + `end`".into())),
        };
        let winding = if a.ccw { Winding::Ccw } else { Winding::Cw };
        let before = self.p.sketches[si].entities.len();
        self.p.add_arc_entity(si, pt(c), pt(sp), pt(ep), winding, purpose(a.construction));
        let entities = new_entities(self, si, before);
        let (center, ps, pe) = match self.p.sketches[si].entities.get(before).map(|e| e.kind) {
            Some(EntityKind::Arc { center, a, b, .. }) => (center, a, b),
            _ => return Err(Error::Invalid("the arc was not created".into())),
        };
        if a.dimensioned {
            let mut dims: Vec<Constraint> = self.pin_dims(si, center, &a.cx, c.0, &a.cy, c.1)?.into();
            match form {
                Form::Angles { r, vr, a0, va0, a1, va1 } => {
                    dims.push(radius_dim(center, vr, r.expr().unwrap_or_default()));
                    let parametric = [r, a0, a1].iter().any(|x| x.expr().is_some());
                    dims.push(self.direction_dim(si, center, ps, r, vr, a0, va0, parametric)?);
                    dims.push(self.direction_dim(si, center, pe, r, vr, a1, va1, parametric)?);
                }
                Form::Points { s, e } => {
                    dims.extend(self.pin_dims(si, ps, &s.0, sp.0, &s.1, sp.1)?);
                    // The end point has one freedom left (along the circle): pin the coordinate that moves
                    // most along it, x where the circle is steep, y where it is flat.
                    let [dx, dy] = self.pin_dims(si, pe, &e.0, ep.0, &e.1, ep.1)?;
                    dims.push(if (ep.1 - c.1).abs() >= (ep.0 - c.0).abs() { dx } else { dy });
                }
            }
            self.add_independent(si, dims);
        }
        self.finish_sketch_edit(si)?;
        Ok(Added { entities, points: vec![center, ps, pe] })
    }

    /// Add a regular polygon.
    pub fn sketch_polygon(&mut self, sketch: Id, g: &PolygonSpec) -> Result<Added> {
        self.transaction(|s| s.sketch_polygon_inner(sketch, g))
    }

    fn sketch_polygon_inner(&mut self, sketch: Id, g: &PolygonSpec) -> Result<Added> {
        let si = self.sketch_si(sketch)?;
        if !(3..=64).contains(&g.sides) {
            return Err(Error::Invalid(format!("a polygon needs 3..=64 sides, got {}", g.sides)));
        }
        let vars = self.p.param_map();
        let c = (g.cx.eval(&vars)?, g.cy.eval(&vars)?);
        let zero_angle = zero();
        let (v, by_radius) = match (&g.r, &g.vertex) {
            (Some(r), None) => {
                let vr = r.eval(&vars)?;
                if vr <= 0.0 {
                    return Err(Error::Invalid(format!("polygon radius must be positive, got {vr}")));
                }
                let ang = g.angle.as_ref().unwrap_or(&zero_angle);
                let va = ang.eval(&vars)?.to_radians();
                ((c.0 + vr * va.cos(), c.1 + vr * va.sin()), Some((r, vr, ang)))
            }
            (None, Some(Xy(x, y))) => {
                if g.angle.is_some() {
                    return Err(Error::Invalid("`angle` goes with `r`; a `vertex` already sets the rotation".into()));
                }
                let v = (x.eval(&vars)?, y.eval(&vars)?);
                if len(v, c) < 1e-6 {
                    return Err(Error::Invalid("the vertex coincides with the centre".into()));
                }
                (v, None)
            }
            _ => return Err(Error::Invalid("give either `r` (and optionally `angle`) or `vertex`".into())),
        };
        let before = self.p.sketches[si].entities.len();
        let (center, _sides) = self.p.add_polygon_param(si, pt(c), pt(v), g.sides, purpose(g.construction));
        let entities = new_entities(self, si, before);
        let verts: Vec<Id> = self.p.sketches[si]
            .constraints
            .iter()
            .filter_map(|k| match *k {
                Constraint::PointOnCircle { p, c } if c == center => Some(p),
                _ => None,
            })
            .collect();
        let radius_idx = self.p.sketches[si].constraints.iter().position(|k| matches!(k, Constraint::Diameter { c, .. } if *c == center));
        let (Some(&v0), Some(ri)) = (verts.first(), radius_idx) else {
            return Err(Error::Invalid("the polygon was not created".into()));
        };
        match by_radius {
            Some((r, vr, ang)) => {
                if let Constraint::Diameter { d, expr, .. } = &mut self.p.sketches[si].constraints[ri] {
                    *d = vr;
                    *expr = r.expr().unwrap_or_default();
                }
                if g.dimensioned {
                    let mut dims: Vec<Constraint> = self.pin_dims(si, center, &g.cx, c.0, &g.cy, c.1)?.into();
                    let va = ang.eval(&vars)?;
                    let parametric = r.expr().is_some() || ang.expr().is_some();
                    dims.push(self.direction_dim(si, center, v0, r, vr, ang, va, parametric)?);
                    self.add_independent(si, dims);
                }
            }
            None if g.dimensioned => {
                // The vertex sets the size: the tool's own radius dimension would make its pin redundant.
                self.p.sketches[si].constraints.remove(ri);
                let Some(Xy(x, y)) = &g.vertex else {
                    return Err(Error::Invalid("give either `r` (and optionally `angle`) or `vertex`".into()));
                };
                let mut dims: Vec<Constraint> = self.pin_dims(si, center, &g.cx, c.0, &g.cy, c.1)?.into();
                dims.extend(self.pin_dims(si, v0, x, v.0, y, v.1)?);
                self.add_independent(si, dims);
            }
            None => {}
        }
        self.finish_sketch_edit(si)?;
        Ok(Added { entities, points: std::iter::once(center).chain(verts).collect() })
    }

    /// Add a slot.
    pub fn sketch_slot(&mut self, sketch: Id, sl: &SlotSpec) -> Result<Added> {
        self.transaction(|s| s.sketch_slot_inner(sketch, sl))
    }

    fn sketch_slot_inner(&mut self, sketch: Id, sl: &SlotSpec) -> Result<Added> {
        let si = self.sketch_si(sketch)?;
        let vars = self.p.param_map();
        let (c1, c2) = ((sl.x1.eval(&vars)?, sl.y1.eval(&vars)?), (sl.x2.eval(&vars)?, sl.y2.eval(&vars)?));
        let w = sl.width.eval(&vars)?;
        if w <= 0.0 {
            return Err(Error::Invalid(format!("slot width must be positive, got {w}")));
        }
        if same(c1, c2) {
            return Err(Error::Invalid("the slot centres coincide; use a circle".into()));
        }
        let before = self.p.sketches[si].entities.len();
        let cons_before = self.p.sketches[si].constraints.len();
        self.p.add_slot_entity(si, pt(c1), pt(c2), w / 2.0, purpose(sl.construction));
        let entities = new_entities(self, si, before);
        // add_slot_entity pushes: side, arc around c2, side, arc around c1.
        let center_of = |s: &Session, k: usize| match s.p.sketches[si].entities.get(before + k).map(|e| e.kind) {
            Some(EntityKind::Arc { center, .. }) => Ok(center),
            _ => Err(Error::Invalid("the slot was not created".into())),
        };
        let (p1, p2) = (center_of(self, 3)?, center_of(self, 1)?);
        tangency_as_perpendicular(&mut self.p.sketches[si], cons_before);
        if sl.dimensioned {
            let mut dims: Vec<Constraint> = self.pin_dims(si, p1, &sl.x1, c1.0, &sl.y1, c1.1)?.into();
            dims.extend(self.pin_dims(si, p2, &sl.x2, c2.0, &sl.y2, c2.1)?);
            dims.push(radius_dim(p1, w / 2.0, sl.width.expr().map(|e| half(&e)).transpose()?.unwrap_or_default()));
            self.add_independent(si, dims);
        }
        self.finish_sketch_edit(si)?;
        Ok(Added { entities, points: vec![p1, p2] })
    }

    /// The driving dimension of the direction from `center` to `p` (on the circle of radius `r` around it), `deg`
    /// degrees counter-clockwise from +x.
    ///
    /// - A plain 0/180 is a horizontal constraint, a plain ±90 a vertical one (no dimension, no kink).
    /// - Other plain angles of a plain-number entity: an angle dimension to the x axis, as a person would draw it.
    ///   QymCAD angles are unsigned (0..180), so the value is folded and the side comes from the geometry.
    /// - Parametric (the angle or the radius is an expression): an arc-length dimension from a construction point
    ///   on the +x side of the centre, `len = r·a·π/180`. QymCAD's arc length is directed (counter-clockwise,
    ///   0..360°), so the parameter may sweep the whole turn (FINDINGS F-3A-7), and it has no soft arm-length
    ///   term, so a radius change settles in one solve (F-3A-2).
    #[allow(clippy::too_many_arguments)]
    fn direction_dim(&mut self, si: usize, center: Id, p: Id, r: &Num, vr: f64, a: &Num, deg: f64, parametric: bool) -> Result<Constraint> {
        let m = fold(deg).abs();
        if a.expr().is_none() {
            if m < 1e-9 || (m - 180.0).abs() < 1e-9 {
                return Ok(Constraint::Horizontal { a: center, b: p });
            }
            if (m - 90.0).abs() < 1e-9 {
                return Ok(Constraint::Vertical { a: center, b: p });
            }
        }
        if !parametric {
            let (o, gx) = self.p.ensure_axis(si, 0);
            return Ok(Constraint::AngleLines {
                a: o,
                b: gx,
                c: center,
                d: p,
                deg: m,
                expr: String::new(),
                driven: false,
                off: 0.0,
                at: None,
            });
        }
        let reference = self.angle_reference(si, center, vr);
        // The turn the value lies in: the dimension measures 0..360 from the reference.
        let turn = (deg / 360.0).floor() * 360.0;
        let txt = |n: &Num, v: f64| n.expr().unwrap_or_else(|| format!("{v}"));
        let ae = if turn == 0.0 { txt(a, deg) } else { format!("({})-({turn})", txt(a, deg)) };
        let len = vr * (deg - turn).to_radians();
        let expr = format!("({})*({ae})*pi/180", txt(r, vr));
        check_expr(&expr)?;
        Ok(Constraint::ArcLength { c: center, a: reference, b: p, ccw: true, len, off: 0.0, expr, driven: false })
    }

    /// A construction point on the circle around `center`, on its +x side (`Horizontal` + `PointOnCircle`):
    /// the zero of the arc-length dimensions that give directions from that centre. One per centre.
    fn angle_reference(&mut self, si: usize, center: Id, r: f64) -> Id {
        let s = &self.p.sketches[si];
        let on_entity = |p: Id| {
            s.entities.iter().any(|e| match e.kind {
                EntityKind::Line { a, b } => a == p || b == p,
                EntityKind::Arc { center, a, b, .. } => center == p || a == p || b == p,
                EntityKind::Circle { center, .. } => center == p,
                EntityKind::Ellipse { c, ma, mi } => c == p || ma == p || mi == p,
            })
        };
        let existing = s.constraints.iter().find_map(|c| match *c {
            Constraint::Horizontal { a, b } if a == center && !on_entity(b) => {
                s.constraints.iter().any(|k| matches!(*k, Constraint::PointOnCircle { p, c } if p == b && c == center)).then_some(b)
            }
            _ => None,
        });
        if let Some(id) = existing {
            return id;
        }
        let (cx, cy) = s.points.iter().find(|q| q.id == center).map(|q| (q.x, q.y)).unwrap_or((0.0, 0.0));
        let id = self.p.alloc_id();
        let s = &mut self.p.sketches[si];
        s.points.push(qymcad_core::model::SketchPoint { id, x: cx + r, y: cy });
        s.constraints.push(Constraint::Horizontal { a: center, b: id });
        s.constraints.push(Constraint::PointOnCircle { p: id, c: center });
        id
    }
}

/// `deg` mapped into (-180, 180].
fn fold(deg: f64) -> f64 {
    let f = deg - 360.0 * (deg / 360.0).round();
    if f <= -180.0 {
        f + 360.0
    } else {
        f
    }
}

/// `e/2`, parenthesised unless `e` is a single name or number.
fn half(e: &str) -> Result<String> {
    let expr = if e.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') { format!("{e}/2") } else { format!("({e})/2") };
    check_expr(&expr)?;
    Ok(expr)
}

/// Rewrite the slot's `Tangent { a, b, c }` constraints (line a-b touches the circle around c) as
/// `Perpendicular` between the side and the radius to its contact point (the endpoint on that circle).
///
/// The geometry is the same — the contact point already lies on the circle (arc intrinsic) — but a tangency at
/// its own contact point is a second-order condition: its Jacobian row is parallel to the intrinsic's, so
/// `sketch_dof` counts a fully dimensioned QymCAD slot as 4 free + 4 redundant (FINDINGS F-3A-3). The
/// perpendicular is first-order, so the slot reports (0, 0) and the rank-based over-constraint check of
/// `sketch_constrain` stays meaningful.
fn tangency_as_perpendicular(sk: &mut qymcad_core::model::Sketch, from: usize) {
    let arc_ends: Vec<(Id, Id, Id)> = sk
        .entities
        .iter()
        .filter_map(|e| match e.kind {
            EntityKind::Arc { center, a, b, .. } => Some((center, a, b)),
            _ => None,
        })
        .collect();
    let touches = |p: Id, c: Id| arc_ends.iter().any(|&(cc, a, b)| cc == c && (a == p || b == p));
    for k in from..sk.constraints.len() {
        if let Constraint::Tangent { a, b, c, .. } = sk.constraints[k] {
            let contact = if touches(a, c) {
                a
            } else if touches(b, c) {
                b
            } else {
                continue;
            };
            sk.constraints[k] = Constraint::Perpendicular { a, b, c, d: contact };
        }
    }
}

fn radius_dim(c: Id, r: f64, expr: String) -> Constraint {
    Constraint::Diameter { c, d: r, off: 0.0, expr, driven: false, diam: false, at: None }
}

fn eval_points(pts: &[Xy], vars: &HashMap<String, f64>) -> Result<Vec<(f64, f64)>> {
    pts.iter().map(|Xy(x, y)| Ok((x.eval(vars)?, y.eval(vars)?))).collect()
}

fn line_ends(ents: &[qymcad_core::model::SketchEntity], id: Id) -> Result<(Id, Id)> {
    match ents.iter().find(|e| e.id == id).map(|e| e.kind) {
        Some(EntityKind::Line { a, b }) => Ok((a, b)),
        _ => Err(Error::Invalid(format!("line {id} was not created"))),
    }
}

fn new_entities(s: &Session, si: usize, before: usize) -> Vec<Id> {
    s.p.sketches[si].entities[before..].iter().map(|e| e.id).collect()
}

fn pt((x, y): (f64, f64)) -> Point2 {
    Point2::new(x, y)
}

fn len(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Points QymCAD would merge into one (`add_line_entity` deduplicates within 1e-6).
fn same(a: (f64, f64), b: (f64, f64)) -> bool {
    len(a, b) <= 1e-6
}

fn open_closed(closed: bool) -> &'static str {
    if closed {
        "closed"
    } else {
        "open"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_fold_into_the_unsigned_range() {
        assert_eq!(fold(180.0), 180.0);
        assert_eq!(fold(-180.0), 180.0);
        assert_eq!(half("w").unwrap(), "w/2");
        assert_eq!(half("w+2").unwrap(), "(w+2)/2");
    }
}
