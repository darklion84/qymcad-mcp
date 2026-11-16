//! Patch through the tool (T4): results, refusals, missing arguments, `feature`, batch, undo, save/open, thicken (the lid). The box is
//! 40 x 30 x 20 (parameter w) shelled 2 inward with its top open: the OUTER outline of the opening is 40 x 30 = 1200.
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

fn open_box() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name": "w", "value": 40}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "base"}));
    call(&mut r, "sketch_add", json!({"sketch": "base", "entities": [{"type": "rect", "w": "w", "h": 30}]}));
    call(&mut r, "extrude", json!({"sketch": "base", "height": 20, "name": "block"}));
    call(&mut r, "shell", json!({"open_faces": {"facing": "+z"}, "thickness": 2, "name": "cup"}));
    r
}

/// Ids of the edges of the top opening of `body`: the outer outline or the inner one.
fn rim(r: &mut Registry, body: &str, outer: bool) -> Vec<u64> {
    let t = call(r, "topology", json!({"body": body}));
    let edges = t["edges"].as_array().unwrap();
    let pts = |e: &Value| -> [[f64; 3]; 2] { [0, 1].map(|i| [0, 1, 2].map(|k| e[["a", "b"][i]][k].as_f64().unwrap())) };
    let top = edges.iter().flat_map(|e| pts(e).map(|p| p[2])).fold(f64::MIN, f64::max);
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for e in edges {
        for p in pts(e) {
            (x0, x1, y0, y1) = (x0.min(p[0]), x1.max(p[0]), y0.min(p[1]), y1.max(p[1]));
        }
    }
    let border = |p: [f64; 3]| (p[0] - x0).abs() < 1e-6 || (p[0] - x1).abs() < 1e-6 || (p[1] - y0).abs() < 1e-6 || (p[1] - y1).abs() < 1e-6;
    edges
        .iter()
        .filter(|e| pts(e).iter().all(|p| (p[2] - top).abs() < 1e-6) && pts(e).iter().all(|p| border(*p)) == outer)
        .map(|e| e["id"].as_u64().unwrap())
        .collect()
}

#[test]
fn patch_is_the_lid_and_the_source_stays() {
    let mut r = open_box();
    let outer = rim(&mut r, "cup", true);
    assert_eq!(outer.len(), 4);
    let out = call(&mut r, "patch", json!({"edges": outer, "name": "lid"}));
    assert!(out["rebuild"].get("errors").is_none(), "{out}");
    assert_eq!((out["feature"].clone(), out["name"].clone(), out["sheet"].clone()), (out["body"].clone(), json!("lid"), json!(true)));
    assert_eq!(out["selected_edges"].as_array().unwrap().len(), 4);
    close(area(&mut r, &out["body"]), 1200., "40 x 30");
    assert_eq!(bodies(&mut r), 2, "the cup stays beside the lid");
    // Tangent is larger; it follows the base.
    let mut t = open_box();
    let outer = rim(&mut t, "cup", true);
    let smooth = call(&mut t, "patch", json!({"edges": outer, "mode": "tangent"}));
    assert!(area(&mut t, &smooth["body"]) > 1205., "tangent bows towards the walls");
    call(&mut r, "param_set", json!({"name": "w", "value": 60}));
    close(area(&mut r, &out["body"]), 1800., "60 x 30");
}

