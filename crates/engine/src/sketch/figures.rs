//! Native ellipse and rectangle figures, held by ordinary driving dimensions.
use super::{dist, purpose, Added};
use crate::error::{Error, Result};
use crate::session::Session;
use crate::value::{check_expr, Num};
use qymcad_core::feature::Winding;
use qymcad_core::geom::Point2;
use qymcad_core::model::{Constraint, EllipseArcSpec as NativeArc, EllipseParts, Id};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn zero() -> Num {
    Num::Value(0.0)
}
fn yes() -> bool {
    true
}

/// An ellipse whose radii are the semi-axis lengths, in its rotated local frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EllipseSpec {
    /// Centre x, in mm.
    #[serde(default = "zero")]
    pub cx: Num,
    /// Centre y, in mm.
    #[serde(default = "zero")]
    pub cy: Num,
    /// Positive first semi-axis length, in mm.
    pub rx: Num,
    /// Positive perpendicular semi-axis length, in mm.
    pub ry: Num,
    /// First axis rotation in degrees: a literal 0 or 180, or a value strictly between 0 and 180.
    #[serde(default = "zero")]
    pub turn: Num,
    /// Construction geometry does not participate in profiles.
    #[serde(default)]
    pub construction: bool,
    /// Add the complete set of driving dimensions (default true).
    #[serde(default = "yes")]
    pub dimensioned: bool,
}

/// An elliptical arc with eccentric (not polar) endpoint angles, in degrees.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EllipseArcSpec {
    /// Centre x, in mm.
    #[serde(default = "zero")]
    pub cx: Num,
    /// Centre y, in mm.
    #[serde(default = "zero")]
    pub cy: Num,
    /// Positive first semi-axis length, in mm.
    pub rx: Num,
    /// Positive perpendicular semi-axis length, in mm.
    pub ry: Num,
    /// First axis rotation in degrees, in the same range as an ellipse.
    #[serde(default = "zero")]
    pub turn: Num,
    /// Start eccentric angle, strictly between 0 and 180 degrees.
    pub start_angle: Num,
    /// End eccentric angle, strictly between 0 and 180 degrees; distinct from start.
    pub end_angle: Num,
    /// Counter-clockwise directed span (default true); otherwise clockwise.
    #[serde(default = "yes")]
    pub ccw: bool,
    /// Construction geometry does not participate in profiles.
    #[serde(default)]
    pub construction: bool,
    /// Add the complete set of driving dimensions (default true).
    #[serde(default = "yes")]
    pub dimensioned: bool,
}

/// The anchor retained by the native rectangle figure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RectAnchor {
    /// Size changes grow equally about the given centre.
    #[default]
    Centre,
    /// The given coordinate is the first corner; the first side follows the turn.
    Corner,
}

/// A native rectangle, preserving its anchor, four sides and figure metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RectSpec {
    /// Anchor x, in mm (centre by default, first corner with `anchor: corner`).
    #[serde(default = "zero")]
    pub cx: Num,
    /// Anchor y, in mm.
    #[serde(default = "zero")]
    pub cy: Num,
    /// Positive first-side length, in mm.
    pub w: Num,
    /// Positive second-side length, in mm.
    pub h: Num,
    /// Turn in degrees: literal 0 or 180, or strictly between 0 and 180.
    #[serde(default = "zero")]
    pub turn: Num,
    /// Centre or corner anchor (default centre).
    #[serde(default)]
    pub anchor: RectAnchor,
    /// Construction geometry does not participate in profiles.
    #[serde(default)]
    pub construction: bool,
}

impl Session {
    /// Add a native ellipse and, by default, fully define its frame with formula dimensions.
    pub fn sketch_ellipse(&mut self, sketch: Id, spec: &EllipseSpec) -> Result<Added> {
        self.transaction(|s| {
            let si = s.sketch_si(sketch)?;
            let vars = s.p.param_map();
            let (cx, cy, rx, ry, turn) =
                (spec.cx.eval(&vars)?, spec.cy.eval(&vars)?, spec.rx.eval(&vars)?, spec.ry.eval(&vars)?, spec.turn.eval(&vars)?);
            positive_axes(rx, ry)?;
            check_turn(&spec.turn, turn)?;
            let centre = s.p.add_ellipse_entity(si, Point2::new(cx, cy), rx, ry, turn.to_radians(), purpose(spec.construction));
            let (id, parts) = s.p.sketches[si]
                .entities
                .iter()
                .find_map(|e| e.kind.as_ellipse().filter(|el| el.c == centre && el.span.is_none()).map(|el| (e.id, el)))
                .ok_or_else(|| Error::Invalid("ellipse was not created".into()))?;
            if spec.dimensioned {
                s.dimension_ellipse(si, parts, spec, [cx, cy, rx, ry, turn])?;
            }
            s.finish_sketch_edit(si)?;
            Ok(Added { entities: vec![id], points: vec![parts.c, parts.ma, parts.mi] })
        })
    }

