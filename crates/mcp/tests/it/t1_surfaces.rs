//! Copy faces and Offset surface through the tools (T1): results, batch, undo, save/open, `feature` re-pick, dimensions.
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
    assert!((actual - expected).abs() < 1e-2, "{what}: got {actual}, expected {expected}");
}

/// A 40 x 30 block `h` high (parameter h = 10), named `block`.
fn block() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name": "h", "value": 10}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "base"}));
    call(&mut r, "sketch_add", json!({"sketch": "base", "entities": [{"type": "rect", "w": 40, "h": 30}]}));
    call(&mut r, "extrude", json!({"sketch": "base", "height": "h", "name": "block"}));
    r
}

fn sheets(info: &Value) -> Vec<&Value> {
    info["bodies"].as_array().unwrap().iter().filter(|b| b["sheet"] == true).collect()
}

#[test]
fn face_copy_makes_a_sheet_and_the_block_stays() {
    let mut r = block();
    let out = call(&mut r, "face_copy", json!({"faces": {"facing": "+z"}, "name": "lid"}));
    assert_eq!(out["name"], "lid");
    assert_eq!(out["feature"], out["body"]);
    assert_eq!(out["selected_faces"].as_array().unwrap().len(), 1);
    assert!(out["rebuild"].get("errors").is_none(), "{out}");
    let info = call(&mut r, "doc_info", json!({}));
    let sheet = sheets(&info);
    assert_eq!(sheet.len(), 1);
    assert_eq!(sheet[0]["id"], out["body"]);
    close(sheet[0]["area_mm2"].as_f64().unwrap(), 1200., "area");
    assert_eq!(sheet[0]["volume_mm3"], 0.0);
    let solid = info["bodies"].as_array().unwrap().iter().find(|b| b["sheet"] == false).unwrap();
    close(solid["volume_mm3"].as_f64().unwrap(), 12000., "the block stays");
    // The sheet is not the default body: a second copy reads the block, not the sheet.
    let again = call(&mut r, "face_copy", json!({"faces": {"facing": "-z"}}));
    assert!(again["rebuild"].get("errors").is_none(), "{again}");
}

#[test]
fn face_copy_refuses_what_the_app_refuses() {
    let mut r = block();
    let e = refused(&mut r, "face_copy", json!({"faces": {"ids": [99999]}}));
    assert!(e.contains("99999"), "{e}");
    let e = refused(&mut r, "face_copy", json!({"faces": {"facing": "+z"}, "distance": 5}));
    assert!(e.contains("distance"), "no numbers on a copy: {e}");
    let e = refused(&mut r, "face_copy", json!({"faces": {"facing": "+z"}, "feature": "nothing"}));
    assert!(e.contains("nothing"), "{e}");
    assert!(call(&mut r, "doc_info", json!({}))["bodies"].as_array().unwrap().len() == 1, "nothing was made");
}

#[test]
fn offset_surface_moves_and_its_distance_is_a_dimension() {
    let mut r = block();
    let out = call(&mut r, "offset_surface", json!({"faces": {"facing": "+z"}, "distance": "h/2", "name": "lift"}));
    assert!(out["rebuild"].get("errors").is_none(), "{out}");
    let info = call(&mut r, "doc_info", json!({}));
    let s = sheets(&info)[0].clone();
    assert_eq!(s["id"], out["feature"]);
    close(s["bbox"][2].as_f64().unwrap(), 15., "z");
    close(s["area_mm2"].as_f64().unwrap(), 1200., "area");
    // `h` drives both the block and the distance.
    call(&mut r, "param_set", json!({"name": "h", "value": 20}));
    close(sheets(&call(&mut r, "doc_info", json!({})))[0]["bbox"][2].as_f64().unwrap(), 30., "z after h = 20");
    // dist edited and named, then listed.
    let set = call(&mut r, "dimension_set", json!({"target": {"feature": "lift", "key": "dist"}, "value": 3, "name": "rise"}));
    assert!(set["rebuild"].get("errors").is_none(), "{set}");
    let info = call(&mut r, "doc_info", json!({}));
    close(sheets(&info)[0]["bbox"][2].as_f64().unwrap(), 23., "z after dist 3");
    let listed = info["named_dimensions"].as_array().unwrap().iter().find(|d| d["name"] == "rise").expect("listed").clone();
    assert_eq!(listed["value"], 3.0);
    // The app's range holds in every way an edit arrives.
    let e = refused(&mut r, "dimension_set", json!({"target": {"feature": "lift", "key": "dist"}, "value": 10001}));
    assert!(e.contains("-10000..10000"), "{e}");
    let e = refused(&mut r, "offset_surface", json!({"faces": {"facing": "+z"}, "distance": -10001}));
    assert!(e.contains("-10000..10000"), "{e}");
    let e = refused(&mut r, "offset_surface", json!({"faces": {"facing": "+z"}}));
    assert!(e.contains("distance"), "{e}");
}

