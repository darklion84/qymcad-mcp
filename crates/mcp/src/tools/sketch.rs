//! Sketch tools.

use super::common::{err, ObjRef, PlaneArg};
use super::{tool, Tool};
use qymcad_engine::{Id, Num};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateArgs {
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
        #[serde(default = "zero")]
        cx: Num,
        #[serde(default = "zero")]
        cy: Num,
        w: Num,
        h: Num,
        /// Construction geometry: not part of any profile.
        #[serde(default)]
        construction: bool,
    },
    /// Circle centred at (cx, cy) with diameter d.
    Circle {
        #[serde(default = "zero")]
        cx: Num,
        #[serde(default = "zero")]
        cy: Num,
        d: Num,
        #[serde(default)]
        construction: bool,
    },
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
    pub sketch: ObjRef,
}

pub fn tools() -> Vec<Tool> {
    vec![
        tool(
            "sketch_create",
            "Create an empty sketch on a plane: \"XY\", \"XZ\", \"YZ\", {\"plane\": <datum plane>} or {\"body\": <body>, \"face\": <face id>}. \
             Returns its id.",
            |st, a: CreateArgs| {
                let s = st.doc()?;
                let plane = a.plane.resolve(s)?;
                let id = s.sketch_create(&plane, a.name.as_deref()).map_err(err)?;
                Ok(json!({ "sketch": id }))
            },
        ),
        tool(
            "sketch_add",
            "Add rectangles/circles to a sketch (all or nothing). Closed shapes form contours; a contour inside another \
             becomes a hole when the outer one is extruded. Returns the new entity ids and the sketch's contours.",
            |st, a: AddArgs| {
                let s = st.doc()?;
                let sketch = a.sketch.resolve(s)?;
                let created: Vec<Value> = s
                    .transaction(|s| {
                        let mut out = Vec::new();
                        for e in &a.entities {
                            out.push(match e {
                                Entity::Rect { cx, cy, w, h, construction } => {
                                    json!({ "rect_lines": s.sketch_rect(sketch, cx, cy, w, h, *construction)? })
                                }
                                Entity::Circle { cx, cy, d, construction } => {
                                    json!({ "circle": s.sketch_circle(sketch, cx, cy, d, *construction)? })
                                }
                            });
                        }
                        Ok(out)
                    })
                    .map_err(err)?;
                let info = s.sketch_info(sketch).map_err(err)?;
                Ok(json!({ "created": created, "sketch": info }))
            },
        ),
        tool(
            "sketch_info",
            "A sketch's plane, contours (id, parent contour, area mm²) and remaining degrees of freedom (0 = fully defined).",
            |st, a: InfoArgs| {
                let s = st.doc()?;
                let id: Id = a.sketch.resolve(s)?;
                Ok(serde_json::to_value(s.sketch_info(id).map_err(err)?).unwrap_or(Value::Null))
            },
        ),
    ]
}
