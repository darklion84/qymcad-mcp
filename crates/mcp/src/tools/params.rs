//! Parameter tools.

use super::common::{err, rebuild_json, ObjRef};
use super::{tool, Tool};
use qymcad_engine::Num;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetArgs {
    /// Parameter name: letters, digits, `_`, starting with a letter. Case-sensitive (`H` and `h` are different names; a
    /// new name differing from an existing one only by case is refused).
    pub name: String,
    /// A number, or an expression over other parameters (`"w/2"`, `"2*t+1"`).
    pub value: Num,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteArgs {
    /// Exact name of the parameter to delete (names are case-sensitive). Refused while an expression still uses it.
    pub name: String,
}

/// Exactly one native sketch dimension or feature/item numeric field.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum DimensionTargetArg {
    /// A zero-based dimensional constraint index from `sketch_detail`.
    Sketch {
        /// Sketch id or name.
        sketch: ObjRef,
        /// Current constraint index; its native entity references must identify one dimension uniquely.
        constraint: usize,
    },
    /// A numeric field of a timeline feature or a sketch item.
    Feature {
        /// Feature id/name, or sketch item's centre/id (items have no native names).
        feature: ObjRef,
        /// Native numeric key, e.g. `height`, `sides`, `w`, `h`, `turn` (ellipse axes are full sizes). Chamfer `d2` is degrees for Leg and angle, mm for TwoDist, and refused for symmetric.
        key: String,
    },
}

/// Atomic dimension value/formula and name edit.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DimensionSetArgs {
    /// One sketch constraint or feature/item dimension, with no mixed selector fields.
    pub target: DimensionTargetArg,
    /// New value or formula. A literal replaces a previous formula. Text height edits are refused;
    /// remove and recreate text to change its fixed geometry.
    pub value: Option<Num>,
    /// Case-sensitive sketch/feature driver name; empty clears the name. New sketch-item names are refused
    /// because native app edits do not dirty their sketch. At least value or name is required.
    pub name: Option<String>,
}

pub fn tools() -> Vec<Tool> {
    vec![
        tool("dimension_set", "Set a native sketch, feature or sketch-item driving dimension's value/formula and/or name. Names make dimensions available to other formulas; doc_info lists them. Chamfer dist/d2 metadata reports mode and unit; d2 is degrees in dist_angle, mm in two_dist, and refused in symmetric. Refuses unknown or ambiguous targets, name collisions, cycles, bad formulas and unsolved edits atomically. An empty name removes the name. New sketch-item names are refused because native app SetDriver leaves their solids stale. Text height edits and changes to existing text through imported formulas are refused: remove and recreate text to change its height, position, angle, font or characters. Item edits that conflict with driving sketch constraints are refused; edit the sketch constraint instead.", |st, a: DimensionSetArgs| {
            let s = st.doc()?;
            let target = match a.target {
                DimensionTargetArg::Sketch { sketch, constraint } => qymcad_engine::DimensionTarget::Sketch { sketch: sketch.resolve(s)?, constraint },
                DimensionTargetArg::Feature { feature, key } => {
                    let reference = match feature { ObjRef::Id(id) => id.to_string(), ObjRef::Name(name) => name };
                    qymcad_engine::DimensionTarget::Feature { feature: s.dimension_feature_resolve(&reference).map_err(err)?, key }
                }
            };
            let r = s.dimension_set(&target, a.value.as_ref(), a.name.as_deref()).map_err(err)?;
            Ok(json!({"dimension": r.dimension, "rebuild": rebuild_json(&r.rebuild)}))
        }),
        tool(
            "param_set",
            "Create or change a named parameter and rebuild everything that depends on it. Use parameters for every \
             dimension the user may want to change, then refer to them in expressions. If the model no longer \
             builds, the change is rolled back and the error returned. Edits that change existing text geometry through imported item formulas are refused; remove and recreate the text instead. After GUI structural edits (re-hosting a datum sketch, \
             creating a new datum sketch, or replacing a datum distance/angle formula or driver name), reopen and save through the server \
             before further GUI parameter edits so its persisted datum dependencies are refreshed (ADR 0007).",
            |st, a: SetArgs| {
                let r = st.doc()?.param_set(&a.name, &a.value).map_err(err)?;
                Ok(json!({ "rebuild": rebuild_json(&r) }))
            },
        ),
        tool("param_delete", "Delete a parameter. Refused while an expression still uses it.", |st, a: DeleteArgs| {
            st.doc()?.param_delete(&a.name).map_err(err)?;
            Ok(json!({ "ok": true }))
        }),
    ]
}