    /// Add a native elliptical arc on the upper eccentric half-plane, with a retained directed span.
    pub fn sketch_ellipse_arc(&mut self, sketch: Id, spec: &EllipseArcSpec) -> Result<Added> {
        self.transaction(|s| {
            let si = s.sketch_si(sketch)?;
            let vars = s.p.param_map();
            let (cx, cy, rx, ry, turn, start, end) = (
                spec.cx.eval(&vars)?,
                spec.cy.eval(&vars)?,
                spec.rx.eval(&vars)?,
                spec.ry.eval(&vars)?,
                spec.turn.eval(&vars)?,
                spec.start_angle.eval(&vars)?,
                spec.end_angle.eval(&vars)?,
            );
            positive_axes(rx, ry)?;
            check_turn(&spec.turn, turn)?;
            if !(0.0 < start && start < 180.0 && 0.0 < end && end < 180.0) || (start - end).abs() < 1e-7 {
                return Err(Error::Invalid(
                    "ellipse arc start_angle and end_angle must be distinct and strictly within the supported range 0..180 degrees".into(),
                ));
            }
            let id = s.p.add_ellipse_arc_entity(
                si,
                NativeArc {
                    centre: Point2::new(cx, cy),
                    radii: Point2::new(rx, ry),
                    rot: turn.to_radians(),
                    angles: [start.to_radians(), end.to_radians()],
                    winding: if spec.ccw { Winding::Ccw } else { Winding::Cw },
                },
                purpose(spec.construction),
            );
            let parts = s.p.sketches[si]
                .entity(id)
                .and_then(|e| e.kind.as_ellipse())
                .ok_or_else(|| Error::Invalid("ellipse arc was not created".into()))?;
            let span = parts.span.ok_or_else(|| Error::Invalid("ellipse arc has no span".into()))?;
            if spec.dimensioned {
                let frame = EllipseSpec {
                    cx: spec.cx.clone(),
                    cy: spec.cy.clone(),
                    rx: spec.rx.clone(),
                    ry: spec.ry.clone(),
                    turn: spec.turn.clone(),
                    construction: spec.construction,
                    dimensioned: true,
                };
                s.dimension_ellipse(si, parts, &frame, [cx, cy, rx, ry, turn])?;
                for (p, t, v) in [(span.from, &spec.start_angle, start), (span.to, &spec.end_angle, end)] {
                    let (a, b, t) = (text(&spec.rx, rx), text(&spec.ry, ry), text(t, v));
                    // The last term is zero inside the supported open half-plane and undefined outside it.
                    // It preserves the branch boundary in native saved formulas, including app parameter edits.
                    let expr = format!("acos(({a})*cos({t})/sqrt((({a})*cos({t}))^2+(({b})*sin({t}))^2))+0/sqrt(({t})*(180-({t})))");
                    check_expr(&expr)?;
                    let deg = Num::Expr(expr.clone()).eval(&vars)?;
                    s.add_independent(si, [angle(parts.c, parts.ma, parts.c, p, deg, expr)]);
                }
            }
            s.finish_sketch_edit(si)?;
            Ok(Added { entities: vec![id], points: vec![parts.c, parts.ma, parts.mi, span.from, span.to] })
        })
    }

    fn dimension_ellipse(&mut self, si: usize, parts: EllipseParts, spec: &EllipseSpec, v: [f64; 5]) -> Result<()> {
        self.pin_point(si, parts.c, &spec.cx, v[0], &spec.cy, v[1])?;
        self.add_independent(
            si,
            [
                dist(parts.c, parts.ma, 0, v[2], spec.rx.magnitude_expr(v[2])?),
                dist(parts.c, parts.mi, 0, v[3], spec.ry.magnitude_expr(v[3])?),
            ],
        );
        if spec.turn.expr().is_none() && (v[4] == 0.0 || v[4] == 180.0) {
            self.add_independent(si, [Constraint::Horizontal { a: parts.c, b: parts.ma }]);
        } else {
            let (o, x) = self.p.ensure_axis(si, 0);
            self.add_independent(si, [angle(o, x, parts.c, parts.ma, v[4], turn_expr(&spec.turn)?)]);
        }
        Ok(())
    }

