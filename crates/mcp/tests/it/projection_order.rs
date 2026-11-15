//! Forward sources must be refused through the actual modelling-tool handler.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Read, path::Path};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let content = r.call(tool, args).unwrap().unwrap_or_else(|error| panic!("{tool}: {error}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

fn saved_state(r: &mut Registry, path: &Path) -> BTreeMap<String, Vec<u8>> {
    call(r, "doc_save", json!({"path":path}));
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        entries.insert(entry.name().to_owned(), bytes);
    }
    assert!(entries.contains_key("document.ron"));
    assert!(entries.iter().any(|(name, bytes)| name.starts_with("brep/") && !bytes.is_empty()));
    entries
}

#[test]
fn r1_1_forward_projection_tool_refuses_edge_and_face_without_history_changes() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"early"}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"profile","entities":[{"type":"rect","w":20,"h":10}]}));
    call(&mut r, "extrude", json!({"sketch":"profile","height":10,"name":"body"}));
    let edge = call(&mut r, "select", json!({"body":"body","edges":{"edges_of":{"facing":"+z"}}}))["edges"][0]["id"].clone();
    let face = call(&mut r, "select", json!({"body":"body","faces":{"facing":"+z"}}))["faces"][0]["id"].clone();
    let path = std::env::temp_dir().join(format!("qymcad-r1-forward-{}.qcad", std::process::id()));
    let before = saved_state(&mut r, &path);
    let doc = call(&mut r, "doc_info", json!({}));
    let sketch = call(&mut r, "sketch_info", json!({"sketch":"early"}));
    for args in [json!({"sketch":"early","body":"body","edges":[edge]}), json!({"sketch":"early","body":"body","face":face})] {
        let error = r.call("sketch_project", args).unwrap().unwrap_err();
        assert!(error.contains("body must be built above the sketch"), "{error}");
        assert_eq!(saved_state(&mut r, &path), before, "refusal changed saved recipe or B-reps");
        assert_eq!(call(&mut r, "doc_info", json!({})), doc);
        assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"early"})), sketch);
    }
    let undone = call(&mut r, "undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "extrude", "refusals added undo entries");
    std::fs::remove_file(path).unwrap();
}
