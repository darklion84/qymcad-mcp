//! Thread schemas, native geometry reporting, strict decoding and modelling history through MCP.
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
    call(&mut registry, "sketch_add", json!({"sketch":"section","entities":[{"type":"circle","d":8}]}));
    call(&mut registry, "extrude", json!({"sketch":"section","height":20,"name":"stock"}));
    registry
}

#[test]
fn thread_schema_documents_every_argument_and_requires_length_and_edge() {
    let registry = Registry::new();
    let tools = registry.list();
    let thread = tools.iter().find(|tool| tool["name"] == "thread").expect("thread tool must be registered");
    let schema = &thread["inputSchema"];
    assert_eq!(schema["additionalProperties"], false);
    let required = schema["required"].as_array().unwrap();
    assert!(required.contains(&json!("edge")));
    assert!(required.contains(&json!("length")));
    for field in [
        "body", "edge", "size", "standard", "nominal", "pitch", "length", "kind", "fit", "lead_in", "lead_out", "starts", "left",
        "crest_r", "root_r", "angle", "depth", "name",
    ] {
        assert!(schema["properties"][field]["description"].as_str().is_some_and(|description| !description.is_empty()), "{field}");
    }
}

#[test]
fn thread_alias_reports_basic_geometry_and_standalone_undo() {
    let mut registry = stock();
    let before = call(&mut registry, "doc_info", json!({}));
    let result = call(&mut registry, "thread", json!({"body":"stock","edge":{"extreme":"-z"},"size":"M8","length":20,"name":"bolt"}));
    assert_eq!(result["kind"], "external");
    assert_eq!(result["name"], "bolt");
    assert_eq!(result["nominal"], 8.0);
    assert_eq!(result["pitch"], 1.25);
    assert_eq!(result["lead"], 1.25);
    let fit = result["fit"].as_f64().unwrap();
    let limit = 0.0525 * 1.25 + 0.13 * 1.25 * 30_f64.to_radians().tan();
    assert!((fit - limit).abs() < 1e-12);
    assert!(result["fit_reason"].as_str().unwrap().contains("lowered"));
    let fundamental_height = 3.0_f64.sqrt() * 1.25 / 2.0;
    // The upstream table stores six-decimal coefficients of pitch; allow their published precision.
    assert!((result["basic_pitch_diameter"].as_f64().unwrap() - (8.0 - 0.75 * fundamental_height)).abs() < 2e-6);
    assert!((result["basic_minor_diameter"].as_f64().unwrap() - (8.0 - 17.0 * fundamental_height / 12.0)).abs() < 2e-6);
    let relief = (fit - (0.49 * 1.25 - (1.25 - 1.25 / 8.0) / 2.0)) / (30.0_f64.to_radians().tan());
    assert!((result["radial_relief"].as_f64().unwrap() - relief).abs() < 1e-9);
    let volume = result["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap();
    assert!((550.0..900.0).contains(&volume), "{result}");
    let undone = call(&mut registry, "undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "thread");
    assert_eq!(call(&mut registry, "doc_info", json!({}))["bodies"], before["bodies"]);
}

#[test]
fn thread_canonical_arguments_are_strict_batchable_and_atomic() {
    let mut registry = stock();
    let before = call(&mut registry, "doc_info", json!({}));
    for (args, expected) in [
        (json!({"edge":{"extreme":"-z"},"size":"M8"}), "length"),
        (json!({"edge":{"extreme":"-z"},"size":"M8","length":20,"typo":1}), "unknown field"),
        (json!({"edge":{"extreme":"-z"},"size":"M8","nominal":8,"length":20}), "size"),
        (json!({"edge":{"kind":"circle"},"size":"M8","length":20}), "one"),
        (json!({"edge":{"extreme":"-z"},"size":"M8","length":20,"kind":"internal"}), "internal"),
        (json!({"edge":{"extreme":"-z"},"size":"M8","length":20,"starts":1.5}), "starts"),
        (json!({"edge":{"extreme":"-z"},"size":"M8","length":20,"fit":0.2}), "0.15944"),
    ] {
        let error = registry.call("thread", args).unwrap().unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert_eq!(call(&mut registry, "doc_info", json!({}))["bodies"], before["bodies"]);
    }
    let result = call(
        &mut registry,
        "batch",
        json!({"verbose":true,"steps":[{"tool":"thread","args":{
            "body":"stock","edge":{"extreme":"-z"},"standard":"metric_iso","nominal":8,"pitch":0,
            "length":20,"kind":"external","fit":0.15,"lead_in":1.5,"lead_out":1.5,"starts":1,"left":false,
            "crest_r":0,"root_r":0,"angle":60,"depth":0,"name":"bolt"
        }}]}),
    );
    assert_eq!(result["steps"][0]["created"]["name"], "bolt");
    assert_eq!(result["steps"][0]["result"]["pitch"], 1.25);
    assert_eq!(call(&mut registry, "undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(call(&mut registry, "doc_info", json!({}))["bodies"], before["bodies"]);
}
