use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, name: &str, args: Value) -> Value {
    let content = r.call(name, args).unwrap_or_else(|e| panic!("{e}")).unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}
fn stock(r: &mut Registry) -> u64 {
    call(r, "doc_new", json!({}));
    call(r, "param_set", json!({"name":"h", "value":20}));
    call(r, "sketch_create", json!({"plane":"XY", "name":"profile"}));
    call(r, "sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":20, "h":20}]}));
    call(r, "extrude", json!({"sketch":"profile", "height":"h", "name":"stock"}))["body"].as_u64().unwrap()
}
#[test]
fn deleting_the_last_feature_and_undo_are_tools() {
    let mut r = Registry::new();
    stock(&mut r);
    let before = call(&mut r, "doc_info", json!({}));
    call(&mut r, "feature_delete", json!({"feature":"stock"}));
    assert!(call(&mut r, "doc_info", json!({}))["bodies"].as_array().unwrap().is_empty());
    call(&mut r, "undo", json!({}));
    assert_eq!(call(&mut r, "doc_info", json!({})), before, "undo restores document");
}
#[test]
fn dependent_deletion_is_refused_unless_cascade_is_set() {
    let mut r = Registry::new();
    stock(&mut r);
    let before = call(&mut r, "doc_info", json!({}));
    let error = r.call("feature_delete", json!({"feature":"profile"})).unwrap().unwrap_err();
    assert!(error.contains("depend") && error.contains("cascade"), "{error}");
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
    call(&mut r, "feature_delete", json!({"feature":"profile", "cascade":true}));
    assert!(call(&mut r, "doc_info", json!({}))["bodies"].as_array().unwrap().is_empty());
}

fn saved_breps(path: &std::path::Path) -> Vec<(u64, Vec<u8>)> {
    let (s, report) = qymcad_engine::Session::open(path).unwrap();
    assert!(report.errors.is_empty());
    s.result_bodies().iter().map(|b| (b.id, s.shape(b.id).unwrap().to_brep_bytes().unwrap())).collect()
}

#[test]
fn modelling_tools_undo_exact_breps_after_failed_read_and_save_calls() {
    for operation in ["extrude", "fillet", "param_set"] {
        let mut r = Registry::new();
        stock(&mut r);
        let dir = std::env::temp_dir().join(format!("qymcad-mcp-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{operation}.qcad"));
        call(&mut r, "doc_save", json!({"path":path}));
        let before = call(&mut r, "doc_info", json!({}));
        let before_breps = saved_breps(&path);
        let (args, volume) = match operation {
            "extrude" => (json!({"sketch":"profile", "height":22}), 20.0 * 20.0 * 22.0),
            // Four vertical corners: remove 4*(square-quarter circle)*height.
            "fillet" => (
                json!({"edges":{"along":"z"}, "radius":2}),
                20.0_f64.powi(3) - 4.0 * (1.0 - std::f64::consts::PI / 4.0) * 2.0_f64.powi(2) * 20.0,
            ),
            _ => (json!({"name":"h", "value":25}), 20.0 * 20.0 * 25.0),
        };
        call(&mut r, operation, args);
        let after = call(&mut r, "doc_info", json!({}));
        assert!((after["bodies"][0]["volume"].as_f64().unwrap() - volume).abs() < 1e-6);
        assert_ne!(after["bodies"], before["bodies"]);
        let failure = r.call("fillet", json!({"edges":{"along":"z"}, "radius":100})).unwrap().unwrap_err();
        assert!(failure.contains("rolled back"), "{failure}");
        assert_eq!(call(&mut r, "doc_info", json!({})), after, "failed calls preserve state");
        call(&mut r, "topology", json!({}));
        call(&mut r, "doc_save", json!({}));
        call(&mut r, "undo", json!({}));
        assert_eq!(call(&mut r, "doc_info", json!({})), before, "{operation}: undo restores the modelling call");
        call(&mut r, "doc_save", json!({}));
        assert_eq!(saved_breps(&path), before_breps, "{operation}: exact B-rep bytes after MCP undo");
    }
}
