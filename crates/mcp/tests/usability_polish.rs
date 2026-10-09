use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, name: &str, args: Value) -> Value {
    let content = r.call(name, args).unwrap().unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

fn stock(r: &mut Registry) {
    call(r, "doc_new", json!({}));
    call(r, "sketch_create", json!({"plane":"XY", "name":"profile"}));
    call(r, "sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":20, "h":20}]}));
    call(r, "extrude", json!({"sketch":"profile", "height":20, "name":"stock"}));
}

#[test]
fn undo_reports_the_call_arguments_and_restored_bodies() {
    let mut r = Registry::new();
    stock(&mut r);
    let args = json!({"feature":"stock"});
    call(&mut r, "feature_delete", args.clone());
    let undone = call(&mut r, "undo", json!({}));
    assert_eq!(undone["undone"], json!({"tool":"feature_delete", "label":"Delete feature", "arguments":args}));
    assert_eq!(undone["bodies"][0]["name"], "stock");
    assert!((undone["bodies"][0]["volume_mm3"].as_f64().unwrap() - 20.0_f64.powi(3)).abs() < 1e-9);
    let args = json!({"name":"n", "value":7});
    call(&mut r, "param_set", args.clone());
    assert_eq!(call(&mut r, "undo", json!({}))["undone"], json!({"tool":"param_set", "label":"Set parameter", "arguments":args}));
}

#[test]
fn undo_returns_restored_cavity_warnings_without_rebuilding() {
    let mut r = Registry::new();
    stock(&mut r);
    call(&mut r, "plane_offset", json!({"base":"XY", "dist":10, "name":"inside"}));
    call(&mut r, "sketch_create", json!({"plane":{"plane":"inside"}, "name":"pocket_profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"pocket_profile", "entities":[{"type":"rect", "w":4, "h":6}]}));
    let pocket =
        call(&mut r, "extrude", json!({"sketch":"pocket_profile", "height":3, "op":"cut", "direction":"reverse", "name":"pocket"}));
    call(&mut r, "feature_delete", json!({"feature":"pocket"}));
    let restored = call(&mut r, "undo", json!({}));
    assert_eq!(restored["warnings"], pocket["rebuild"]["warnings"], "undo reports restored cavity diagnostics");
    assert!(!restored["warnings"].as_array().unwrap().is_empty());
    // Interior cutter includes 0.001 mm entry clearance: 20³ - 4*6*3.001.
    assert!((restored["bodies"][0]["volume_mm3"].as_f64().unwrap() - (20.0_f64.powi(3) - 4.0 * 6.0 * 3.001)).abs() < 0.00005);
}

#[test]
fn undo_distinguishes_empty_history_from_exhausted_limit() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    assert!(r.call("undo", json!({})).unwrap().unwrap_err().contains("nothing to undo"));
    for n in 0..19 {
        call(&mut r, "param_set", json!({"name":"n", "value":n}));
    }
    for _ in 0..16 {
        call(&mut r, "undo", json!({}));
    }
    // 19 edits minus 16 retained edits restores the value before edit 3: n=2.
    assert_eq!(call(&mut r, "doc_info", json!({}))["params"][0]["value"], 2.0);
    let error = r.call("undo", json!({})).unwrap().unwrap_err();
    assert!(error.contains("history limit reached (16): older calls cannot be undone"), "{error}");
    call(&mut r, "doc_new", json!({}));
    assert!(r.call("undo", json!({})).unwrap().unwrap_err().contains("nothing to undo"));
}

#[test]
fn empty_save_requires_override_to_replace_a_body_containing_file() {
    for operation in ["undo", "feature_delete"] {
        let mut r = Registry::new();
        stock(&mut r);
        let dir = std::env::temp_dir().join(format!("qymcad-empty-save-{}-{operation}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("part.qcad");
        call(&mut r, "doc_save", json!({"path":path}));
        let saved = std::fs::read(&path).unwrap();
        if operation == "undo" {
            call(&mut r, "undo", json!({}));
        } else {
            call(&mut r, "feature_delete", json!({"feature":"stock"}));
        }
        assert!(call(&mut r, "doc_info", json!({}))["bodies"].as_array().unwrap().is_empty());
        let error = r.call("doc_save", json!({})).unwrap().expect_err("empty document must not overwrite saved bodies");
        assert!(error.contains("allow_empty") && error.contains("no bodies"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), saved, "refusal preserves file bytes");
        assert!(r.call("doc_save", json!({"path":path})).unwrap().is_err());
        call(&mut r, "doc_save", json!({"allow_empty":true}));
        call(&mut r, "doc_open", json!({"path":path}));
        assert!(call(&mut r, "doc_info", json!({}))["bodies"].as_array().unwrap().is_empty());
        call(&mut r, "doc_save", json!({})); // Replacing an already empty file needs no override.
        call(&mut r, "doc_save", json!({"path":dir.join("new-empty.qcad")}));
    }
}

#[test]
fn undo_keeps_tool_and_arguments_with_a_localized_feature_label() {
    for name in [None, Some("soft corners")] {
        let mut r = Registry::new();
        stock(&mut r);
        let mut args = json!({"edges":{"along":"z"}, "radius":1.23456789});
        if let Some(name) = name {
            args["name"] = json!(name);
        }
        call(&mut r, "fillet", args.clone());
        let undone = call(&mut r, "undo", json!({}));
        assert_eq!(undone["undone"]["label"], name.unwrap_or("Fillet"), "undo needs a localized feature label");
        assert_eq!(undone["undone"]["tool"], "fillet");
        assert_eq!(undone["undone"]["arguments"], args);
    }
}
