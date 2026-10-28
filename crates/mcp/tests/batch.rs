use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let content = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "text");
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

fn error(r: &mut Registry, args: Value) -> String {
    r.call("batch", args).unwrap().unwrap_err()
}

fn fresh() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    r
}

fn step(tool: &str, args: Value) -> Value {
    json!({"tool":tool, "args":args})
}

fn part() -> Vec<Value> {
    let mut steps = Vec::new();
    for (name, value) in [
        ("w", 40),
        ("h", 30),
        ("t", 8),
        ("r", 2),
        ("d", 3),
        ("x1", 8),
        ("x2", 20),
        ("x3", 32),
        ("y", 15),
        ("gap", 4),
        ("lip", 2),
        ("clearance", 1),
        ("pitch", 12),
        ("count", 3),
        ("extra", 6),
    ] {
        steps.push(step("param_set", json!({"name":name,"value":value})));
    }
    steps.extend([
        step("sketch_create", json!({"plane":"XY","name":"outline"})),
        step(
            "sketch_add",
            json!({"sketch":"outline","entities":[{"type":"polyline","closed":true,
            "points":[[0,0],["w",0],["w","h"],[0,"h"]]}]}),
        ),
        step("extrude", json!({"sketch":"outline","height":"t","name":"stock"})),
        step("fillet", json!({"body":"stock","edges":{"along":"z"},"radius":"r","name":"rounded"})),
        step("sketch_create", json!({"plane":"XY","name":"holes"})),
        step(
            "sketch_add",
            json!({"sketch":"holes","entities":[
            {"type":"circle","cx":"x1","cy":"y","d":"d"},
            {"type":"circle","cx":"x2","cy":"y","d":"d"},
            {"type":"circle","cx":"x3","cy":"y","d":"d"}]}),
        ),
        step("extrude", json!({"sketch":"holes","op":"cut","through":true,"target":"rounded","name":"drilled"})),
    ]);
    steps
}

#[test]
fn equivalence_realistic_part_compact_and_verbose() {
    let steps = part();
    let mut single = fresh();
    let results: Vec<_> = steps.iter().map(|s| call(&mut single, s["tool"].as_str().unwrap(), s["args"].clone())).collect();
    let expected = call(&mut single, "doc_info", json!({}));
    let volume =
        40.0 * 30.0 * 8.0 - 4.0 * (1.0 - std::f64::consts::PI / 4.0) * 4.0 * 8.0 - 3.0 * std::f64::consts::PI * 1.5_f64.powi(2) * 8.0;
    assert!((expected["bodies"][0]["volume_mm3"].as_f64().unwrap() - volume).abs() < 1e-5);
    for verbose in [false, true] {
        for split in [false, true] {
            let mut r = fresh();
            let mut entries = Vec::new();
            let chunks = if split { vec![&steps[..15], &steps[15..]] } else { vec![steps.as_slice()] };
            for chunk in chunks {
                let args = if verbose { json!({"steps":chunk,"verbose":true}) } else { json!({"steps":chunk}) };
                let result = call(&mut r, "batch", args);
                assert_eq!(result["ok"], true);
                assert_eq!(result["warnings"], json!([]));
                assert_eq!(result["errors"], json!([]));
                assert_eq!(result["bodies"], call(&mut r, "doc_info", json!({}))["bodies"]);
                for (index, entry) in result["steps"].as_array().unwrap().iter().enumerate() {
                    assert_eq!(entry["step"], index + 1);
                    assert_eq!(entry["tool"], chunk[index]["tool"]);
                }
                entries.extend(result["steps"].as_array().unwrap().iter().cloned());
            }
            assert_eq!(call(&mut r, "doc_info", json!({})), expected, "including ids, params, timeline and exact metrics");
            for (index, entry) in entries.iter().enumerate() {
                if verbose {
                    assert_eq!(entry["result"], results[index], "full standalone result");
                } else {
                    assert!(entry.get("result").is_none());
                    assert!(entry.get("rebuild").is_none());
                }
                if let Some(id) = results[index]["body"].as_u64().or_else(|| results[index]["sketch"].as_u64()) {
                    assert_eq!(entry["created"]["id"], id);
                    let name = expected["timeline"].as_array().unwrap().iter().find(|n| n["id"] == id).unwrap()["name"].clone();
                    assert_eq!(entry["created"]["name"], name);
                } else {
                    assert!(entry.get("created").is_none(), "editing an existing sketch does not create a node");
                }
            }
        }
    }
}

