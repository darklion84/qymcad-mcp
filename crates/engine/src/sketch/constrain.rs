//! Geometric constraints and driving dimensions between existing sketch points and entities, and removal of
//! entities and constraints.
//!
//! A constraint that would over-constrain the sketch is refused (rank analysis: `sketch_dof` before and after),
//! except a geometric one that is already implied and satisfied, which is simply not added. A dimension that
//! is independent but cannot be satisfied fails the solve and is rolled back.

use super::dist;
use crate::error::{Error, Result};
use crate::session::Session;
use crate::value::Num;
use qymcad_core::model::{Constraint, EntityKind, Id};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A reference to sketch geometry: a point or entity id from `sketch_info`, or the sketch's frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SketchRef {
    /// A point id or an entity id (line, arc, circle) from `sketch_info`.
    Id(Id),
    /// `"origin"` (a point), `"x_axis"` or `"y_axis"` (lines).
    Frame(FrameRef),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FrameRef {
    Origin,
    XAxis,
    YAxis,
}

/// What to constrain. Geometric constraints take no value; dimensions (`distance`, `angle`, `diameter`,
/// `radius`) take one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    /// [point, point]: the points coincide (two circles/arcs: concentric).
    Coincident,
    /// [line] or [point, point]: horizontal.
    Horizontal,
    /// [line] or [point, point]: vertical.
    Vertical,
    /// [line, line].
    Parallel,
    /// [line, line].
    Perpendicular,
    /// [line, line]: on one straight line.
    Collinear,
    /// [line, line]: equal length; [circle/arc, circle/arc]: equal radius.
    Equal,
    /// [line, circle/arc] or [circle/arc, circle/arc] (external or internal, as they are now).
    Tangent,
    /// [circle/arc or point, circle/arc or point]: same centre.
    Concentric,
    /// [point, line]: the point is the middle of the line.
    Midpoint,
    /// [point, line]: the point lies on the (infinite) line; [point, circle/arc]: on the circle.
    PointOnLine,
    /// [point, point, line]: the points are mirror images about the line.
    Symmetric,
    /// [point]: pinned where it is now.
    Fix,
    /// [point, point], [line] (its length), [point, line] or [line, line] (perpendicular distance). Circles and
    /// arcs stand for their centre. `axis` x/y measures only along that axis (point pairs and lines).
    Distance,
    /// [line, line]: the angle between them, degrees (0..180). The line directions are picked from the
    /// current geometry so the closer of θ and 180−θ is meant.
    Angle,
    /// [circle/arc].
    Diameter,
    /// [circle/arc].
    Radius,
}

/// Direction of a distance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DistAxis {
    /// The straight distance (default).
    #[default]
    Aligned,
    /// Only the x difference.
    X,
    /// Only the y difference.
    Y,
}

/// A request to `sketch_constrain`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstrainSpec {
    pub kind: ConstraintKind,
    pub refs: Vec<SketchRef>,
    pub value: Option<Num>,
    pub axis: DistAxis,
    /// A reference (driven) dimension: shows the measured value, constrains nothing.
    pub reference: bool,
}

/// The outcome of `sketch_constrain`.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Constrained {
    /// Index of the new constraint (`sketch_info.constraints`); `None` when it was already implied.
    pub index: Option<usize>,
    /// Set when nothing was added, and why.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The sketch's degrees of freedom and redundant constraints afterwards.
    pub dof: (i32, i32),
}

/// Resolved geometry.
#[derive(Clone, Copy, Debug)]
enum G {
    Point(Id),
    Line(Id, Id),
    Curve { center: Id, r: f64 },
}

impl G {
    fn desc(&self) -> &'static str {
        match self {
            G::Point(_) => "point",
            G::Line(..) => "line",
            G::Curve { .. } => "circle/arc",
        }
    }

    /// A point, or the centre of a circle/arc.
    fn point(&self) -> Option<Id> {
        match *self {
            G::Point(p) | G::Curve { center: p, .. } => Some(p),
            G::Line(..) => None,
        }
    }
}

