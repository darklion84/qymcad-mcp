//! Tool registry. Each group lives in its own module and exposes `fn tools() -> Vec<Tool>`; adding a group
//! means adding a module and one line in `Registry::new` (keeps parallel work conflict-free).

mod appearance;
mod assembly;
mod batch;
mod common;
mod datums;
mod doc;
mod features;
mod history;
mod import;
mod import_mesh;
mod measure;
mod output;
mod params;
mod primitives;
mod print;
mod recognise;
mod rename;
mod sketch;
mod sketch_edit;
mod sketch_import;
mod sketch_pattern;
mod topology;

pub use topology::SEL_BRIEF;

use qymcad_engine::Session;
use std::borrow::Cow;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

/// MCP content items (`[{ "type": "text", "text": ... }, { "type": "image", ... }]`).
pub type Content = Vec<Value>;

/// Server state shared by all tools: the one open document.
#[derive(Default)]
pub struct State {
    pub session: Option<Session>,
    created_node: Option<qymcad_engine::Id>,
    /// A successful same-state no-op restores its snapshot instead of consuming an undo entry.
    skip_history: bool,
    /// Set when a tool panicked (ADR 0058): the call was rolled back, but the process is suspect, so only
    /// reading and saving the work elsewhere stay available until the server restarts.
    tainted: Option<String>,
}

impl State {
    pub fn doc(&mut self) -> Result<&mut Session, String> {
        self.session.as_mut().ok_or_else(|| "no document is open: call doc_new or doc_open first".to_string())
    }
}

type Handler = Box<dyn Fn(&[Tool], &mut State, Value) -> Result<Content, String>>;
type Validator = Box<dyn Fn(Value) -> Result<(), String>>;

pub struct Tool {
    pub name: &'static str,
    pub description: Cow<'static, str>,
    pub schema: Value,
    /// A modelling tool: runs under one tool-edit snapshot and may be a `batch` step.
    pub mutating: bool,
    handler: Handler,
    validate: Validator,
}

/// A tool whose result is a JSON value, returned to the model as compact JSON text.
pub fn tool<A, F>(name: &'static str, description: &'static str, f: F) -> Tool
where
    A: DeserializeOwned + JsonSchema,
    F: Fn(&mut State, A) -> Result<Value, String> + 'static,
{
    tool_content(name, description, move |st, a: A| {
        f(st, a).map(|v| {
            // Creation responses return their node id under body, plane or sketch. Capture it before
            // serialization so undo labels follow the returned feature rather than auxiliary nodes.
            st.created_node = if name == "sketch_project" {
                None // Projection returns its source body and target sketch; neither node was created.
            } else if name == "primitive" {
                v["feature"].as_u64() // `body` is the union node there, not the node the call created.
            } else {
                ["body", "plane", "sketch", "pattern"].iter().find_map(|key| v[*key].as_u64()).or_else(|| match name {
                    "part_new" | "part_instance" | "part_copy" => v["part"]["id"].as_u64(),
                    "joint_create" => v["joint"]["id"].as_u64(),
                    "datum_point" => v["point"].as_u64(),
                    "datum_axis" => v["axis"].as_u64(),
                    _ => None,
                })
            };
            vec![json!({ "type": "text", "text": v.to_string() })]
        })
    })
}

/// A tool that builds its own content items (e.g. images).
pub fn tool_content<A, F>(name: &'static str, description: &'static str, f: F) -> Tool
where
    A: DeserializeOwned + JsonSchema,
    F: Fn(&mut State, A) -> Result<Content, String> + 'static,
{
    let schema = input_schema::<A>();
    let argument_schema = schema.clone();
    let validation_schema = schema.clone();
    let validate: Validator = Box::new(move |args| decode_arguments::<A>(name, &validation_schema, args).map(|_| ()));
    let handler: Handler = Box::new(move |_, st, args| {
        let a = decode_arguments::<A>(name, &argument_schema, args)?;
        f(st, a)
    });
    Tool { name, description: description.into(), schema, mutating: MODELLING_TOOLS.contains(&name), handler, validate }
}