fn loaded(path: &std::path::Path) -> (Value, Vec<(u64, Vec<u8>)>) {
    let (s, rebuild) = qymcad_engine::Session::open(path).unwrap();
    assert!(rebuild.errors.is_empty());
    let mut info = serde_json::to_value(s.info()).unwrap();
    info["path"] = Value::Null;
    let breps = s.result_bodies().iter().map(|b| (b.id, s.shape(b.id).unwrap().to_brep_bytes().unwrap())).collect();
    (info, breps)
}

#[test]
fn failure_step_seven_restores_loaded_state_exact_breps_and_history() {
    for fail in [step("extrude", json!({"sketch":"missing","height":4})), step("fillet", json!({"edges":{"along":"z"},"radius":100}))] {
        let mut r = fresh();
        for s in part() {
            call(&mut r, s["tool"].as_str().unwrap(), s["args"].clone());
        }
        let dir = std::env::temp_dir().join(format!("qymcad-batch-{}-{}", std::process::id(), fail["tool"].as_str().unwrap()));
        std::fs::create_dir_all(&dir).unwrap();
        let before_path = dir.join("before.qcad");
        let after_path = dir.join("after.qcad");
        call(&mut r, "doc_save", json!({"path":before_path}));
        let before = call(&mut r, "doc_info", json!({}));
        let saved = loaded(&before_path);
        let mut steps = vec![
            step("param_set", json!({"name":"t","value":9})),
            step("param_set", json!({"name":"temporary","value":5})),
            step("plane_offset", json!({"base":"XY","dist":10,"name":"temporary datum"})),
            step("sketch_create", json!({"plane":"XY","name":"temporary sketch"})),
            step("sketch_add", json!({"sketch":"temporary sketch","entities":[{"type":"rect","w":2,"h":2}]})),
            step("feature_delete", json!({"feature":"holes","cascade":true})),
        ];
        steps.push(fail.clone());
        steps.extend((0..3).map(|_| step("param_set", json!({"name":"never","value":1}))));
        let message = error(&mut r, json!({"steps":steps}));
        assert!(
            message.contains("step 7") && message.contains(fail["tool"].as_str().unwrap()) && message.contains("nothing was changed"),
            "{message}"
        );
        if fail["tool"] == "extrude" {
            assert!(message.contains("missing"), "{message}");
        } else {
            assert!(message.contains("radius") || message.contains("fillet"), "{message}");
        }
        assert_eq!(call(&mut r, "doc_info", json!({})), before);
        call(&mut r, "doc_save", json!({"path":after_path}));
        assert_eq!(loaded(&after_path), saved, "loaded document and exact B-rep bytes");
        assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "extrude", "failed batch consumes no undo entry");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn prevalidation_step_ten_runs_nothing() {
    let invalid = [
        step("param_set", json!({"name":"bad","value":true})),
        step("param_set", json!({"name":"bad","value":1,"typo":true})),
        step("sketch_add", json!({"sketch":"missing","entities":"[broken"})),
        step("render", json!({})),
    ];
    for last in invalid {
        let mut r = fresh();
        let before = call(&mut r, "doc_info", json!({}));
        let mut steps = vec![step("param_set", json!({"name":"must_not_exist","value":1})); 9];
        steps[0] = step("sketch_create", json!({"plane":"XY","name":"must not run"}));
        steps.push(last.clone());
        let message = error(&mut r, json!({"steps":steps}));
        assert!(message.contains("step 10") && message.contains(last["tool"].as_str().unwrap()), "{message}");
        assert_eq!(call(&mut r, "doc_info", json!({})), before, "validation must precede every handler");
        assert!(r.call("undo", json!({})).unwrap().unwrap_err().contains("nothing to undo"));
        let mut baseline = fresh();
        let expected = call(&mut baseline, "sketch_create", json!({"plane":"XY"}))["sketch"].clone();
        assert_eq!(
            call(&mut r, "sketch_create", json!({"plane":"XY"}))["sketch"],
            expected,
            "even the monotonic allocator must be untouched by prevalidation failure"
        );
    }
}

#[test]
fn limits_allowlist_and_outer_schema() {
    let mut r = fresh();
    for count in [0, 201] {
        let message = error(&mut r, json!({"steps":vec![step("param_set",json!({"name":"n","value":1}));count]}));
        assert!(message.contains("200") && message.contains('1'), "{message}");
    }
    let allowed = [
        "param_set",
        "dimension_set",
        "param_delete",
        "sketch_create",
        "sketch_add",
        "sketch_project",
        "sketch_constrain",
        "sketch_remove",
        "sketch_edit",
        "plane_offset",
        "extrude",
        "revolve",
        "sweep",
        "loft",
        "fillet",
        "chamfer",
        "hole",
        "shell",
        "push_face",
        "linear_array",
        "circular_array",
        "mirror",
        "thread",
        "split",
        "feature_delete",
    ];
    for name in r.list().iter().map(|t| t["name"].as_str().unwrap()).filter(|n| !allowed.contains(n)).chain(["unknown"]) {
        let message = error(&mut r, json!({"steps":[step(name,json!({}))]}));
        assert!(message.contains("step 1") && message.contains(name) && message.contains("not allowed"), "{message}");
    }
    for name in allowed {
        let message = error(&mut r, json!({"steps":[step(name,json!({"unknown_argument":0}))]}));
        assert!(message.contains("step 1") && message.contains(name) && message.contains("bad arguments"), "{message}");
    }
    for args in [
        json!({"steps":[],"typo":1}),
        json!({"steps":[],"verbose":1}),
        json!({}),
        json!({"steps":[{"tool":"param_set","args":{},"typo":1}]}),
    ] {
        assert!(error(&mut r, args).contains("bad arguments"));
    }
    call(&mut r, "batch", json!({"steps":vec![step("param_set",json!({"name":"n","value":1}));200]}));
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["label"], "Batch (200 steps)");
    assert!(r.call("undo", json!({})).unwrap().unwrap_err().contains("nothing to undo"));
}

