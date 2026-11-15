//! F3: deleting a sketch removes the formulas and names owned by its points, entities and patterns.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let content = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}
fn refused(r: &mut Registry, tool: &str, args: Value) -> String {
    r.call(tool, args).unwrap().expect_err("call must be refused")
}
fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() <= 1e-6, "actual {actual}, expected {expected}");
}
/// Three regular hexagons (circumradius 2), height 5: 3 x (3*sqrt(3)/2 x 2^2) x 5 = 90*sqrt(3).
const HEX_VOLUME: f64 = 30.0 * 1.7320508075688772;

/// A sketch with a polygon, a linear pattern whose count is `param`, and (optionally) an extrusion.
fn hexes(r: &mut Registry, sketch: &str, param: &str, y: f64, extrude: bool) -> Value {
    call(r, "sketch_create", json!({"plane":"XY","name":sketch}));
    let added = call(r, "sketch_add", json!({"sketch":sketch,"entities":[{"type":"polygon","cx":0,"cy":y,"r":2,"sides":6}]}));
    let pattern = call(
        r,
        "sketch_pattern",
        json!({"sketch":sketch,"entities":added["created"][0]["entities"],"kind":"linear","count":param,"dx":10,"dy":0}),
    );
    if extrude {
        call(r, "extrude", json!({"sketch":sketch,"height":5,"op":"new_body","name":format!("{sketch}_body")}));
    }
    pattern
}
fn fresh() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name":"repeat_count","value":3}));
    r
}
fn volume(r: &mut Registry) -> f64 {
    call(r, "doc_info", json!({}))["bodies"][0]["volume_mm3"].as_f64().unwrap()
}

#[test]
fn f3_cascade_delete_frees_the_pattern_count_parameter() {
    let mut r = fresh();
    hexes(&mut r, "hexes", "repeat_count", 0.0, true);
    close(volume(&mut r), 3.0 * HEX_VOLUME);
    let deleted = call(&mut r, "feature_delete", json!({"feature":"hexes","cascade":true}));
    assert_eq!(deleted["deleted"].as_array().unwrap().len(), 2, "sketch + extrude only: {deleted}");
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(info["sketches"], json!([]));
    assert_eq!(info["bodies"], json!([]));
    call(&mut r, "param_delete", json!({"name":"repeat_count"}));
}

#[test]
fn f3_metadata_cleanup_survives_save_and_reopen() {
    let mut r = fresh();
    hexes(&mut r, "hexes", "repeat_count", 0.0, true);
    call(&mut r, "feature_delete", json!({"feature":"hexes","cascade":true}));
    let path = std::env::temp_dir().join(format!("qymcad-f3-reopen-{}.qcad", std::process::id()));
    call(&mut r, "doc_save", json!({"path":path,"allow_empty":true}));
    let mut reopened = Registry::new();
    call(&mut reopened, "doc_open", json!({"path":path}));
    std::fs::remove_file(&path).unwrap();
    call(&mut reopened, "param_delete", json!({"name":"repeat_count"}));
}

#[test]
fn f3_leaf_sketch_delete_without_cascade_frees_the_parameter() {
    let mut r = fresh();
    hexes(&mut r, "hexes", "repeat_count", 0.0, false);
    call(&mut r, "feature_delete", json!({"feature":"hexes","cascade":false}));
    call(&mut r, "param_delete", json!({"name":"repeat_count"}));
}

#[test]
fn f3_surviving_sketch_keeps_its_pattern_formula() {
    let mut r = fresh();
    hexes(&mut r, "first", "repeat_count", 0.0, true);
    let second = hexes(&mut r, "second", "repeat_count", 50.0, true);
    call(&mut r, "feature_delete", json!({"feature":"first","cascade":true}));
    let error = refused(&mut r, "param_delete", json!({"name":"repeat_count"}));
    let pattern = second["pattern"].as_u64().or_else(|| second["id"].as_u64()).expect("pattern id in result");
    assert!(error.contains(&format!("used by feature {pattern}")), "{error}");
    call(&mut r, "param_set", json!({"name":"repeat_count","value":4}));
    // The surviving pattern still follows the parameter: four hexagons.
    close(volume(&mut r), 4.0 * HEX_VOLUME);
}

#[test]
fn f3_undo_restores_pattern_formula_and_geometry_in_one_step() {
    let mut r = fresh();
    hexes(&mut r, "hexes", "repeat_count", 0.0, true);
    let before = call(&mut r, "doc_info", json!({}));
    call(&mut r, "feature_delete", json!({"feature":"hexes","cascade":true}));
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "feature_delete");
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
    let error = refused(&mut r, "param_delete", json!({"name":"repeat_count"}));
    assert!(error.contains("used by feature"), "{error}");
    call(&mut r, "param_set", json!({"name":"repeat_count","value":2}));
    close(volume(&mut r), 2.0 * HEX_VOLUME);
}

#[test]
fn f3_failed_batch_after_delete_restores_metadata() {
    let mut r = fresh();
    hexes(&mut r, "hexes", "repeat_count", 0.0, true);
    let before = call(&mut r, "doc_info", json!({}));
    let error = refused(
        &mut r,
        "batch",
        json!({"steps":[
            {"tool":"feature_delete","args":{"feature":"hexes","cascade":true}},
            {"tool":"extrude","args":{"sketch":"missing","height":2}}
        ]}),
    );
    assert!(error.contains("step 2"), "{error}");
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
    assert!(refused(&mut r, "param_delete", json!({"name":"repeat_count"})).contains("used by feature"));
}

#[test]
fn f3_named_dimension_of_deleted_feature_is_removed() {
    let mut r = fresh();
    hexes(&mut r, "hexes", "repeat_count", 0.0, true);
    call(&mut r, "dimension_set", json!({"target":{"feature":"hexes_body","key":"height"},"name":"hex_h"}));
    assert_eq!(call(&mut r, "doc_info", json!({}))["named_dimensions"].as_array().unwrap().len(), 1);
    call(&mut r, "feature_delete", json!({"feature":"hexes","cascade":true}));
    assert_eq!(call(&mut r, "doc_info", json!({}))["named_dimensions"], json!([]));
    // The name is free again for a new parameter.
    call(&mut r, "param_set", json!({"name":"hex_h","value":1}));
}

#[test]
fn f3_name_read_by_a_surviving_parameter_is_kept_until_the_alias_goes() {
    let mut r = fresh();
    hexes(&mut r, "hexes", "repeat_count", 0.0, true);
    call(&mut r, "dimension_set", json!({"target":{"feature":"hexes_body","key":"height"},"name":"hex_h"}));
    call(&mut r, "param_set", json!({"name":"double_h","value":"hex_h*2"}));
    // Existing policy (golden_medium_r1): deletion proceeds; the user removes the alias explicitly.
    call(&mut r, "feature_delete", json!({"feature":"hexes","cascade":true}));
    let names = call(&mut r, "doc_info", json!({}))["named_dimensions"].clone();
    assert_eq!(names.as_array().unwrap().len(), 1, "{names}");
    assert_eq!(names[0]["name"], "hex_h");
    call(&mut r, "param_delete", json!({"name":"double_h"}));
    call(&mut r, "param_delete", json!({"name":"repeat_count"}));
}
