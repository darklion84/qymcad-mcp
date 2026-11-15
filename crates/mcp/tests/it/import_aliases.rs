use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Read, path::Path};

fn call(registry: &mut Registry, tool: &str, args: Value) -> Value {
    let content = registry.call(tool, args).unwrap().unwrap_or_else(|error| panic!("{tool}: {error}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

fn fixture() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/fixtures/server-box-10x10x5-inch.igs")
}

fn imported() -> (Registry, Value) {
    let mut registry = Registry::new();
    call(&mut registry, "doc_new", json!({}));
    let imported = call(&mut registry, "import", json!({"path":fixture(),"name":"Imported"}));
    assert_eq!(imported["bodies"][0]["name"], "Imported");
    (registry, imported["bodies"][0].clone())
}

fn saved_state(registry: &mut Registry, path: &Path) -> BTreeMap<String, Vec<u8>> {
    call(registry, "doc_save", json!({"path":path}));
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
fn invalid_import_and_sketch_names_preserve_saved_state_and_undo_history() {
    let (mut registry, _) = imported();
    let path = std::env::temp_dir().join(format!("qymcad-invalid-object-names-{}.qcad", std::process::id()));
    let before = saved_state(&mut registry, &path);
    let doc = call(&mut registry, "doc_info", json!({}));
    for name in [" Imported ", "\u{2003}Imported"] {
        for (tool, args) in [("import", json!({"path":fixture(),"name":name})), ("sketch_create", json!({"plane":"XY","name":name}))] {
            let error = registry.call(tool, args).unwrap().expect_err("invalid object names must be refused");
            assert!(
                error.contains("name") && (error.contains("whitespace") || error.contains("empty") || error.contains("blank")),
                "{error}"
            );
            assert_eq!(call(&mut registry, "doc_info", json!({})), doc, "{tool} with {name:?} changed document state");
            assert_eq!(saved_state(&mut registry, &path), before, "{tool} with {name:?} changed recipe, source or B-reps");
        }
    }
    let undone = call(&mut registry, "undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "import", "refusals added undo entries");
    assert_eq!(call(&mut registry, "doc_info", json!({}))["bodies"], json!([]));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn reported_import_name_resolves_in_topology_select_and_fillet() {
    let (mut registry, imported) = imported();
    let name = &imported["name"];
    let info = call(&mut registry, "doc_info", json!({}));
    assert_eq!(info["bodies"][0]["name"], *name);
    let topology = call(&mut registry, "topology", json!({"body":name}));
    assert_eq!(topology["faces"].as_array().unwrap().len(), 6);
    let selected = call(&mut registry, "select", json!({"body":name,"edges":{"along":"z"}}));
    assert_eq!(selected["body"], imported["body"]);
    assert_eq!(selected["count"], 4);
    let edge = selected["edges"][0]["id"].clone();
    let result = call(&mut registry, "fillet", json!({"body":name,"edges":[edge],"radius":1,"name":"Rounded import"}));
    assert!(result["rebuild"].get("errors").is_none());
    // One vertical R1 edge removes (1 - pi/4) * 5 from the 500 mm³ fixture.
    let expected = 500.0 - (1.0 - std::f64::consts::PI / 4.0) * 5.0;
    let volume = call(&mut registry, "doc_info", json!({}))["bodies"][0]["volume_mm3"].as_f64().unwrap();
    assert!((volume - expected).abs() < 1e-4, "{volume} != {expected}");
}

#[test]
fn reported_import_name_resolves_in_boolean_in_its_own_part() {
    let (mut registry, imported) = imported();
    let name = &imported["name"];
    // Obtain the top face by id first so the failing-before observation specifically reaches
    // boolean operand resolution rather than an earlier inspection call.
    let top = call(&mut registry, "select", json!({"body":imported["body"],"faces":{"facing":"+z"}}))["faces"][0]["id"].clone();
    call(&mut registry, "sketch_create", json!({"plane":{"body":imported["body"],"face":top},"name":"import profile"}));
    call(&mut registry, "sketch_add", json!({"sketch":"import profile","entities":[{"type":"rect","w":2,"h":2}]}));
    call(&mut registry, "extrude", json!({"sketch":"import profile","height":2,"op":"new_body","name":"Boss"}));
    let result = call(&mut registry, "body_boolean", json!({"a":name,"b":"Boss","op":"union","name":"Joined"}));
    assert!(result["rebuild"].get("errors").is_none());
    assert_eq!(result["pieces"].as_array().unwrap().len(), 1);
    assert!((result["pieces"][0]["volume_mm3"].as_f64().unwrap() - 508.0).abs() < 1e-4);
    assert_eq!(call(&mut registry, "select", json!({"body":"Joined","faces":"largest"}))["body"], result["body"]);
}

#[test]
fn batch_can_immediately_target_reported_import_name() {
    let mut registry = Registry::new();
    call(&mut registry, "doc_new", json!({}));
    let result = call(
        &mut registry,
        "batch",
        json!({"verbose":true,"steps":[
            {"tool":"import","args":{"path":fixture(),"name":"Imported"}},
            {"tool":"fillet","args":{"body":"Imported","edges":{"along":"z"},"radius":1,"name":"Rounded import"}}
        ]}),
    );
    assert_eq!(result["steps"][0]["result"]["bodies"][0]["name"], "Imported");
    assert_eq!(result["errors"], json!([]));
    assert_eq!(result["bodies"].as_array().unwrap().len(), 1);
    let expected = 500.0 - 4.0 * (1.0 - std::f64::consts::PI / 4.0) * 5.0;
    assert!((result["bodies"][0]["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-4);
}

#[test]
fn imported_alias_conflicting_with_timeline_name_requires_an_id() {
    let (mut registry, imported) = imported();
    call(&mut registry, "sketch_create", json!({"plane":"XY","name":imported["name"]}));
    let error = registry.call("topology", json!({"body":imported["name"]})).unwrap().unwrap_err();
    assert!(error.contains("several objects") && error.contains("use the id"), "{error}");
    assert_eq!(call(&mut registry, "topology", json!({"body":imported["body"]}))["faces"].as_array().unwrap().len(), 6);
}

#[test]
fn a_blank_import_name_uses_the_default_name_which_resolves() {
    let mut registry = Registry::new();
    call(&mut registry, "doc_new", json!({}));
    let imported = call(&mut registry, "import", json!({"path":fixture(),"name":"  "}));
    let name = imported["bodies"][0]["name"].as_str().unwrap().to_string();
    assert!(!name.trim().is_empty() && name == name.trim(), "blank name reported as {name:?}");
    assert_eq!(call(&mut registry, "topology", json!({"body":name}))["faces"].as_array().unwrap().len(), 6);
}

#[test]
fn tools_default_to_the_only_body_when_it_was_imported_into_its_own_part() {
    let (mut registry, imported) = imported();
    let topology = call(&mut registry, "topology", json!({}));
    assert_eq!(topology["body"], imported["body"], "{topology}");
    let selected = call(&mut registry, "select", json!({"edges":{"along":"z"}}));
    assert_eq!(selected["body"], imported["body"]);
    // A second body in another part makes the default ambiguous: the refusal names the candidates.
    call(&mut registry, "import", json!({"path":fixture(),"name":"Second"}));
    let error = registry.call("topology", json!({})).unwrap().unwrap_err();
    assert!(error.contains("pass body") && error.contains("Second"), "{error}");
}

#[test]
fn a_fillet_on_the_defaulted_imported_body_matches_the_explicit_one() {
    // Not a volume: every vertical R1 edge of the box removes the same volume. The stored recipe (edge reference,
    // owner part, restored active part) and the resulting topology tell them apart. Saved B-rep bytes are not
    // compared: two identical runs already write different bytes.
    let mut saved = Vec::new();
    for explicit in [false, true] {
        let (mut registry, imported) = imported();
        let edge = call(&mut registry, "select", json!({"body":imported["body"],"edges":{"along":"z"}}))["edges"][0]["id"].clone();
        let mut args = json!({"edges":[edge],"radius":1});
        if explicit {
            args["body"] = imported["body"].clone();
        }
        let result = call(&mut registry, "fillet", args);
        assert!(result["rebuild"].get("errors").is_none(), "{result}");
        let bodies = call(&mut registry, "doc_info", json!({}))["bodies"].clone();
        assert_eq!(bodies.as_array().unwrap().len(), 1, "{bodies}");
        assert!(bodies[0]["volume_mm3"].as_f64().unwrap() < 500.0, "{bodies}");
        let path = std::env::temp_dir().join(format!("qymcad-default-fillet-{explicit}-{}.qcad", std::process::id()));
        let recipe = String::from_utf8(saved_state(&mut registry, &path).remove("document.ron").unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        saved.push((recipe, call(&mut registry, "topology", json!({"body":bodies[0]["id"]}))));
    }
    assert_eq!(saved[0].0, saved[1].0, "the defaulted fillet saved a different recipe than the explicit one");
    assert_eq!(saved[0].1, saved[1].1, "the defaulted fillet made different topology than the explicit one");
}
