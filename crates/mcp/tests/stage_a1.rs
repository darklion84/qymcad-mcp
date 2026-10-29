//! Stage A1 wire contracts: native two-sided extrusions and isolated sketch points.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Command, Stdio},
};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}
fn fresh() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    r
}
fn sketch(r: &mut Registry) {
    call(r, "sketch_create", json!({"plane":"XY","name":"profile"}));
}
fn rect(r: &mut Registry) {
    sketch(r);
    call(r, "sketch_add", json!({"sketch":"profile","entities":[{"type":"rect","w":10,"h":6}]}));
}
fn state(r: &mut Registry, path: &Path) -> BTreeMap<String, Vec<u8>> {
    call(r, "doc_save", json!({"path":path}));
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..z.len())
        .map(|i| {
            let mut e = z.by_index(i).unwrap();
            let n = e.name().to_owned();
            let mut b = Vec::new();
            e.read_to_end(&mut b).unwrap();
            (n, b)
        })
        .collect()
}
fn same_archive(actual: &BTreeMap<String, Vec<u8>>, expected: &BTreeMap<String, Vec<u8>>) {
    assert_eq!(actual.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
    for (name, bytes) in expected {
        assert!(actual[name] == *bytes, "archive entry {name} differs ({} vs {} bytes)", actual[name].len(), bytes.len());
    }
}
fn body(r: &mut Registry, volume: f64, bbox: Value) {
    let b = call(r, "doc_info", json!({}))["bodies"][0].clone();
    assert!((b["volume_mm3"].as_f64().unwrap() - volume).abs() < 1e-6, "{b}");
    assert_eq!(b["bbox"], bbox);
}
#[test]
fn two_sides_registry_spans_formulas_named_dimensions_batch_and_undo() {
    for (direction, bbox) in [("normal", json!([-5.0, -3.0, -3.0, 5.0, 3.0, 7.0])), ("reverse", json!([-5.0, -3.0, -7.0, 5.0, 3.0, 3.0]))] {
        let mut r = fresh();
        rect(&mut r);
        call(&mut r, "param_set", json!({"name":"back","value":3}));
        call(&mut r, "extrude", json!({"sketch":"profile","height":7,"down":"back","direction":direction,"name":"two sides"}));
        body(&mut r, 600.0, bbox);
        call(&mut r, "dimension_set", json!({"target":{"feature":"two sides","key":"down"},"name":"second"}));
        let batch = call(&mut r, "batch", json!({"steps":[{"tool":"param_set","args":{"name":"back","value":5}}]}));
        assert!((batch["bodies"][0]["volume_mm3"].as_f64().unwrap() - 720.0).abs() < 1e-6);
        assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "batch");
        body(
            &mut r,
            600.0,
            if direction == "normal" { json!([-5.0, -3.0, -3.0, 5.0, 3.0, 7.0]) } else { json!([-5.0, -3.0, -7.0, 5.0, 3.0, 3.0]) },
        );
    }
}
#[test]
fn two_sides_refusals_preserve_exact_archive_and_undo() {
    let mut r = fresh();
    rect(&mut r);
    call(&mut r, "param_set", json!({"name":"back","value":3}));
    call(&mut r, "extrude", json!({"sketch":"profile","height":7,"down":"back","name":"solid"}));
    let path = std::env::temp_dir().join(format!("a1-down-{}.qcad", std::process::id()));
    let before = state(&mut r, &path);
    for (tool, args) in [
        ("extrude", json!({"sketch":"profile","height":7,"down":3,"direction":"symmetric"})),
        ("extrude", json!({"sketch":"profile","height":7,"down":3,"through":true})),
        ("extrude", json!({"sketch":"profile","height":7,"down":0})),
        ("extrude", json!({"sketch":"profile","height":7,"down":1e-9})),
        ("param_set", json!({"name":"back","value":-1})),
        ("dimension_set", json!({"target":{"feature":"solid","key":"down"},"value":0})),
        (
            "batch",
            json!({"steps":[{"tool":"param_set","args":{"name":"back","value":5}},{"tool":"dimension_set","args":{"target":{"feature":"solid","key":"down"},"value":1e-10}}]}),
        ),
    ] {
        assert!(r.call(tool, args.clone()).unwrap().is_err(), "accepted {tool} {args}");
        same_archive(&state(&mut r, &path), &before);
    }
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "extrude");
    std::fs::remove_file(path).unwrap();
}
#[test]
fn point_defaults_reuse_free_coordinates_and_independent_named_dimensions() {
    let mut r = fresh();
    sketch(&mut r);
    let p = call(&mut r, "sketch_add", json!({"sketch":"profile","entities":[{"type":"point"}]}));
    let id = p["created"][0]["point"].clone();
    assert!(id.is_number());
    assert_eq!(p["sketch"]["isolated_points"], json!([{"id":id,"x":0.0,"y":0.0,"dimensioned":true}]));
    let constraints = p["sketch"]["constraints"].clone();
    let reused = call(&mut r, "sketch_add", json!({"sketch":"profile","entities":[{"type":"point"}]}));
    assert_eq!(reused["created"][0]["point"], id);
    assert_eq!(reused["created"][0]["reused"], true);
    assert_eq!(reused["sketch"]["constraints"], constraints);
    call(&mut r, "param_set", json!({"name":"px","value":4}));
    let p = call(
        &mut r,
        "sketch_add",
        json!({"sketch":"profile","entities":[{"type":"point","x":"px","y":3},{"type":"point","x":8,"y":9,"dimensioned":false}]}),
    );
    let point = p["created"][0]["point"].clone();
    assert_eq!(p["sketch"]["entities"], json!([]));
    assert_eq!(p["sketch"]["contours"], json!([]));
    let dims: Vec<_> = p["sketch"]["constraints"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["points"].as_array().unwrap().contains(&point) && c["value"].is_number())
        .map(|c| c["index"].clone())
        .collect();
    assert_eq!(dims.len(), 2);
    for (i, name) in dims.iter().zip(["point_x", "point_y"]) {
        call(&mut r, "dimension_set", json!({"target":{"sketch":"profile","constraint":i},"name":name}));
    }
    let info = call(&mut r, "sketch_info", json!({"sketch":"profile"}));
    assert_eq!(info["isolated_points"][2]["dimensioned"], false);
    call(&mut r, "param_set", json!({"name":"px","value":6}));
    let info = call(&mut r, "sketch_info", json!({"sketch":"profile"}));
    let p = info["isolated_points"].as_array().unwrap().iter().find(|p| p["id"] == point).unwrap();
    assert_eq!(p["x"], 6.0);
    assert_eq!(p["y"], 3.0);
    assert_eq!(p["dimensioned"], true);
    assert!(r.call("param_set", json!({"name":"px","value":-1})).unwrap().is_err());
    assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"profile"})), info);
    let e = r.call("extrude", json!({"sketch":"profile","height":3})).unwrap().unwrap_err();
    assert!(e.contains("contour") || e.contains("profile"), "{e}");
    let e = r.call("revolve", json!({"sketch":"profile","axis":"y"})).unwrap().unwrap_err();
    assert!(e.contains("contour") || e.contains("profile") || e.contains("axis"), "{e}");
}
#[test]
fn point_remove_batch_undo_and_refusals_preserve_archive() {
    let mut r = fresh();
    rect(&mut r);
    call(&mut r, "extrude", json!({"sketch":"profile","height":7,"down":3}));
    let p = call(&mut r, "sketch_add", json!({"sketch":"profile","entities":[{"type":"point","x":20,"y":20}]}));
    let id = p["created"][0]["point"].clone();
    let used = p["sketch"]["entities"][0]["points"][0].clone();
    let entity = p["sketch"]["entities"][0]["id"].clone();
    let origin = p["sketch"]["points"].as_array().unwrap().iter().find(|p| p["role"] == "origin").unwrap()["id"].clone();
    let path = std::env::temp_dir().join(format!("a1-point-{}.qcad", std::process::id()));
    let before = state(&mut r, &path);
    for args in [
        json!({"sketch":"profile","point":used}),
        json!({"sketch":"profile","point":origin}),
        json!({"sketch":"profile","point":99999999}),
        json!({"sketch":"profile","point":id,"entity":entity}),
        json!({"sketch":"profile","point":id,"constraint":0}),
    ] {
        let e = r.call("sketch_remove", args.clone()).unwrap().unwrap_err();
        if args["point"] == used {
            assert!(e.contains(&entity.to_string()), "entity to remove is missing: {e}");
        }
        same_archive(&state(&mut r, &path), &before);
    }
    let refused = r
        .call(
            "batch",
            json!({"steps":[
                {"tool":"sketch_remove","args":{"sketch":"profile","point":id}},
                {"tool":"sketch_remove","args":{"sketch":"profile","point":used}}
            ]}),
        )
        .unwrap()
        .unwrap_err();
    assert!(refused.contains("step 2"), "{refused}");
    same_archive(&state(&mut r, &path), &before);
    let failed = r
        .call(
            "batch",
            json!({"steps":[
                {"tool":"sketch_remove","args":{"sketch":"profile","point":id}},
                {"tool":"sketch_add","args":{"sketch":"profile","entities":[{"type":"point","x":50,"y":50}]}},
                {"tool":"sketch_remove","args":{"sketch":"profile","point":used}}
            ]}),
        )
        .unwrap()
        .unwrap_err();
    assert!(failed.contains("step 3"), "{failed}");
    let after_allocating_failure = state(&mut r, &path);
    // ADR 0020/F-056 explicitly retains allocated ids even on failed batches. Every other
    // byte of the saved recipe and every B-rep must still match, including removed point rows.
    let old = std::str::from_utf8(&before["document.ron"]).unwrap();
    let new = std::str::from_utf8(&after_allocating_failure["document.ron"]).unwrap();
    let allocation = |text: &str| -> (String, u64) {
        let lines: Vec<_> = text.lines().filter(|line| line.trim_start().starts_with("next_id:")).collect();
        assert_eq!(lines.len(), 1);
        let id = lines[0].trim().strip_prefix("next_id:").unwrap().trim().trim_end_matches(',').parse().unwrap();
        (lines[0].to_owned(), id)
    };
    let (old_line, old_id) = allocation(old);
    let (new_line, new_id) = allocation(new);
    assert!(new_id > old_id, "a failed allocating batch preserves the monotonic allocator");
    let mut normalized = after_allocating_failure.clone();
    normalized.insert("document.ron".into(), new.replacen(&new_line, &old_line, 1).into_bytes());
    same_archive(&normalized, &before);
    let before = after_allocating_failure;
    let original_sketch = call(&mut r, "sketch_info", json!({"sketch":"profile"}));
    let original_bodies = call(&mut r, "doc_info", json!({}))["bodies"].clone();
    call(
        &mut r,
        "batch",
        json!({"steps":[{"tool":"sketch_remove","args":{"sketch":"profile","point":id}},{"tool":"sketch_add","args":{"sketch":"profile","entities":[{"type":"point","x":30,"y":30}]}}]}),
    );
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "batch");
    // Undo preserves the monotonic id allocator (F-056); compare restored model and B-reps.
    assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"profile"})), original_sketch);
    assert_eq!(call(&mut r, "doc_info", json!({}))["bodies"], original_bodies);
    let after_batch = state(&mut r, &path);
    for (name, bytes) in &before {
        if name != "document.ron" {
            assert!(after_batch[name] == *bytes, "undo changed {name}");
        }
    }
    call(&mut r, "sketch_remove", json!({"sketch":"profile","point":id}));
    let remaining = call(&mut r, "sketch_info", json!({"sketch":"profile"}));
    // Native rectangle anchor points may also be isolated. Remove only the requested marker.
    let expected: Vec<_> = original_sketch["isolated_points"].as_array().unwrap().iter().filter(|p| p["id"] != id).cloned().collect();
    assert_eq!(remaining["isolated_points"].as_array().cloned().unwrap_or_default(), expected);
    call(&mut r, "undo", json!({}));
    same_archive(&state(&mut r, &path), &after_batch);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn stage_a1_schema_and_unknown_fields_are_strict() {
    let r = Registry::new();
    let list = r.list();
    let tools = &list;
    let ex = tools.iter().find(|t| t["name"] == "extrude").unwrap();
    assert!(ex["inputSchema"]["properties"]["down"].is_object());
    let rm = tools.iter().find(|t| t["name"] == "sketch_remove").unwrap();
    assert!(rm["inputSchema"]["properties"]["point"].is_object());
    let mut r = fresh();
    sketch(&mut r);
    for entity in [json!({"type":"point","x":1,"y":2,"construction":true}), json!({"type":"point","x":1,"y":2,"unknown":true})] {
        assert!(r.call("sketch_add", json!({"sketch":"profile","entities":[entity]})).unwrap().unwrap_err().contains("unknown field"));
    }
}
#[test]
fn stage_a1_real_stdio_batch_and_errors() {
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"a1","version":"0"}}}),
        rpc(2, "doc_new", json!({})),
        rpc(
            3,
            "batch",
            json!({"verbose":true,"steps":[
                {"tool":"sketch_create","args":{"plane":"XY","name":"wire"}},
                {"tool":"sketch_add","args":{"sketch":"wire","entities":[{"type":"point","x":20,"y":30},{"type":"rect","w":10,"h":6}]}},
                {"tool":"extrude","args":{"sketch":"wire","height":7,"down":3,"direction":"reverse"}}
            ]}),
        ),
        rpc(4, "extrude", json!({"sketch":"wire","height":7,"down":0})),
        rpc(5, "undo", json!({})),
        rpc(6, "doc_info", json!({})),
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_qymcad-mcp"))
        .env("QYMCAD_MCP_LOG", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut request = |message: &Value| -> Value {
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], message["id"]);
        response
    };
    let payload = |v: &Value| -> Value { serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap() };
    let mut responses = Vec::new();
    for (i, message) in messages.iter().enumerate() {
        let response = request(message);
        if i == 2 {
            let batch = payload(&response);
            let sketch = &batch["steps"][1]["result"]["sketch"];
            let marker = batch["steps"][1]["result"]["created"][0]["point"].clone();
            let endpoint = sketch["entities"][0]["points"][0].clone();
            let refused = request(&rpc(100, "sketch_remove", json!({"sketch":"wire","point":endpoint})));
            assert_eq!(refused["result"]["isError"], true);
            assert_eq!(payload(&request(&rpc(101, "sketch_info", json!({"sketch":"wire"})))), *sketch);
            let removed = request(&rpc(102, "sketch_remove", json!({"sketch":"wire","point":marker})));
            assert_ne!(removed["result"]["isError"], true, "{removed}");
            assert!(!payload(&removed)["sketch"]["points"].as_array().unwrap().iter().any(|p| p["id"] == marker));
            let undone = payload(&request(&rpc(103, "undo", json!({}))));
            assert_eq!(undone["undone"]["tool"], "sketch_remove");
            assert_eq!(payload(&request(&rpc(104, "sketch_info", json!({"sketch":"wire"})))), *sketch);
        }
        responses.push(response);
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(responses.len(), messages.len());
    assert_ne!(responses[2]["result"]["isError"], true, "{}", responses[2]);
    let batch: Value = serde_json::from_str(responses[2]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!((batch["bodies"][0]["volume_mm3"].as_f64().unwrap() - 600.0).abs() < 1e-6);
    assert_eq!(batch["bodies"][0]["bbox"], json!([-5.0, -3.0, -7.0, 5.0, 3.0, 3.0]));
    assert_eq!(batch["steps"][1]["result"]["created"][0]["type"], "point");
    assert_eq!(responses[3]["result"]["isError"], true);
    assert!(responses[3]["error"].is_null());
    let undo: Value = serde_json::from_str(responses[4]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(undo["undone"]["tool"], "batch");
    let info: Value = serde_json::from_str(responses[5]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(info["sketches"], json!([]));
    assert_eq!(info["bodies"], json!([]));
}
fn rpc(id: u64, name: &str, args: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}})
}
