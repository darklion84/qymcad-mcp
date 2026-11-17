//! `tools/list` is sent in full to clients without deferred tool loading, on every session. These tests keep its
//! size from growing back (review finding T1b/T1b+).
use qymcad_mcp::tools::{Registry, SEL_BRIEF};
use serde_json::json;

/// Size of the serialized `tools/list` result when the budget was set (209135 bytes) plus about 5%.
/// Raising it is a decision: shorten a description or a doc comment first.
const BUDGET_BYTES: usize = 212_400;

#[test]
fn tools_list_stays_within_budget() {
    let list = Registry::new().list();
    let total = serde_json::to_string(&list).unwrap().len();
    if total > BUDGET_BYTES {
        let mut rows: Vec<(usize, String)> =
            list.iter().map(|t| (serde_json::to_string(t).unwrap().len(), t["name"].as_str().unwrap().to_string())).collect();
        rows.sort_by(|a, b| b.cmp(a));
        let top: Vec<String> = rows.iter().take(6).map(|(n, name)| format!("{name} {n}")).collect();
        panic!("tools/list is {total} bytes, budget {BUDGET_BYTES}; largest: {}", top.join(", "));
    }
}

#[test]
fn selection_grammar_is_sent_once_and_in_select() {
    let list = Registry::new().list();
    let all = serde_json::to_string(&list).unwrap();
    assert_eq!(all.matches("Selection grammar (faces or edges)").count(), 1, "the grammar must not be repeated per tool");
    assert_eq!(all.matches("tangent_chain").count(), 1, "grammar keywords must not leak into other schemas");
    let select = list.iter().find(|t| t["name"] == "select").unwrap()["description"].as_str().unwrap();
    assert!(select.contains("Selection grammar (faces or edges)"), "the grammar lives in `select`");
    // Every other selection argument points at it.
    assert!(all.contains("Full grammar: tool `select`."));
}

#[test]
fn selection_brief_is_short_and_in_every_selection_schema() {
    assert!(SEL_BRIEF.chars().count() <= 300, "SEL_BRIEF is {} characters", SEL_BRIEF.chars().count());
    for key in ["ids", "edges_of", "along", "facing", "largest"] {
        assert!(SEL_BRIEF.contains(key), "SEL_BRIEF must mention {key}");
    }
    assert!(SEL_BRIEF.contains("Full grammar: tool `select`"));
    let list = Registry::new().list();
    for name in
        ["fillet", "chamfer", "hole", "shell", "push_face", "draft", "thicken", "face_copy", "offset_surface", "surface_replace", "patch"]
    {
        let tool = list.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("no tool {name}"));
        // Escaped as it appears inside the serialized schema.
        let escaped = serde_json::to_string(SEL_BRIEF).unwrap();
        let escaped = &escaped[1..escaped.len() - 1];
        assert!(serde_json::to_string(tool).unwrap().contains(escaped), "{name}: its selection schema lacks SEL_BRIEF");
    }
}

#[test]
fn selector_parse_error_carries_the_brief() {
    let mut registry = Registry::new();
    let result = registry.call("select", json!({"faces": {"bogus": 1}})).unwrap();
    let error = result.expect_err("an unknown selection key must be refused");
    assert!(error.contains(SEL_BRIEF), "{error}");
    let error = registry.call("select", json!({"faces": true})).unwrap().expect_err("a boolean is not a selection");
    assert!(error.contains(SEL_BRIEF), "{error}");
}

/// The compaction pass (`compact_schema`): generator noise is gone from every schema and the meaning is kept.
#[test]
fn schemas_carry_no_generator_noise_and_keep_their_meaning() {
    let list = Registry::new().list();
    fn walk(v: &serde_json::Value, path: &str, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(o) => {
                if o.get("format").is_some_and(|f| f.is_string()) && o.get("type").is_some_and(|t| t != "string") {
                    out.push(format!("{path}: format"));
                }
                if o.get("default").is_some_and(|d| d.is_null()) {
                    out.push(format!("{path}: default null"));
                }
                for (k, child) in o {
                    walk(child, &format!("{path}/{k}"), out);
                }
            }
            serde_json::Value::Array(a) => a.iter().enumerate().for_each(|(i, child)| walk(child, &format!("{path}/{i}"), out)),
            _ => {}
        }
    }
    let mut noise = Vec::new();
    for tool in &list {
        walk(&tool["inputSchema"], tool["name"].as_str().unwrap(), &mut noise);
    }
    assert!(noise.is_empty(), "{}", noise.join("; "));
    let fillet = &list.iter().find(|t| t["name"] == "fillet").unwrap()["inputSchema"];
    assert_eq!(fillet["$defs"]["ObjRef"]["type"], json!(["integer", "string"]), "an id or a name");
    assert!(fillet["$defs"]["ObjRef"]["description"].is_string());
    assert_eq!(fillet["$defs"]["Num"]["type"], json!(["number", "string"]), "a number or an expression");
    let extrude = &list.iter().find(|t| t["name"] == "extrude").unwrap()["inputSchema"];
    assert!(extrude["required"].as_array().is_some_and(|r| r.contains(&json!("sketch"))), "{extrude}");
}
