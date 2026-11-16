//! Trim through the tool (T3): results, refusals, batch, undo, save/open, `feature` re-pick. The plate is 60 x 40 x 12 centred on the
//! origin, the tool box stands on x 0..60, so the cut line is x = 0 and either piece is 30 x 40 = 1200.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}
fn refused(r: &mut Registry, tool: &str, args: Value) -> String {
    r.call(tool, args).unwrap().unwrap_err()
}
fn close(actual: f64, expected: f64, what: &str) {
    assert!((actual - expected).abs() < 5.0, "{what}: got {actual}, expected {expected}");
}
fn area(r: &mut Registry, id: &Value) -> f64 {
    let info = call(r, "doc_info", json!({}));
    info["bodies"].as_array().unwrap().iter().find(|b| &b["id"] == id).unwrap_or_else(|| panic!("no body {id} in {info}"))["area_mm2"]
        .as_f64()
        .unwrap()
}
fn bodies(r: &mut Registry) -> usize {
    call(r, "doc_info", json!({}))["bodies"].as_array().unwrap().len()
}

/// The plate `block` (parameters w, d), its top copied to the sheet `lid`, and the box `cutter` on x 0..60 running past the plate.
fn scene() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name": "w", "value": 60}));
    call(&mut r, "param_set", json!({"name": "d", "value": 40}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "base"}));
    call(&mut r, "sketch_add", json!({"sketch": "base", "entities": [{"type": "rect", "w": "w", "h": "d"}]}));
    call(&mut r, "extrude", json!({"sketch": "base", "height": 12, "name": "block"}));
    call(&mut r, "face_copy", json!({"faces": {"facing": "+z"}, "name": "lid"}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "box"}));
    call(&mut r, "sketch_add", json!({"sketch": "box", "entities": [{"type": "rect", "cx": 30, "w": 60, "h": 200}]}));
    call(&mut r, "extrude", json!({"sketch": "box", "height": 30, "op": "new_body", "name": "cutter"}));
    r
}

#[test]
fn trim_keeps_the_piece_under_the_point_and_consumes_the_sheet() {
    let mut r = scene();
    let out = call(&mut r, "trim", json!({"body": "lid", "tool": "cutter", "keep": [-15, 0, 12], "name": "left"}));
    assert!(out["rebuild"].get("errors").is_none(), "{out}");
    assert_eq!((out["feature"].clone(), out["name"].clone(), out["sheet"].clone()), (out["body"].clone(), json!("left"), json!(true)));
    close(area(&mut r, &out["body"]), 1200., "30 x 40");
    let info = call(&mut r, "doc_info", json!({}));
    let names: Vec<&str> = info["bodies"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap()).collect();
    assert!(!names.contains(&"lid") && names.contains(&"cutter") && names.contains(&"block"), "{names:?}");
    // The other side on a fresh scene.
    let mut r = scene();
    let out = call(&mut r, "trim", json!({"body": "lid", "tool": "cutter", "keep": [15, 0, 12]}));
    close(area(&mut r, &out["body"]), 1200., "30 x 40, right");
    // The trimmed sheet follows its base.
    call(&mut r, "param_set", json!({"name": "d", "value": 60}));
    close(area(&mut r, &out["body"]), 1800., "30 x 60");
}

#[test]
fn trim_refusals_leave_the_document_unchanged() {
    let mut r = scene();
    let before = call(&mut r, "doc_info", json!({}));
    let e = refused(&mut r, "trim", json!({"body": "block", "tool": "cutter", "keep": [-15, 0, 12]}));
    assert!(e.contains("is a solid") && e.contains("must be a sheet"), "{e}");
    assert!(refused(&mut r, "trim", json!({"body": "lid", "tool": "lid", "keep": [-15, 0, 12]})).contains("cannot trim itself"));
    assert!(refused(&mut r, "trim", json!({"body": "lid", "tool": "nothing", "keep": [-15, 0, 12]})).contains("nothing"));
    assert!(refused(&mut r, "trim", json!({"tool": "cutter", "keep": [1, 2, 3]})).contains("`body`"));
    assert!(refused(&mut r, "trim", json!({"body": "lid", "keep": [1, 2, 3]})).contains("`tool`"));
    assert!(refused(&mut r, "trim", json!({"body": "lid", "tool": "cutter"})).contains("`keep`"));
    assert!(refused(&mut r, "trim", json!({"body": "lid", "tool": "cutter", "keep": [1, 2]})).len() > 0);
    assert!(refused(&mut r, "trim", json!({"body": "lid", "tool": "cutter", "keep": [1, 2, 3], "extra": 1})).len() > 0);
    // A tool that does not reach the sheet.
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "far sketch"}));
    call(&mut r, "sketch_add", json!({"sketch": "far sketch", "entities": [{"type": "rect", "cx": 500, "w": 60, "h": 200}]}));
    call(&mut r, "extrude", json!({"sketch": "far sketch", "height": 30, "op": "new_body", "name": "far"}));
    let before = call(&mut r, "doc_info", json!({}));
    assert!(refused(&mut r, "trim", json!({"body": "lid", "tool": "far", "keep": [-15, 0, 12]})).contains("rolled back"));
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
}