#[test]
fn mixed_calls_one_undo_per_batch_and_sixteen_call_limit() {
    let mut r = fresh();
    let initial = call(&mut r, "doc_info", json!({}));
    let args = json!({"steps":[step("param_set",json!({"name":"a","value":1})),step("param_set",json!({"name":"b","value":2}))]});
    call(&mut r, "batch", args.clone());
    let after_first = call(&mut r, "doc_info", json!({}));
    call(&mut r, "param_set", json!({"name":"a","value":3}));
    let after_single = call(&mut r, "doc_info", json!({}));
    call(&mut r, "batch", json!({"steps":[step("param_delete",json!({"name":"b"})),step("param_set",json!({"name":"c","value":4}))]}));
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(call(&mut r, "doc_info", json!({})), after_single);
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "param_set");
    assert_eq!(call(&mut r, "doc_info", json!({})), after_first);
    let undone = call(&mut r, "undo", json!({}));
    assert_eq!(undone["undone"], json!({"tool":"batch","label":"Batch (2 steps)","arguments":args}));
    assert_eq!(call(&mut r, "doc_info", json!({})), initial);
    for _ in 0..17 {
        call(&mut r, "batch", args.clone());
    }
    for _ in 0..16 {
        assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "batch");
    }
    assert!(r.call("undo", json!({})).unwrap().unwrap_err().contains("history limit reached (16)"));
}

