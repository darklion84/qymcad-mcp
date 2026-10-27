//! Sketch tools.

use super::common::{err, rebuild_json, ObjRef, PlaneArg};
use super::{tool, Tool};
use qymcad_engine::{
    ArcSpec, ConstrainSpec, ConstraintKind, DistAxis, EllipseArcSpec, EllipseSpec, Id, LineSpec, Num, PolygonSpec, PolylineSpec, RectSpec,
    SketchRef, SlotSpec, SplineSpec, TangentCircleSpec, TextFont, TextSpec, Xy,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateArgs {
    /// Sketch plane: "XY", "XZ", "YZ", {"plane": <datum plane>} or {"body": <body>, "face": <planar face id>}.
    pub plane: PlaneArg,
    /// Name to refer to the sketch later.
    #[serde(default)]
    pub name: Option<String>,
}

fn zero() -> Num {
    Num::Value(0.0)
}

fn yes() -> bool {
    true
}

/// A polygon whose native side-count item value can follow a formula.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolygonArgs {
    /// Centre x, in mm (default 0).
    #[serde(default = "zero")]
    pub cx: Num,
    /// Centre y, in mm (default 0).
    #[serde(default = "zero")]
    pub cy: Num,
    /// Integral side count in 3..=64, a number or formula; `n` is an alias.
    #[serde(alias = "n")]
    pub sides: Num,
    /// Positive circumradius; give this or `vertex`.
    pub r: Option<Num>,
    /// First vertex direction in degrees, with `r`; `turn` is an alias. Default 0.
    #[serde(alias = "turn")]
    pub angle: Option<Num>,
    /// First vertex coordinates instead of `r` and `angle`.
    pub vertex: Option<Xy>,
    /// Construction geometry, excluded from profiles (default false).
    #[serde(default)]
    pub construction: bool,
    /// Fully dimension the polygon (default true).
    #[serde(default = "yes")]
    pub dimensioned: bool,
}

/// Fixed native text geometry with baked glyphs; recreate the text to resize it.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextArgs {
    /// Text to outline; every non-whitespace glyph must exist in the selected font.
    pub text: String,
    /// Anchor [x, y] in sketch mm, fixed numbers. Native text has no placement formula path.
    #[serde(deserialize_with = "fixed_text_at")]
    pub at: [f64; 2],
    /// Positive em height in mm, a fixed number. Recreate text to resize: app height edits can swap letters and counters.
    #[serde(deserialize_with = "fixed_text_number")]
    pub height: f64,
    /// Rotation in degrees about the anchor, a fixed number. Default 0; no native angle formula path.
    #[serde(default, deserialize_with = "fixed_text_number")]
    pub angle: f64,
    /// Exact font path and face index. Default Arial face 0 at /System/Library/Fonts/Supplemental/Arial.ttf; missing fonts are errors, without substitution.
    #[serde(default)]
    pub font: Option<TextFont>,
    /// Construction text, excluded from extrusion profiles. Default false.
    #[serde(default)]
    pub construction: bool,
}

const FIXED_TEXT_REASON: &str = "text height, at and angle must be fixed numbers; recreate text to resize it: QymCAD 0.1.1 height edits can swap glyph/counter contour ids, and at/angle have no native formula path";

fn fixed_text_value<E: serde::de::Error>(value: Value) -> Result<f64, E> {
    value.as_f64().filter(|v| v.is_finite()).ok_or_else(|| E::custom(FIXED_TEXT_REASON))
}

fn fixed_text_number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    fixed_text_value(Value::deserialize(deserializer)?)
}

fn fixed_text_at<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[f64; 2], D::Error> {
    let [x, y] = <[Value; 2]>::deserialize(deserializer)?;
    Ok([fixed_text_value(x)?, fixed_text_value(y)?])
}

