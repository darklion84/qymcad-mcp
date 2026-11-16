//! Replace face and Stitch through the tools (T2): results, refusals, batch, undo, save/open, `feature` re-pick, dimensions, Parts.
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
    assert!((actual - expected).abs() < 1.0, "{what}: got {actual}, expected {expected}");
}

/// A `w` x `d` block `h` high named `block` (parameter w = `w`).
fn block(w: f64, d: f64, h: f64) -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name": "w", "value": w}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "base"}));
    call(&mut r, "sketch_add", json!({"sketch": "base", "entities": [{"type": "rect", "w": "w", "h": d}]}));
    call(&mut r, "extrude", json!({"sketch": "base", "height": h, "name": "block"}));
    r
}

fn body<'a>(info: &'a Value, id: &Value) -> &'a Value {
    info["bodies"].as_array().unwrap().iter().find(|b| &b["id"] == id).unwrap_or_else(|| panic!("no body {id} in {info}"))
}
fn bodies(info: &Value) -> usize {
    info["bodies"].as_array().unwrap().len()
}

#[test]
fn surface_replace_swaps_the_top_and_consumes_both() {
    let mut r = block(40., 30., 10.);
    let lift = call(&mut r, "offset_surface", json!({"faces": {"facing": "+z"}, "distance": 5, "name": "lift"}));
    let out = call(&mut r, "surface_replace", json!({"faces": {"facing": "+z"}, "surface": "lift", "name": "swap"}));
    assert!(out["rebuild"].get("errors").is_none(), "{out}");
    assert_eq!(out["feature"], out["body"]);
    assert_eq!(out["name"], "swap");
    assert_eq!(out["selected_faces"].as_array().unwrap().len(), 1);
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(bodies(&info), 1, "the block and the sheet are consumed: {info}");
    let solid = body(&info, &out["body"]);
    assert_eq!(solid["sheet"], false);
    close(solid["volume_mm3"].as_f64().unwrap(), 18000., "40 x 30 x 15");
    assert_ne!(solid["id"], lift["body"]);
    // The result takes later features: a fillet of its top rim.
    let f = call(&mut r, "fillet", json!({"edges": {"edges_of": {"facing": "+z"}}, "radius": 1}));
    assert!(f["rebuild"].get("errors").is_none(), "{f}");
    assert!(body(&call(&mut r, "doc_info", json!({})), &f["body"])["volume_mm3"].as_f64().unwrap() < 18000. - 1.);
}

#[test]
fn a_sunk_sheet_cuts_the_block_down() {
    let mut r = block(40., 30., 10.);
    call(&mut r, "offset_surface", json!({"faces": {"facing": "+z"}, "distance": -3, "name": "sunk"}));
    let out = call(&mut r, "surface_replace", json!({"faces": {"facing": "+z"}, "surface": "sunk"}));
    assert!(out["rebuild"].get("errors").is_none(), "{out}");
    close(body(&call(&mut r, "doc_info", json!({})), &out["body"])["volume_mm3"].as_f64().unwrap(), 8400., "40 x 30 x 7");
}

#[test]
fn surface_replace_refusals_leave_the_document_unchanged() {
    let mut r = block(60., 40., 12.);
    call(&mut r, "face_copy", json!({"faces": {"facing": "+z"}, "name": "lid"}));
    let before = call(&mut r, "doc_info", json!({}));
    // Does not close: top and front replaced by a copy of the top.
    let e = refused(&mut r, "surface_replace", json!({"faces": {"union": [{"facing": "+z"}, {"facing": "-y"}]}, "surface": "lid"}));
    assert!(e.contains("does not close what it replaces"), "{e}");
    let e = refused(&mut r, "surface_replace", json!({"faces": {"ids": [99999]}, "surface": "lid"}));
    assert!(e.contains("99999"), "{e}");
    // Wrong kinds.
    let e = refused(&mut r, "surface_replace", json!({"faces": {"facing": "+z"}, "surface": "block"}));
    assert!(e.contains("is a solid") && e.contains("must be a sheet"), "{e}");
    let e = refused(&mut r, "surface_replace", json!({"body": "lid", "faces": {"ids": [1]}, "surface": "lid"}));
    assert!(e.contains("is a sheet"), "{e}");
    // Missing arguments.
    assert!(refused(&mut r, "surface_replace", json!({"faces": {"facing": "+z"}})).contains("`surface` is required"));
    assert!(refused(&mut r, "surface_replace", json!({"surface": "lid"})).contains("`faces` is required"));
    assert!(refused(&mut r, "surface_replace", json!({"faces": {"facing": "+z"}, "surface": "nothing"})).contains("nothing"));
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
}

