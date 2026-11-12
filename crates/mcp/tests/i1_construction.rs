//! S2 over the registry: `sketch_edit` op construction, strict arguments, batch, undo.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn construction_is_strict_batchable_undoable_and_reports_states() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"s"}));
    let added = call(&mut r, "sketch_add", json!({"sketch":"s","entities":[{"type":"circle","cx":0,"cy":0,"d":4}]}));
    let id = added["created"][0]["circle"].as_u64().unwrap();
    let before = call(&mut r, "sketch_info", json!({"sketch":"s"}));
    for args in [
        json!({"sketch":"s","op":"construction","entities":[id]}),
        json!({"sketch":"s","op":"construction","entities":[id],"to":true,"typo":1}),
        json!({"sketch":"s","op":"construction","entities":[],"to":true}),
        json!({"sketch":"s","op":"construction","entities":[id],"to":false}),
    ] {
        assert!(r.call("sketch_edit", args).unwrap().is_err());
        assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"s"})), before);
    }
    let out = call(&mut r, "sketch_edit", json!({"sketch":"s","op":"construction","entities":[id],"to":true}));
    assert_eq!(out["construction"], json!([{"id":id,"construction":true}]));
    assert!(out["edit"]["changed_entities"].as_array().is_some());
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "sketch_edit");
    assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"s"})), before);
    // A batch is one step; a failing second step rolls the first back.
    let steps = |second_to: bool| {
        json!({"steps":[
            {"tool":"sketch_edit","args":{"sketch":"s","op":"construction","entities":[id],"to":true}},
            {"tool":"sketch_edit","args":{"sketch":"s","op":"construction","entities":[id],"to":second_to}}]})
    };
    assert!(r.call("batch", steps(true)).unwrap().is_err());
    assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"s"})), before);
    call(&mut r, "batch", steps(false));
    // Contour ids are re-allocated by every rebuild, the entities are as they were.
    let after = call(&mut r, "sketch_info", json!({"sketch":"s"}));
    assert_eq!(after["entities"], before["entities"]);
    assert_eq!(after["contours"][0]["area"], before["contours"][0]["area"]);
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"s"}))["contours"], before["contours"]);
}