/// A sketch entity. Coordinates are in the sketch plane (XY: x=X, y=Y; XZ: x=X, y=Z; YZ: x=Y, y=Z), mm, from
/// the sketch origin. Curve dimensions accept expressions; text geometry uses fixed numbers.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Entity {
    /// Baked text outlines with a deterministic font and fixed numeric height, anchor and angle.
    Text(TextArgs),
    /// Native rectangle figure, with centre/corner anchor and formula-driven turn.
    Rect(RectSpec),
    /// Native ellipse with semi-axes rx/ry and turn in degrees.
    Ellipse(EllipseSpec),
    /// Native elliptical arc with eccentric start/end angles in degrees.
    EllipseArc(EllipseArcSpec),
    /// Circle touching exactly two uniquely selected native curves, near a chosen centre.
    CircleTangent(TangentCircleSpec),
    /// Circle centred at (cx, cy) with diameter d.
    Circle {
        /// Centre x (default 0).
        #[serde(default = "zero")]
        cx: Num,
        /// Centre y (default 0).
        #[serde(default = "zero")]
        cy: Num,
        /// Diameter.
        d: Num,
        /// Construction geometry: not part of any profile.
        #[serde(default)]
        construction: bool,
    },
    /// Straight segment from (x1, y1) to (x2, y2).
    Line(LineSpec),
    /// Connected segments through `points` [[x, y], ...]; `closed` joins the last to the first (a profile).
    Polyline(PolylineSpec),
    /// Native interpolating spline through nodes; closed splines need at least three nodes.
    Spline(SplineSpec),
    /// Arc around (cx, cy): `r` + `start_angle` + `end_angle` (degrees, ccw from +x), or `start` + `end` points.
    /// Ends that coincide with existing points (e.g. a polyline's ends) are joined to them.
    Arc(ArcSpec),
    /// Regular polygon around (cx, cy) with `sides`: circumradius `r` (+ optional `angle` of the first vertex)
    /// or one `vertex`.
    Polygon(PolygonArgs),
    /// Slot (stadium): round ends of diameter `width` around (x1, y1) and (x2, y2), tangent sides.
    Slot(SlotSpec),
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddArgs {
    /// Sketch id or name.
    pub sketch: ObjRef,
    /// Entities to add, all or nothing.
    pub entities: Vec<Entity>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoArgs {
    /// Sketch id or name.
    pub sketch: ObjRef,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConstrainArgs {
    /// Sketch id or name.
    pub sketch: ObjRef,
    /// What to add. Geometric: coincident, horizontal, vertical, parallel, perpendicular, collinear, equal, tangent,
    /// concentric, midpoint, point_on_line, point_on_curve, symmetric, fix. Dimensions (need `value`): distance,
    /// curve_distance, angle, diameter, radius. Tangent/point_on_curve support ellipses and splines.
    pub kind: ConstraintKind,
    /// What it acts on: point or entity ids from sketch_info, or "origin", "x_axis", "y_axis". Circles/arcs stand
    /// for their centre where a point is expected. Shapes per kind: horizontal/vertical [line] or [point, point];
    /// parallel/perpendicular/collinear/angle [line, line]; equal [line, line] or [circle, circle]; tangent [line,
    /// circle] or [circle, circle]; midpoint/point_on_line [point, line] (point_on_line also [point, circle]);
    /// symmetric [point, point, line]; fix [point]; distance [point, point], [line], [point, line] or [line, line];
    /// diameter/radius [circle or arc]. point_on_curve takes [point, curve]; tangent takes [curve, curve];
    /// curve_distance takes [point, curve] or [curve, curve] with axis aligned. Curves also accept a unique
    /// kind/construction/nearest selector, and points accept {curve, role, node_index?}.
    pub refs: Vec<SketchRef>,
    /// The dimension: mm or degrees, a number or an expression over parameters (e.g. "w/2"). Positive.
    #[serde(default)]
    pub value: Option<Num>,
    /// For distance between points or a line's length: aligned (default), x or y (only that component).
    #[serde(default)]
    pub axis: DistAxis,
    /// A reference dimension: only shows the measured value, constrains nothing (no `value` needed). Use it for a
    /// size that is already determined.
    #[serde(default)]
    pub reference: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveArgs {
    /// Sketch id or name.
    pub sketch: ObjRef,
    /// A curve entity id (with its unused points and constraints) or a text id from sketch_info.texts.
    #[serde(default)]
    pub entity: Option<Id>,
    /// A constraint index to delete (later indices shift down by one).
    #[serde(default)]
    pub constraint: Option<usize>,
}

/// The sketch detail as compact JSON, plus the rebuild of dependent features when there was one.
pub(super) fn sketch_result(s: &qymcad_engine::Session, sketch: Id, rebuild: Option<&qymcad_engine::Rebuild>) -> Result<Value, String> {
    let detail = s.sketch_detail(sketch).map_err(err)?;
    let mut o = json!({ "sketch": serde_json::to_value(detail).unwrap_or(Value::Null) });
    if let Some(r) = rebuild {
        o["rebuild"] = rebuild_json(r);
    }
    Ok(o)
}

pub fn tools() -> Vec<Tool> {
    vec![
        tool(
            "sketch_create",
            "Create an empty sketch on a plane: \"XY\", \"XZ\", \"YZ\", {\"plane\": <datum plane>} or {\"body\": <body>, \"face\": <face id>}. \
             A face sketch's origin is the world origin projected onto the face plane (in the owning component's local coordinates, \
             then carried by its placement). Its x/y directions are positive coordinate axes of that component in right-handed order: \
             +Z: +X/+Y, -Z: +Y/+X, +X: +Y/+Z, -X: +Z/+Y, +Y: +Z/+X, -Y: +X/+Z. \
             For tilted faces x = normalize(+Z cross normal), y = normal cross x. Read sketch_info.world_frame for the resolved world axes. Returns its id.",
            |st, a: CreateArgs| {
                let s = st.doc()?;
                let plane = a.plane.resolve(s)?;
                let id = s.sketch_create(&plane, a.name.as_deref()).map_err(err)?;
                Ok(json!({ "sketch": id }))
            },
        ),
        tool(
            "sketch_add",
            "Add entities to a sketch (all or nothing): rect, circle, circle_tangent, line, polyline, spline, arc, ellipse, ellipse_arc, polygon, slot, text. Text uses fixed numeric height, at and angle; recreate it to resize because app height edits can swap glyph/counter contour ids. Its default font is Arial face 0 at /System/Library/Fonts/Supplemental/Arial.ttf; missing fonts or glyphs are refused. Baked glyphs reopen without the font; retyping in the app needs its recorded path. Curves are fully \
             dimensioned from the sketch origin with the given numbers/expressions, so it stays parametric and editable in \
             QymCAD (`dimensioned: false` leaves line/polyline/spline/arc/ellipse/ellipse_arc/polygon/slot free for sketch_constrain). Ellipse/rectangle turn formulas must stay strictly between 0 and 180 degrees (literal 0/180 allowed); ellipse-arc eccentric endpoint angles must stay strictly between 0 and 180. Polygon sides accept integral formulas in 3..=64. Points that \
             coincide with existing ones are joined (a polyline end and an arc end become one point). Closed shapes form \
             contours; a contour inside another becomes a hole when the outer one is extruded. Returns the new entity and \
             point ids and the sketch (entities, points, constraints, contours, dof; dof [0, 0] = fully defined). Features \
             built from the sketch are rebuilt.",
            |st, a: AddArgs| {
                let s = st.doc()?;
                let sketch = a.sketch.resolve(s)?;
                if a.entities.is_empty() {
                    return Err("`entities` is empty: give at least one entity to add".into());
                }
                let (created, r) = s
                    .sketch_edit(sketch, |s| {
                        let mut out = Vec::new();
                        for e in &a.entities {
                            let added = |kind: &str, x: qymcad_engine::Added| json!({ "type": kind, "entities": x.entities, "points": x.points });
                            out.push(match e {
                                Entity::Text(t) => json!({ "type": "text", "text": s.sketch_text(sketch, &TextSpec {
                                    text: t.text.clone(), at: t.at, height: t.height, angle: t.angle,
                                    font: t.font.clone(), construction: t.construction,
                                })? }),
                                Entity::Rect(spec) => {
                                    json!({ "rect_lines": s.sketch_rectangle(sketch, spec)? })
                                }
                                Entity::Ellipse(spec) => added("ellipse", s.sketch_ellipse(sketch, spec)?),
                                Entity::EllipseArc(spec) => added("ellipse_arc", s.sketch_ellipse_arc(sketch, spec)?),
                                Entity::CircleTangent(spec) => added("circle_tangent", s.sketch_circle_tangent(sketch, spec)?),
                                Entity::Circle { cx, cy, d, construction } => {
                                    json!({ "circle": s.sketch_circle(sketch, cx, cy, d, *construction)? })
                                }
                                Entity::Line(l) => added("line", s.sketch_line(sketch, l)?),
                                Entity::Polyline(p) => added("polyline", s.sketch_polyline(sketch, p)?),
                                Entity::Spline(p) => added("spline", s.sketch_spline(sketch, p)?),
                                Entity::Arc(x) => added("arc", s.sketch_arc(sketch, x)?),
                                Entity::Polygon(g) => added("polygon", s.sketch_polygon_sides(sketch, &PolygonSpec {
                                    cx:g.cx.clone(), cy:g.cy.clone(), sides:3, r:g.r.clone(), angle:g.angle.clone(), vertex:g.vertex.clone(), construction:g.construction, dimensioned:g.dimensioned,
                                }, &g.sides)?),
                                Entity::Slot(sl) => added("slot", s.sketch_slot(sketch, sl)?),
                            });
                        }
                        Ok(out)
                    })
                    .map_err(err)?;
                let mut o = sketch_result(s, sketch, r.as_ref())?;
                o["created"] = json!(created);
                Ok(o)
            },
        ),
        tool(
            "sketch_constrain",
            "Add a geometric constraint or a driving dimension between existing points/entities of a sketch (ids from \
             sketch_info, or \"origin\", \"x_axis\", \"y_axis\"). Origin/frame/axis points are materialized only when geometry or constraints need them, usually after the first sketch_add; named aliases materialize their reference points. Use it to dimension geometry added with \
             `dimensioned: false`, or to relate entities (tangent, equal, symmetric, ...). Refused, with nothing changed, \
             when it would over-constrain the sketch (the message names the constraints it duplicates or contradicts; \
             remove one with sketch_remove, or add a dimension as `reference: true`) or cannot be solved. A geometric \
             constraint that already holds and is implied returns index null. Returns the constraint index, dof and the \
             sketch; features built from the sketch are rebuilt.",
            |st, a: ConstrainArgs| {
                let s = st.doc()?;
                let sketch = a.sketch.resolve(s)?;
                let spec = ConstrainSpec { kind: a.kind, refs: a.refs, value: a.value, axis: a.axis, reference: a.reference };
                let (c, r) = s.sketch_edit(sketch, |s| s.sketch_constrain(sketch, &spec)).map_err(err)?;
                let mut o = sketch_result(s, sketch, r.as_ref())?;
                o["index"] = json!(c.index);
                o["indices"] = json!(c.indices);
                o["dof"] = json!(c.dof);
                if let Some(note) = c.note {
                    o["note"] = json!(note);
                }
                Ok(o)
            },
        ),
        tool(
            "sketch_remove",
            "Delete one entity (with the points only it used and the constraints on them), one text id from sketch_info.texts (using entity), or one constraint (by index) \
             from a sketch. Give exactly one of `entity`, `constraint`. Returns the sketch; features built from it are \
             rebuilt (refused if one would break).",
            |st, a: RemoveArgs| {
                let s = st.doc()?;
                let sketch = a.sketch.resolve(s)?;
                let ((), r) = match (a.entity, a.constraint) {
                    (Some(e), None) => s.sketch_edit(sketch, |s| s.sketch_remove_entity(sketch, e)),
                    (None, Some(i)) => s.sketch_edit(sketch, |s| s.sketch_remove_constraint(sketch, i)),
                    _ => return Err("give exactly one of `entity` or `constraint`".into()),
                }
                .map_err(err)?;
                sketch_result(s, sketch, r.as_ref())
            },
        ),
        tool(
            "sketch_info",
            "A sketch's plane (named datums show name and plane id) and world_frame (origin mm, x_axis, y_axis, normal; null for an unresolved host; coordinates/directions use 4 decimal places for presentation); dof [free, redundant] ([0, 0] = fully defined); contours (id, parent contour, area mm²); \
             texts (id, text, resolved font path/index and family, fixed height/at/angle, construction, baked contour count); entities (id, type line/arc/circle/ellipse/ellipse_arc/spline, point ids (ordered spline nodes), spline nodes/closed, r for circles and arcs, ccw for arcs, construction); \
             points (id, x, y; special points have a role: origin, frame, x_axis, y_axis, angle_reference). Origin/frame/axis points appear only when geometry or constraints need them, usually after the first sketch_add; constraints \
             (index, kind, point ids, value, expr, reference). Ids and indices are what sketch_constrain and sketch_remove \
             take. Radii and constraint values retain full precision for reuse as inputs. Contour areas are tessellation-based and can be below analytic; errors can reach up to ~2% on very small curved regions, including flat faces with curved boundaries, as observed, not a guaranteed bound. Use analytic dimensions for exact areas.",
            |st, a: InfoArgs| {
                let s = st.doc()?;
                let id: Id = a.sketch.resolve(s)?;
                Ok(sketch_result(s, id, None)?["sketch"].take())
            },
        ),
    ]
}