#[test]
fn surface_replace_feature_repicks_the_sheet() {
    let mut r = block(40., 30., 10.);
    call(&mut r, "offset_surface", json!({"faces": {"facing": "+z"}, "distance": 5, "name": "high"}));
    call(&mut r, "offset_surface", json!({"faces": {"facing": "+z"}, "distance": 2, "name": "low"}));
    let out = call(&mut r, "surface_replace", json!({"faces": {"facing": "+z"}, "surface": "high", "name": "swap"}));
    for extra in [json!({"body": "block"}), json!({"name": "x"})] {
        let mut args = json!({"surface": "low", "feature": "swap"});
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert!(refused(&mut r, "surface_replace", args).contains("not accepted"));
    }
    assert!(refused(&mut r, "surface_replace", json!({"feature": "swap"})).contains("give `faces` and/or `surface`"));
    let again = call(&mut r, "surface_replace", json!({"surface": "low", "feature": "swap"}));
    assert_eq!(again["feature"], out["feature"]);
    assert!(again.get("selected_faces").is_some_and(|f| f.as_array().unwrap().is_empty()), "{again}");
    close(body(&call(&mut r, "doc_info", json!({})), &out["body"])["volume_mm3"].as_f64().unwrap(), 14400., "40 x 30 x 12");
    let e = refused(&mut r, "surface_replace", json!({"surface": "block", "feature": "swap"}));
    assert!(e.contains("is a solid"), "{e}");
}

#[test]
fn surface_replace_batch_undo_and_save_open() {
    let mut r = block(60., 40., 12.);
    let steps = json!({"steps": [
        {"tool": "face_copy", "args": {"faces": {"facing": "+z"}, "name": "lid"}},
        {"tool": "surface_replace", "args": {"faces": {"facing": "+z"}, "surface": "lid", "name": "swap"}},
    ]});
    let out = call(&mut r, "batch", steps);
    assert_eq!(out["steps"].as_array().unwrap().len(), 2, "{out}");
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(bodies(&info), 1);
    close(info["bodies"][0]["volume_mm3"].as_f64().unwrap(), 28800., "the same plate");
    // One undo takes the batch back: the block alone.
    call(&mut r, "undo", json!({}));
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(bodies(&info), 1);
    assert_eq!(info["bodies"][0]["name"], "block");
    // A failing second step rolls the first back.
    call(&mut r, "face_copy", json!({"faces": {"facing": "+z"}, "name": "lid"}));
    let before = call(&mut r, "doc_info", json!({}));
    let e = refused(
        &mut r,
        "batch",
        json!({"steps": [
            {"tool": "offset_surface", "args": {"faces": {"facing": "-z"}, "distance": 1, "name": "under"}},
            {"tool": "surface_replace", "args": {"faces": {"union": [{"facing": "+z"}, {"facing": "-y"}]}, "surface": "lid"}},
        ]}),
    );
    assert!(e.contains("step 2") && e.contains("does not close"), "{e}");
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
    // Save and open.
    let out = call(&mut r, "surface_replace", json!({"faces": {"facing": "+z"}, "surface": "lid", "name": "swap"}));
    let path = std::env::temp_dir().join(format!("t2-replace-{}.qcad", std::process::id()));
    call(&mut r, "doc_save", json!({"path": path.to_str().unwrap()}));
    let mut fresh = Registry::new();
    let opened = call(&mut fresh, "doc_open", json!({"path": path.to_str().unwrap()}));
    assert!(opened.get("errors").is_none() && !opened.to_string().contains("sealed"), "{opened}");
    let info = call(&mut fresh, "doc_info", json!({}));
    close(body(&info, &out["body"])["volume_mm3"].as_f64().unwrap(), 28800., "after open");
    call(&mut fresh, "param_set", json!({"name": "w", "value": 80}));
    close(body(&call(&mut fresh, "doc_info", json!({})), &out["body"])["volume_mm3"].as_f64().unwrap(), 80. * 40. * 12., "edit after open");
    let _ = std::fs::remove_file(path);
}

fn copy_of(r: &mut Registry, facing: &str, name: &str) -> Value {
    call(r, "face_copy", json!({"body": "block", "faces": {"facing": facing}, "name": name}))
}