impl Session {
    /// Add a geometric constraint or a driving dimension between existing points/entities of a sketch. Refused
    /// (and nothing changes) when it over-constrains the sketch or cannot be satisfied.
    pub fn sketch_constrain(&mut self, sketch: Id, c: &ConstrainSpec) -> Result<Constrained> {
        self.transaction(|s| s.sketch_constrain_inner(sketch, c))
    }

    fn sketch_constrain_inner(&mut self, sketch: Id, spec: &ConstrainSpec) -> Result<Constrained> {
        let si = self.sketch_si(sketch)?;
        let gs: Vec<G> = spec.refs.iter().map(|r| self.geo(si, sketch, r)).collect::<Result<_>>()?;
        let c = self.build_constraint(si, spec, &gs)?;
        let is_dim = c.dim_value().is_some();
        let before = self.p.sketch_dof(si);
        let idx = self.p.sketches[si].constraints.len();
        self.p.sketches[si].constraints.push(c);
        if !spec.reference {
            let after = self.p.sketch_dof(si);
            if after.1 > before.1 {
                let satisfied = self.p.sketch_residuals(si).get(idx).is_some_and(|r| *r < 1e-6);
                let mut with: Vec<usize> = self.p.sketch_redundant_constraints(si);
                with.retain(|&i| i != idx);
                if !is_dim && satisfied && after.0 == before.0 {
                    self.p.sketches[si].constraints.pop();
                    return Ok(Constrained {
                        index: None,
                        note: Some(format!("already implied by constraints {with:?}; nothing added")),
                        dof: before,
                    });
                }
                let advice = if is_dim {
                    "Remove the dimension it duplicates first (sketch_remove), or add this one with `reference: true` to only show the value."
                } else {
                    "Remove one of those constraints first (sketch_remove)."
                };
                return Err(Error::Invalid(format!(
                    "this {} over-constrains sketch {sketch}: it is {} by constraints {with:?} (see sketch_info). {advice}",
                    kind_name(spec.kind),
                    if satisfied { "already determined" } else { "contradicted" },
                )));
            }
        }
        self.finish_sketch_edit(si)?;
        Ok(Constrained { index: Some(idx), note: None, dof: self.p.sketch_dof(si) })
    }

    /// Delete an entity together with the points only it used and the constraints on them; a helper point left
    /// with only its own dimensions (e.g. a rectangle's centre) goes too.
    pub fn sketch_remove_entity(&mut self, sketch: Id, entity: Id) -> Result<()> {
        self.transaction(|s| {
            let si = s.sketch_si(sketch)?;
            if !s.p.sketches[si].entities.iter().any(|e| e.id == entity) {
                return Err(Error::NotFound(format!("entity {entity} in sketch {sketch} (see sketch_info)")));
            }
            // `delete_entities` keeps only points that entities use (and the frame and midpoints), so it would drop
            // every spline's control points (FINDINGS F-3A-8). Spline points are marked as midpoints of themselves
            // for the call, which it protects, and the markers removed afterwards.
            let marker = |p: Id| Constraint::Midpoint { p, a: p, b: p };
            let spline_pts: Vec<Id> = s.p.sketches[si].splines.iter().flat_map(|sp| sp.points.iter().copied()).collect();
            s.p.sketches[si].constraints.extend(spline_pts.iter().map(|&p| marker(p)));
            s.p.delete_entities(si, &[entity]);
            s.p.sketches[si].constraints.retain(|c| !matches!(*c, Constraint::Midpoint { p, a, b } if p == a && a == b));
            s.prune_debris(si);
            s.finish_sketch_edit(si)
        })
    }

