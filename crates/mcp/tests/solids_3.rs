//! sweep, loft, profiles_at and hole offsets through the tools: shapes, defaults and refusals.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}
fn refused(r: &mut Registry, tool: &str, args: Value) -> String {
    r.call(tool, args).unwrap().unwrap_err()
}
fn volume(v: &Value) -> f64 {
    v["rebuild"]["bodies"].as_array().unwrap().last().unwrap()["volume_mm3"].as_f64().unwrap()
}

#[test]
fn sweep_loft_profiles_at_and_hole_offset_through_the_tools() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name": "L", "value": 40}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "profile"}));
    call(&mut r, "sketch_add", json!({"sketch": "profile", "entities": [{"type": "rect", "cx": 7, "w": 4, "h": 4}]}));
    call(&mut r, "sketch_create", json!({"plane": "XZ", "name": "path"}));
    call(&mut r, "sketch_add", json!({"sketch": "path", "entities": [{"type": "line", "x1": 0, "y1": 0, "x2": 0, "y2": "L"}]}));
    let swept = call(&mut r, "sweep", json!({"sketch": "profile", "path": {"sketch": "path"}, "name": "rail"}));
    assert!((volume(&swept) - 640.0).abs() < 1e-6, "{swept}");
    let e = refused(&mut r, "sweep", json!({"sketch": "profile", "path": {"sketch": "path", "edges": [1]}}));
    assert!(e.contains("bad arguments") || e.contains("did not match"), "a path is a sketch or edges, not both: {e}");

    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "low"}));
    call(
        &mut r,
        "sketch_add",
        json!({"sketch": "low", "entities": [{"type": "rect", "w": 20, "h": 20}, {"type": "rect", "cx": 40, "w": 20, "h": 20}]}),
    );
    let plane = call(&mut r, "plane_offset", json!({"base": "XY", "dist": 10}));
    call(&mut r, "sketch_create", json!({"plane": {"plane": plane["plane"]}, "name": "high"}));
    call(&mut r, "sketch_add", json!({"sketch": "high", "entities": [{"type": "rect", "w": 10, "h": 10}]}));
    let e = refused(&mut r, "loft", json!({"sections": [{"sketch": "low"}, {"sketch": "high"}]}));
    assert!(e.contains("several closed contours"), "{e}");
    let lofted = call(&mut r, "loft", json!({"sections": [{"sketch": "low", "profile_at": [0, 0]}, {"sketch": "high"}], "ruled": true}));
    assert!((volume(&lofted) - 10.0 / 3.0 * (400.0 + 100.0 + 200.0)).abs() < 1e-6, "{lofted}");

    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name": "u", "value": 5}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "plate"}));
    call(
        &mut r,
        "sketch_add",
        json!({"sketch": "plate", "entities": [{"type": "rect", "cx": 20, "cy": 15, "w": 40, "h": 30}, {"type": "circle", "cx": 10, "cy": 15, "d": 6}]}),
    );
    let e = refused(&mut r, "extrude", json!({"sketch": "plate", "profiles": [1], "profiles_at": [[1, 1]], "height": 10}));
    assert!(e.contains("not both"), "{e}");
    // A point in the plate (outside the circle) takes the plate minus its hole.
    let plate = call(&mut r, "extrude", json!({"sketch": "plate", "profiles_at": [[30, 15]], "height": 10}));
    let plate_volume = 40.0 * 30.0 * 10.0 - std::f64::consts::PI * 9.0 * 10.0;
    assert!((volume(&plate) - plate_volume).abs() < 1e-3, "{plate}");
    let holed = call(&mut r, "hole", json!({"face": {"facing": "+z"}, "offset": ["u", 3], "diameter": 4, "depth": 6}));
    assert!((volume(&holed) - (plate_volume - std::f64::consts::PI * 4.0 * 6.0)).abs() < 1e-3, "{holed}");
    let e = refused(&mut r, "hole", json!({"face": {"extreme": "+z"}, "offset": [1, 1], "at": [1, 1, 10], "diameter": 2, "depth": 2}));
    assert!(e.contains("not both"), "{e}");
}
