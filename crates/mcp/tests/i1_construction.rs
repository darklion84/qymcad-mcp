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
    for (args, word) in [
        (json!({"sketch":"s","op":"construction","entities":[id]}), "to"),
        (json!({"sketch":"s","op":"construction","entities":[id],"to":true,"typo":1}), "typo"),
        (json!({"sketch":"s","op":"construction","entities":[],"to":true}), "non-empty"),
        (json!({"sketch":"s","op":"construction","entities":[id],"to":false}), "would become construction"),
    ] {
        let refused = r.call("sketch_edit", args).unwrap().unwrap_err();
        assert!(refused.contains(word), "{word}: {refused}");
        assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"s"})), before);
    }
    let out = call(&mut r, "sketch_edit", json!({"sketch":"s","op":"construction","entities":[id],"to":true}));
    assert_eq!(out["construction"], json!([{"id":id,"construction":true}]));
    assert_eq!(out["edit"]["changed_entities"], json!([id]), "the edit report names the toggled curve: {out}");
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

/// F-168 over the registry: an island that a two-profile extrude holds cannot vanish; a hole may become construction.
#[test]
fn an_island_under_a_feature_is_refused_and_a_hole_is_followed() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"two"}));
    let added = call(
        &mut r,
        "sketch_add",
        json!({"sketch":"two","entities":[{"type":"rect","cx":-20,"w":10,"h":10},{"type":"circle","cx":20,"d":10}]}),
    );
    let disc = added["created"][1]["circle"].as_u64().unwrap();
    let body = call(&mut r, "extrude", json!({"sketch":"two","height":5,"op":"new_body"}));
    let volume = |v: &Value| v["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap();
    assert!((volume(&body) - (500.0 + std::f64::consts::PI * 125.0)).abs() < 1e-3, "{body}");
    let refused = r.call("sketch_edit", json!({"sketch":"two","op":"construction","entities":[disc],"to":true})).unwrap().unwrap_err();
    assert!(refused.contains("F-168") && refused.contains("silently drops out"), "{refused}");
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"plate"}));
    let added = call(&mut r, "sketch_add", json!({"sketch":"plate","entities":[{"type":"rect","w":20,"h":10},{"type":"circle","d":4}]}));
    let hole = added["created"][1]["circle"].as_u64().unwrap();
    let plate = call(&mut r, "extrude", json!({"sketch":"plate","height":5,"op":"new_body"}));
    let plate_volume =
        plate["rebuild"]["bodies"].as_array().unwrap().iter().find(|b| b["id"] == plate["body"]).unwrap()["volume_mm3"].as_f64().unwrap();
    assert!((plate_volume - (1000.0 - std::f64::consts::PI * 20.0)).abs() < 1e-3, "{plate}");
    let done = call(&mut r, "sketch_edit", json!({"sketch":"plate","op":"construction","entities":[hole],"to":true}));
    assert_eq!(done["construction"], json!([{"id":hole,"construction":true}]));
    let info = call(&mut r, "doc_info", json!({}));
    let grown = info["bodies"].as_array().unwrap().iter().find(|b| b["id"] == plate["body"]).unwrap()["volume_mm3"].as_f64().unwrap();
    assert!((grown - 1000.0).abs() < 1e-3, "the plate lost its hole: {info}");
}
