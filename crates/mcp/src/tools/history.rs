//! Timeline deletion and session modelling undo.
use super::{
    common::{err, rebuild_json, ObjRef},
    doc::NoArgs,
    tool, Tool,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeleteArgs {
    /// Timeline feature, sketch or datum id/name to delete.
    feature: ObjRef,
    /// Delete all transitive dependents too. Default false refuses while dependents exist.
    #[serde(default)]
    cascade: bool,
}

pub fn tools() -> Vec<Tool> {
    vec![
        tool("feature_delete", "Delete a timeline feature, sketch or datum atomically. Refuses when other nodes depend on it unless cascade=true; cascade removes all transitive dependents. Deleting the final modifier restores its consumed source as the result. Undo can restore the deletion.", |st, a: DeleteArgs| {
            let s = st.doc()?;
            let id = a.feature.resolve(s)?;
            Ok(rebuild_json(&s.feature_delete(id, a.cascade).map_err(err)?))
        }),
        tool("undo", "Undo the last successful modelling tool call in this document, restoring its recipe and exact live B-reps. Retains at most 16 calls; failed calls and read/save/export tools do not consume history. doc_new/doc_open start fresh history. This session undo does not revert files written to disk; no redo.", |st, _: NoArgs| {
            st.doc()?.undo().map_err(err)?;
            Ok(json!({"ok": true}))
        }),
    ]
}
