//! P19 over the registry: `primitive` arguments are strict, the tool is batchable and undoable, sizes are reachable by dimension_set.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}

fn fail(r: &mut Registry, tool: &str, args: Value) -> String {
    r.call(tool, args).unwrap().unwrap_err()
}

fn volume(r: &mut Registry) -> f64 {
    let info = call(r, "doc_info", json!({}));
    info["bodies"][0]["volume_mm3"].as_f64().unwrap()
}

#[test]
fn arguments_are_strict_and_each_kind_takes_only_its_own_sizes() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    for (bad, word) in [
        (json!({}), "kind"),
        (json!({"kind":"pyramid","r":1}), "pyramid"),
        (json!({"kind":"box","dx":1,"dy":1}), "dz"),
        (json!({"kind":"box","dx":1,"dy":1,"dz":1,"r":3}), "r"),
        (json!({"kind":"cylinder","r":1,"h":1,"dx":3}), "dx"),
        (json!({"kind":"sphere","r":1,"typo":1}), "typo"),
        (json!({"kind":"cone","r1":3,"r2":1}), "h"),
        (json!({"kind":"torus","major":3}), "minor"),
        (json!({"kind":"prism","r":3,"h":2}), "n"),
        (json!({"kind":"box","dx":1,"dy":1,"dz":1,"place":{"point":1,"typo":2}}), "typo"),
    ] {
        let e = fail(&mut r, "primitive", bad.clone());
        assert!(e.contains("bad arguments for `primitive`") && e.contains(word), "{bad}: {e}");
    }
    let e = fail(&mut r, "primitive", json!({"kind":"box","dx":1,"dy":1,"dz":1,"place":{}}));
    assert!(e.contains("`place` is empty"), "{e}");
    let e = fail(&mut r, "primitive", json!({"kind":"box","dx":1,"dy":1,"dz":1,"place":{"point":1,"vertex":{"edge":1,"end":"start"}}}));
    assert!(e.contains("exactly one of `point` or `vertex`"), "{e}");
    // Nothing above created anything.
    assert!(call(&mut r, "doc_info", json!({}))["timeline"].as_array().unwrap().is_empty());
}

#[test]
fn a_primitive_reports_its_node_sizes_and_union_and_is_named_dimensioned_and_undone() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name":"w","value":30}));
    let a = call(&mut r, "primitive", json!({"kind":"box","dx":"w","dy":20,"dz":10,"name":"slab"}));
    assert_eq!((a["kind"].as_str(), a["name"].as_str()), (Some("box"), Some("slab")));
    assert_eq!(a["feature"], a["body"], "the first primitive is the body");
    assert_eq!(a["sizes"], json!({"dx":30.0,"dy":20.0,"dz":10.0}));
    assert!(a.get("joined").is_none() && a.get("placed_at").is_none());
    assert!(a["rebuild"].get("errors").is_none() && a["rebuild"]["bodies"].as_array().unwrap().len() == 1);
    let b = call(&mut r, "primitive", json!({"kind":"cylinder","r":5,"h":40,"name":"post"}));
    assert_ne!(b["feature"], b["body"]);
    assert_eq!(b["joined"], json!({"prior":a["body"],"union":b["body"]}));
    let info = call(&mut r, "doc_info", json!({}));
    let kinds: Vec<&str> = info["timeline"].as_array().unwrap().iter().map(|n| n["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["box3", "cylinder", "bodyboolean"]);
    // dimension_set takes the size name as the key and the node as the feature.
    call(&mut r, "dimension_set", json!({"target":{"feature":b["feature"],"key":"h"},"value":50}));
    call(&mut r, "dimension_set", json!({"target":{"feature":a["feature"],"key":"dz"},"value":"w/3"}));
    let expected = 30.0 * 20.0 * 10.0 + std::f64::consts::PI * 25.0 * 40.0;
    assert!((volume(&mut r) - expected).abs() < 1e-3, "{} vs {expected}", volume(&mut r));
    // Undo labels: a dimension edit keeps its action label; the primitive is labelled by its own node (below), not the union.
    let undone = call(&mut r, "undo", json!({}));
    assert_eq!(
        (undone["undone"]["tool"].as_str(), undone["undone"]["label"].as_str()),
        (Some("dimension_set"), Some("Dimension set")),
        "{undone}"
    );
    call(&mut r, "undo", json!({}));
    let undone = call(&mut r, "undo", json!({}));
    assert_eq!((undone["undone"]["tool"].as_str(), undone["undone"]["label"].as_str()), (Some("primitive"), Some("post")), "{undone}");
    assert_eq!(call(&mut r, "doc_info", json!({}))["timeline"].as_array().unwrap().len(), 1, "one step took the primitive and its union");
}

#[test]
fn place_by_a_datum_point_name_and_json_string_leniency() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "primitive", json!({"kind":"box","dx":20,"dy":20,"dz":20}));
    call(&mut r, "datum_point", json!({"at":[100,0,0],"name":"far"}));
    let s = call(&mut r, "primitive", json!({"kind":"sphere","r":5,"place":{"point":"far"}}));
    assert_eq!(s["placed_at"], json!([100.0, 0.0, 0.0]));
    let c = call(&mut r, "primitive", json!({"kind":"sphere","r":5,"place":"{\"point\":\"far\"}"}));
    assert_eq!(c["placed_at"], json!([100.0, 0.0, 0.0]), "a structured value sent as a string is accepted like everywhere");
    let kinds: Vec<String> = call(&mut r, "doc_info", json!({}))["timeline"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["kind"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(kinds, ["box3", "datumpoint", "sphere", "move", "bodyboolean", "sphere", "move", "bodyboolean"]);
}

#[test]
fn refusals_come_as_tool_errors_and_a_batch_with_a_bad_second_step_changes_nothing() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    for (bad, word) in [
        (json!({"kind":"box","dx":0.05,"dy":1,"dz":1}), "box dx must be within 0.1..=100000"),
        (json!({"kind":"prism","r":5,"h":5,"n":2.5}), "prism n must be a whole number"),
        (json!({"kind":"cone","r1":4,"r2":4,"h":5}), "cone r1 and r2 are equal"),
        (json!({"kind":"torus","major":5,"minor":5}), "torus minor must be within"),
    ] {
        let e = fail(&mut r, "primitive", bad.clone());
        assert!(e.contains(word), "{bad}: {e}");
    }
    let steps = |second: Value| json!({"steps":[{"tool":"primitive","args":{"kind":"box","dx":10,"dy":10,"dz":10,"name":"first"}},{"tool":"primitive","args":second}]});
    let e = fail(&mut r, "batch", steps(json!({"kind":"sphere","r":0.01})));
    assert!(e.contains("step 2") && e.contains("sphere r must be within 0.05"), "{e}");
    assert!(call(&mut r, "doc_info", json!({}))["timeline"].as_array().unwrap().is_empty(), "the first step was rolled back");
    let ok = call(&mut r, "batch", steps(json!({"kind":"sphere","r":5,"name":"ball"})));
    assert_eq!(ok["steps"][0]["created"]["name"], "first");
    assert_eq!(ok["steps"][1]["created"]["name"], "ball", "the created node is the primitive, not the union");
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "batch");
    assert!(call(&mut r, "doc_info", json!({}))["timeline"].as_array().unwrap().is_empty());
}

