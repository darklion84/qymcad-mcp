//! Opaque Axis children preserve JSON-looking literal body names.

use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, name: &str, args: Value) -> Value {
    let content = r.call(name, args).unwrap().unwrap();
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn opaque_axis_body_names_are_literal_in_native_and_encoded_arguments() {
    for name in ["{x}", "[x]", "{\"x\":1}", "[1]"] {
        for encoded in [false, true] {
            let mut r = Registry::new();
            call(&mut r, "doc_new", json!({}));
            call(&mut r, "sketch_create", json!({"plane":"XY", "name":"axis profile"}));
            call(&mut r, "sketch_add", json!({"sketch":"axis profile", "entities":[{"type":"circle", "d":4}]}));
            call(&mut r, "extrude", json!({"sketch":"axis profile", "height":3, "name":name}));
            let picked = call(&mut r, "select", json!({"body":name, "faces":{"kind":"cylinder"}}));
            assert_eq!(picked["count"], 1);
            let axis = json!({"body":name, "face":picked["faces"][0]["id"]});

            call(&mut r, "sketch_create", json!({"plane":"XY", "name":"seed profile"}));
            call(&mut r, "sketch_add", json!({"sketch":"seed profile", "entities":[{"type":"circle", "cx":10, "d":2}]}));
            let seed = call(&mut r, "extrude", json!({"sketch":"seed profile", "height":3, "op":"new_body"}));
            let axis = if encoded { json!(axis.to_string()) } else { axis };
            let array = call(&mut r, "circular_array", json!({"body":seed["body"], "count":2, "axis":axis}));
            let body = array["rebuild"]["bodies"].as_array().unwrap().iter().find(|b| b["id"] == array["body"]).unwrap();
            // Two Ø2 × 3 cylinders at (±10, 0): V = 2πr²h, bounds x=±(10+r), y=±r, z=[0,h].
            let expected_volume = 2.0 * std::f64::consts::PI * 1.0_f64.powi(2) * 3.0;
            assert!((body["volume_mm3"].as_f64().unwrap() - expected_volume).abs() < 1e-6, "{name}, encoded={encoded}: {body}");
            let tolerance = 0.005 + f32::EPSILON as f64 * 11.0 + 0.00005;
            for (actual, expected) in body["bbox"].as_array().unwrap().iter().zip([-11.0, -1.0, 0.0, 11.0, 1.0, 3.0]) {
                assert!((actual.as_f64().unwrap() - expected).abs() <= tolerance, "{name}, encoded={encoded}: {body}");
            }
        }
    }
}
