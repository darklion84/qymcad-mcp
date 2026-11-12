//! D4 over the registry: datum_point arguments are strict, the tool is batchable and undoable, names reach the app's pool.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn datum_point_is_strict_batchable_undoable_and_named() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name":"w","value":40}));
    for bad in [
        json!({}),
        json!({"at":[1,2,3],"vertex":{"edge":1,"end":"start"}}),
        json!({"at":[1,2]}),
        json!({"at":[1,2,3],"typo":1}),
        json!({"vertex":{"edge":1,"end":"middle"}}),
        json!({"at":[1,2,"w*100000000"]}),
    ] {
        assert!(r.call("datum_point", bad).unwrap().is_err());
    }
    let out = call(&mut r, "datum_point", json!({"at":["w/2",5,0],"name":"anchor"}));
    assert_eq!(out["at"], json!([20.0, 5.0, 0.0]));
    assert_eq!(out["def"], "manual");
    let id = out["point"].as_u64().unwrap();
    assert_eq!(call(&mut r, "doc_info", json!({}))["timeline"].as_array().unwrap().iter().filter(|n| n["id"] == id).count(), 1);
    let undone = call(&mut r, "undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "datum_point");
    assert_eq!(undone["undone"]["label"], "anchor");
    // A batch of two points whose second step fails leaves nothing; a good batch is one undo step.
    let steps = |second: Value| json!({"steps":[{"tool":"datum_point","args":{"at":[1,1,1]}},{"tool":"datum_point","args":second}]});
    assert!(r.call("batch", steps(json!({"at":[1,2]}))).unwrap().is_err());
    let batch = call(&mut r, "batch", steps(json!({"at":[2,2,2],"name":"second"})));
    assert_eq!(batch["steps"][1]["created"]["name"], "second");
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "batch");
    assert!(call(&mut r, "doc_info", json!({}))["timeline"].as_array().unwrap().is_empty());
}
