//! Sketch tools.

use super::common::{err, rebuild_json, round, ObjRef, PlaneArg};
use super::{tool, Tool};
use qymcad_engine::{ArcSpec, ConstrainSpec, ConstraintKind, DistAxis, Id, LineSpec, Num, PolygonSpec, PolylineSpec, SketchRef, SlotSpec};
use schemars::JsonSchema;
use serde::Deserialize;
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

/// A sketch entity. Coordinates are in the sketch plane (XY: x=X, y=Y; XZ: x=X, y=Z; YZ: x=Y, y=Z), mm, from
/// the sketch origin. Every value can be an expression; each entity is fully dimensioned with them.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Entity {
    /// Rectangle centred at (cx, cy), width w (along x), height h (along y).
    Rect {
        /// Centre x (default 0).
        #[serde(default = "zero")]
        cx: Num,
        /// Centre y (default 0).
        #[serde(default = "zero")]
        cy: Num,
        /// Width, along x.
        w: Num,
        /// Height, along y.
        h: Num,
        /// Construction geometry: not part of any profile.
        #[serde(default)]
        construction: bool,
    },
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
    /// Arc around (cx, cy): `r` + `start_angle` + `end_angle` (degrees, ccw from +x), or `start` + `end` points.
    /// Ends that coincide with existing points (e.g. a polyline's ends) are joined to them.
    Arc(ArcSpec),
    /// Regular polygon around (cx, cy) with `sides`: circumradius `r` (+ optional `angle` of the first vertex)
    /// or one `vertex`.
    Polygon(PolygonSpec),
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
    /// concentric, midpoint, point_on_line, symmetric, fix. Dimensions (need `value`): distance, angle, diameter,
    /// radius.
    pub kind: ConstraintKind,
    /// What it acts on: point or entity ids from sketch_info, or "origin", "x_axis", "y_axis". Circles/arcs stand
    /// for their centre where a point is expected. Shapes per kind: horizontal/vertical [line] or [point, point];
    /// parallel/perpendicular/collinear/angle [line, line]; equal [line, line] or [circle, circle]; tangent [line,
    /// circle] or [circle, circle]; midpoint/point_on_line [point, line] (point_on_line also [point, circle]);
    /// symmetric [point, point, line]; fix [point]; distance [point, point], [line], [point, line] or [line, line];
    /// diameter/radius [circle or arc].
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
    /// An entity id to delete (with the points only it used and the constraints on them).
    #[serde(default)]
    pub entity: Option<Id>,
    /// A constraint index to delete (later indices shift down by one).
    #[serde(default)]
    pub constraint: Option<usize>,
}

/// Round every non-integer number in a JSON value (coordinates, radii, values) for a compact result.
fn rounded(v: Value) -> Value {
    match v {
        // A non-finite number has no JSON form: null, never a panic or a bogus value.
        Value::Number(n) if n.is_f64() => match n.as_f64() {
            Some(x) if x.is_finite() => json!(round(x, 6)),
            _ => Value::Null,
        },
        Value::Array(a) => Value::Array(a.into_iter().map(rounded).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, rounded(v))).collect()),
        other => other,
    }
}

/// The sketch detail as compact JSON, plus the rebuild of dependent features when there was one.
fn sketch_result(s: &qymcad_engine::Session, sketch: Id, rebuild: Option<&qymcad_engine::Rebuild>) -> Result<Value, String> {
    let detail = s.sketch_detail(sketch).map_err(err)?;
    let mut o = json!({ "sketch": rounded(serde_json::to_value(detail).unwrap_or(Value::Null)) });
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
            "Add entities to a sketch (all or nothing): rect, circle, line, polyline, arc, polygon, slot. Each is fully \
             dimensioned from the sketch origin with the given numbers/expressions, so it stays parametric and editable in \
             QymCAD (`dimensioned: false` leaves line/polyline/arc/polygon/slot free for sketch_constrain). Points that \
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
                                Entity::Rect { cx, cy, w, h, construction } => {
                                    json!({ "rect_lines": s.sketch_rect(sketch, cx, cy, w, h, *construction)? })
                                }
                                Entity::Circle { cx, cy, d, construction } => {
                                    json!({ "circle": s.sketch_circle(sketch, cx, cy, d, *construction)? })
                                }
                                Entity::Line(l) => added("line", s.sketch_line(sketch, l)?),
                                Entity::Polyline(p) => added("polyline", s.sketch_polyline(sketch, p)?),
                                Entity::Arc(x) => added("arc", s.sketch_arc(sketch, x)?),
                                Entity::Polygon(g) => added("polygon", s.sketch_polygon(sketch, g)?),
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
             sketch_info, or \"origin\", \"x_axis\", \"y_axis\"). Use it to dimension geometry added with \
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
                o["dof"] = json!(c.dof);
                if let Some(note) = c.note {
                    o["note"] = json!(note);
                }
                Ok(o)
            },
        ),
        tool(
            "sketch_remove",
            "Delete one entity (with the points only it used and the constraints on them) or one constraint (by index) \
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
            "A sketch's plane and world_frame (origin mm, x_axis, y_axis, normal; null for an unresolved host); dof [free, redundant] ([0, 0] = fully defined); contours (id, parent contour, area mm²); \
             entities (id, type line/arc/circle/ellipse, point ids, r for circles and arcs, ccw for arcs, construction); \
             points (id, x, y; special points have a role: origin, frame, x_axis, y_axis, angle_reference); constraints \
             (index, kind, point ids, value, expr, reference). Ids and indices are what sketch_constrain and sketch_remove \
             take. Contour areas are tessellation-based, typically about 0.1–0.2% below analytic areas for curved \
             contours; use analytic dimensions for exact areas.",
            |st, a: InfoArgs| {
                let s = st.doc()?;
                let id: Id = a.sketch.resolve(s)?;
                Ok(sketch_result(s, id, None)?["sketch"].take())
            },
        ),
    ]
}
