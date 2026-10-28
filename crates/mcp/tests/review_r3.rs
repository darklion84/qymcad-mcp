//! Review r3: blank optional object names behave exactly like omitted names.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let content = r.call(tool, args.clone()).unwrap().unwrap_or_else(|e| panic!("{tool} {args}: {e}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

fn fresh() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    r
}

fn named(mut args: Value, name: Option<&str>) -> Value {
    if let Some(name) = name {
        args["name"] = json!(name);
    }
    args
}

fn stock(r: &mut Registry) {
    for (name, x) in [("A", 0), ("B", 5)] {
        let sketch = call(r, "sketch_create", json!({"plane":"XY"}))["sketch"].clone();
        call(r, "sketch_add", json!({"sketch":sketch,"entities":[{"type":"rect","cx":x,"w":10,"h":10}]}));
        call(r, "extrude", json!({"sketch":sketch,"height":10,"op":"new_body","name":name}));
    }
}

fn boolean_defaults(op: &str, volume: f64) {
    let mut baseline = fresh();
    stock(&mut baseline);
    let expected = call(&mut baseline, "body_boolean", json!({"a":"A","b":"B","op":op}));
    for name in ["", "  ", "\u{2003}"] {
        let mut r = fresh();
        stock(&mut r);
        let actual = call(&mut r, "body_boolean", json!({"a":"A","b":"B","op":op,"name":name}));
        assert_eq!(actual["name"], expected["name"]);
        assert_eq!(actual["pieces"], expected["pieces"], "blank name {name:?}");
        assert!((actual["pieces"][0]["volume_mm3"].as_f64().unwrap() - volume).abs() < 1e-6);
        let piece = actual["pieces"][0]["name"].clone();
        assert_eq!(call(&mut r, "topology", json!({"body":piece}))["body"], actual["pieces"][0]["id"]);
    }
}

#[test]
fn blank_union_name_uses_default() {
    boolean_defaults("union", 1500.0);
}

#[test]
fn blank_cut_name_uses_default() {
    boolean_defaults("cut", 500.0);
}

#[test]
fn blank_intersect_name_uses_default() {
    boolean_defaults("intersect", 500.0);
}

#[test]
fn blank_sketch_plane_extrude_and_import_names_use_defaults() {
    let mut expected = None;
    for name in [None, Some(""), Some("  "), Some("\u{2003}")] {
        let mut r = fresh();
        let sketch = call(&mut r, "sketch_create", named(json!({"plane":"XY"}), name));
        let plane = call(&mut r, "plane_offset", named(json!({"base":"XY","dist":20}), name));
        call(&mut r, "sketch_add", json!({"sketch":sketch["sketch"],"entities":[{"type":"rect","w":10,"h":10}]}));
        let extrusion = call(&mut r, "extrude", named(json!({"sketch":sketch["sketch"],"height":10}), name));
        let import = call(&mut r, "import", named(json!({"path":"../engine/tests/fixtures/server-box-10x10x5-inch.igs"}), name));
        let actual = json!([sketch, plane, extrusion, import, call(&mut r, "doc_info", json!({}))]);
        if let Some(expected) = &expected {
            assert_eq!(&actual, expected, "blank name {name:?}");
        } else {
            expected = Some(actual);
        }
    }
}

#[test]
fn blank_boolean_in_batch_succeeds_and_undo_restores_operands() {
    for name in ["", "  ", "\u{2003}"] {
        let mut r = fresh();
        stock(&mut r);
        let before = call(&mut r, "doc_info", json!({}));
        let batch = call(
            &mut r,
            "batch",
            json!({"steps":[
                {"tool":"param_set","args":{"name":"probe","value":7}},
                {"tool":"body_boolean","args":{"a":"A","b":"B","op":"union","name":name}}
            ]}),
        );
        assert_eq!(batch["errors"], json!([]));
        assert!((batch["bodies"][0]["volume_mm3"].as_f64().unwrap() - 1500.0).abs() < 1e-6);
        assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "batch");
        assert_eq!(call(&mut r, "doc_info", json!({})), before);
    }
}

#[test]
fn blank_piece_entries_use_defaults_and_reserve_explicit_names() {
    for tool in ["split", "body_boolean"] {
        let mut expected = None;
        for names in [None, Some(json!(["", "  "])), Some(json!(["\u{2003}", ""]))] {
            let mut r = fresh();
            stock(&mut r);
            let mut args = if tool == "split" {
                json!({"body":"A","plane":"XY","offset":5,"name":"Pieces"})
            } else {
                // A narrow cutter divides A into two output bodies.
                let sketch = call(&mut r, "sketch_create", json!({"plane":"XY"}))["sketch"].clone();
                call(&mut r, "sketch_add", json!({"sketch":sketch,"entities":[{"type":"rect","w":2,"h":12}]}));
                call(&mut r, "extrude", json!({"sketch":sketch,"height":10,"op":"new_body","name":"Cutter"}));
                json!({"a":"A","b":"Cutter","op":"cut","name":"Pieces"})
            };
            if let Some(names) = names {
                args["piece_names"] = names;
            }
            let result = call(&mut r, tool, args.clone());
            if let Some(expected) = &expected {
                assert_eq!(&result["pieces"], expected);
            } else {
                expected = Some(result["pieces"].clone());
            }
            assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], tool);
            args["piece_names"] = json!(["", "Pieces 1"]);
            let mixed = call(&mut r, tool, args);
            assert_eq!(mixed["pieces"][0]["name"], "Pieces 2");
            assert_eq!(mixed["pieces"][1]["name"], "Pieces 1");
            for piece in mixed["pieces"].as_array().unwrap() {
                assert_eq!(call(&mut r, "topology", json!({"body":piece["name"]}))["body"], piece["id"]);
            }
        }
    }
}
