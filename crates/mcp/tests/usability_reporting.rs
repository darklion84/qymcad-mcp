//! Reporting descriptions expose the coordinate and file-system conventions agents need.

use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, name: &str, args: Value) -> Value {
    let content = r.call(name, args).unwrap().unwrap();
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn deletion_reports_every_removed_node_and_preserves_rebuild_result() {
    for cascade in [false, true] {
        let mut r = Registry::new();
        call(&mut r, "doc_new", json!({}));
        let sketch = call(&mut r, "sketch_create", json!({"plane":"XY", "name":"profile"}))["sketch"].clone();
        call(&mut r, "sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":20, "h":16}]}));
        let body = call(&mut r, "extrude", json!({"sketch":"profile", "height":10, "name":"stock"}))["body"].clone();
        let fillet = call(&mut r, "fillet", json!({"body":"stock", "edges":{"along":"z"}, "radius":2, "name":"rounded"}))["body"].clone();
        let target = if cascade { "profile" } else { "rounded" };
        let deleted = call(&mut r, "feature_delete", json!({"feature":target, "cascade":cascade}));
        let expected = if cascade {
            json!([
                {"id":sketch, "name":"profile", "kind":"sketch", "suppressed":false},
                {"id":body, "name":"stock", "kind":"extrude", "suppressed":false},
                {"id":fillet, "name":"rounded", "kind":"fillet", "suppressed":false}
            ])
        } else {
            json!([{"id":fillet, "name":"rounded", "kind":"fillet", "suppressed":false}])
        };
        assert_eq!(deleted["deleted"], expected, "all deleted nodes have ids, names and kinds");
        assert!(deleted.get("warnings").is_none());
        if cascade {
            assert_eq!(deleted["bodies"], json!([]));
        } else {
            // Deleting the leaf restores the rectangular stock: width * depth * height.
            let volume = deleted["bodies"][0]["volume_mm3"].as_f64().unwrap();
            assert!((volume - 20.0 * 16.0 * 10.0).abs() < 1e-6, "restored stock volume: {volume}");
        }
    }
}

#[test]
fn deletion_description_explains_refusal_without_app_relinking() {
    let tools = Registry::new().list();
    let description = tools.iter().find(|t| t["name"] == "feature_delete").unwrap()["description"].as_str().unwrap();
    for required in ["dependents", "refused", "unlike the app", "not relinked", "cascade", "leaf", "ids", "names", "kinds"] {
        assert!(description.contains(required), "feature_delete must explain {required}: {description}");
    }
}

#[test]
fn reporting_descriptions_explain_frames_save_directory_and_bbox_padding() {
    let tools = Registry::new().list();
    for (name, required) in [
        ("sketch_create", "world origin projected onto the face plane"),
        ("sketch_create", "x/y directions"),
        ("sketch_info", "world_frame"),
        ("doc_save", "directory must exist"),
        ("doc_info", "OCCT tolerance padding"),
        ("doc_info", "doc_open"),
    ] {
        let description = tools.iter().find(|t| t["name"] == name).unwrap()["description"].as_str().unwrap();
        assert!(description.contains(required), "{name} must explain {required}: {description}");
    }
}
