use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, name: &str, args: Value) -> Value {
    let content = r.call(name, args).unwrap().unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

fn stock(r: &mut Registry) {
    call(r, "doc_new", json!({}));
    call(r, "param_set", json!({"name":"height", "value":6}));
    call(r, "sketch_create", json!({"plane":"XY", "name":"profile"}));
    call(r, "sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":20, "h":30}]}));
    call(r, "extrude", json!({"sketch":"profile", "height":"height", "name":"stock"}));
}

fn path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("qymcad-save-lineage-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn a_different_model_after_undo_or_delete_cannot_use_the_previous_save_path() {
    for operation in ["undo", "feature_delete"] {
        let mut r = Registry::new();
        stock(&mut r);
        let path = path(&format!("different-{operation}.qcad"));
        call(&mut r, "doc_save", json!({"path":path}));
        let saved = std::fs::read(&path).unwrap();
        if operation == "undo" {
            call(&mut r, "undo", json!({}));
        } else {
            call(&mut r, "feature_delete", json!({"feature":"stock"}));
        }
        call(&mut r, "extrude", json!({"sketch":"profile", "height":9, "name":"replacement"}));
        let error = r.call("doc_save", json!({})).unwrap().expect_err("different model must not overwrite the previous save path");
        assert!(error.contains("different model") && error.contains("overwrite=true") && error.contains("path"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), saved, "refusal preserves the saved file");
        // Explicit intent permits replacement and establishes the replacement's association.
        call(&mut r, "doc_save", json!({"overwrite":true}));
        call(&mut r, "doc_save", json!({}));
        call(&mut r, "doc_open", json!({"path":path}));
        let info = call(&mut r, "doc_info", json!({}));
        assert_eq!(info["bodies"][0]["name"], "replacement");
    }
}

#[test]
fn normal_edits_and_reopened_edits_can_save_without_a_path() {
    let mut r = Registry::new();
    stock(&mut r);
    let path = path("normal.qcad");
    call(&mut r, "doc_save", json!({"path":path}));
    call(&mut r, "param_set", json!({"name":"height", "value":8}));
    call(&mut r, "doc_save", json!({}));
    call(&mut r, "doc_open", json!({"path":path}));
    call(&mut r, "param_set", json!({"name":"height", "value":10}));
    call(&mut r, "doc_save", json!({}));
    call(&mut r, "doc_open", json!({"path":path}));
    // V = width * depth * height, preserving identity across parameter edits.
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(info["bodies"][0]["volume_mm3"], 20.0 * 30.0 * 10.0);
}

#[test]
fn doc_new_needs_a_path_even_after_another_document_was_saved() {
    let mut r = Registry::new();
    stock(&mut r);
    call(&mut r, "doc_save", json!({"path":path("new.qcad")}));
    call(&mut r, "doc_new", json!({}));
    for args in [json!({}), json!({"overwrite":true})] {
        let error = r.call("doc_save", args).unwrap().unwrap_err();
        assert!(error.contains("no file yet; give a path"), "{error}");
    }
}

#[test]
fn saving_an_initially_empty_document_records_its_first_model_lineage() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    let path = path("initially-empty.qcad");
    call(&mut r, "doc_save", json!({"path":path}));
    call(&mut r, "sketch_create", json!({"plane":"XY", "name":"profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":20, "h":30}]}));
    call(&mut r, "extrude", json!({"sketch":"profile", "height":6}));
    call(&mut r, "doc_save", json!({}));
    call(&mut r, "undo", json!({}));
    call(&mut r, "extrude", json!({"sketch":"profile", "height":9}));
    assert!(r.call("doc_save", json!({})).unwrap().unwrap_err().contains("different model"));
    // An explicit path is also deliberate permission to establish the replacement's identity.
    call(&mut r, "doc_save", json!({"path":path}));
    call(&mut r, "doc_save", json!({}));
}

#[test]
fn extrude_description_documents_internal_entry_clearance() {
    let r = Registry::new();
    let tools = r.list();
    let description = tools.iter().find(|t| t["name"] == "extrude").unwrap()["description"].as_str().unwrap();
    assert!(
        description.contains("0.001 mm") && description.contains("internal"),
        "extrude must document the 0.001 mm internal-plane entry clearance"
    );
}