#[test]
fn feature_repicks_the_faces_and_refuses_other_arguments() {
    let mut r = block();
    let copy = call(&mut r, "face_copy", json!({"faces": {"facing": "+z"}, "name": "lid"}));
    let off = call(&mut r, "offset_surface", json!({"faces": {"facing": "+z"}, "distance": 2, "name": "lift"}));
    for (feature, extra) in [("lid", json!({"body": "block"})), ("lid", json!({"name": "x"})), ("lift", json!({"distance": 3}))] {
        let mut args = json!({"faces": {"facing": "-z"}, "feature": feature});
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let tool = if feature == "lid" { "face_copy" } else { "offset_surface" };
        let e = refused(&mut r, tool, args);
        assert!(e.contains("not accepted"), "{e}");
    }
    let out = call(&mut r, "face_copy", json!({"faces": {"facing": "+x"}, "feature": "lid"}));
    assert_eq!(out["feature"], copy["feature"]);
    let out = call(&mut r, "offset_surface", json!({"faces": {"facing": "+x"}, "feature": "lift"}));
    assert_eq!(out["feature"], off["feature"]);
    let info = call(&mut r, "doc_info", json!({}));
    let areas: Vec<f64> = sheets(&info).iter().map(|s| s["area_mm2"].as_f64().unwrap()).collect();
    assert_eq!(areas.len(), 2);
    for a in areas {
        close(a, 300., "the +x face is 30 x 10");
    }
    let e = refused(&mut r, "offset_surface", json!({"faces": {"facing": "+x"}, "feature": "lid"}));
    assert!(e.contains("not a offset_surface"), "{e}");
}

#[test]
fn batch_undo_and_save_open() {
    let mut r = block();
    let steps = json!({"steps": [
        {"tool": "face_copy", "args": {"faces": {"facing": "+z"}, "name": "lid"}},
        {"tool": "offset_surface", "args": {"faces": {"facing": "-z"}, "distance": 4, "name": "under"}},
    ]});
    let out = call(&mut r, "batch", steps);
    assert_eq!(out["steps"].as_array().unwrap().len(), 2, "{out}");
    assert_eq!(out["steps"][0]["created"]["name"], "lid");
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(sheets(&info).len(), 2);
    // A failing second step rolls the whole batch back.
    let before = call(&mut r, "doc_info", json!({}));
    let e = refused(
        &mut r,
        "batch",
        json!({"steps": [
            {"tool": "face_copy", "args": {"faces": {"facing": "+x"}}},
            {"tool": "offset_surface", "args": {"faces": {"facing": "+x"}, "distance": 20000}},
        ]}),
    );
    assert!(e.contains("step 2") && e.contains("-10000..10000"), "{e}");
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
    // One undo takes the whole batch.
    call(&mut r, "undo", json!({}));
    assert_eq!(sheets(&call(&mut r, "doc_info", json!({}))).len(), 0, "one undo entry for the batch");
    // Undo of a single call.
    call(&mut r, "offset_surface", json!({"faces": {"facing": "+z"}, "distance": 5}));
    assert_eq!(sheets(&call(&mut r, "doc_info", json!({}))).len(), 1);
    call(&mut r, "undo", json!({}));
    assert_eq!(sheets(&call(&mut r, "doc_info", json!({}))).len(), 0);
    // Save and open.
    call(&mut r, "face_copy", json!({"faces": {"facing": "+z"}, "name": "lid"}));
    call(&mut r, "offset_surface", json!({"faces": {"facing": "+z"}, "distance": "h/2", "name": "lift"}));
    let path = crate::scratch::path("t1-surfaces.qcad");
    call(&mut r, "doc_save", json!({"path": path.to_str().unwrap()}));
    let saved = call(&mut r, "doc_info", json!({}));
    let mut fresh = Registry::new();
    let opened = call(&mut fresh, "doc_open", json!({"path": path.to_str().unwrap()}));
    assert!(opened.get("errors").is_none() && !opened.to_string().contains("sealed"), "{opened}");
    let info = call(&mut fresh, "doc_info", json!({}));
    assert_eq!(sheets(&info).len(), 2);
    for (a, b) in sheets(&saved).iter().zip(sheets(&info)) {
        close(a["area_mm2"].as_f64().unwrap(), b["area_mm2"].as_f64().unwrap(), "area after open");
    }
    // A parameter edit after the reopen still moves the offset sheet.
    call(&mut fresh, "param_set", json!({"name": "h", "value": 20}));
    let lifted = call(&mut fresh, "doc_info", json!({}));
    let top = sheets(&lifted).iter().map(|s| s["bbox"][2].as_f64().unwrap()).fold(f64::MIN, f64::max);
    close(top, 30., "the lifted sheet follows h");
    let _ = std::fs::remove_file(path);
}

/// The node is wrapped in the source's own Part, like the other tools, whichever Part is active.
#[test]
fn the_sheet_belongs_to_the_part_of_its_source() {
    let mut r = block();
    let first = call(&mut r, "doc_info", json!({}))["active_component"].clone();
    call(&mut r, "part_new", json!({"name": "second"}));
    let out = call(&mut r, "face_copy", json!({"body": "block", "faces": {"facing": "+z"}}));
    assert!(out["rebuild"].get("errors").is_none(), "{out}");
    let info = call(&mut r, "doc_info", json!({}));
    let sheet = sheets(&info)[0].clone();
    assert_eq!(sheet["owner_part"], first, "{sheet}");
}
