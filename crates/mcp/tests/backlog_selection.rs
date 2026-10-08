//! Backlog selection and reporting contracts, checked through real tool handlers.

use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, name: &str, args: Value) -> Value {
    let content = r.call(name, args).unwrap().unwrap();
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

fn cylinder() -> (Registry, Value) {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY", "name":"profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"profile", "entities":[{"type":"circle", "d":10}]}));
    let built = call(&mut r, "extrude", json!({"sketch":"profile", "height":20, "name":"stock"}));
    (r, built)
}

#[test]
fn volume_has_one_name_and_preserves_kernel_precision() {
    let (mut r, built) = cylinder();
    let info = call(&mut r, "doc_info", json!({}));
    let rebuilt = built["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap();
    let expected = std::f64::consts::PI * 5.0_f64.powi(2) * 20.0;
    assert!((rebuilt - expected).abs() < 1e-9, "full-precision cylinder volume πr²h: got {rebuilt}, expected {expected}");
    assert_eq!(
        info["bodies"][0]["volume_mm3"], built["rebuild"]["bodies"][0]["volume_mm3"],
        "doc_info and rebuild use identical full-precision volume_mm3"
    );
    assert!(info["bodies"][0].get("volume").is_none(), "one public volume field");
}

#[test]
fn topology_and_select_rows_are_sorted_by_persistent_id() {
    let (mut r, _) = cylinder();
    for el in ["faces", "edges"] {
        let topo = call(&mut r, "topology", json!({"adjacency":true}));
        let ids: Vec<_> = topo[el].as_array().unwrap().iter().map(|row| row["id"].as_u64().unwrap()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "topology {el} rows sorted by id");
        let mut reverse = sorted.clone();
        reverse.reverse();
        let selected = call(&mut r, "select", json!({el: reverse}));
        let actual: Vec<_> = selected[el].as_array().unwrap().iter().map(|row| row["id"].as_u64().unwrap()).collect();
        assert_eq!(actual, sorted, "select {el} rows sorted by id, independent of pick order");
    }
}

#[test]
fn and_accepts_three_or_more_filters() {
    let (mut r, _) = cylinder();
    let selection = json!({"and":[{"convex":true}, {"extreme":"+z"}, {"edges_of":{"facing":"+z"}}]});
    let picked = call(&mut r, "select", json!({"edges":selection}));
    // Two cylinder rims, restricted to the top cap, leave one circle at z=h with circumference 2πr.
    assert_eq!(picked["count"], 1);
    assert_eq!(picked["edges"][0]["mid"][2], 20.0);
    assert!((picked["edges"][0]["length"].as_f64().unwrap() - 2.0 * std::f64::consts::PI * 5.0).abs() < 0.00005);
    let wide = json!({"and": vec![json!({"facing":"+z"}); 200]});
    assert_eq!(call(&mut r, "select", json!({"faces":wide}))["count"], 1, "200-way balanced intersection retains the one top cap");
    let over_budget = json!({"and": vec![json!({"facing":"+z"}); 300]});
    assert!(
        r.call("select", json!({"faces":over_budget})).unwrap().unwrap_err().contains("at most 512"),
        "wide intersections retain the selection size budget"
    );
    for count in [0, 1] {
        let args = json!({"edges":{"and":vec![json!({"convex":true});count]}});
        let error = r.call("select", args).unwrap().unwrap_err();
        assert!(error.contains("at least two selections"), "{error}");
    }
}

#[test]
fn kind_filters_match_topology_taxonomy() {
    let (mut r, _) = cylinder();
    let topo = call(&mut r, "topology", json!({}));
    for (el, kinds) in [("edges", vec!["line", "circle", "arc", "other"]), ("faces", vec!["plane", "cylinder", "cone", "sphere", "other"])]
    {
        for kind in kinds {
            let picked = call(&mut r, "select", json!({el:{"kind":kind}}));
            let expected: Vec<_> =
                topo[el].as_array().unwrap().iter().filter(|row| row["kind"] == kind).map(|row| row["id"].clone()).collect();
            let actual: Vec<_> = picked[el].as_array().unwrap().iter().map(|row| row["id"].clone()).collect();
            assert_eq!(actual, expected, "{el} kind={kind} agrees with topology");
        }
    }
    // A cylinder has exactly two plane caps, one cylindrical wall, two circular rims and one line seam.
    assert_eq!(call(&mut r, "select", json!({"faces":{"kind":"plane"}}))["count"], 2);
    assert_eq!(call(&mut r, "select", json!({"faces":{"kind":"cylinder"}}))["count"], 1);
    assert_eq!(call(&mut r, "select", json!({"edges":{"kind":"circle"}}))["count"], 2);
    assert_eq!(call(&mut r, "select", json!({"edges":{"kind":"curve"}}))["count"], 0);
    for args in [json!({"faces":{"kind":"circle"}}), json!({"edges":{"kind":"plane"}}), json!({"edges":{"kind":"typo"}})] {
        assert!(r.call("select", args).unwrap().is_err(), "incompatible or unknown kind must fail");
    }
}

#[test]
fn bbox_description_explains_tessellation_accuracy() {
    let list = Registry::new().list();
    let description = list.iter().find(|t| t["name"] == "doc_info").unwrap()["description"].as_str().unwrap();
    for required in ["0.005 mm", "nominal", "f32", "under-bound", "fallback"] {
        assert!(description.contains(required), "doc_info must explain {required}: {description}");
    }
}

#[test]
fn corner_descriptions_state_remaining_degree_thresholds() {
    let list = Registry::new().list();
    for name in ["select", "fillet"] {
        let description = list.iter().find(|t| t["name"] == name).unwrap()["description"].as_str().unwrap();
        for threshold in ["3 degrees", "0.9 degrees", "1.5 degrees", "20–30 degrees"] {
            assert!(description.contains(threshold), "{name} must state {threshold}: {description}");
        }
    }
}

#[test]
fn uncertain_corner_previews_report_omitted_edges() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY", "name":"profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":32, "h":32}]}));
    call(&mut r, "extrude", json!({"sketch":"profile", "height":12, "name":"stock"}));
    // Cone depth follows tan(slope) = depth/(R-r), with R=6, r=2 and slope 0.9°.
    call(
        &mut r,
        "hole",
        json!({"face":{"facing":"+z"}, "diameter":4, "depth":6, "kind":"countersink", "dia2":12, "depth2":4.0*0.9_f64.to_radians().tan()}),
    );
    for kind in ["concave", "convex"] {
        let picked = call(&mut r, "select", json!({"edges":{kind:true}}));
        assert!(
            picked["note"].as_str().is_some_and(|note| note.starts_with("omitted ") && note.ends_with(" uncertain edges")),
            "uncertain shallow rim must be reported for {kind}: {picked}"
        );
    }
}