    /// Delete constraint `index` (indices of later constraints shift down by one).
    pub fn sketch_remove_constraint(&mut self, sketch: Id, index: usize) -> Result<()> {
        self.transaction(|s| {
            let si = s.sketch_si(sketch)?;
            let sk = &s.p.sketches[si];
            let c = sk.constraints.get(index).ok_or_else(|| {
                Error::NotFound(format!("constraint {index} in sketch {sketch} (it has {}; see sketch_info)", sk.constraints.len()))
            })?;
            if matches!(c, Constraint::Fixed { p } if sk.system_ids().contains(p)) {
                return Err(Error::Invalid(format!("constraint {index} anchors the sketch's origin/axes and cannot be removed")));
            }
            s.p.delete_sketch_constraint(si, index);
            s.finish_sketch_edit(si)
        })
    }

    /// Remove points that no entity uses and that are tied to no live geometry by a constraint (only to the
    /// frame or to other such points), with their constraints.
    fn prune_debris(&mut self, si: usize) {
        let s = &mut self.p.sketches[si];
        let sys: HashSet<Id> = s.system_ids().into_iter().collect();
        let mut used: HashSet<Id> = s.entities.iter().flat_map(|e| entity_points(&e.kind)).collect();
        used.extend(s.splines.iter().flat_map(|sp| sp.points.iter().copied()));
        let orphan: HashSet<Id> = s.points.iter().map(|p| p.id).filter(|id| !used.contains(id) && !sys.contains(id)).collect();
        let tied: HashSet<Id> = s
            .constraints
            .iter()
            .map(|c| c.points())
            .filter(|pts| pts.iter().any(|p| used.contains(p)))
            .flatten()
            .filter(|p| orphan.contains(p))
            .collect();
        let debris: HashSet<Id> = orphan.difference(&tied).copied().collect();
        if debris.is_empty() {
            return;
        }
        s.points.retain(|p| !debris.contains(&p.id));
        s.constraints.retain(|c| !c.points().iter().any(|p| debris.contains(p)));
    }

    fn geo(&mut self, si: usize, sketch: Id, r: &SketchRef) -> Result<G> {
        let id = match *r {
            SketchRef::Frame(FrameRef::Origin) => return Ok(G::Point(self.p.ensure_origin(si))),
            SketchRef::Frame(FrameRef::XAxis) => {
                let (a, b) = self.p.ensure_axis(si, 0);
                return Ok(G::Line(a, b));
            }
            SketchRef::Frame(FrameRef::YAxis) => {
                let (a, b) = self.p.ensure_axis(si, 1);
                return Ok(G::Line(a, b));
            }
            SketchRef::Id(id) => id,
        };
        let s = &self.p.sketches[si];
        if let Some(e) = s.entities.iter().find(|e| e.id == id) {
            return match e.kind {
                EntityKind::Line { a, b } => Ok(G::Line(a, b)),
                EntityKind::Circle { center, r } => Ok(G::Curve { center, r }),
                EntityKind::Arc { center, a, .. } => {
                    let r = match (self.xy(si, center), self.xy(si, a)) {
                        (Some(c), Some(p)) => (p.0 - c.0).hypot(p.1 - c.1),
                        _ => 0.0,
                    };
                    Ok(G::Curve { center, r })
                }
                EntityKind::Ellipse { .. } => Err(Error::Invalid(format!("entity {id} is an ellipse; constrain its points instead"))),
            };
        }
        if s.points.iter().any(|p| p.id == id) {
            return Ok(G::Point(id));
        }
        Err(Error::NotFound(format!("no point or entity {id} in sketch {sketch} (see sketch_info)")))
    }

    fn xy(&self, si: usize, id: Id) -> Option<(f64, f64)> {
        self.p.sketches[si].points.iter().find(|p| p.id == id).map(|p| (p.x, p.y))
    }

