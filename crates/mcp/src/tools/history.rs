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
        tool("feature_delete", "Delete a timeline feature, sketch or datum atomically and return deleted node ids, names and kinds. A node with dependents is refused (unlike the app, it is not relinked); use cascade=true to remove all transitive dependents, or delete the leaf. Deleting the final modifier restores its consumed source as the result. Undo can restore the deletion.", |st, a: DeleteArgs| {
            let s = st.doc()?;
            let id = a.feature.resolve(s)?;
            let before = s.info().timeline;
            let mut result = rebuild_json(&s.feature_delete(id, a.cascade).map_err(err)?);
            let surviving: std::collections::HashSet<_> = s.info().timeline.iter().map(|n| n.id).collect();
            let deleted: Vec<_> = before.into_iter().filter(|n| !surviving.contains(&n.id)).collect();
            result["deleted"] = json!(deleted);
            Ok(result)
        }),
        tool("undo", "Undo the last successful modelling tool call in this document, returning its name/arguments and the restored bodies and rebuild diagnostics. Restores the recipe and exact live B-reps. Retains at most 16 calls; reports when older calls were dropped at this limit. Failed calls and read/save/export tools do not consume history. doc_new/doc_open start fresh history. This session undo does not revert files written to disk; no redo.", |st, _: NoArgs| {
            let restored = st.doc()?.undo().map_err(err)?;
            let mut result = rebuild_json(&restored.rebuild);
            result["ok"] = json!(true);
            result["undone"] = json!(restored.call);
            Ok(result)
        }),
    ]
}