#[test]
fn stitch_makes_a_sheet_or_a_solid() {
    let mut r = block(60., 40., 12.);
    copy_of(&mut r, "+z", "top");
    copy_of(&mut r, "-y", "front");
    let out = call(&mut r, "stitch", json!({"parts": ["top", "front"], "tolerance": 0.0001, "name": "skin"}));
    assert!(out["rebuild"].get("errors").is_none(), "{out}");
    assert_eq!((out["sheet"].clone(), out["name"].clone()), (json!(true), json!("skin")));
    assert_eq!(out["feature"], out["body"]);
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(bodies(&info), 2, "the block and the stitched sheet: {info}");
    close(body(&info, &out["body"])["area_mm2"].as_f64().unwrap(), 3120., "2400 + 720");
    // Six faces of the plate make the plate again, as a solid; the default tolerance is the app's 0.01.
    let mut r = block(60., 40., 12.);
    let names: Vec<String> = ["+z", "-z", "+x", "-x", "+y", "-y"]
        .iter()
        .map(|f| {
            copy_of(&mut r, f, &format!("c{f}"));
            format!("c{f}")
        })
        .collect();
    let out = call(&mut r, "stitch", json!({"parts": names, "name": "closed"}));
    assert!(out["rebuild"].get("errors").is_none(), "{out}");
    assert_eq!(out["sheet"], json!(false));
    close(body(&call(&mut r, "doc_info", json!({})), &out["body"])["volume_mm3"].as_f64().unwrap(), 28800., "the plate");
    let named = call(&mut r, "dimension_set", json!({"target": {"feature": "closed", "key": "tol"}, "name": "seam"}));
    assert!(named["rebuild"].get("errors").is_none(), "{named}");
    let listed =
        call(&mut r, "doc_info", json!({}))["named_dimensions"].as_array().unwrap().iter().find(|d| d["name"] == "seam").unwrap().clone();
    assert_eq!(listed["value"], 0.01, "the app's default tolerance");
}

#[test]
fn stitch_refusals_leave_the_document_unchanged() {
    let mut r = block(60., 40., 12.);
    copy_of(&mut r, "+z", "top");
    copy_of(&mut r, "-y", "front");
    copy_of(&mut r, "-z", "bottom");
    let before = call(&mut r, "doc_info", json!({}));
    assert!(refused(&mut r, "stitch", json!({})).contains("`parts` is required"));
    assert!(refused(&mut r, "stitch", json!({"parts": ["top"]})).contains("at least two"));
    let e = refused(&mut r, "stitch", json!({"parts": ["top", "block"]}));
    assert!(e.contains("is a solid") && e.contains("must be a sheet"), "{e}");
    assert!(refused(&mut r, "stitch", json!({"parts": ["top", "top"]})).contains("twice"));
    assert!(refused(&mut r, "stitch", json!({"parts": ["top", "nothing"]})).contains("nothing"));
    // The app's range of the tolerance.
    for bad in [json!(0), json!(11), json!(1e-7)] {
        let e = refused(&mut r, "stitch", json!({"parts": ["top", "front"], "tolerance": bad}));
        assert!(e.contains("1e-6..10"), "{bad}: {e}");
    }
    // Top and bottom do not touch: the app's named refusal.
    let e = refused(&mut r, "stitch", json!({"parts": ["top", "bottom"]}));
    assert!(e.contains("share no edges"), "{e}");
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
}

#[test]
fn stitch_feature_repicks_the_parts_and_the_tolerance_is_a_dimension() {
    let mut r = block(60., 40., 12.);
    copy_of(&mut r, "+z", "top");
    copy_of(&mut r, "-y", "front");
    copy_of(&mut r, "+x", "side");
    let out = call(&mut r, "stitch", json!({"parts": ["top", "front"], "name": "skin"}));
    for extra in [json!({"tolerance": 0.1}), json!({"name": "x"})] {
        let mut args = json!({"parts": ["top", "side"], "feature": "skin"});
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert!(refused(&mut r, "stitch", args).contains("not accepted"));
    }
    let again = call(&mut r, "stitch", json!({"parts": ["top", "side"], "feature": "skin"}));
    assert_eq!(again["feature"], out["feature"]);
    close(body(&call(&mut r, "doc_info", json!({})), &out["body"])["area_mm2"].as_f64().unwrap(), 2400. + 480., "top and right side");
    assert!(refused(&mut r, "stitch", json!({"parts": ["top"], "feature": "skin"})).contains("at least two"));
    assert!(refused(&mut r, "stitch", json!({"parts": ["top", "block"], "feature": "skin"})).contains("is a solid"));
    assert!(refused(&mut r, "stitch", json!({"parts": ["top", "front"], "feature": "top"})).contains("not a stitch"));
    // The tolerance through `dimension_set`, in the app's range.
    let set = call(&mut r, "dimension_set", json!({"target": {"feature": "skin", "key": "tol"}, "value": 0.5, "name": "seam"}));
    assert!(set["rebuild"].get("errors").is_none(), "{set}");
    let e = refused(&mut r, "dimension_set", json!({"target": {"feature": "skin", "key": "tol"}, "value": 11}));
    assert!(e.contains("1e-6..10"), "{e}");
}