#[test]
fn corner_hint_is_reused_and_between_requires_multiple_body_features() {
    let (mut r, _) = cylinder();
    let empty = call(&mut r, "select", json!({"edges":{"concave":true}}));
    assert_eq!(empty["hint"], "0 concave (inward) edges; this body has 2 convex edges");
    let convex = call(&mut r, "select", json!({"edges":{"and":[{"convex":true}, []]}}));
    assert_eq!(
        convex["hint"],
        "this body has 2 convex (outward) edges, but none survives the rest of the selection; it has 0 concave edges"
    );
    for (tool, args) in
        [("fillet", json!({"edges":{"concave":true}, "radius":1})), ("chamfer", json!({"edges":{"concave":true}, "dist":1}))]
    {
        let message = r.call(tool, args).unwrap().unwrap_err();
        assert!(message.contains(empty["hint"].as_str().unwrap()), "preview hint repeated in {tool}: {message}");
    }
    // One extrusion followed by one bevel is a two-feature body.
    call(&mut r, "chamfer", json!({"edges":{"and":[{"convex":true}, {"extreme":"+z"}]}, "dist":1}));
    let empty = call(&mut r, "select", json!({"edges":{"concave":true}}));
    assert!(empty["hint"].as_str().unwrap().contains("between"), "two-feature body offers a between example: {empty}");
}

#[test]
fn face_kind_modifiers_refuse_unsupported_persistent_kind_queries() {
    let (mut r, _) = cylinder();
    let before = call(&mut r, "doc_info", json!({}));
    let face = json!({"and":[{"kind":"plane"}, {"facing":"+z"}]});
    assert_eq!(call(&mut r, "select", json!({"faces":face}))["count"], 1);
    for (tool, args) in [
        ("hole", json!({"face":face, "diameter":2, "depth":3})),
        ("shell", json!({"open_faces":face, "thickness":1})),
        ("push_face", json!({"face":face, "dist":1})),
    ] {
        let message = r.call(tool, args).unwrap().unwrap_err();
        assert!(
            message.contains("preview-only") && message.contains("no persistent kind query"),
            "unsupported face kind persistence: {tool}: {message}"
        );
        assert_eq!(call(&mut r, "doc_info", json!({})), before, "{tool} refusal preserves the document");
    }
}

#[test]
fn opened_shelf_reports_tight_bounds_in_both_fields_and_render_caption() {
    let mut r = Registry::new();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../engine/tests/fixtures/hanging_shelf.qcad");
    let opened = call(&mut r, "doc_open", json!({"path":path}));
    // Centered stock width/length/height; subtractive features leave the side-wall extrema.
    // .005 nominal chord deflection + f32 rounding at coordinate 134 mm.
    let tolerance = 0.005 + f32::EPSILON as f64 * 134.0;
    let info = call(&mut r, "doc_info", json!({}));
    for b in [&opened["rebuild"]["bodies"][0], &opened["doc"]["bodies"][0], &info["bodies"][0]] {
        for (actual, expected) in b["bbox"].as_array().unwrap().iter().zip([-134.0, -85.0, 0.0, 134.0, 85.0, 17.0]) {
            assert!((actual.as_f64().unwrap() - expected).abs() <= tolerance, "tight bbox: {b}");
        }
    }
    let render = r.call("render", json!({"view":"top", "width":128, "height":128})).unwrap().unwrap();
    let caption = render.iter().find_map(|item| item["text"].as_str()).unwrap();
    assert!(caption.contains("bbox x -134..134 y -85..85 z") && caption.contains("..17 mm"), "tight render caption: {caption}");
}
