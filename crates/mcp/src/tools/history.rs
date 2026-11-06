//! Timeline deletion and session modelling undo.
use super::{
    common::{err, rebuild_json, ObjRef},
    doc::NoArgs,
    tool, Tool,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};
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

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SuppressArgs {
    /// Body-producing timeline feature id/name. Sketches, datums, parameters and mesh-born Moves are refused.
    feature: ObjRef,
    /// Explicit desired suppression state. Same-state requests return changed:false without a rebuild or undo entry.
    suppressed: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReorderArgs {
    /// Exact body-producing timeline feature id/name, including consumed, suppressed and rollback-tail features. Single-output B-rep targets only.
    feature: ObjRef,
    /// Move "up" or "down" across the nearest body-producing feature of the same parent, as in the app menu. No neighbour returns changed:false.
    direction: String,
}

fn reorder_direction(direction: &str) -> Result<(), String> {
    match direction {
        "up" | "down" => Ok(()),
        value => Err(format!("invalid request: unknown reorder direction `{value}`; use up or down")),
    }
}

fn reorder_tool() -> Tool {
    let mut reorder = tool("feature_reorder", "Move a single-output B-rep timeline feature up or down across its same-parent body-feature neighbour, using the app's native reorder and chain rewrite. Consumed, suppressed and rollback-tail feature ids remain addressable. Sketch/datum targets, mesh-born target/neighbour and multi-output target/neighbour are refused; native dependencies and unsafe projected-spline source changes also refuse. Returns feature{id,name}, direction, changed, from_index/to_index (zero-based diagnostics), complete order, rewired body inputs, rehosted_sketches, canonical rollback, lost_body_metadata and rebuild{bodies,warnings,errors}. Rebuilds and checks all timeline bodies atomically; no silent re-pick or dependency repair. The numeric rollback position and suppression flags remain native. Body metadata lost when native rollback membership removes records is reported. Boundary no-op returns changed:false without rebuild or undo; a real move is one undo entry and may run in batch. Re-read topology after a changed move.", |st, a: ReorderArgs| {
        reorder_direction(&a.direction)?;
        let s = st.doc()?;
        let id = a.feature.resolve(s)?;
        let result = s.feature_reorder(id, &a.direction).map_err(err)?;
        st.skip_history = !result.changed;
        Ok(json!(result))
    });
    // A direction is a string in the public schema so its stable policy error is shared with
    // direct calls. Validate it before a batch opens its snapshot or runs any earlier step.
    let schema = reorder.schema.clone();
    reorder.validate = Box::new(move |args| {
        let args = super::decode_arguments::<ReorderArgs>("feature_reorder", &schema, args)?;
        reorder_direction(&args.direction)
    });
    reorder
}

// Omission selects the other field; explicit null is not a selector.
fn present<'de, T: Deserialize<'de>, D: Deserializer<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RollbackArgs {
    /// Timeline node id/name to keep active immediately before the marker. Includes sketches and datums; the last node means end.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ObjRef")]
    after: Option<ObjRef>,
    /// Move to "start" (no active nodes) or "end" (entire timeline). Supply exactly one of after or position; no numeric indexes.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "String")]
    position: Option<String>,
}

pub fn tools() -> Vec<Tool> {
    vec![
        reorder_tool(),
        tool("history_rollback", "Move the native history rollback marker with exactly one selector: after keeps that timeline node active, position:start activates no nodes, position:end activates the entire timeline. Sketches and datums count as timeline nodes. After the last node normalizes to end. Returns position, after{id,name} or null, next_feature{id,name} or null, active_nodes, total_nodes, changed, lost_body_metadata[{body,body_name,visible}] and rebuild{bodies,warnings,errors}. Native removal drops body names/visibility; lost_body_metadata lists removed records with non-default names or visible:false, otherwise []; resume uses native defaults while lineage colour survives. Native rebuild errors refuse and restore exact state. Creation inserts at the marker and advances it; the next same-parent consumer is remapped natively. Save preserves the marker; inspection/export/render/measure use only active results. Same-position requests return changed:false with no rebuild or undo entry. Real changes are one undo step and allowed in batch.", |st, a: RollbackArgs| {
            let s = st.doc()?;
            let after = a.after.as_ref().map(|reference| reference.resolve(s)).transpose()?;
            let result = s.history_rollback(after, a.position.as_deref()).map_err(err)?;
            st.skip_history = !result.changed;
            Ok(json!(result))
        }),
        tool("feature_suppress", "Suppress or enable a body-producing timeline feature through native checked regeneration. A suppressed modifier keeps its output id as an identity pass-through; a suppressed base and its dependents become intentionally unbuilt. Returns feature{id,name}, suppressed, changed, unbuilt body ids, lost_body_metadata[{body,body_name,visible}] and rebuild{bodies,warnings,errors}. Native removal drops body names/visibility; lost_body_metadata lists removed records with non-default names or visible:false, otherwise []; enabling uses native defaults while lineage colour survives. Multi-output targets and changes removing/recreating downstream multi-output pieces are refused under this pin: native suppression leaves stale or overlapping pieces and the identity guard cannot follow them. Native errors (including lost downstream picks) refuse and restore exact state. Mesh-born Moves, sketches and datums are refused. An unchanged flag adds no undo entry; a real change is one undo step and is allowed in batch.", |st, a: SuppressArgs| {
            let s = st.doc()?;
            let id = a.feature.resolve(s)?;
            let result = s.feature_suppress(id, a.suppressed).map_err(err)?;
            st.skip_history = !result.changed;
            Ok(json!(result))
        }),
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
        tool("undo", "Undo the last successful modelling tool call in this document, returning undone.tool, original undone.arguments and undone.label (localized created feature name, or a human action label), plus the restored parameter expressions/values, bodies and rebuild diagnostics. The params list shows what is now in effect, including parameter removal after undoing creation. Restores the recipe and exact live B-reps. Retains at most 16 calls; reports when older calls were dropped at this limit. Failed calls and read/save/export tools do not consume history. doc_new/doc_open start fresh history. This session undo does not revert files written to disk; no redo.", |st, _: NoArgs| {
            let s = st.doc()?;
            let restored = s.undo().map_err(err)?;
            let mut result = rebuild_json(&restored.rebuild);
            result["ok"] = json!(true);
            result["undone"] = json!(restored.call);
            result["params"] = json!(s.params());
            Ok(result)
        }),
    ]
}
