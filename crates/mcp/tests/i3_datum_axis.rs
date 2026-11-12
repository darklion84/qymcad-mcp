//! D3 over the registry: datum_axis arguments are strict, batchable, undoable; the axis feeds circular_array; Part ownership.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}

fn seed(r: &mut Registry, name: &str) -> Value {
    call(r, "sketch_create", json!({"plane":"XY", "name":name}));
    call(r, "sketch_add", json!({"sketch":name, "entities":[{"type":"circle", "cx":10, "d":2}]}));
    call(r, "extrude", json!({"sketch":name, "height":3, "op":"new_body"}))["body"].clone()
}

#[test]
fn datum_axis_feeds_an_array_is_strict_and_deletes_in_order() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    let body = seed(&mut r, "seed");
    for bad in [
        json!({}),
        json!({"origin":[0,0,0]}),
        json!({"dir":[0,0,1]}),
        json!({"origin":[0,0,0],"dir":[0,0,0]}),
        json!({"origin":["w",0,0],"dir":[0,0,1]}),
        json!({"edge":{"edge":1},"face":{"face":2}}),
        json!({"points":["a","b"],"origin":[0,0,0],"dir":[0,0,1]}),
        json!({"edge":{"edge":1,"typo":1}}),
        json!({"origin":[0,0,0],"dir":[0,0,1],"typo":1}),
    ] {
        assert!(r.call("datum_axis", bad).unwrap().is_err());
    }
    let axis = call(&mut r, "datum_axis", json!({"origin":[0,0,0],"dir":[0,0,1],"name":"pivot"}));
    assert_eq!((axis["def"].as_str(), axis["name"].as_str()), (Some("manual"), Some("pivot")));
    let id = axis["axis"].as_u64().unwrap();
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(info["datums"]["axes"][0]["id"], id);
    assert_eq!(info["datums"]["axes"][0]["dir"], json!([0.0, 0.0, 1.0]));
    let array = call(&mut r, "circular_array", json!({"body":body, "count":2, "axis":{"datum":id}}));
    let made = array["rebuild"]["bodies"].as_array().unwrap().iter().find(|b| b["id"] == array["body"]).unwrap();
    assert!((made["volume_mm3"].as_f64().unwrap() - 2.0 * std::f64::consts::PI * 3.0).abs() < 1e-6, "{made}");
    // The axis under the array: refused without cascade, undo restores everything after a cascade.
    let refused = r.call("feature_delete", json!({"feature":id})).unwrap().unwrap_err();
    assert!(refused.contains("has dependents"), "{refused}");
    call(&mut r, "feature_delete", json!({"feature":id,"cascade":true}));
    assert!(call(&mut r, "doc_info", json!({})).get("datums").is_none());
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "feature_delete");
    assert_eq!(call(&mut r, "doc_info", json!({}))["datums"]["axes"].as_array().unwrap().len(), 1);
}

#[test]
fn points_by_name_in_a_batch_and_one_undo_step() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    let steps = json!({"steps":[
        {"tool":"datum_point","args":{"at":[0,0,0],"name":"base"}},
        {"tool":"datum_point","args":{"at":[0,0,5],"name":"tip"}},
        {"tool":"datum_axis","args":{"points":["base","tip"],"name":"stem"}}]});
    let out = call(&mut r, "batch", steps.clone());
    assert_eq!(out["steps"][2]["created"]["name"], "stem");
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(info["datums"]["axes"][0]["def"], "two_points");
    assert_eq!(info["datums"]["axes"][0]["dir"], json!([0.0, 0.0, 1.0]));
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "batch");
    assert!(call(&mut r, "doc_info", json!({})).get("datums").is_none());
    // A late failure rolls the whole batch back.
    let mut late = steps;
    late["steps"][2]["args"] = json!({"points":["base","base"]});
    assert!(r.call("batch", late).unwrap().is_err());
    assert!(call(&mut r, "doc_info", json!({})).get("datums").is_none());
}

#[test]
fn an_axis_of_another_part_is_refused() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    let axis = call(&mut r, "datum_axis", json!({"origin":[0,0,0],"dir":[0,0,1]}))["axis"].clone();
    call(&mut r, "part_new", json!({"name":"other"}));
    let body = seed(&mut r, "seed2");
    let e = r.call("circular_array", json!({"body":body, "count":2, "axis":{"datum":axis}})).unwrap().unwrap_err();
    assert!(e.contains("another Part"), "{e}");
}