    fn build_constraint(&self, si: usize, spec: &ConstrainSpec, gs: &[G]) -> Result<Constraint> {
        use ConstraintKind as K;
        let kind = spec.kind;
        let is_dim = matches!(kind, K::Distance | K::Angle | K::Diameter | K::Radius);
        if !is_dim {
            if spec.value.is_some() {
                return Err(Error::Invalid(format!("`{}` takes no value", kind_name(kind))));
            }
            if spec.reference {
                return Err(Error::Invalid("`reference` applies to dimensions only".into()));
            }
        }
        if kind != K::Distance && spec.axis != DistAxis::Aligned {
            return Err(Error::Invalid("`axis` applies to distances only".into()));
        }
        let got = || gs.iter().map(G::desc).collect::<Vec<_>>().join(", ");
        let wrong = |want: &str| Error::Invalid(format!("`{}` takes {want}; got [{}]", kind_name(kind), got()));
        let pt = |g: &G, want: &str| g.point().ok_or_else(|| wrong(want));
        // The value of a dimension: (number, expression). Measured when it is a reference dimension.
        let value = |measured: f64| -> Result<(f64, String)> {
            match (&spec.value, spec.reference) {
                (Some(v), _) => {
                    let x = v.eval(&self.p.param_map())?;
                    if x <= 0.0 && !spec.reference {
                        return Err(Error::Invalid(format!(
                            "a {} must be positive, got {x} (for zero use coincident, horizontal/vertical or point_on_line)",
                            kind_name(kind)
                        )));
                    }
                    Ok((x, v.expr().unwrap_or_default()))
                }
                (None, true) => Ok((measured, String::new())),
                (None, false) => Err(Error::Invalid(format!("a {} needs a `value`", kind_name(kind)))),
            }
        };
        let driven = spec.reference;
        Ok(match (kind, gs) {
            (K::Coincident, [G::Curve { center: a, .. }, G::Curve { center: b, .. }]) => Constraint::Concentric { c1: *a, c2: *b },
            (K::Coincident, [a, b]) => Constraint::Coincident { a: pt(a, "[point, point]")?, b: pt(b, "[point, point]")? },
            (K::Horizontal, [G::Line(a, b)]) => Constraint::Horizontal { a: *a, b: *b },
            (K::Vertical, [G::Line(a, b)]) => Constraint::Vertical { a: *a, b: *b },
            (K::Horizontal, [a, b]) => {
                Constraint::Horizontal { a: pt(a, "[line] or [point, point]")?, b: pt(b, "[line] or [point, point]")? }
            }
            (K::Vertical, [a, b]) => Constraint::Vertical { a: pt(a, "[line] or [point, point]")?, b: pt(b, "[line] or [point, point]")? },
            (K::Parallel, [G::Line(a, b), G::Line(c, d)]) => Constraint::Parallel { a: *a, b: *b, c: *c, d: *d },
            (K::Perpendicular, [G::Line(a, b), G::Line(c, d)]) => Constraint::Perpendicular { a: *a, b: *b, c: *c, d: *d },
            (K::Collinear, [G::Line(a, b), G::Line(c, d)]) => Constraint::Collinear { a: *a, b: *b, c: *c, d: *d },
            (K::Equal, [G::Line(a, b), G::Line(c, d)]) => Constraint::Equal { a: *a, b: *b, c: *c, d: *d },
            (K::Equal, [G::Curve { center: a, .. }, G::Curve { center: b, .. }]) => Constraint::EqualRadius { c1: *a, c2: *b },
            (K::Tangent, [G::Line(a, b), G::Curve { center, r }] | [G::Curve { center, r }, G::Line(a, b)]) => {
                // A line that ends on the arc touches it at that end: a perpendicular to the radius there is the
                // same condition, but first-order (a Tangent at its own contact point is rank-deficient and would
                // be refused as redundant; FINDINGS F-3A-3).
                let ends_on_arc = |p: Id| {
                    self.p.sketches[si]
                        .entities
                        .iter()
                        .any(|e| matches!(e.kind, EntityKind::Arc { center: c, a: ea, b: eb, .. } if c == *center && (ea == p || eb == p)))
                };
                match [*a, *b].into_iter().find(|p| ends_on_arc(*p)) {
                    Some(contact) => Constraint::Perpendicular { a: *a, b: *b, c: *center, d: contact },
                    None => Constraint::Tangent { a: *a, b: *b, c: *center, r: *r },
                }
            }
            (K::Tangent, [G::Curve { center: c1, r: r1 }, G::Curve { center: c2, r: r2 }]) => {
                let d = self.distance(si, *c1, *c2);
                Constraint::CircleTangent { c1: *c1, c2: *c2, external: (d - (r1 + r2)).abs() <= (d - (r1 - r2).abs()).abs() }
            }
            (K::Concentric, [a, b]) => {
                Constraint::Concentric { c1: pt(a, "[circle/arc, circle/arc]")?, c2: pt(b, "[circle/arc, circle/arc]")? }
            }
            (K::Midpoint, [p, G::Line(a, b)] | [G::Line(a, b), p]) => Constraint::Midpoint { p: pt(p, "[point, line]")?, a: *a, b: *b },
            (K::PointOnLine, [p, G::Line(a, b)] | [G::Line(a, b), p]) => {
                Constraint::PointOnLine { p: pt(p, "[point, line]")?, a: *a, b: *b }
            }
            (K::PointOnLine, [p @ G::Point(_), G::Curve { center, .. }] | [G::Curve { center, .. }, p @ G::Point(_)]) => {
                Constraint::PointOnCircle { p: pt(p, "[point, circle/arc]")?, c: *center }
            }
            (K::Symmetric, [a, b, G::Line(la, lb)]) => {
                Constraint::Symmetric { a: pt(a, "[point, point, line]")?, b: pt(b, "[point, point, line]")?, la: *la, lb: *lb }
            }
            (K::Fix, [p]) => Constraint::Fixed { p: pt(p, "[point]")? },
            (K::Distance, [G::Line(a, b)]) => self.point_distance(si, *a, *b, spec.axis, &value, driven)?,
            (K::Distance, [p, G::Line(a, b)] | [G::Line(a, b), p]) if p.point().is_some() => {
                self.line_distance(si, p.point().unwrap_or_default(), *a, *b, spec.axis, &value, driven)?
            }
            (K::Distance, [G::Line(p, _), G::Line(a, b)]) => self.line_distance(si, *p, *a, *b, spec.axis, &value, driven)?,
            (K::Distance, [a, b]) => {
                let want = "[point, point], [line], [point, line] or [line, line]";
                self.point_distance(si, pt(a, want)?, pt(b, want)?, spec.axis, &value, driven)?
            }
            (K::Angle, [G::Line(a, b), G::Line(c, d)]) => {
                let (pa, pb, pc, pd) = (self.xy_or0(si, *a), self.xy_or0(si, *b), self.xy_or0(si, *c), self.xy_or0(si, *d));
                let (ux, uy, vx, vy) = (pb.0 - pa.0, pb.1 - pa.1, pd.0 - pc.0, pd.1 - pc.1);
                let now = (ux * vy - uy * vx).abs().atan2(ux * vx + uy * vy).to_degrees();
                let (deg, expr) = value(now)?;
                if deg > 180.0 {
                    return Err(Error::Invalid(format!("an angle between lines is 0..180 degrees, got {deg}")));
                }
                // The supplement is the same pair of lines: pick the directions that make the request the nearer one.
                let (c, d) = if (180.0 - now - deg).abs() < (now - deg).abs() { (*d, *c) } else { (*c, *d) };
                Constraint::AngleLines { a: *a, b: *b, c, d, deg, expr, driven, off: 0.0, at: None }
            }
            (K::Diameter | K::Radius, [G::Curve { center, r }]) => {
                let diam = kind == K::Diameter;
                let (d, expr) = value(if diam { 2.0 * r } else { *r })?;
                Constraint::Diameter { c: *center, d, off: 0.0, expr, driven, diam, at: None }
            }
            _ => return Err(wrong(expected(kind))),
        })
    }