/// The validator and standalone handler use precisely the same decoding and typed deserialization.
fn decode_arguments<A: DeserializeOwned>(name: &str, schema: &Value, mut args: Value) -> Result<A, String> {
    decode_structured_strings(&mut args, &[schema], schema, "arguments", 0).map_err(|e| format!("bad arguments for `{name}`: {e}"))?;
    serde_json::from_value(args).map_err(|e| format!("bad arguments for `{name}`: {e}"))
}

/// What a tainted server still does (ADR 0058), as told to the model.
const TAINTED_RULE: &str = "Until the server restarts, only doc_info, doc_save to a new path, export and render are available; \
    save the work under a new name and restart the server to continue modelling";

/// After a panic the process is suspect: like a CAD app's recovery mode, it reads the model and writes it out
/// elsewhere, but never edits it or overwrites the file it came from.
fn tainted_allows(name: &str, args: &Value, session: Option<&Session>) -> Result<(), String> {
    match name {
        "doc_info" | "export" | "render" => Ok(()),
        "doc_save" => {
            let target = args.get("path").and_then(Value::as_str).map(std::path::Path::new);
            let current = session.and_then(Session::path);
            let same = |a: &std::path::Path, b: &std::path::Path| {
                a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
            };
            match (target, current) {
                (None, _) => Err("doc_save needs a new path".into()),
                (Some(t), Some(c)) if same(t, c) => Err("doc_save must not overwrite the file the document came from".into()),
                _ => Ok(()),
            }
        }
        _ => Err(format!("`{name}` is refused")),
    }
}

fn panic_text(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".into())
}

/// The one list of modelling tools: those that mutate the document. It decides the history boundary of a
/// standalone call (`Tool::mutating`, set from here when the tool is built) and the batch allowlist; the batch
/// description and its `tool` argument schema are generated from the registry flag, never typed out again.
const MODELLING_TOOLS: &[&str] = &[
    "param_set",
    "dimension_set",
    "param_delete",
    "sketch_create",
    "sketch_add",
    "sketch_project",
    "sketch_constrain",
    "sketch_remove",
    "sketch_edit",
    "sketch_pattern",
    "sketch_import",
    "plane_offset",
    "datum_point",
    "datum_axis",
    "primitive",
    "extrude",
    "revolve",
    "sweep",
    "loft",
    "fillet",
    "chamfer",
    "hole",
    "shell",
    "push_face",
    "draft",
    "thicken",
    "remove_face",
    "move",
    "linear_array",
    "circular_array",
    "mirror",
    "thread",
    "body_boolean",
    "split",
    "split_face",
    "import",
    "import_mesh",
    "recognise",
    "feature_delete",
    "feature_suppress",
    "feature_reorder",
    "history_rollback",
    "rename",
    "appearance",
    "part_new",
    "part_select",
    "part_place",
    "ground",
    "joint_create",
    "joint_set",
    "joint_delete",
    "part_instance",
    "part_copy",
    "part_delete",
    "joint_limits",
];

/// Resolve schema references and alternatives without losing the original `$defs` root.
fn schema_variants<'a>(schema: &'a Value, root: &'a Value, out: &mut Vec<&'a Value>) {
    if let Some(reference) = schema["$ref"].as_str().and_then(|r| r.strip_prefix('#')).and_then(|r| root.pointer(r)) {
        schema_variants(reference, root, out);
    } else if let Some(alternatives) = ["anyOf", "oneOf", "allOf"].iter().find_map(|key| schema[*key].as_array()) {
        for alternative in alternatives {
            schema_variants(alternative, root, out);
        }
    } else {
        out.push(schema);
    }
}

fn schema_type(schema: &Value, kind: &str) -> bool {
    schema["type"] == kind || schema["type"].as_array().is_some_and(|types| types.iter().any(|t| t == kind))
}

