//! Explicit rename targets keep the app's three native label stores independent.
use super::{
    common::{err, ObjRef},
    tool, Tool,
};
use qymcad_engine::RenameTarget;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
enum Target {
    Feature {
        /// Timeline feature id/name, including a sketch's timeline node. Changes only its timeline label.
        feature: ObjRef,
    },
    Sketch {
        /// Sketch id/name. Changes only Sketch.name, leaving its timeline label unchanged.
        sketch: ObjRef,
    },
    Body {
        /// Body id/name. Changes only native Body.name, leaving its producing feature label unchanged.
        body: ObjRef,
    },
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RenameArgs {
    /// Exactly one of {feature: id/name}, {sketch: id/name} or {body: id/name}.
    target: Target,
    /// New label: Unicode controls and numeric ids are refused; surrounding whitespace is trimmed. Case and localisation keys are retained. Duplicates are allowed.
    name: String,
}

pub fn tools() -> Vec<Tool> {
    vec![tool("rename", "Rename exactly one native feature, sketch or body label without rebuilding geometry. Returns target.kind/id, previous_name, stored name, localised display_name, changed and duplicates (other ids sharing the label). Duplicate text refs require an id. A same-name request changes nothing and adds no undo entry. Undoable and allowed in batch; later steps may use the new name. Body reporting preserves name (the existing feature/import label) and adds body_name for the native body label.", |st, a: RenameArgs| {
        let s = st.doc()?;
        let target = match a.target {
            Target::Feature {feature} => RenameTarget::Feature(feature.resolve(s)?),
            Target::Sketch {sketch} => RenameTarget::Sketch(sketch.resolve(s)?),
            Target::Body {body} => RenameTarget::Body(body.resolve(s)?),
        };
        let result = s.rename(target, &a.name).map_err(err)?;
        st.skip_history = !result.changed;
        Ok(json!(result))
    })]
}