#[test]
fn encoded_step_arguments_match_standalone_and_id_references_work() {
    let mut r = fresh();
    let mut single = fresh();
    let steps = [
        step("plane_offset", json!({"base":"XY","dist":2,"name":"datum"})),
        step("sketch_create", json!({"plane":"{\"plane\":\"datum\"}","name":"{profile"})),
        step("sketch_add", json!({"sketch":"{profile","entities":"[{\"type\":\"rect\",\"w\":10,\"h\":8}]"})),
        step("extrude", json!({"sketch":"{profile","height":3})),
    ];
    let expected: Vec<_> = steps.iter().map(|s| call(&mut single, s["tool"].as_str().unwrap(), s["args"].clone())).collect();
    let out = call(&mut r, "batch", json!({"steps":steps,"verbose":true}));
    for (entry, result) in out["steps"].as_array().unwrap().iter().zip(expected) {
        assert_eq!(entry["result"], result);
    }
    assert_eq!(call(&mut r, "doc_info", json!({})), call(&mut single, "doc_info", json!({})));
    let mut r = fresh();
    let id = call(&mut r, "sketch_create", json!({"plane":"XY","name":"known"}))["sketch"].clone();
    call(&mut r, "undo", json!({}));
    // Monotonic allocator: discover the next id in a separate fresh document with the same history.
    let next = call(&mut r, "sketch_create", json!({"plane":"XY","name":"existing"}))["sketch"].clone();
    assert!(next.as_u64().unwrap() > id.as_u64().unwrap());
    let out = call(
        &mut r,
        "batch",
        json!({"steps":[
        step("sketch_add",json!({"sketch":next,"entities":[{"type":"rect","w":4,"h":5}]})),
        step("extrude",json!({"sketch":next,"height":2}))]}),
    );
    assert!((out["bodies"][0]["volume_mm3"].as_f64().unwrap() - 40.0).abs() < 1e-6);
    let mut known_ids = fresh();
    let id = call(&mut known_ids, "sketch_create", json!({"plane":"XY"}))["sketch"].clone();
    let mut r = fresh();
    let out = call(
        &mut r,
        "batch",
        json!({"steps":[
        step("sketch_create",json!({"plane":"XY"})),
        step("sketch_add",json!({"sketch":id,"entities":[{"type":"rect","w":4,"h":5}]})),
        step("extrude",json!({"sketch":id,"height":2}))]}),
    );
    assert_eq!(out["steps"][0]["created"]["id"], id);
    assert!((out["bodies"][0]["volume_mm3"].as_f64().unwrap() - 40.0).abs() < 1e-6);
}

#[test]
fn final_warnings_include_live_topology_naming_diagnostics() {
    let mut r = fresh();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/fixtures/cone_negative_chamfer.qcad");
    call(&mut r, "doc_open", json!({"path":path}));
    let result = call(&mut r, "batch", json!({"steps":[step("param_set",json!({"name":"unused","value":1}))],"verbose":true}));
    let warnings = &result["warnings"];
    assert!(
        warnings.as_array().unwrap().iter().any(|w| w["message"].as_str().unwrap().contains("ambiguous")),
        "final batch diagnostics must include naming warnings: {warnings}"
    );
    assert_eq!(warnings, &result["steps"][0]["result"]["rebuild"]["warnings"]);
}

#[test]
fn warnings_restore_on_failure_and_successful_batch_undo() {
    let mut r = fresh();
    let steps = vec![
        step("sketch_create", json!({"plane":"XY","name":"stock profile"})),
        step("sketch_add", json!({"sketch":"stock profile","entities":[{"type":"rect","w":20,"h":20}]})),
        step("extrude", json!({"sketch":"stock profile","height":20,"name":"stock"})),
        step("plane_offset", json!({"base":"XY","dist":10,"name":"inside"})),
        step("sketch_create", json!({"plane":{"plane":"inside"},"name":"pocket profile"})),
        step("sketch_add", json!({"sketch":"pocket profile","entities":[{"type":"rect","w":4,"h":6}]})),
        step("extrude", json!({"sketch":"pocket profile","height":3,"op":"cut","direction":"reverse","name":"pocket"})),
    ];
    for s in steps {
        call(&mut r, s["tool"].as_str().unwrap(), s["args"].clone());
    }
    let before = call(&mut r, "doc_info", json!({}));
    assert!(before["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("sealed internal void")));
    let edit = step("feature_delete", json!({"feature":"inside","cascade":true}));
    let message = error(&mut r, json!({"steps":[edit.clone(),step("extrude",json!({"sketch":"missing","height":1}))]}));
    assert!(message.contains("step 2"), "{message}");
    assert_eq!(call(&mut r, "doc_info", json!({})), before, "restore deleted warnings and exact recipe");
    let result = call(&mut r, "batch", json!({"steps":[edit]}));
    assert_eq!(result["warnings"], json!([]));
    let undone = call(&mut r, "undo", json!({}));
    assert!(undone["warnings"].as_array().unwrap().iter().any(|w| w["message"].as_str().unwrap().contains("sealed internal void")));
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
}