#[test]
fn stitch_batch_undo_and_save_open() {
    let mut r = block(60., 40., 12.);
    let steps = json!({"steps": [
        {"tool": "face_copy", "args": {"body": "block", "faces": {"facing": "+z"}, "name": "top"}},
        {"tool": "face_copy", "args": {"body": "block", "faces": {"facing": "-y"}, "name": "front"}},
        {"tool": "stitch", "args": {"parts": ["top", "front"], "tolerance": 0.0001, "name": "skin"}},
    ]});
    let out = call(&mut r, "batch", steps);
    assert_eq!(out["steps"].as_array().unwrap().len(), 3, "{out}");
    let sheets = |info: &Value| info["bodies"].as_array().unwrap().iter().filter(|b| b["sheet"] == true).count();
    assert_eq!(sheets(&call(&mut r, "doc_info", json!({}))), 1);
    // A failing last step rolls the whole batch back; one undo takes a good one.
    let before = call(&mut r, "doc_info", json!({}));
    let e = refused(
        &mut r,
        "batch",
        json!({"steps": [
            {"tool": "face_copy", "args": {"body": "block", "faces": {"facing": "-z"}, "name": "bottom"}},
            {"tool": "face_copy", "args": {"body": "block", "faces": {"facing": "+z"}, "name": "top2"}},
            {"tool": "stitch", "args": {"parts": ["bottom", "top2"]}},
        ]}),
    );
    assert!(e.contains("step 3") && e.contains("share no edges"), "{e}");
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
    call(&mut r, "undo", json!({}));
    assert_eq!(sheets(&call(&mut r, "doc_info", json!({}))), 0, "one undo entry for the batch");
    // Save and open.
    let steps = json!({"steps": [
        {"tool": "face_copy", "args": {"body": "block", "faces": {"facing": "+z"}, "name": "top"}},
        {"tool": "face_copy", "args": {"body": "block", "faces": {"facing": "-y"}, "name": "front"}},
        {"tool": "stitch", "args": {"parts": ["top", "front"], "tolerance": 0.0001, "name": "skin"}},
    ]});
    call(&mut r, "batch", steps);
    let path = std::env::temp_dir().join(format!("t2-stitch-{}.qcad", std::process::id()));
    call(&mut r, "doc_save", json!({"path": path.to_str().unwrap()}));
    let mut fresh = Registry::new();
    let opened = call(&mut fresh, "doc_open", json!({"path": path.to_str().unwrap()}));
    assert!(opened.get("errors").is_none() && !opened.to_string().contains("sealed"), "{opened}");
    let info = call(&mut fresh, "doc_info", json!({}));
    let skin = info["bodies"].as_array().unwrap().iter().find(|b| b["sheet"] == true).unwrap().clone();
    close(skin["area_mm2"].as_f64().unwrap(), 3120., "after open");
    call(&mut fresh, "param_set", json!({"name": "w", "value": 90}));
    let skin = call(&mut fresh, "doc_info", json!({}))["bodies"].as_array().unwrap().iter().find(|b| b["sheet"] == true).unwrap().clone();
    assert!((skin["area_mm2"].as_f64().unwrap() - 4680.).abs() < 5., "{skin}");
    let _ = std::fs::remove_file(path);
}

/// Both nodes are wrapped in the Part of their inputs (the first sheet's, the solid's), whichever Part is active.
#[test]
fn the_result_belongs_to_the_part_of_its_inputs() {
    let mut r = block(60., 40., 12.);
    let first = call(&mut r, "doc_info", json!({}))["active_component"].clone();
    copy_of(&mut r, "+z", "top");
    copy_of(&mut r, "-y", "front");
    call(&mut r, "part_new", json!({"name": "second"}));
    let skin = call(&mut r, "stitch", json!({"parts": ["top", "front"], "tolerance": 0.0001}));
    assert!(skin["rebuild"].get("errors").is_none(), "{skin}");
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(body(&info, &skin["body"])["owner_part"], first, "stitch");
    // And the replacement, made while the second Part is still active.
    call(&mut r, "face_copy", json!({"body": "block", "faces": {"facing": "-z"}, "name": "bottom"}));
    let out = call(&mut r, "surface_replace", json!({"body": "block", "faces": {"facing": "-z"}, "surface": "bottom"}));
    assert!(out["rebuild"].get("errors").is_none(), "{out}");
    assert_eq!(body(&call(&mut r, "doc_info", json!({})), &out["body"])["owner_part"], first, "surface_replace");
}
