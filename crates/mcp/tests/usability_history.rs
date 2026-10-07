use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, name: &str, args: Value) -> Value {
    let content = r.call(name, args).unwrap_or_else(|e| panic!("{e}")).unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}
fn stock(r: &mut Registry) -> u64 {
    call(r, "doc_new", json!({}));
    call(r, "param_set", json!({"name":"h", "value":20}));
    call(r, "sketch_create", json!({"plane":"XY", "name":"profile"}));
    call(r, "sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":20, "h":20}]}));
    call(r, "extrude", json!({"sketch":"profile", "height":"h", "name":"stock"}))["body"].as_u64().unwrap()
}
#[test]
fn deleting_the_last_feature_and_undo_are_tools() {
    let mut r = Registry::new();
    stock(&mut r);
    let before = call(&mut r, "doc_info", json!({}));
    call(&mut r, "feature_delete", json!({"feature":"stock"}));
    assert!(call(&mut r, "doc_info", json!({}))["bodies"].as_array().unwrap().is_empty());
    call(&mut r, "undo", json!({}));
    assert_eq!(call(&mut r, "doc_info", json!({})), before, "undo restores document");
}
#[test]
fn dependent_deletion_is_refused_unless_cascade_is_set() {
    let mut r = Registry::new();
    stock(&mut r);
    let before = call(&mut r, "doc_info", json!({}));
    let error = r.call("feature_delete", json!({"feature":"profile"})).unwrap().unwrap_err();
    assert!(error.contains("depend") && error.contains("cascade"), "{error}");
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
    call(&mut r, "feature_delete", json!({"feature":"profile", "cascade":true}));
    assert!(call(&mut r, "doc_info", json!({}))["bodies"].as_array().unwrap().is_empty());
}
