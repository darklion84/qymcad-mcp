//! J3 coordinate presentation and curved reporting regressions.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, name: &str, args: Value) -> Value {
    let content = r.call(name, args).unwrap().unwrap();
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn frames_and_sketch_points_share_four_decimal_reporting() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    let plane = call(&mut r, "plane_offset", json!({"base":"XY","dist":"1.2*3"}))["plane"].clone();
    let sketch = call(&mut r, "sketch_create", json!({"plane":{"plane":plane}}))["sketch"].clone();
    let added = call(&mut r, "sketch_add", json!({"sketch":sketch,"entities":[{"type":"circle","cx":1.23456789,"cy":-2.34567891,"d":2}]}));
    // Frame z=1.2*3=3.6; circle centre is exactly its pinned coordinates, rounded to 10^-4 mm.
    let expected_origin = json!([0.0, 0.0, 3.6]);
    assert_eq!(call(&mut r, "doc_info", json!({}))["sketches"][0]["world_frame"]["origin"], expected_origin);
    for summary in [added["sketch"].clone(), call(&mut r, "sketch_info", json!({"sketch":sketch}))] {
        assert_eq!(summary["world_frame"]["origin"], expected_origin);
        let centre = summary["entities"][0]["points"][0].clone();
        let point = summary["points"].as_array().unwrap().iter().find(|p| p["id"] == centre).unwrap();
        assert_eq!(point["x"], json!(1.2346));
        assert_eq!(point["y"], json!(-2.3457));
    }
}

#[test]
fn input_ready_dimensions_params_and_undo_arguments_keep_precision() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    let diameter = 1.234567891234_f64;
    call(&mut r, "param_set", json!({"name":"diameter","value":diameter}));
    let sketch = call(&mut r, "sketch_create", json!({"plane":"XY"}))["sketch"].clone();
    call(&mut r, "sketch_add", json!({"sketch":sketch,"entities":[{"type":"circle","d":"diameter"}]}));
    let detail = call(&mut r, "sketch_info", json!({"sketch":sketch}));
    // A circle's stored radius is d/2; its driving diameter is the unrounded parameter.
    assert!(
        (detail["entities"][0]["r"].as_f64().unwrap() - diameter / 2.0).abs() <= 8.0 * f64::EPSILON * diameter,
        "radius must retain input-ready precision: {detail}"
    );
    assert_eq!(detail["constraints"].as_array().unwrap().iter().find(|c| c["kind"] == "diameter").unwrap()["value"], json!(diameter));
    assert_eq!(call(&mut r, "doc_info", json!({}))["params"][0]["value"], json!(diameter));
    let value = 9.876543219876_f64;
    call(&mut r, "param_set", json!({"name":"unused","value":value}));
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["arguments"]["value"], json!(value));
}

#[test]
fn curved_feature_bounds_equal_doc_info_after_parameter_rebuild() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name":"h","value":10}));
    let sketch = call(&mut r, "sketch_create", json!({"plane":"XY"}))["sketch"].clone();
    call(&mut r, "sketch_add", json!({"sketch":sketch,"entities":[{"type":"circle","d":7.2}]}));
    let built = call(&mut r, "extrude", json!({"sketch":sketch,"height":"h"}));
    let bbox = built["rebuild"]["bodies"][0]["bbox"].clone();
    call(&mut r, "param_set", json!({"name":"h","value":12}));
    let rebuilt = call(&mut r, "param_set", json!({"name":"h","value":10}));
    assert_eq!(bbox, rebuilt["rebuild"]["bodies"][0]["bbox"]);
    assert_eq!(bbox, call(&mut r, "doc_info", json!({}))["bodies"][0]["bbox"]);
    // Cylinder V=πr²h; exact bounds ±r x/y and [0,h], approximate mesh extrema allowed .005 mm
    // deflection + f32 coordinate rounding + half a fourth-decimal reporting step.
    let expected_volume = std::f64::consts::PI * 3.6_f64.powi(2) * 10.0;
    assert!((built["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - expected_volume).abs() < 1e-6);
    let tolerance = 0.005 + f32::EPSILON as f64 * 10.0 + 0.00005;
    for (actual, expected) in bbox.as_array().unwrap().iter().zip([-3.6, -3.6, 0.0, 3.6, 3.6, 10.0]) {
        assert!((actual.as_f64().unwrap() - expected).abs() <= tolerance);
    }
}

// These literals deliberately assert the four-decimal presentation of FRAC_1_SQRT_2.
#[allow(clippy::approx_constant)]
#[test]
fn serialized_frame_directions_round_without_changing_internal_vectors() {
    let diagonal = 1.0 / 2.0_f64.sqrt();
    let frame = qymcad_engine::SketchWorldFrame {
        origin: [1.23456789, -2.34567891, -0.0],
        x_axis: [diagonal, diagonal, 0.0],
        y_axis: [-diagonal, diagonal, 0.0],
        normal: [0.0, 0.0, 1.0],
    };
    // A 45° in-plane rotation has components ±1/sqrt(2), displayed as ±0.7071.
    let serialized = serde_json::to_value(&frame).unwrap();
    assert_eq!(serialized["origin"], json!([1.2346, -2.3457, 0.0]));
    assert_eq!(serialized["x_axis"], json!([0.7071, 0.7071, 0.0]));
    assert_eq!(serialized["y_axis"], json!([-0.7071, 0.7071, 0.0]));
    assert_eq!(frame.x_axis[0], diagonal, "serialization does not round the internal frame");
    assert_eq!(serialized["origin"][2].as_f64().unwrap().to_bits(), 0.0_f64.to_bits());
}
