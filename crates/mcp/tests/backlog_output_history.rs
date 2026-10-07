use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, name: &str, args: Value) -> Value {
    let content = r.call(name, args).unwrap().unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

fn sealed_pocket(r: &mut Registry) -> Vec<String> {
    call(r, "doc_new", json!({}));
    call(r, "sketch_create", json!({"plane":"XY", "name":"profile"}));
    call(r, "sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":20, "h":20}]}));
    call(r, "extrude", json!({"sketch":"profile", "height":20, "name":"stock"}));
    call(r, "plane_offset", json!({"base":"XY", "dist":10, "name":"inside"}));
    call(r, "sketch_create", json!({"plane":{"plane":"inside"}, "name":"pocket_profile"}));
    call(r, "sketch_add", json!({"sketch":"pocket_profile", "entities":[{"type":"rect", "w":4, "h":6}]}));
    call(r, "extrude", json!({"sketch":"pocket_profile", "height":3, "op":"cut", "direction":"reverse", "name":"pocket"}));
    let info = call(r, "doc_info", json!({}));
    // An interior cut includes the 0.001 mm entry clearance: V=20³−4*6*(3+0.001).
    assert!((info["bodies"][0]["volume"].as_f64().unwrap() - (20.0_f64.powi(3) - 4.0 * 6.0 * 3.001)).abs() < 1e-6);
    let warnings: Vec<String> = serde_json::from_value(info["warnings"].clone()).unwrap();
    assert!(warnings.iter().any(|w| w.contains("pocket") && w.contains("sealed internal void")));
    warnings
}

#[test]
fn export_repeats_current_document_warnings() {
    let mut r = Registry::new();
    let warnings = sealed_pocket(&mut r);
    let path = std::env::temp_dir().join(format!("qymcad-backlog-warnings-{}.stl", std::process::id()));
    let result = call(&mut r, "export", json!({"format":"stl", "path":path}));
    assert_eq!(result["warnings"], json!(warnings), "export must repeat current document warnings");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn render_repeats_current_document_warnings() {
    let mut r = Registry::new();
    let warnings = sealed_pocket(&mut r);
    let content = r.call("render", json!({"width":64, "height":64})).unwrap().unwrap();
    assert!(content.iter().any(|item| item["type"] == "image"));
    let text = content.iter().filter_map(|item| item["text"].as_str()).collect::<Vec<_>>().join("\n");
    for warning in warnings {
        assert!(text.contains(&warning), "render must repeat current document warning: {warning}; got: {text}");
    }
}

#[test]
fn undo_reports_restored_parameter_expressions_values_and_bodies() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name":"t", "value":5}));
    call(&mut r, "param_set", json!({"name":"h", "value":"t+1"}));
    call(&mut r, "sketch_create", json!({"plane":"XY", "name":"profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":20, "h":30}]}));
    call(&mut r, "extrude", json!({"sketch":"profile", "height":"h", "name":"stock"}));
    let before = call(&mut r, "doc_info", json!({}));
    let args = json!({"name":"T", "value":9});
    call(&mut r, "param_set", args.clone());
    let result = call(&mut r, "undo", json!({}));
    assert_eq!(result["params"], before["params"], "undo must report restored parameter expressions and values");
    assert_eq!(result["params"], json!([{"name":"t", "expr":"5", "value":5.0}, {"name":"h", "expr":"t+1", "value":6.0}]));
    assert_eq!(result["undone"], json!({"tool":"param_set", "arguments":args}));
    // Restored prism height is h=t+1: V=20*30*(5+1).
    assert_eq!(result["bodies"][0]["volume_mm3"], 20.0 * 30.0 * (5.0 + 1.0));
    call(&mut r, "param_set", json!({"name":"spare", "value":7}));
    let creation_undo = call(&mut r, "undo", json!({}));
    assert_eq!(creation_undo["params"], before["params"], "undoing parameter creation reports its absence");
    call(&mut r, "param_set", json!({"name":"spare", "value":7}));
    let with_spare = call(&mut r, "doc_info", json!({}));
    call(&mut r, "param_delete", json!({"name":"spare"}));
    assert_eq!(call(&mut r, "undo", json!({}))["params"], with_spare["params"], "undoing parameter deletion reports its restored value");
}
