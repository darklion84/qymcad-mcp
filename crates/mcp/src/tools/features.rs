//! Feature tools: datum planes, extrude.

use super::common::{err, rebuild_json, ObjRef, PlaneArg};
use super::{tool, Tool};
use qymcad_engine::{Direction, Extrude, Id, Num, Op};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaneOffsetArgs {
    /// A base plane ("XY", "XZ", "YZ") or {"plane": <datum plane>}.
    pub base: PlaneArg,
    /// Distance along the base plane's normal, mm (number or expression).
    pub dist: Num,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtrudeArgs {
    /// Sketch id or name.
    pub sketch: ObjRef,
    /// Contour ids from sketch_info. Default: every top-level contour, each minus the contours inside it.
    #[serde(default)]
    pub profiles: Option<Vec<Id>>,
    /// Distance, mm (number or expression). Not needed with `through`.
    #[serde(default)]
    pub height: Option<Num>,
    /// add (default; creates the first body), cut, intersect, new_body.
    #[serde(default)]
    pub op: Op,
    /// normal (default), reverse, symmetric — relative to the sketch plane's normal (XY +Z, XZ −Y, YZ +X, face: outward).
    #[serde(default)]
    pub direction: Direction,
    /// Go through the whole body (cut/add/intersect only).
    #[serde(default)]
    pub through: bool,
    /// Body to modify (id or name). Default: the part's current body.
    #[serde(default)]
    pub target: Option<ObjRef>,
    #[serde(default)]
    pub name: Option<String>,
}

pub fn tools() -> Vec<Tool> {
    vec![
        tool(
            "plane_offset",
            "Create a datum plane parallel to a base or datum plane at a distance (e.g. the top face level for a pocket: \
             {\"base\": \"XY\", \"dist\": \"t\"}). Returns its id; sketch on it with {\"plane\": id}.",
            |st, a: PlaneOffsetArgs| {
                let s = st.doc()?;
                let base = a.base.resolve(s)?;
                let (id, r) = s.plane_offset(&base, &a.dist, a.name.as_deref()).map_err(err)?;
                Ok(json!({ "plane": id, "rebuild": rebuild_json(&r) }))
            },
        ),
        tool(
            "extrude",
            "Extrude sketch contours: add material (the first add creates the part's body), cut, intersect, or a new \
             body. Atomic: if the feature does not build, nothing changes and the error is returned. Returns the new \
             body id and the result bodies (volume, bbox).",
            |st, a: ExtrudeArgs| {
                let s = st.doc()?;
                let sketch = a.sketch.resolve(s)?;
                let target = match &a.target {
                    Some(t) => Some(t.resolve(s)?),
                    None => None,
                };
                let height = match (a.height, a.through) {
                    (Some(h), _) => h,
                    (None, true) => Num::Value(1.0),
                    (None, false) => return Err("`height` is required unless `through` is set".into()),
                };
                let x = Extrude {
                    sketch,
                    profiles: a.profiles,
                    height,
                    op: a.op,
                    direction: a.direction,
                    through: a.through,
                    target,
                    name: a.name,
                };
                let (id, r) = s.extrude(&x).map_err(err)?;
                Ok(json!({ "body": id, "rebuild": rebuild_json(&r) }))
            },
        ),
    ]
}