    /// Add a native rectangle using the same anchor, side dimensions and turn-transfer path as the app.
    pub fn sketch_rectangle(&mut self, sketch: Id, spec: &RectSpec) -> Result<Vec<Id>> {
        self.transaction(|s| s.sketch_rectangle_inner(sketch, spec))
    }

    fn sketch_rectangle_inner(&mut self, sketch: Id, spec: &RectSpec) -> Result<Vec<Id>> {
        let si = self.sketch_si(sketch)?;
        let vars = self.p.param_map();
        let (cx, cy, w, h, turn) =
            (spec.cx.eval(&vars)?, spec.cy.eval(&vars)?, spec.w.eval(&vars)?, spec.h.eval(&vars)?, spec.turn.eval(&vars)?);
        if w <= 0.0 || h <= 0.0 {
            return Err(Error::Invalid(format!("rectangle size must be positive, got {w} × {h}")));
        }
        check_turn(&spec.turn, turn)?;
        let lines = match spec.anchor {
            RectAnchor::Centre => {
                self.p.add_rect_from_centre(si, Point2::new(cx, cy), Point2::new(cx + w / 2.0, cy + h / 2.0), purpose(spec.construction))
            }
            RectAnchor::Corner => self.p.add_rect_entity(si, cx, cy, cx + w, cy + h, purpose(spec.construction)),
        };
        let rect = self.p.sketches[si]
            .rects()
            .into_iter()
            .find(|r| r.sides[0] == lines[0])
            .ok_or_else(|| Error::Invalid("rectangle figure was not created".into()))?;
        let before = self.p.sketches[si].constraints.len();
        if !self.p.dimension_rect(si, lines[0]) {
            return Err(Error::Invalid("cannot dimension rectangle".into()));
        }
        let mut dims = self.p.sketches[si].constraints.split_off(before);
        for (dim, value) in dims.iter_mut().zip([&spec.w, &spec.h]) {
            if let Constraint::Distance { expr, .. } = dim {
                *expr = value.expr().unwrap_or_default();
            }
        }
        self.add_independent(si, dims);
        let anchor = match spec.anchor {
            RectAnchor::Centre => rect.centre,
            RectAnchor::Corner => rect.corners[0],
        };
        self.pin_point(si, anchor, &spec.cx, cx, &spec.cy, cy)?;
        if spec.turn.expr().is_some() || turn != 0.0 {
            let (o, x) = self.p.ensure_axis(si, 0);
            let dim = angle(o, x, rect.corners[0], rect.corners[1], turn, turn_expr(&spec.turn)?);
            self.p.give_rect_turn_to(si, &dim);
            self.p.sketches[si].constraints.push(dim);
        }
        self.finish_sketch_edit(si)?;
        Ok(lines)
    }
}

fn positive_axes(rx: f64, ry: f64) -> Result<()> {
    if rx < 0.01 || ry < 0.01 {
        return Err(Error::Invalid(format!("ellipse semi-axes must be at least 0.01 mm (native minimum), got {rx} × {ry}")));
    }
    Ok(())
}
fn check_turn(n: &Num, turn: f64) -> Result<()> {
    if (0.0 < turn && turn < 180.0) || (n.expr().is_none() && (turn == 0.0 || turn == 180.0)) {
        return Ok(());
    }
    Err(Error::Invalid("turn must be strictly within the supported range 0..180 degrees; literal 0 and 180 are also allowed".into()))
}
fn text(n: &Num, v: f64) -> String {
    n.expr().unwrap_or_else(|| v.to_string())
}
fn angle(a: Id, b: Id, c: Id, d: Id, deg: f64, expr: String) -> Constraint {
    Constraint::AngleLines { a, b, c, d, deg, expr, driven: false, off: 0.0, at: None }
}

// An unsigned native AngleLines holds one open half-plane. Keep that domain in the
// saved expression too, so an unsupported parameter edit cannot select its mirror.
fn turn_expr(n: &Num) -> Result<String> {
    let Some(t) = n.expr() else { return Ok(String::new()) };
    let expr = format!("({t})+0/sqrt(({t})*(180-({t})))");
    check_expr(&expr)?;
    Ok(expr)
}
