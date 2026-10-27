//! Split tool identity, geometric plane direction, per-piece exports and history through MCP.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(registry: &mut Registry, tool: &str, args: Value) -> Value {
    let content = registry.call(tool, args).unwrap().unwrap_or_else(|error| panic!("{tool}: {error}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

fn stock() -> Registry {
    let mut registry = Registry::new();
    call(&mut registry, "doc_new", json!({}));
    call(&mut registry, "sketch_create", json!({"plane":"XY","name":"section"}));
    call(&mut registry, "sketch_add", json!({"sketch":"section","entities":[{"type":"rect","w":20,"h":20}]}));
    call(&mut registry, "extrude", json!({"sketch":"section","height":20,"name":"stock"}));
    registry
}

fn sorted_volumes(value: &Value) -> Vec<f64> {
    let mut volumes: Vec<f64> = value.as_array().unwrap().iter().map(|piece| piece["volume_mm3"].as_f64().unwrap()).collect();
    volumes.sort_by(f64::total_cmp);
    volumes
}

#[test]
fn split_schema_requires_plane_and_documents_explicit_body_after_split() {
    let registry = Registry::new();
    let tools = registry.list();
    let split = tools.iter().find(|tool| tool["name"] == "split").expect("split tool must be registered");
    let schema = &split["inputSchema"];
    assert_eq!(schema["additionalProperties"], false);
    assert!(schema["required"].as_array().unwrap().contains(&json!("plane")));
    assert!(split["description"].as_str().unwrap().contains("explicit body"));
    for field in ["body", "plane", "offset", "name", "piece_names"] {
        assert!(schema["properties"][field]["description"].as_str().is_some_and(|description| !description.is_empty()), "{field}");
    }
}

#[test]
fn split_secondary_piece_names_ids_exports_and_roundtrip() {
    let mut registry = stock();
    call(&mut registry, "param_set", json!({"name":"seam","value":3}));
    let result = call(
        &mut registry,
        "split",
        json!({"body":"stock","plane":"XZ","offset":"seam","name":"halves","piece_names":["piece A","piece B"]}),
    );
    assert_eq!(result["body"], result["feature"]);
    assert_eq!(result["pieces"].as_array().unwrap().len(), 2);
    let volumes = sorted_volumes(&result["pieces"]);
    assert!((volumes[0] - 2800.0).abs() < 1e-6 && (volumes[1] - 5200.0).abs() < 1e-6, "{result}");
    let second = &result["pieces"][1];
    assert_eq!(second["name"], "piece B");
    assert_eq!(call(&mut registry, "doc_info", json!({}))["bodies"], result["pieces"]);
    for reference in [second["id"].clone(), json!("piece B")] {
        let topology = call(&mut registry, "topology", json!({"body":reference}));
        assert_eq!(topology["body"], second["id"]);
    }
    let folder = std::env::temp_dir().join(format!("qymcad-split-mcp-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    for format in ["stl", "3mf", "step"] {
        let path = folder.join(format!("piece.{format}"));
        let exported = call(&mut registry, "export", json!({"format":format,"path":path,"bodies":["piece B"]}));
        assert_eq!(exported["bodies"].as_array().unwrap().len(), 1, "{exported}");
        assert_eq!(exported["bodies"][0]["id"], second["id"]);
        assert!(std::fs::metadata(path).unwrap().len() > 0);
    }
    let document = folder.join("split.qcad");
    call(&mut registry, "doc_save", json!({"path":document}));
    call(&mut registry, "doc_open", json!({"path":document}));
    assert_eq!(call(&mut registry, "doc_info", json!({}))["bodies"], result["pieces"]);
    call(&mut registry, "param_set", json!({"name":"seam","value":2}));
    let volumes = sorted_volumes(&call(&mut registry, "doc_info", json!({}))["bodies"]);
    assert!((volumes[0] - 3200.0).abs() < 1e-6 && (volumes[1] - 4800.0).abs() < 1e-6);
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn split_face_datum_and_strict_plane_arguments_are_atomic() {
    let mut registry = stock();
    let before = call(&mut registry, "doc_info", json!({}))["bodies"].clone();
    for args in [
        json!({"plane":"XY","offset":10,"unknown":true}),
        json!({"plane":{"plane":"missing","extra":true},"offset":10}),
        json!({"plane":{"body":"stock","face":{"kind":"plane"}},"offset":-10}),
        json!({"plane":{"body":"stock","face":{"facing":"+z"},"extra":true},"offset":-10}),
        json!({"plane":"XY","offset":30}),
    ] {
        assert!(registry.call("split", args).unwrap().is_err());
        assert_eq!(call(&mut registry, "doc_info", json!({}))["bodies"], before);
    }
    let result = call(
        &mut registry,
        "split",
        json!({"body":"stock","plane":{"body":"stock","face":{"facing":"+z"}},"offset":-10,"name":"face halves"}),
    );
    assert!(sorted_volumes(&result["pieces"]).iter().all(|volume| (volume - 4000.0).abs() < 1e-6));
    call(&mut registry, "undo", json!({}));
    call(&mut registry, "plane_offset", json!({"base":"XY","dist":7,"name":"seam plane"}));
    let result = call(&mut registry, "split", json!({"body":"stock","plane":{"plane":"seam plane"},"name":"datum halves"}));
    let volumes = sorted_volumes(&result["pieces"]);
    assert!((volumes[0] - 2800.0).abs() < 1e-6 && (volumes[1] - 5200.0).abs() < 1e-6);
}

#[test]
fn split_is_batchable_with_piece_targets_and_one_undo() {
    let mut registry = stock();
    let before = call(&mut registry, "doc_info", json!({}))["bodies"].clone();
    let result = call(
        &mut registry,
        "batch",
        json!({"steps":[
            {"tool":"split","args":{"body":"stock","plane":"XY","offset":10,"name":"halves","piece_names":["bottom","top"]}},
            {"tool":"chamfer","args":{"body":"top","edges":{"extreme":"+z"},"dist":0.5,"name":"eased"}}
        ]}),
    );
    assert_eq!(result["steps"][0]["created"]["name"], "halves");
    assert_eq!(result["bodies"].as_array().unwrap().len(), 2);
    assert_eq!(call(&mut registry, "undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(call(&mut registry, "doc_info", json!({}))["bodies"], before);
}