    #[allow(clippy::type_complexity)]
    fn point_distance(
        &self,
        si: usize,
        a: Id,
        b: Id,
        axis: DistAxis,
        value: &dyn Fn(f64) -> Result<(f64, String)>,
        driven: bool,
    ) -> Result<Constraint> {
        let (pa, pb) = (self.xy_or0(si, a), self.xy_or0(si, b));
        let (code, now) = match axis {
            DistAxis::Aligned => (0, (pa.0 - pb.0).hypot(pa.1 - pb.1)),
            DistAxis::X => (1, (pa.0 - pb.0).abs()),
            DistAxis::Y => (2, (pa.1 - pb.1).abs()),
        };
        let (d, expr) = value(now)?;
        let mut c = dist(a, b, code, d, expr);
        if let Constraint::Distance { driven: dr, .. } = &mut c {
            *dr = driven;
        }
        Ok(c)
    }

    /// Perpendicular distance from `p` to the line a-b. QymCAD stores it signed (the side): the sign is read
    /// from the current geometry; an expression is evaluated as a magnitude with that sign (FINDINGS).
    #[allow(clippy::too_many_arguments)]
    fn line_distance(
        &self,
        si: usize,
        p: Id,
        a: Id,
        b: Id,
        axis: DistAxis,
        value: &dyn Fn(f64) -> Result<(f64, String)>,
        driven: bool,
    ) -> Result<Constraint> {
        if axis != DistAxis::Aligned {
            return Err(Error::Invalid("a distance to a line is perpendicular to it; leave `axis` out".into()));
        }
        let (pp, pa, pb) = (self.xy_or0(si, p), self.xy_or0(si, a), self.xy_or0(si, b));
        let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
        let signed = (dx * (pp.1 - pa.1) - dy * (pp.0 - pa.0)) / dx.hypot(dy).max(1e-9);
        let (d, expr) = value(signed.abs())?;
        let d = if signed < 0.0 { -d.abs() } else { d.abs() };
        Ok(Constraint::DistancePL { p, a, b, d, off: 0.0, expr, driven, at: None })
    }

