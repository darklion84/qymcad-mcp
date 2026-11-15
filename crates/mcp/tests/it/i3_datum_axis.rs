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

fn block_with_boss(r: &mut Registry) -> (Value, u64, Value, u64) {
    call(r, "sketch_create", json!({"plane":"XY", "name":"base"}));
    call(r, "sketch_add", json!({"sketch":"base", "entities":[{"type":"rect", "w":10, "h":6}]}));
    let block = call(r, "extrude", json!({"sketch":"base", "height":4, "op":"new_body"}))["body"].clone();
    let edges = call(r, "topology", json!({"body":block}))["edges"].clone();
    let edge = edges
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "line" && (e["a"][2].as_f64().unwrap() - e["b"][2].as_f64().unwrap()).abs() > 1.0)
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    call(r, "sketch_create", json!({"plane":"XY", "name":"boss"}));
    call(r, "sketch_add", json!({"sketch":"boss", "entities":[{"type":"circle", "cx":30, "d":10}]}));
    let boss = call(r, "extrude", json!({"sketch":"boss", "height":10, "op":"new_body"}))["body"].clone();
    let faces = call(r, "topology", json!({"body":boss}))["faces"].clone();
    let face = faces.as_array().unwrap().iter().find(|f| f["kind"] == "cylinder").unwrap()["id"].as_u64().unwrap();
    (block, edge, boss, face)
}

#[test]
fn edge_and_face_axes_over_the_registry_report_unit_directions_and_guard_their_bodies() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    let (block, edge, boss, face) = block_with_boss(&mut r);
    let on_edge = call(&mut r, "datum_axis", json!({"edge":{"edge":edge, "body":block}, "name":"rim"}));
    assert_eq!(on_edge["def"], "edge");
    assert_eq!(on_edge["source"], json!({"body":block, "edge":edge}));
    assert_eq!(on_edge["dir"][2].as_f64().unwrap().abs(), 1.0, "the block's vertical edge: {on_edge}");
    let on_face = call(&mut r, "datum_axis", json!({"face":{"face":face, "body":boss}}));
    assert_eq!(on_face["def"], "face");
    assert!((on_face["origin"][0].as_f64().unwrap() - 30.0).abs() < 1e-6, "{on_face}");
    // Over a seam or a plane face the tool says why.
    let refused = r.call("datum_axis", json!({"edge":{"edge":999999, "body":block}})).unwrap().unwrap_err();
    assert!(refused.contains("edge 999999"), "{refused}");
    let refused = r.call("datum_axis", json!({"face":{"face":999999, "body":boss}})).unwrap().unwrap_err();
    assert!(refused.contains("face 999999"), "{refused}");
    // A manual axis reports a unit direction.
    let manual = call(&mut r, "datum_axis", json!({"origin":[0,0,0], "dir":[0,0,4]}));
    assert_eq!(manual["dir"], json!([0.0, 0.0, 1.0]));
    // The bodies under the edge and face axes cannot be suppressed.
    for body in [&block, &boss] {
        let refused = r.call("feature_suppress", json!({"feature":body, "suppressed":true})).unwrap().unwrap_err();
        assert!(refused.contains("F-169"), "{refused}");
    }
}

#[test]
fn a_revolve_with_the_axis_of_another_part_is_refused_and_a_marker_dimension_is_not_f170() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    let axis = call(&mut r, "datum_axis", json!({"origin":[0,0,0], "dir":[0,1,0]}))["axis"].clone();
    let refused = r.call("dimension_set", json!({"target":{"feature":axis, "key":"__explicit"}, "value":0})).unwrap().unwrap_err();
    assert!(refused.contains("marker") && !refused.contains("F-170"), "{refused}");
    call(&mut r, "part_new", json!({"name":"other"}));
    call(&mut r, "sketch_create", json!({"plane":"XY", "name":"ring"}));
    call(&mut r, "sketch_add", json!({"sketch":"ring", "entities":[{"type":"rect", "cx":5, "w":2, "h":10}]}));
    let refused = r.call("revolve", json!({"sketch":"ring", "axis":{"datum":axis}, "op":"new_body"})).unwrap().unwrap_err();
    assert!(refused.contains("datum axis") && refused.contains("another Part"), "{refused}");
}

#[test]
fn part_instance_leaves_the_datums_with_the_source() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY", "name":"base"}));
    call(&mut r, "sketch_add", json!({"sketch":"base", "entities":[{"type":"rect", "w":10, "h":6}]}));
    call(&mut r, "extrude", json!({"sketch":"base", "height":4, "op":"new_body"}));
    call(&mut r, "datum_point", json!({"at":[0,0,0], "name":"origin_pt"}));
    call(&mut r, "datum_axis", json!({"origin":[0,0,0], "dir":[0,0,1], "name":"spin"}));
    let part = call(&mut r, "doc_info", json!({}))["active_component"].clone();
    let twin = call(&mut r, "part_instance", json!({"source":part, "name":"twin", "translation_mm":[100, 0, 0]}));
    assert_eq!(twin["part"]["instance_of"], part);
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(info["datums"]["points"].as_array().unwrap().len(), 1, "the clone repeats the body, not the datums: {info}");
    assert_eq!(info["datums"]["axes"].as_array().unwrap().len(), 1);
    assert_eq!(info["datums"]["points"][0]["owner_part"], part);
    assert!(info["datums"]["errors"].as_array().unwrap().is_empty());
}
