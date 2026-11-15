//! Stage A3 move wire contracts, native reuse, formulas, rollback and history.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Command, Stdio},
};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap_or_else(|e| panic!("{tool}: {e}")).unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}
fn stock() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"profile","entities":[{"type":"rect","w":10,"h":20}]}));
    call(&mut r, "extrude", json!({"sketch":"profile","height":30,"name":"stock"}));
    r
}
fn geometry(r: &mut Registry, expected: [f64; 6]) {
    let body = call(r, "doc_info", json!({}))["bodies"][0].clone();
    assert!((body["volume_mm3"].as_f64().unwrap() - 6000.).abs() < 1e-6, "{body}");
    for (actual, expected) in body["bbox"].as_array().unwrap().iter().zip(expected) {
        assert!((actual.as_f64().unwrap() - expected).abs() < 1e-4, "{body}, expected {expected}");
    }
}
fn archive(r: &mut Registry, path: &Path) -> BTreeMap<String, Vec<u8>> {
    call(r, "doc_save", json!({"path":path}));
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..z.len())
        .map(|i| {
            let mut e = z.by_index(i).unwrap();
            let name = e.name().to_owned();
            let mut bytes = Vec::new();
            e.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}
fn same_archive(actual: &BTreeMap<String, Vec<u8>>, expected: &BTreeMap<String, Vec<u8>>) {
    assert_eq!(actual.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
    for (name, bytes) in expected {
        assert!(actual[name] == *bytes, "archive entry {name} differs");
    }
}

#[test]
fn a3_move_registered_schema_literal_rotation_and_strict_refusals() {
    let mut r = stock();
    let list = r.list();
    let schema = &list.iter().find(|t| t["name"] == "move").expect("move must be registered")["inputSchema"];
    assert_eq!(schema["additionalProperties"], false);
    for field in ["body", "dx", "dy", "dz", "rotation", "name"] {
        assert!(schema["properties"][field].is_object(), "missing {field}");
    }
    for args in [
        json!({}),
        json!({"dx":1,"rotation":{"axis":"z","angle":90}}),
        json!({"rotation":{"axis":"z","angle":"90"}}),
        json!({"rotation":{"axis":"z","angle":90,"pivot":["0",0,0]}}),
        json!({"rotation":{"axis":"z","angle":90,"extra":1}}),
        json!({"rotation":{"axis":"bad","angle":90}}),
        json!({"dx":1,"unknown":1}),
    ] {
        assert!(r.call("move", args.clone()).unwrap().is_err(), "accepted {args}");
    }
    geometry(&mut r, [-5., -10., 0., 5., 10., 30.]);
    call(&mut r, "move", json!({"body":"stock","rotation":{"axis":"z","angle":90},"name":"turned"}).to_string().into());
    geometry(&mut r, [-10., -5., 0., 10., 5., 30.]);
}

#[test]
fn a3_translation_formulas_full_precision_reuse_and_named_dimensions() {
    let mut r = stock();
    call(&mut r, "param_set", json!({"name":"Shift","value":7}));
    let first = call(&mut r, "move", json!({"body":"stock","dx":"Shift+3","dy":-4,"dz":2,"name":"placed"}));
    assert_eq!(first["reused"], false);
    assert_ne!(first["body"], first["source_body"]);
    geometry(&mut r, [5., -14., 2., 15., 6., 32.]);
    call(&mut r, "dimension_set", json!({"target":{"feature":"placed","key":"dx"},"name":"MoveX"}));
    let reused = call(&mut r, "move", json!({"body":first["body"],"dx":0.000001,"dy":"Shift/7"}));
    assert_eq!(reused["reused"], true);
    assert_eq!(reused["body"], first["body"]);
    assert_eq!(reused["source_body"], first["source_body"]);
    assert!((reused["transform"][3].as_f64().unwrap() - 10.000001).abs() < 1e-10, "{reused}");
    call(&mut r, "param_set", json!({"name":"Shift","value":14}));
    geometry(&mut r, [12.000001, -12., 2., 22.000001, 8., 32.]);
    call(&mut r, "dimension_set", json!({"target":{"feature":first["body"],"key":"dz"},"value":3}));
    geometry(&mut r, [12.000001, -12., 3., 22.000001, 8., 33.]);
    assert!(r.call("dimension_set", json!({"target":{"feature":"placed","key":"angle"},"value":45})).unwrap().is_err());
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "dimension_set");
    geometry(&mut r, [12.000001, -12., 2., 22.000001, 8., 32.]);
}

#[test]
fn a3_formula_rotation_refusal_and_batch_rollback_restore_exact_archive() {
    let mut r = stock();
    call(&mut r, "param_set", json!({"name":"shift","value":7}));
    call(&mut r, "move", json!({"dx":"shift+3","name":"placed"}));
    let path = std::env::temp_dir().join(format!("a3-mcp-rollback-{}.qcad", std::process::id()));
    let before = archive(&mut r, &path);
    let error = r.call("move", json!({"rotation":{"axis":"z","angle":90,"pivot":[0,0,0]}})).unwrap().unwrap_err();
    assert_eq!(error,"invalid request: rotation accumulation on a formula-driven Move is unsupported: native accumulate_move adds numeric deltas to stored translation formulas instead of rotating their variable coefficients");
    same_archive(&archive(&mut r, &path), &before);
    assert!(r
        .call("batch", json!({"steps":[{"tool":"move","args":{"dy":4}},{"tool":"move","args":{"rotation":{"axis":"x","angle":30}}}]}))
        .unwrap()
        .is_err());
    same_archive(&archive(&mut r, &path), &before);
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "move");
    geometry(&mut r, [-5., -10., 0., 5., 10., 30.]);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a3_real_stdio_batch_compact_reuse_literal_rotation_and_undo() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_qymcad-mcp"))
        .env("QYMCAD_MCP_LOG", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut request = |id: u64, name: &str, args: Value| -> Value {
        let message = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}});
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], id);
        response
    };
    let payload = |v: &Value| -> Value { serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap() };
    assert_ne!(request(1, "doc_new", json!({}))["result"]["isError"], true);
    let response = request(
        2,
        "batch",
        json!({"steps":[{"tool":"sketch_create","args":{"plane":"XY","name":"profile"}},{"tool":"sketch_add","args":{"sketch":"profile","entities":[{"type":"rect","w":10,"h":20}]}},{"tool":"extrude","args":{"sketch":"profile","height":30}},{"tool":"move","args":{"rotation":{"axis":"z","angle":90},"name":"placed"}},{"tool":"move","args":{"dx":12}},{"tool":"move","args":{"rotation":{"axis":"z","angle":90,"pivot":[0,0,0]}}}]}),
    );
    assert_ne!(response["result"]["isError"], true, "{response}");
    let batch = payload(&response);
    assert_eq!(batch["steps"][3]["reused"], false);
    assert_eq!(batch["steps"][4]["reused"], true);
    assert_eq!(batch["steps"][5]["reused"], true);
    assert_eq!(batch["steps"][3]["created"]["id"], batch["steps"][4]["created"]["id"]);
    // Z90 around its centre, +12X, then Z90 around zero: x[-5,5], y[2,22].
    let body = &batch["bodies"][0];
    assert!((body["volume_mm3"].as_f64().unwrap() - 6000.).abs() < 1e-6);
    for (actual, expected) in body["bbox"].as_array().unwrap().iter().zip([-5., 2., 0., 5., 22., 30.]) {
        assert!((actual.as_f64().unwrap() - expected).abs() < 1e-4, "{body}");
    }
    assert_eq!(request(3, "move", json!({"rotation":{"axis":"z","angle":"90"}}))["result"]["isError"], true);
    assert_eq!(payload(&request(4, "undo", json!({})))["undone"]["tool"], "batch");
    assert_eq!(payload(&request(5, "doc_info", json!({})))["bodies"], json!([]));
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
