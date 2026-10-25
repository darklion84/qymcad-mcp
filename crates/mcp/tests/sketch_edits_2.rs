//! Editing discriminators, strict selectors, atomic batch and undo.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};
fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}
#[test]
fn sketch_edit_break_is_strict_batchable_and_undoable() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"edit"}));
    let a = call(&mut r, "sketch_add", json!({"sketch":"edit","entities":[{"type":"line","x1":0,"y1":0,"x2":10,"y2":0}]}));
    let id = a["created"][0]["entities"][0].clone();
    let out = call(&mut r, "sketch_edit", json!({"sketch":"edit","op":"break","curve":id,"at":[4,0]}));
    assert_eq!(out["edit"]["created_entities"].as_array().unwrap().len(), 1);
    assert!(out["edit"]["dof"][0].as_i64().unwrap() > 0);
    assert!(!out["edit"]["free_points"].as_array().unwrap().is_empty());
    let undone = call(&mut r, "undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "sketch_edit");
    let before = call(&mut r, "sketch_info", json!({"sketch":"edit"}));
    for args in [
        json!({"sketch":"edit","op":"break","curve":id,"at":[4,0],"typo":true}),
        json!({"sketch":"edit","op":"break","curve":id,"at":[4,0],"intersections":true}),
        json!({"sketch":"edit","op":"trim","curve":{"kind":"line","typo":true},"at":[4,0]}),
    ] {
        assert!(r.call("sketch_edit", args).unwrap().is_err());
        assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"edit"})), before);
    }
    let out = call(
        &mut r,
        "batch",
        json!({"verbose":true,"steps":[{"tool":"sketch_edit","args":{"sketch":"edit","op":"break","curve":id,"at":[4,0]}}]}),
    );
    assert_eq!(out["steps"][0]["tool"], "sketch_edit");
    call(&mut r, "undo", json!({}));
    assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"edit"})), before);
}

#[test]
fn every_sketch_edit_op_runs_through_the_tool_and_reports_its_dimensions() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name": "g", "value": 2}));
    call(&mut r, "param_set", json!({"name": "k", "value": 3}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "s"}));
    let circle = call(&mut r, "sketch_add", json!({"sketch": "s", "entities": [{"type": "circle", "cx": 0, "cy": 0, "d": 10}]}));
    let circle = circle["created"][0]["circle"].clone();
    let off = call(&mut r, "sketch_edit", json!({"sketch": "s", "op": "offset", "curves": circle, "distance": "g", "toward": [9, 0]}));
    let dims = off["edit"]["dimensions"].as_array().unwrap();
    assert_eq!(dims.len(), 1, "{off}");
    let row = &off["sketch"]["constraints"][dims[0].as_u64().unwrap() as usize];
    assert_eq!((row["kind"].as_str(), row["expr"].as_str()), (Some("curve_distance"), Some("g")), "{row}");

    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "t"}));
    let tri = call(
        &mut r,
        "sketch_add",
        json!({"sketch": "t", "entities": [{"type": "polyline", "closed": true, "points": [[30, 0], [0, 0], [0, 30]]}]}),
    );
    let ids = tri["created"][0]["entities"].clone();
    let corner = call(
        &mut r,
        "sketch_edit",
        json!({"sketch": "t", "op": "corner", "curves": [ids[0], ids[1]], "point": {"curve": ids[0], "role": "end"}, "size": {"mode": "radius", "value": "k"}}),
    );
    assert_eq!(corner["edit"]["dof"], json!([0, 0]), "{corner}");
    let row = &corner["sketch"]["constraints"][corner["edit"]["dimensions"][0].as_u64().unwrap() as usize];
    assert_eq!(row["expr"], "k");

    let mirror = call(&mut r, "sketch_edit", json!({"sketch": "t", "op": "mirror_point", "curves": ids, "about": "origin"}));
    assert_eq!(mirror["edit"]["dof"], json!([0, 0]), "the mirrored copy is tied: {mirror}");
    assert!(!mirror["edit"]["created_entities"].as_array().unwrap().is_empty());

    let err =
        r.call("sketch_edit", json!({"sketch": "t", "op": "offset", "curves": ids, "distance": 0, "toward": [1, 1]})).unwrap().unwrap_err();
    assert!(err.contains("distance"), "{err}");
}