/// Some MCP clients encode structured argument values a second time. Decode only where the schema permits
/// objects/arrays, preserving string-only names, paths and expressions. Opaque object children have no
/// structural schema: retain their strings literally and leave validation to their hand-written parser.
fn decode_structured_strings(value: &mut Value, schemas: &[&Value], root: &Value, path: &str, depth: usize) -> Result<(), String> {
    if depth > 32 {
        return Err(format!("structured argument nesting exceeds 32 at {path}"));
    }
    let mut variants = Vec::new();
    for schema in schemas {
        schema_variants(schema, root, &mut variants);
    }
    let unrestricted = |schema: &Value| schema == &Value::Bool(true) || schema.as_object().is_some_and(|o| o.is_empty());
    if variants.iter().any(|s| schema_type(s, "object") || schema_type(s, "array")) {
        if let Some(encoded) = value.as_str().filter(|s| s.trim_start().starts_with(['{', '['])) {
            *value = serde_json::from_str(encoded).map_err(|e| format!("invalid JSON object/array string at {path}: {e}"))?;
        }
    }
    let any = Value::Bool(true);
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let mut child_schemas: Vec<&Value> = variants.iter().filter_map(|s| s["properties"].get(key)).collect();
                if child_schemas.is_empty() {
                    for schema in &variants {
                        if let Some(additional) = schema.get("additionalProperties") {
                            child_schemas.push(additional);
                        } else if unrestricted(schema) || (schema_type(schema, "object") && schema.get("properties").is_none()) {
                            child_schemas.push(&any);
                        }
                    }
                }
                decode_structured_strings(child, &child_schemas, root, &format!("{path}.{key}"), depth + 1)?;
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter_mut().enumerate() {
                let mut child_schemas = Vec::new();
                for schema in &variants {
                    if let Some(item) = schema["prefixItems"].get(index).or_else(|| schema.get("items")) {
                        child_schemas.push(item);
                    } else if unrestricted(schema) || schema_type(schema, "array") {
                        child_schemas.push(&any);
                    }
                }
                decode_structured_strings(child, &child_schemas, root, &format!("{path}[{index}]"), depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// The JSON schema of an argument struct, trimmed to what MCP clients need.
fn input_schema<A: JsonSchema>() -> Value {
    let mut v = serde_json::to_value(schemars::schema_for!(A)).unwrap_or_else(|_| json!({}));
    if let Some(o) = v.as_object_mut() {
        o.remove("$schema");
        o.remove("title");
        o.entry("type").or_insert(json!("object"));
    }
    v
}

pub struct Registry {
    tools: Vec<Tool>,
    state: State,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Registry {
        let mut tools = Vec::new();
        tools.extend(doc::tools());
        tools.extend(params::tools());
        tools.extend(sketch::tools());
        tools.extend(sketch_edit::tools());
        tools.extend(sketch_pattern::tools());
        tools.extend(sketch_import::tools());
        tools.extend(features::tools());
        tools.extend(datums::tools());
        tools.extend(primitives::tools());
        tools.extend(print::tools());
        tools.extend(import::tools());
        tools.extend(import_mesh::tools());
        tools.extend(recognise::tools());
        tools.extend(output::tools());
        tools.extend(history::tools());
        tools.extend(rename::tools());
        tools.extend(appearance::tools());
        tools.extend(assembly::tools());
        tools.extend(topology::tools());
        tools.extend(measure::tools());
        let modelling: Vec<&'static str> = tools.iter().filter(|t| t.mutating).map(|t| t.name).collect();
        tools.push(batch::tool(&modelling));
        Registry { tools, state: State::default() }
    }

    pub fn list(&self) -> Vec<Value> {
        self.tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": t.schema })).collect()
    }

    /// Whether a tool exists, checked before recording an executable call.
    pub fn contains(&self, name: &str) -> bool {
        self.tools.iter().any(|tool| tool.name == name)
    }

    /// `Err` = no such tool (a protocol error). `Ok(Err)` = the tool ran and failed (a tool error for the model).
    pub fn call(&mut self, name: &str, args: Value) -> Result<Result<Content, String>, String> {
        let t = self.tools.iter().find(|t| t.name == name).ok_or_else(|| format!("unknown tool: {name}"))?;
        if let Some(cause) = &self.state.tainted {
            if let Err(refusal) = tainted_allows(name, &args, self.state.session.as_ref()) {
                return Ok(Err(format!("{cause}. {refusal}. {TAINTED_RULE}")));
            }
        }
        // All modelling mutations share one undo/rollback boundary, including multi-entity sketch_add.
        let mutating = t.mutating;
        let snapshot = if mutating {
            match self.state.doc().and_then(|s| s.begin_tool_edit().map_err(|e| e.to_string())) {
                Ok(mut snapshot) => {
                    snapshot.record_call(name, args.clone());
                    Some(snapshot)
                }
                Err(e) => return Ok(Err(e)),
            }
        } else {
            None
        };
        self.state.created_node = None;
        self.state.skip_history = false;
        let handler = &t.handler;
        let (tools, state) = (&self.tools, &mut self.state);
        let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(tools, state, args))) {
            Ok(result) => result,
            Err(panic) => {
                let reason = panic_text(&*panic);
                if let (Some(snapshot), Some(s)) = (snapshot, self.state.session.as_mut()) {
                    s.finish_tool_edit(snapshot, false);
                }
                self.state.tainted = Some(format!("`{name}` hit an internal error: {reason}"));
                return Ok(Err(format!(
                    "internal error in `{name}`: {reason}. The document is as it was before this call. {TAINTED_RULE}"
                )));
            }
        };
        if let Some(mut snapshot) = snapshot {
            if let Some(id) = self.state.created_node {
                snapshot.record_created_node(id);
            }
            if let Some(s) = self.state.session.as_mut() {
                s.finish_tool_edit(snapshot, result.is_ok() && !self.state.skip_history);
            }
        }
        Ok(result)
    }

    /// docs/TOOLS.md, generated from the registry so it cannot drift.
    pub fn markdown(&self) -> String {
        let mut s =
            String::from("# Tools\n\nGenerated by `cargo run -p qymcad-mcp -- --dump-tools > docs/TOOLS.md`. Do not edit by hand.\n\n");
        for t in &self.tools {
            s.push_str(&format!("- [`{}`](#{})\n", t.name, t.name));
        }
        for t in &self.tools {
            let schema = serde_json::to_string_pretty(&t.schema).unwrap_or_default();
            s.push_str(&format!("\n## {}\n\n{}\n\n```json\n{}\n```\n", t.name, t.description, schema));
        }
        s
    }
}

#[cfg(test)]
mod decoder_tests {
    use super::*;

    #[test]
    fn structured_depth_boundary_includes_native_and_encoded_values() {
        let schema = json!({"type":"array", "items":true});
        for encoded in [false, true] {
            for depth in [32, 33] {
                let mut value = json!(0);
                for _ in 0..depth {
                    value = json!([value]);
                }
                if encoded {
                    value = json!(value.to_string());
                }
                let result = decode_structured_strings(&mut value, &[&schema], &schema, "arguments", 0);
                if depth == 32 {
                    assert!(result.is_ok(), "32 levels allowed: {result:?}");
                } else {
                    assert!(result.unwrap_err().contains("structured argument nesting exceeds 32 at arguments[0]"));
                }
            }
        }
    }
}

#[cfg(test)]
mod undo_label_tests {
    use super::*;
    use qymcad_engine::{BaseName, PlaneRef};

    #[test]
    fn undo_label_follows_the_returned_node_instead_of_the_last_auxiliary_node() {
        let mut registry = Registry::new();
        registry.state.session = Some(Session::new_part());
        registry.tools = vec![tool("fillet", "Test feature with an auxiliary node", |state, _: Value| {
            let session = state.doc()?;
            let created = session.sketch_create(&PlaneRef::Base(BaseName::XY), Some("Requested feature")).unwrap();
            session.sketch_create(&PlaneRef::Base(BaseName::XY), Some("Auxiliary feature")).unwrap();
            Ok(json!({"body": created}))
        })];
        registry.call("fillet", json!({})).unwrap().unwrap();
        let call = registry.state.session.as_mut().unwrap().undo().unwrap().call.unwrap();
        assert_eq!(call.label, "Requested feature", "the result id identifies the tool's created node");
        assert_eq!(call.tool, "fillet");
        assert_eq!(call.arguments, json!({}));
    }

    #[test]
    fn an_empty_created_node_name_keeps_a_human_action_label() {
        let mut registry = Registry::new();
        registry.state.session = Some(Session::new_part());
        let arguments = json!({"plane":"XY", "name":""});
        registry.call("sketch_create", arguments.clone()).unwrap().unwrap();
        let call = registry.state.session.as_mut().unwrap().undo().unwrap().call.unwrap();
        assert_eq!(call.label, "Create sketch", "empty names must not erase the action label");
        assert_eq!(call.tool, "sketch_create");
        assert_eq!(call.arguments, arguments);
    }
}

#[cfg(test)]
mod modelling_list_tests {
    use super::*;

    #[test]
    fn every_listed_modelling_tool_is_registered_and_batch_names_exactly_them() {
        let registry = Registry::new();
        for name in MODELLING_TOOLS {
            let tool = registry.tools.iter().find(|t| t.name == *name);
            assert!(tool.is_some_and(|t| t.mutating), "{name} is listed as modelling but not registered");
        }
        let batch = registry.tools.iter().find(|t| t.name == "batch").unwrap();
        assert!(!batch.mutating, "nested batch is refused");
        let listed = batch.schema.pointer("/$defs/Step/properties/tool/description").and_then(Value::as_str).unwrap();
        for t in &registry.tools {
            assert_eq!(listed.contains(&format!(" {},", t.name)) || listed.contains(&format!(" {}.", t.name)), t.mutating, "{}", t.name);
        }
        assert!(batch.description.contains(&format!("{} modelling tools", MODELLING_TOOLS.len())));
    }
}

#[cfg(test)]
mod panic_tests {
    use super::*;
    use qymcad_engine::{BaseName, PlaneRef};

    fn panicking_fillet() -> Tool {
        tool("fillet", "Test feature that panics after editing", |state, _: Value| {
            state.doc()?.sketch_create(&PlaneRef::Base(BaseName::XY), Some("Half-made")).unwrap();
            panic!("kernel exploded");
        })
    }

    fn timeline_len(registry: &Registry) -> usize {
        registry.state.session.as_ref().unwrap().project().timeline.len()
    }

    #[test]
    fn a_panicking_tool_is_rolled_back_reported_and_taints_the_server() {
        let mut registry = Registry::new();
        registry.call("doc_new", json!({})).unwrap().unwrap();
        let before = timeline_len(&registry);
        registry.tools.retain(|t| t.name != "fillet");
        registry.tools.push(panicking_fillet());

        let error = registry.call("fillet", json!({})).unwrap().unwrap_err();
        assert!(error.contains("internal error in `fillet`: kernel exploded"), "{error}");
        assert!(error.contains("as it was before this call"), "{error}");
        assert_eq!(timeline_len(&registry), before, "the half-made edit is rolled back");
        assert!(registry.state.session.as_mut().unwrap().undo().is_err(), "a failed call leaves no undo entry");

        let refused = registry.call("sketch_create", json!({"plane":"XY"})).unwrap().unwrap_err();
        assert!(refused.contains("`sketch_create` is refused") && refused.contains("restart"), "{refused}");
        assert_eq!(timeline_len(&registry), before);
        assert!(registry.call("doc_info", json!({})).unwrap().is_ok());
        assert!(registry.call("doc_save", json!({})).unwrap().unwrap_err().contains("needs a new path"));

        let dir = std::env::temp_dir().join(format!("qymcad-m7-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("recovered.qcad");
        registry.call("doc_save", json!({"path": path, "allow_empty": true})).unwrap().unwrap();
        let again = registry.call("doc_save", json!({"path": path, "allow_empty": true})).unwrap().unwrap_err();
        assert!(again.contains("must not overwrite"), "{again}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_panic_inside_batch_rolls_back_every_step_before_it() {
        let mut registry = Registry::new();
        registry.call("doc_new", json!({})).unwrap().unwrap();
        let before = timeline_len(&registry);
        registry.tools.retain(|t| t.name != "fillet" && t.name != "batch");
        registry.tools.push(panicking_fillet());
        let modelling: Vec<&'static str> = registry.tools.iter().filter(|t| t.mutating).map(|t| t.name).collect();
        registry.tools.push(batch::tool(&modelling));

        let steps = json!({"steps": [
            {"tool": "sketch_create", "args": {"plane": "XY", "name": "First"}},
            {"tool": "fillet", "args": {}}
        ]});
        let error = registry.call("batch", steps).unwrap().unwrap_err();
        assert!(error.contains("internal error in `batch`: kernel exploded"), "{error}");
        assert_eq!(timeline_len(&registry), before, "the completed first step is rolled back too");
        assert!(registry.call("param_set", json!({"name":"w","value":"5"})).unwrap().is_err());
    }
}
