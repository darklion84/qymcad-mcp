//! Ordered modelling handlers under one document snapshot; no transport or per-step history calls.
use super::{input_schema, modelling_tool, Content, State, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

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
    /// A modelling tool: param_set, dimension_set, param_delete, sketch_create, sketch_add, sketch_project, sketch_constrain, sketch_remove, sketch_edit,
    /// plane_offset, extrude, revolve, sweep, loft, fillet, chamfer, hole, shell, push_face, linear_array, circular_array,
    /// mirror, thread, split or feature_delete. Reads, document/file tools, undo and nested batch are refused.
    tool: String,
    /// Exactly the named tool's standalone arguments, including its JSON-string object/array leniency.
    args: Value,
}

fn decode(args: Value) -> Result<BatchArgs, String> {
    // Step args are opaque here: each tool's own schema must decode them from depth zero and attribute
    // malformed strings/depth errors to the step, rather than to the outer batch envelope.
    serde_json::from_value(args).map_err(|e| format!("bad arguments for `batch`: {e}"))
}

pub(super) fn tool() -> Tool {
    Tool {
        name: "batch",
        description: "Run 1..=200 predictable modelling operations in order with one undo entry and all-or-nothing rollback. Use batch for known parameters, sketches and features referenced by name/id; use single calls when you need topology ids, sketch_info, select or a render between operations. Only param_set, dimension_set, param_delete, sketch_create, sketch_add, sketch_project, sketch_constrain, sketch_remove, sketch_edit, plane_offset, extrude, revolve, sweep, loft, fillet, chamfer, hole, shell, push_face, linear_array, circular_array, mirror, thread, split and feature_delete are allowed. Every step's args are validated with that tool's standalone argument struct and JSON-string leniency before anything runs. Each handler keeps its normal rebuild/checks. Errors identify the 1-based step and tool; nothing was changed. Compact results contain step/tool and created node id/name when applicable, final bodies/warnings/errors; verbose=true adds each step's full normal JSON result as result. Undo reports batch and Batch (N steps); counts as one of 16 retained calls.",
        schema: input_schema::<BatchArgs>(),
        validate: Box::new(|args| decode(args).map(|_| ())),
        handler: Box::new(run),
    }
}

fn run(tools: &[Tool], state: &mut State, arguments: Value) -> Result<Content, String> {
    let batch = decode(arguments.clone())?;
    if !(1..=200).contains(&batch.steps.len()) {
        return Err("batch requires 1..=200 steps; nothing was changed".into());
    }
    let mut handlers = Vec::with_capacity(batch.steps.len());
    for (index, step) in batch.steps.iter().enumerate() {
        let validate = || -> Result<&Tool, String> {
            if !modelling_tool(&step.tool) {
                return Err("tool is not allowed in batch".into());
            }
            let tool = tools.iter().find(|t| t.name == step.tool).ok_or_else(|| "tool is unavailable".to_string())?;
            (tool.validate)(step.args.clone())?;
            Ok(tool)
        };
        handlers.push(validate().map_err(|e| step_error(index, step, &e))?);
    }
    let mut snapshot = state.doc()?.begin_tool_edit().map_err(|e| e.to_string())?;
    snapshot.record_call("batch", arguments);
    let result = execute(tools, state, &batch, &handlers);
    // No allowed handler replaces the Session; both success and failure close the same boundary.
    state.doc()?.finish_tool_edit(snapshot, result.is_ok());
    state.created_node = None;
    result.map(|value| vec![json!({"type":"text", "text":value.to_string()})])
}

fn step_error(index: usize, step: &Step, reason: &str) -> String {
    format!("batch step {} (`{}`): {reason}; nothing was changed", index + 1, step.tool)
}

fn execute(tools: &[Tool], state: &mut State, batch: &BatchArgs, handlers: &[&Tool]) -> Result<Value, String> {
    let mut steps = Vec::with_capacity(batch.steps.len());
    for (index, (step, tool)) in batch.steps.iter().zip(handlers).enumerate() {
        state.created_node = None;
        let content = (tool.handler)(tools, state, step.args.clone()).map_err(|e| step_error(index, step, &e))?;
        let result: Value = content
            .first()
            .and_then(|c| c["text"].as_str())
            .ok_or_else(|| step_error(index, step, "expected JSON text result"))
            .and_then(|text| serde_json::from_str(text).map_err(|e| step_error(index, step, &e.to_string())))?;
        let mut entry = json!({"step":index + 1, "tool":step.tool});
        if let Some(id) = state.created_node {
            entry["created"] = json!({"id":id, "name":state.doc()?.node_name(id)});
        }
        // Small operation metadata is useful without repeating full sketch details or body metrics.
        for key in ["ok", "deleted", "index", "dof", "notes"] {
            if let Some(value) = result.get(key) {
                entry[key] = value.clone();
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
    let report = state.doc()?.current_rebuild();
    Ok(json!({"ok":true, "steps":steps, "bodies":report.bodies, "warnings":report.warnings, "errors":report.errors}))
}