    fn xy_or0(&self, si: usize, id: Id) -> (f64, f64) {
        self.xy(si, id).unwrap_or((0.0, 0.0))
    }

    fn distance(&self, si: usize, a: Id, b: Id) -> f64 {
        let (pa, pb) = (self.xy_or0(si, a), self.xy_or0(si, b));
        (pa.0 - pb.0).hypot(pa.1 - pb.1)
    }
}

fn entity_points(k: &EntityKind) -> Vec<Id> {
    match *k {
        EntityKind::Line { a, b } => vec![a, b],
        EntityKind::Arc { center, a, b, .. } => vec![center, a, b],
        EntityKind::Circle { center, .. } => vec![center],
        EntityKind::Ellipse { c, ma, mi } => vec![c, ma, mi],
    }
}

fn kind_name(k: ConstraintKind) -> &'static str {
    use ConstraintKind as K;
    match k {
        K::Coincident => "coincident",
        K::Horizontal => "horizontal",
        K::Vertical => "vertical",
        K::Parallel => "parallel",
        K::Perpendicular => "perpendicular",
        K::Collinear => "collinear",
        K::Equal => "equal",
        K::Tangent => "tangent",
        K::Concentric => "concentric",
        K::Midpoint => "midpoint",
        K::PointOnLine => "point_on_line",
        K::Symmetric => "symmetric",
        K::Fix => "fix",
        K::Distance => "distance",
        K::Angle => "angle",
        K::Diameter => "diameter",
        K::Radius => "radius",
    }
}

fn expected(k: ConstraintKind) -> &'static str {
    use ConstraintKind as K;
    match k {
        K::Coincident => "[point, point]",
        K::Horizontal | K::Vertical => "[line] or [point, point]",
        K::Parallel | K::Perpendicular | K::Collinear | K::Angle => "[line, line]",
        K::Equal => "[line, line] or [circle/arc, circle/arc]",
        K::Tangent => "[line, circle/arc] or [circle/arc, circle/arc]",
        K::Concentric => "[circle/arc, circle/arc]",
        K::Midpoint => "[point, line]",
        K::PointOnLine => "[point, line] or [point, circle/arc]",
        K::Symmetric => "[point, point, line]",
        K::Fix => "[point]",
        K::Distance => "[point, point], [line], [point, line] or [line, line]",
        K::Diameter | K::Radius => "[circle/arc]",
    }
}
