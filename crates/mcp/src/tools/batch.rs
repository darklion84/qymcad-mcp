//! Ordered modelling handlers under one document snapshot; no transport or per-step history calls.
use super::{input_schema, Content, State, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_STEPS: usize = 200;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BatchArgs {
    /// Ordered modelling operations (1 to 200). Every argument struct is validated before any step runs.
    steps: Vec<Step>,
    /// Include each tool's full normal JSON result as result. Default false returns compact step summaries.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Step {
    /// A modelling tool; the allowed names are listed here when the registry is built.
    tool: String,
    /// Exactly the named tool's standalone arguments, including its JSON-string object/array leniency.
    args: Value,
}

fn decode(args: Value) -> Result<BatchArgs, String> {
    // Step args are opaque here: each tool's own schema must decode them from depth zero and attribute
    // malformed strings/depth errors to the step, rather than to the outer batch envelope.
    serde_json::from_value(args).map_err(|e| format!("bad arguments for `batch`: {e}"))
}

/// The batch description. The allowed tool names are listed once, in the `tool` argument of a step; both texts
/// come from the registry's `mutating` flag, so adding a modelling tool needs no edit here.
fn description(modelling: &[&str]) -> String {
    format!(
        "Run 1..={MAX_STEPS} predictable modelling operations in order with one undo entry and all-or-nothing rollback. \
         Use batch for known parameters, sketches, features, Parts and joints referenced by name/id; joint anchors accept \
         descriptive selections without a topology call. Use single calls when you need topology ids, sketch_info, select or a \
         render between operations. Only the {} modelling tools named in `steps[].tool` are allowed; reads, document/file \
         tools, undo and nested batch are refused. Every step's args are validated with that tool's standalone argument \
         struct and JSON-string leniency before anything runs. Each handler keeps its normal rebuild/checks. Errors identify \
         the 1-based step and tool; nothing was changed, including active Part, grounding, connectors, joints, formulas and \
         placements. Compact results contain step/tool and created node/part/joint id/name when applicable, a step's clamped \
         (a limit moved its request) and part_delete's native_leftovers, feature_reorder action/changed/index metadata, final \
         bodies/warnings/errors (the final assembly report lists every clamp of the batch); verbose=true adds each step's \
         full normal JSON result as result.",
        modelling.len()
    )
}

/// The argument schema with the `tool` field's description listing the allowed tools.
fn schema(modelling: &[&str]) -> Value {
    let mut schema = input_schema::<BatchArgs>();
    if let Some(tool) = schema.pointer_mut("/$defs/Step/properties/tool").and_then(Value::as_object_mut) {
        tool.insert("description".into(), json!(format!("A modelling tool: {}.", modelling.join(", "))));
    }
    schema
}

pub(super) fn tool(modelling: &[&'static str]) -> Tool {
    Tool {
        name: "batch",
        mutating: false,
        description: description(modelling).into(),
        schema: schema(modelling),
        validate: Box::new(|args| decode(args).map(|_| ())),
        handler: Box::new(run),
    }
}

fn run(tools: &[Tool], state: &mut State, arguments: Value) -> Result<Content, String> {
    let batch = decode(arguments.clone())?;
    if !(1..=MAX_STEPS).contains(&batch.steps.len()) {
        return Err(format!("batch requires 1..={MAX_STEPS} steps; nothing was changed"));
    }
    let mut handlers = Vec::with_capacity(batch.steps.len());
    for (index, step) in batch.steps.iter().enumerate() {
        let validate = || -> Result<&Tool, String> {
            let tool = tools.iter().find(|t| t.name == step.tool && t.mutating).ok_or("tool is not allowed in batch")?;
            (tool.validate)(step.args.clone())?;
            Ok(tool)
        };
        handlers.push(validate().map_err(|e| step_error(index, step, &e))?);
    }
    let mut snapshot = state.doc()?.begin_tool_edit().map_err(|e| e.to_string())?;
    snapshot.record_call("batch", arguments);
    // Steps' body reports are dropped (unless verbose): skip their exact tessellation; `execute` clears it for the final one.
    state.doc()?.defer_body_metrics(!batch.verbose);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute(tools, state, &batch, &handlers)));
    if let Ok(session) = state.doc() {
        session.defer_body_metrics(false);
    }
    let result = match result {
        Ok(result) => result,
        Err(panic) => {
            // This boundary owns the snapshot: roll back here, then let `Registry::call` report and taint (ADR 0058).
            if let Ok(session) = state.doc() {
                session.finish_tool_edit(snapshot, false);
            }
            std::panic::resume_unwind(panic);
        }
    };
    // No allowed handler replaces the Session; both success and failure close the same boundary.
    state.doc()?.finish_tool_edit(snapshot, result.is_ok());
    state.created_node = None;
    // A standalone no-op skips history. Batch keeps its existing one-entry policy, including
    // all-no-op batches; an individual no-op must never cancel earlier real changes.
    state.skip_history = false;
    result.map(|value| vec![json!({"type":"text", "text":value.to_string()})])
}

fn step_error(index: usize, step: &Step, reason: &str) -> String {
    format!("batch step {} (`{}`): {reason}; nothing was changed", index + 1, step.tool)
}

fn execute(tools: &[Tool], state: &mut State, batch: &BatchArgs, handlers: &[&Tool]) -> Result<Value, String> {
    let mut steps = Vec::with_capacity(batch.steps.len());
    let mut made: Vec<u64> = Vec::new();
    for (index, (step, tool)) in batch.steps.iter().zip(handlers).enumerate() {
        state.created_node = None;
        state.skip_history = false;
        // A node an earlier step made is rolled back with the whole batch: its id means nothing to the caller.
        let content = (tool.handler)(tools, state, step.args.clone())
            .map_err(|e| step_error(index, step, &made.iter().fold(e, |text, id| text.replace(&format!(" ({id})"), ""))))?;
        let result: Value = content
            .first()
            .and_then(|c| c["text"].as_str())
            .ok_or_else(|| step_error(index, step, "expected JSON text result"))
            .and_then(|text| serde_json::from_str(text).map_err(|e| step_error(index, step, &e.to_string())))?;
        let mut entry = json!({"step":index + 1, "tool":step.tool});
        if let Some(id) = state.created_node {
            made.push(id);
            entry["created"] = json!({"id":id, "name":state.doc()?.node_name(id)});
        }
        // Small operation metadata is useful without repeating full sketch details or body metrics.
        for key in ["ok", "deleted", "index", "dof", "notes", "reused", "native_leftovers"] {
            if let Some(value) = result.get(key) {
                entry[key] = value.clone();
            }
        }
        // A limit that moved a request is never silent, so a step reports it as its own result does; a step with no
        // clamp has no key.
        if let Some(clamped) = result.get("clamped").filter(|v| !v.is_null()) {
            entry["clamped"] = clamped.clone();
        }
        if step.tool == "feature_reorder" {
            for key in ["feature", "direction", "changed", "from_index", "to_index"] {
                entry[key] = result[key].clone();
            }
        }
        if let Some(notes) = result["rebuild"].get("notes") {
            entry["notes"] = notes.clone();
        }
        if batch.verbose {
            entry["result"] = result;
        }
        steps.push(entry);
    }
    state.doc()?.defer_body_metrics(false);
    let report = state.doc()?.current_rebuild();
    let mut out = json!({"ok":true, "steps":steps, "bodies":report.bodies, "warnings":report.warnings, "errors":report.errors});
    if !report.assembly.is_null() {
        out["assembly"] = report.assembly;
    }
    Ok(out)
}