#[test]
fn the_assembly_root_is_refused_and_a_part_works() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "part_select", json!({"part":{"root":true}}));
    let e = fail(&mut r, "primitive", json!({"kind":"box","dx":1,"dy":1,"dz":1}));
    assert!(e.contains("the active context is an assembly"), "{e}");
    call(&mut r, "part_new", json!({"name":"second"}));
    let ok = call(&mut r, "primitive", json!({"kind":"torus","major":12,"minor":4}));
    assert_eq!(ok["kind"], "torus");
    assert!((volume(&mut r) - 2.0 * std::f64::consts::PI.powi(2) * 12.0 * 16.0).abs() < 1e-3);
}

#[test]
fn the_first_placed_primitive_is_its_move_and_survives_undo_save_and_open() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "datum_point", json!({"at":[10,0,0],"name":"p"}));
    let s = call(&mut r, "primitive", json!({"kind":"sphere","r":5,"place":{"point":"p"}}));
    // `body` is the Move node, not the primitive: three variants (primitive; Move when placed into an empty Part; union).
    assert_ne!(s["feature"], s["body"]);
    assert!(s.get("joined").is_none(), "{s}");
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(info["bodies"][0]["id"], s["body"]);
    let expected = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
    assert!((volume(&mut r) - expected).abs() < 1e-3);
    let kinds = |r: &mut Registry| -> Vec<String> {
        call(r, "doc_info", json!({}))["timeline"].as_array().unwrap().iter().map(|n| n["kind"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(kinds(&mut r), ["datumpoint", "sphere", "move"]);
    let dir = std::env::temp_dir().join(format!("qymcad-mcp-j1-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("placed.qcad").to_str().unwrap().to_string();
    call(&mut r, "doc_save", json!({"path":path}));
    // One undo takes the primitive and its Move together.
    call(&mut r, "undo", json!({}));
    assert_eq!(kinds(&mut r), ["datumpoint"]);
    // Open keeps the same body.
    call(&mut r, "doc_open", json!({"path":path}));
    assert_eq!(kinds(&mut r), ["datumpoint", "sphere", "move"]);
    assert!((volume(&mut r) - expected).abs() < 1e-3);
    assert_eq!(call(&mut r, "doc_info", json!({}))["bodies"][0]["id"], s["body"]);
}
