//! Round I live-test output contracts.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};
fn call(r: &mut Registry, name: &str, args: Value) -> Value {
    let content = r.call(name, args).unwrap().unwrap();
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}
#[test]
fn duplicate_face_warning_and_flags_survive_protocol_formatting() {
    let mut r = Registry::new();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/fixtures/cone_negative_chamfer.qcad");
    call(&mut r, "doc_open", json!({"path":path}));
    let topology = call(&mut r, "topology", json!({}));
    assert!(topology["warnings"][0].as_str().unwrap().contains("ambiguous"));
    let faces = topology["faces"].as_array().unwrap();
    let duplicated: Vec<_> = faces.iter().filter(|f| f["ambiguous_id"] == true).collect();
    assert_eq!(duplicated.len(), 2 * 2, "two blend rims split into two patches each");
    assert!(faces.iter().all(|f| f["kind"] != "sphere"));
    for f in duplicated {
        let error = r.call("select", json!({"faces":f["id"]})).unwrap().unwrap_err();
        assert!(error.contains("ambiguous") && error.contains("reverse"), "{error}");
    }
    // Unique stock planes still select normally; the solid is usable after refusal.
    assert_eq!(call(&mut r, "select", json!({"faces":{"facing":"+z"}}))["count"], 1);
}

#[test]
fn revolve_description_gives_concrete_conditional_workaround() {
    let tools = Registry::new().list();
    let description = tools.iter().find(|t| t["name"] == "revolve").unwrap()["description"].as_str().unwrap();
    for text in ["sketch's +y", "larger x", "endpoints", "overlapping", "orientation alone", "partial turn"] {
        assert!(description.contains(text), "{text}: {description}");
    }
    assert!(!description.contains("F-067") && !description.contains("negative side"));
}

#[test]
fn bbox_json_uses_one_four_decimal_precision_on_build_open_info_and_undo() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY", "name":"stock"}));
    call(&mut r, "sketch_add", json!({"sketch":"stock", "entities":[{"type":"rect", "w":72.345,"h":7.2}]}));
    let built = call(&mut r, "extrude", json!({"sketch":"stock","height":1.23456}));
    // Centered prism extrema are ±width/2, ±depth/2, [0,height].
    // Decimal reporting rounds 1.23456 to 1.2346; ±36.1725 must retain the fourth decimal.
    let expected = json!([-72.345 / 2.0, -7.2 / 2.0, 0.0, 72.345 / 2.0, 7.2 / 2.0, 1.2346]);
    assert_eq!(built["rebuild"]["bodies"][0]["bbox"], expected, "feature bbox has four decimals");
    assert_eq!(call(&mut r, "doc_info", json!({}))["bodies"][0]["bbox"], expected, "info removes f32 noise");
    call(&mut r, "param_set", json!({"name":"unused","value":1}));
    assert_eq!(call(&mut r, "undo", json!({}))["bodies"][0]["bbox"], expected);
    let path = std::env::temp_dir().join(format!("qymcad-round-i-bbox-{}.qcad", std::process::id()));
    call(&mut r, "doc_save", json!({"path":path}));
    let opened = call(&mut r, "doc_open", json!({"path":path}));
    assert_eq!(opened["doc"]["bodies"][0]["bbox"], expected);
    assert_eq!(opened["rebuild"]["bodies"][0]["bbox"], expected);
}

#[test]
fn bbox_descriptions_explain_decimal_rounding_and_display_caption() {
    let tools = Registry::new().list();
    let info = tools.iter().find(|t| t["name"] == "doc_info").unwrap()["description"].as_str().unwrap();
    for text in ["4 decimal", "half away from zero", "negative zero", "over-bound"] {
        assert!(info.contains(text), "{text}: {info}");
    }
    let render = tools.iter().find(|t| t["name"] == "render").unwrap()["description"].as_str().unwrap();
    assert!(render.contains("caption") && render.contains("rounded") && render.contains("display"));
}

#[test]
fn curved_area_descriptions_do_not_understate_small_face_error() {
    let tools = Registry::new().list();
    for name in ["topology", "sketch_info"] {
        let description = tools.iter().find(|t| t["name"] == name).unwrap()["description"].as_str().unwrap();
        for text in ["tessellation", "up to ~2%", "small curved", "flat faces with curved boundaries", "not a guaranteed bound"] {
            assert!(description.contains(text), "{name} {text}: {description}");
        }
        assert!(!description.contains("0.1–0.2%"), "{description}");
    }
}
