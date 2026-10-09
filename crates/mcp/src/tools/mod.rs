//! Tool registry. Each group lives in its own module and exposes `fn tools() -> Vec<Tool>`; adding a group
//! means adding a module and one line in `Registry::new` (keeps parallel work conflict-free).

mod common;
mod doc;
mod features;
mod history;
mod output;
mod params;
mod sketch;
mod topology;

use qymcad_engine::Session;
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
}

impl State {
    pub fn doc(&mut self) -> Result<&mut Session, String> {
        self.session.as_mut().ok_or_else(|| "no document is open: call doc_new or doc_open first".to_string())
    }
}

type Handler = Box<dyn Fn(&mut State, Value) -> Result<Content, String>>;

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
    handler: Handler,
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
            st.created_node = ["body", "plane", "sketch"].iter().find_map(|key| v[*key].as_u64());
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
    let handler: Handler = Box::new(move |st, mut args| {
        decode_structured_strings(&mut args, &[&argument_schema], &argument_schema, "arguments", 0)
            .map_err(|e| format!("bad arguments for `{name}`: {e}"))?;
        let a: A = serde_json::from_value(args).map_err(|e| format!("bad arguments for `{name}`: {e}"))?;
        f(st, a)
    });
    Tool { name, description, schema, handler }
}

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
        tools.extend(features::tools());
        tools.extend(output::tools());
        tools.extend(history::tools());
        tools.extend(topology::tools());
        Registry { tools, state: State::default() }
    }

    pub fn list(&self) -> Vec<Value> {
        self.tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": t.schema })).collect()
    }

    /// `Err` = no such tool (a protocol error). `Ok(Err)` = the tool ran and failed (a tool error for the model).
    pub fn call(&mut self, name: &str, args: Value) -> Result<Result<Content, String>, String> {
        let t = self.tools.iter().find(|t| t.name == name).ok_or_else(|| format!("unknown tool: {name}"))?;
        // All modelling mutations share one undo/rollback boundary, including multi-entity sketch_add.
        let mutating = matches!(
            name,
            "param_set"
                | "param_delete"
                | "sketch_create"
                | "sketch_add"
                | "sketch_constrain"
                | "sketch_remove"
                | "plane_offset"
                | "extrude"
                | "revolve"
                | "fillet"
                | "chamfer"
                | "hole"
                | "shell"
                | "push_face"
                | "linear_array"
                | "circular_array"
                | "mirror"
                | "feature_delete"
        );
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
        let result = (t.handler)(&mut self.state, args);
        if let Some(mut snapshot) = snapshot {
            if let Some(id) = self.state.created_node {
                snapshot.record_created_node(id);
            }
            if let Some(s) = self.state.session.as_mut() {
                s.finish_tool_edit(snapshot, result.is_ok());
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
