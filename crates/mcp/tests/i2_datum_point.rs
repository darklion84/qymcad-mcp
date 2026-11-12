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

/// A 10 x 6 x 4 block named "blk"; its first vertical edge id.
fn block(r: &mut Registry) -> (Value, u64) {
    call(r, "sketch_create", json!({"plane":"XY", "name":"base"}));
    call(r, "sketch_add", json!({"sketch":"base", "entities":[{"type":"rect", "w":10, "h":6}]}));
    let body = call(r, "extrude", json!({"sketch":"base", "height":4, "op":"new_body"}))["body"].clone();
    let edges = call(r, "topology", json!({"body":body}))["edges"].clone();
    let edge = edges
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "line" && (e["a"][2].as_f64().unwrap() - e["b"][2].as_f64().unwrap()).abs() > 1.0)
        .unwrap();
    (body, edge["id"].as_u64().unwrap())
}

#[test]
fn a_vertex_point_over_the_registry_follows_dimensions_refuses_dimension_set_and_names_its_owner() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    let (body, edge) = block(&mut r);
    let out = call(&mut r, "datum_point", json!({"vertex":{"edge":edge, "end":"end", "body":body}, "name":"corner"}));
    assert_eq!(out["def"], "vertex");
    assert_eq!(out["vertex"], json!({"body":body, "edge":edge, "end":"end"}));
    let at = out["at"].as_array().unwrap().clone();
    assert_eq!(at.len(), 3);
    let id = out["point"].as_u64().unwrap();
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(info["datums"]["points"][0]["at"], out["at"], "doc_info reads the same place");
    assert!(info["datums"]["points"][0].get("owner_part").is_none(), "one unplaced Part: no owner field");
    // F-174: the app ignores a typed coordinate on a vertex point.
    let refused = r.call("dimension_set", json!({"target":{"feature":id, "key":"x"}, "value":5})).unwrap().unwrap_err();
    assert!(refused.contains("F-174") && refused.contains("vertex"), "{refused}");
    // The body under the point: delete needs cascade, suppress is refused outright.
    let refused = r.call("feature_suppress", json!({"feature":body, "suppressed":true})).unwrap().unwrap_err();
    assert!(refused.contains("F-169"), "{refused}");
    let refused = r.call("feature_delete", json!({"feature":body})).unwrap().unwrap_err();
    assert!(refused.contains("has dependents"), "{refused}");
    // Both ends or neither: a body argument of another kind is refused with the offending id in the text.
    let refused = r.call("datum_point", json!({"vertex":{"edge":999999, "end":"start"}})).unwrap().unwrap_err();
    assert!(refused.contains("edge 999999"), "{refused}");
}

#[test]
fn datums_with_the_assembly_root_active_are_refused_and_owners_appear_with_two_parts() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "part_new", json!({"name":"second"}));
    let made = call(&mut r, "datum_point", json!({"at":[1,2,3]}));
    let info = call(&mut r, "doc_info", json!({}));
    let owner = info["datums"]["points"][0]["owner_part"].as_u64().expect("two Parts: the owner is reported");
    assert_eq!(info["active_component"], owner);
    assert_eq!(made["point"], info["datums"]["points"][0]["id"]);
    call(&mut r, "part_select", json!({"part":{"root":true}}));
    for (tool, args) in [("datum_point", json!({"at":[0,0,0]})), ("datum_axis", json!({"origin":[0,0,0], "dir":[0,0,1]}))] {
        let refused = r.call(tool, args).unwrap().unwrap_err();
        assert!(refused.contains("assembly root") && refused.contains("not covered"), "{tool}: {refused}");
    }
}