#[test]
fn patch_over_a_hole_rim_and_thicken_close_the_loop() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "base"}));
    call(&mut r, "sketch_add", json!({"sketch": "base", "entities": [{"type": "rect", "w": 40, "h": 30}]}));
    call(&mut r, "extrude", json!({"sketch": "base", "height": 10, "name": "block"}));
    call(&mut r, "hole", json!({"face": {"facing": "+z"}, "diameter": 20, "depth": 30}));
    let t = call(&mut r, "topology", json!({}));
    let ring: Vec<u64> = t["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "circle" && (e["a"][2].as_f64().unwrap() - 10.).abs() < 1e-6)
        .map(|e| e["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ring.len(), 1);
    let disc = call(&mut r, "patch", json!({"edges": ring}));
    assert!(disc["rebuild"].get("errors").is_none(), "{disc}");
    close(area(&mut r, &disc["body"]), std::f64::consts::PI * 100., "disc r10");
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(info["bodies"].as_array().unwrap().len(), 2);
    // The lid: a plate of the disc closes the hole (thicken of a sheet takes the whole sheet).
    let lid = call(&mut r, "thicken", json!({"body": disc["body"], "face": "largest", "thickness": 2}));
    assert!(lid["rebuild"].get("errors").is_none(), "{lid}");
    let info = call(&mut r, "doc_info", json!({}));
    let solids: Vec<&Value> = info["bodies"].as_array().unwrap().iter().filter(|b| b["sheet"] == false).collect();
    assert_eq!(solids.len(), 1, "{info}");
    close(solids[0]["volume_mm3"].as_f64().unwrap(), 12000. - 3141.59 + 628.32, "block with the hole plus the plug");
}

#[test]
fn patch_refusals_leave_the_document_unchanged() {
    let mut r = open_box();
    let outer = rim(&mut r, "cup", true);
    let inner = rim(&mut r, "cup", false);
    let before = call(&mut r, "doc_info", json!({}));
    let e = refused(&mut r, "patch", json!({"edges": [outer[0]]}));
    assert!(e.contains("The edges do not close"), "{e}");
    let e = refused(&mut r, "patch", json!({"edges": outer[..3]}));
    assert!(e.contains("The edges do not close"), "{e}");
    let both: Vec<u64> = outer.iter().chain(&inner).copied().collect();
    let e = refused(&mut r, "patch", json!({"edges": both}));
    assert!(e.contains("2 separate boundaries") && e.contains("F-199"), "{e}");
    assert!(refused(&mut r, "patch", json!({"edges": [999999]})).contains("not on body"));
    assert!(refused(&mut r, "patch", json!({})).contains("`edges`"));
    assert!(refused(&mut r, "patch", json!({"edges": outer, "mode": "round"})).contains("round"));
    assert!(refused(&mut r, "patch", json!({"edges": outer, "extra": 1})).contains("extra"));
    assert!(refused(&mut r, "patch", json!({"edges": outer, "body": "nothing"})).contains("nothing"));
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
}

#[test]
fn patch_feature_repicks_edges_and_mode() {
    let mut r = open_box();
    let outer = rim(&mut r, "cup", true);
    let inner = rim(&mut r, "cup", false);
    let out = call(&mut r, "patch", json!({"edges": outer, "name": "lid"}));
    assert!(refused(&mut r, "patch", json!({"feature": "lid", "name": "x", "mode": "tangent"})).contains("not accepted"));
    assert!(refused(&mut r, "patch", json!({"feature": "lid", "body": "cup", "mode": "tangent"})).contains("not accepted"));
    assert!(refused(&mut r, "patch", json!({"feature": "lid"})).contains("`edges` and/or `mode`"));
    assert!(refused(&mut r, "patch", json!({"feature": "block", "mode": "tangent"})).contains("not a patch"));
    let again = call(&mut r, "patch", json!({"feature": "lid", "edges": inner}));
    assert_eq!(again["feature"], out["feature"]);
    close(area(&mut r, &out["body"]), 936., "the inner outline, 36 x 26");
    call(&mut r, "patch", json!({"feature": "lid", "mode": "tangent"}));
    assert!(area(&mut r, &out["body"]) > 940.);
    call(&mut r, "patch", json!({"feature": "lid", "edges": outer, "mode": "flat"}));
    close(area(&mut r, &out["body"]), 1200., "back to the lid");
    // An open chain on a re-pick reaches the rebuild and is rolled back.
    let before = call(&mut r, "doc_info", json!({}));
    assert!(refused(&mut r, "patch", json!({"feature": "lid", "edges": [outer[0]]})).contains("rolled back"));
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
}

#[test]
fn patch_batch_undo_and_save_open() {
    let mut r = open_box();
    let outer = rim(&mut r, "cup", true);
    let steps = json!({"steps": [
        {"tool": "patch", "args": {"edges": outer, "name": "lid"}},
        {"tool": "param_set", "args": {"name": "w", "value": 50}},
    ]});
    let out = call(&mut r, "batch", steps);
    assert_eq!(out["steps"].as_array().unwrap().len(), 2, "{out}");
    let lid = call(&mut r, "doc_info", json!({}))["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "lid").unwrap()["id"].clone();
    close(area(&mut r, &lid), 1500., "50 x 30");
    call(&mut r, "undo", json!({}));
    assert_eq!(bodies(&mut r), 1, "one undo takes the whole batch back");
    // A failing second step rolls the first back.
    let before = call(&mut r, "doc_info", json!({}));
    let e = refused(
        &mut r,
        "batch",
        json!({"steps": [
            {"tool": "patch", "args": {"edges": outer}},
            {"tool": "patch", "args": {"edges": [outer[0]]}},
        ]}),
    );
    assert!(e.contains("step 2"), "{e}");
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
    // Save and open: the patch comes back and follows an edit.
    let out = call(&mut r, "patch", json!({"edges": outer, "name": "lid", "mode": "tangent"}));
    let path = std::env::temp_dir().join(format!("t4-patch-{}.qcad", std::process::id()));
    call(&mut r, "doc_save", json!({"path": path.to_str().unwrap()}));
    let a0 = area(&mut r, &out["body"]);
    let mut fresh = Registry::new();
    let opened = call(&mut fresh, "doc_open", json!({"path": path.to_str().unwrap()}));
    assert!(opened.get("errors").is_none(), "{opened}");
    assert!((area(&mut fresh, &out["body"]) - a0).abs() < 1e-3);
    call(&mut fresh, "param_set", json!({"name": "w", "value": 60}));
    assert!(area(&mut fresh, &out["body"]) > 1800., "edit after open");
    let _ = std::fs::remove_file(path);
}
