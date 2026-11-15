//! Fixed baked sketch text, strict arguments, profile defaults and modelling history through MCP.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";
// Independent shoelace-with-nesting reference for pinned Arial ABC at em10 (stage-0 probe).
const ABC_AREA: f64 = 50.826060120016;

fn call(registry: &mut Registry, tool: &str, args: Value) -> Value {
    let content = registry.call(tool, args).unwrap().unwrap_or_else(|error| panic!("{tool}: {error}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

fn sketch() -> Registry {
    let mut registry = Registry::new();
    call(&mut registry, "doc_new", json!({}));
    call(&mut registry, "sketch_create", json!({"plane":"XY","name":"labels"}));
    registry
}

fn text_schema(value: &Value) -> Option<&Value> {
    if value["properties"]["type"]["const"] == "text" {
        return Some(value);
    }
    match value {
        Value::Object(map) => map.values().find_map(text_schema),
        Value::Array(values) => values.iter().find_map(text_schema),
        _ => None,
    }
}

#[test]
fn text_schema_requires_fixed_numbers_and_documents_font_and_removal() {
    let registry = Registry::new();
    let tools = registry.list();
    let add = tools.iter().find(|tool| tool["name"] == "sketch_add").unwrap();
    let schema = text_schema(&add["inputSchema"]).expect("sketch_add must advertise text");
    for field in ["text", "at", "height"] {
        assert!(schema["required"].as_array().unwrap().contains(&json!(field)), "{field}");
    }
    for field in ["text", "at", "height", "angle", "font", "construction"] {
        assert!(schema["properties"][field]["description"].as_str().is_some_and(|s| !s.is_empty()), "{field}");
    }
    assert_eq!(schema["properties"]["height"]["type"], "number");
    assert_eq!(schema["properties"]["angle"]["type"], "number");
    assert_eq!(schema["properties"]["at"]["items"]["type"], "number");
    assert!(add["description"].as_str().unwrap().contains("fixed"));
    let remove = tools.iter().find(|tool| tool["name"] == "sketch_remove").unwrap();
    assert!(remove["description"].as_str().unwrap().contains("text id"));
}

#[test]
fn text_profiles_keep_all_letters_and_counters_through_edit_and_reopen() {
    let mut registry = sketch();
    call(&mut registry, "param_set", json!({"name":"Depth","value":2}));
    let added = call(
        &mut registry,
        "sketch_add",
        json!({"sketch":"labels","entities":[{
            "type":"text","text":"ABC","at":[3,4],"height":10
        }]}),
    );
    let id = added["created"][0]["text"].as_u64().expect("new native text id");
    let detail = call(&mut registry, "sketch_info", json!({"sketch":"labels"}));
    assert_eq!(detail["texts"][0]["id"], id);
    assert_eq!(detail["texts"][0]["text"], "ABC");
    assert_eq!(detail["texts"][0]["font"]["path"], ARIAL);
    assert_eq!(detail["texts"][0]["font"]["index"], 0);
    assert_eq!(detail["texts"][0]["height"], 10.0);
    assert_eq!(detail["texts"][0]["at"], json!([3.0, 4.0]));
    assert_eq!(detail["texts"][0]["angle"], 0.0);
    assert!(detail["texts"][0]["contours"].as_u64().unwrap() >= 6);
    let extruded = call(&mut registry, "extrude", json!({"sketch":"labels","height":"Depth","name":"letters"}));
    let volume = extruded["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap();
    assert!((volume - 2.0 * ABC_AREA).abs() < 1e-6, "all letters and holes: {extruded}");
    let path = std::env::temp_dir().join(format!("qymcad-text-mcp-{}.qcad", std::process::id()));
    call(&mut registry, "doc_save", json!({"path":path}));
    call(&mut registry, "doc_open", json!({"path":path}));
    assert_eq!(call(&mut registry, "sketch_info", json!({"sketch":"labels"}))["texts"], detail["texts"]);
    call(&mut registry, "param_set", json!({"name":"Depth","value":3}));
    let info = call(&mut registry, "doc_info", json!({}));
    assert!((info["bodies"][0]["volume_mm3"].as_f64().unwrap() - 3.0 * ABC_AREA).abs() < 1e-6);
    let before = call(&mut registry, "sketch_info", json!({"sketch":"labels"}));
    assert!(registry.call("sketch_remove", json!({"sketch":"labels","entity":id})).unwrap().is_err());
    assert_eq!(call(&mut registry, "sketch_info", json!({"sketch":"labels"})), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn text_rejects_formulas_fonts_glyphs_and_typos_atomically() {
    let mut registry = sketch();
    let before = call(&mut registry, "sketch_info", json!({"sketch":"labels"}));
    for field in ["height", "angle", "at"] {
        let mut entity = json!({"type":"text","text":"ABC","at":[3,4],"height":10});
        entity[field] = if field == "at" { json!(["Position", 4]) } else { json!("Size") };
        let error = registry.call("sketch_add", json!({"sketch":"labels","entities":[entity]})).unwrap().unwrap_err();
        assert!(error.contains("fixed") && error.contains("recreate"), "{error}");
        assert_eq!(call(&mut registry, "sketch_info", json!({"sketch":"labels"})), before);
    }
    for entity in [
        json!({"type":"text","text":"A","height":10}),
        json!({"type":"text","text":"A","at":[0,0]}),
        json!({"type":"text","text":"A","at":[0,0],"height":0}),
        json!({"type":"text","text":"A","at":[0,0],"height":10,"typo":1}),
        json!({"type":"text","text":"A","at":[0,0],"height":10,"font":{"path":ARIAL,"typo":1}}),
        json!({"type":"text","text":"A","at":[0,0],"height":10,"font":{"path":ARIAL,"index":99999}}),
        json!({"type":"text","text":"A","at":[0,0],"height":10,"font":{"path":"/private/tmp/qymcad-nonexistent-font-print1.ttf"}}),
        json!({"type":"text","text":"A\u{10ffff}","at":[0,0],"height":10}),
    ] {
        let error = registry
            .call(
                "sketch_add",
                json!({"sketch":"labels","entities":[
                    {"type":"circle","cx":-20,"d":2}, entity
                ]}),
            )
            .unwrap()
            .unwrap_err();
        assert!(!error.is_empty());
        assert_eq!(call(&mut registry, "sketch_info", json!({"sketch":"labels"})), before);
    }
    let error = registry
        .call(
            "sketch_add",
            json!({"sketch":"labels","entities":[{
                "type":"text","text":"A\u{10ffff}","at":[0,0],"height":10
            }]}),
        )
        .unwrap()
        .unwrap_err();
    assert!(error.contains('\u{10ffff}'), "missing glyph is listed: {error}");
}

#[test]
fn text_removal_standalone_and_batch_share_undo_and_rollback() {
    let mut registry = sketch();
    let added = call(
        &mut registry,
        "sketch_add",
        json!({"sketch":"labels","entities":[{
            "type":"text","text":"ABC","at":[0,0],"height":10,"font":{"path":ARIAL}
        }]}),
    );
    let id = added["created"][0]["text"].as_u64().unwrap();
    call(&mut registry, "sketch_remove", json!({"sketch":"labels","entity":id}));
    assert!(call(&mut registry, "sketch_info", json!({"sketch":"labels"}))["texts"].as_array().unwrap().is_empty());
    call(&mut registry, "undo", json!({}));
    assert_eq!(call(&mut registry, "sketch_info", json!({"sketch":"labels"}))["texts"][0]["id"], id);
    call(&mut registry, "undo", json!({})); // undo original text creation
    let before = call(&mut registry, "sketch_info", json!({"sketch":"labels"}));
    let result = call(
        &mut registry,
        "batch",
        json!({"steps":[
            {"tool":"sketch_add","args":{"sketch":"labels","entities":[{"type":"text","text":"ABC","at":[3,4],"height":10}]}},
            {"tool":"extrude","args":{"sketch":"labels","height":2}}
        ]}),
    );
    assert_eq!(result["bodies"].as_array().unwrap().len(), 1);
    assert_eq!(call(&mut registry, "undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(call(&mut registry, "sketch_info", json!({"sketch":"labels"})), before);
    assert!(registry
        .call(
            "batch",
            json!({"steps":[
                {"tool":"sketch_add","args":{"sketch":"labels","entities":[{"type":"text","text":"ABC","at":[3,4],"height":10}]}},
                {"tool":"sketch_add","args":{"sketch":"labels","entities":[{"type":"text","text":"\u{10ffff}","at":[0,0],"height":10}]}}
            ]})
        )
        .unwrap()
        .is_err());
    assert_eq!(call(&mut registry, "sketch_info", json!({"sketch":"labels"})), before);
}