#[test]
fn trim_feature_repicks_point_tool_and_sheet() {
    let mut r = scene();
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "box2"}));
    call(&mut r, "sketch_add", json!({"sketch": "box2", "entities": [{"type": "rect", "cx": 40, "w": 60, "h": 200}]}));
    call(&mut r, "extrude", json!({"sketch": "box2", "height": 30, "op": "new_body", "name": "cutter 2"}));
    call(&mut r, "offset_surface", json!({"body": "block", "faces": {"facing": "+z"}, "distance": 5, "name": "high"}));
    let out = call(&mut r, "trim", json!({"body": "lid", "tool": "cutter", "keep": [-15, 0, 12], "name": "cut"}));
    assert!(refused(&mut r, "trim", json!({"feature": "cut", "name": "x", "keep": [1, 2, 3]})).contains("not accepted"));
    assert!(refused(&mut r, "trim", json!({"feature": "cut"})).contains("give `body`, `tool` and/or `keep`"));
    assert!(refused(&mut r, "trim", json!({"feature": "cut", "body": "high"})).contains("own `keep`"));
    // The point only: the other piece.
    let again = call(&mut r, "trim", json!({"feature": "cut", "keep": [15, 0, 12]}));
    assert_eq!(again["feature"], out["feature"]);
    close(area(&mut r, &out["body"]), 1200., "right");
    // The tool: the cut moves to x = 10, the left piece is 40 x 40.
    call(&mut r, "trim", json!({"feature": "cut", "tool": "cutter 2", "keep": [-15, 0, 12]}));
    close(area(&mut r, &out["body"]), 1600., "40 x 40");
    // The sheet.
    call(&mut r, "trim", json!({"feature": "cut", "body": "high", "tool": "cutter", "keep": [-15, 0, 17]}));
    close(area(&mut r, &out["body"]), 1200., "the lifted sheet");
    let e = refused(&mut r, "trim", json!({"feature": "cut", "body": "block", "keep": [0, 0, 0]}));
    assert!(e.contains("is a solid"), "{e}");
}

#[test]
fn trim_batch_undo_and_save_open() {
    let mut r = scene();
    let steps = json!({"steps": [
        {"tool": "trim", "args": {"body": "lid", "tool": "cutter", "keep": [-15, 0, 12], "name": "left"}},
        {"tool": "param_set", "args": {"name": "d", "value": 50}},
    ]});
    let out = call(&mut r, "batch", steps);
    assert_eq!(out["steps"].as_array().unwrap().len(), 2, "{out}");
    let left =
        call(&mut r, "doc_info", json!({}))["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "left").unwrap()["id"].clone();
    close(area(&mut r, &left), 1500., "30 x 50");
    // One undo takes the whole batch back.
    call(&mut r, "undo", json!({}));
    let info = call(&mut r, "doc_info", json!({}));
    let names: Vec<&str> = info["bodies"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"lid") && !names.contains(&"left"), "{names:?}");
    // A failing second step rolls the first back.
    let before = call(&mut r, "doc_info", json!({}));
    let e = refused(
        &mut r,
        "batch",
        json!({"steps": [
            {"tool": "trim", "args": {"body": "lid", "tool": "cutter", "keep": [-15, 0, 12]}},
            {"tool": "trim", "args": {"body": "block", "tool": "cutter", "keep": [-15, 0, 12]}},
        ]}),
    );
    assert!(e.contains("step 2"), "{e}");
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
    assert_eq!(bodies(&mut r), 3);
    // Save and open: the trim comes back and follows an edit.
    let out = call(&mut r, "trim", json!({"body": "lid", "tool": "cutter", "keep": [-15, 0, 12], "name": "left"}));
    let path = std::env::temp_dir().join(format!("t3-trim-{}.qcad", std::process::id()));
    call(&mut r, "doc_save", json!({"path": path.to_str().unwrap()}));
    let mut fresh = Registry::new();
    let opened = call(&mut fresh, "doc_open", json!({"path": path.to_str().unwrap()}));
    assert!(opened.get("errors").is_none(), "{opened}");
    close(area(&mut fresh, &out["body"]), 1200., "after open");
    call(&mut fresh, "param_set", json!({"name": "d", "value": 60}));
    close(area(&mut fresh, &out["body"]), 1800., "edit after open");
    let _ = std::fs::remove_file(path);
}
