//! A4 sketch-hole wire contract, per-site validation, atomic batches and real stdio.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Command, Stdio},
};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let content = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}
fn fixture() -> (Registry, Vec<Value>) {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name":"pitch","value":8}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"profile","entities":[{"type":"rect","w":40,"h":30}]}));
    call(&mut r, "extrude", json!({"sketch":"profile","height":10,"name":"stock"}));
    call(&mut r, "plane_offset", json!({"base":"XY","dist":10,"name":"top"}));
    call(&mut r, "sketch_create", json!({"plane":{"plane":"top"},"name":"markers"}));
    let added = call(
        &mut r,
        "sketch_add",
        json!({"sketch":"markers","entities":[{"type":"point","x":"pitch","y":6},{"type":"point","x":-8,"y":-6}]}),
    );
    let ids = added["created"].as_array().unwrap().iter().map(|v| v["point"].clone()).collect();
    (r, ids)
}
fn volume(r: &mut Registry, expected: f64) {
    let body = call(r, "doc_info", json!({}))["bodies"][0].clone();
    assert!((body["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6, "{body}");
}
fn bores(r: &mut Registry, expected_x: f64) {
    let topo = call(r, "topology", json!({}));
    let faces: Vec<_> = topo["faces"].as_array().unwrap().iter().filter(|f| f["kind"] == "cylinder").collect();
    assert_eq!(faces.len(), 2, "{topo}");
    for [x, y, z] in [[expected_x, 6., 5.], [-8., -6., 5.]] {
        assert!(
            faces.iter().any(|f| f["centroid"]
                .as_array()
                .unwrap()
                .iter()
                .zip([x, y, z])
                .all(|(a, e)| (a.as_f64().unwrap() - e).abs() < 1e-4)),
            "{topo}"
        );
    }
}
fn archive(r: &mut Registry, path: &Path) -> BTreeMap<String, Vec<u8>> {
    call(r, "doc_save", json!({"path":path}));
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut entry = zip.by_index(i).unwrap();
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}
fn same_archive(a: &BTreeMap<String, Vec<u8>>, b: &BTreeMap<String, Vec<u8>>) {
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for (name, bytes) in b {
        assert!(a[name] == *bytes, "archive entry {name} differs");
    }
}

#[test]
fn hole_sketch_schema_and_branch_refusals_preserve_archive() {
    let (mut r, ids) = fixture();
    let tools = r.list();
    let hole = tools.iter().find(|t| t["name"] == "hole").unwrap();
    let schema = &hole["inputSchema"];
    assert_eq!(schema["additionalProperties"], false);
    for field in ["face", "sketch", "points", "flip"] {
        assert!(schema["properties"][field].is_object(), "missing {field}");
    }
    let description = hole["description"].as_str().unwrap();
    assert!(description.contains("unnamed") && description.contains("stale"));
    let path = std::env::temp_dir().join(format!("a4-mcp-branches-{}.qcad", std::process::id()));
    let before = archive(&mut r, &path);
    for args in [
        json!({"diameter":4,"depth":10}),
        json!({"sketch":"markers","face":{"facing":"+z"},"diameter":4,"depth":10}),
        json!({"sketch":"markers","at":[8,6,10],"diameter":4,"depth":10}),
        json!({"sketch":"markers","offset":[0,0],"diameter":4,"depth":10}),
        json!({"face":{"facing":"+z"},"points":ids,"diameter":4,"depth":10}),
        json!({"face":{"facing":"+z"},"flip":false,"diameter":4,"depth":10}),
        json!({"sketch":"markers","diameter":4,"depth":10,"through":true}),
        json!({"sketch":"markers","diameter":4}),
        json!({"sketch":"markers","points":[ids[0]],"diameter":4,"depth":10}),
        json!({"sketch":"markers","points":[ids[0],ids[0]],"diameter":4,"depth":10}),
        json!({"sketch":"markers","points":[],"diameter":4,"depth":10}),
        json!({"sketch":"markers","diameter":4,"depth":10,"unexpected":1}),
    ] {
        assert!(r.call("hole", args.clone()).unwrap().is_err(), "accepted {args}");
        same_archive(&archive(&mut r, &path), &before);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn hole_sketch_registry_explicit_points_formulas_counterbores_and_undo() {
    let (mut r, mut ids) = fixture();
    ids.reverse();
    call(&mut r, "hole", json!({"sketch":"markers","points":ids,"diameter":4,"through":true,"name":"drilled"}).to_string().into());
    volume(&mut r, 12000. - 2. * std::f64::consts::PI * 4. * 10.);
    bores(&mut r, 8.);
    call(&mut r, "param_set", json!({"name":"pitch","value":12}));
    bores(&mut r, 12.);
    for key in ["off_u", "off_v"] {
        assert!(r.call("dimension_set", json!({"target":{"feature":"drilled","key":key},"value":1})).unwrap().is_err());
    }
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "param_set");
    bores(&mut r, 8.);
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "hole");
    volume(&mut r, 12000.);
    call(&mut r, "hole", json!({"sketch":"markers","diameter":4,"depth":8,"kind":"counterbore","dia2":6,"depth2":2}));
    volume(&mut r, 12000. - 2. * std::f64::consts::PI * (4. * 8. + (9. - 4.) * 2.));
}

#[test]
fn hole_sketch_missing_site_and_batch_failure_restore_exact_archive() {
    let (mut r, ids) = fixture();
    let path = std::env::temp_dir().join(format!("a4-mcp-batch-{}.qcad", std::process::id()));
    let before = archive(&mut r, &path);
    let error = r
        .call(
            "batch",
            json!({"steps":[
                {"tool":"param_set","args":{"name":"pitch","value":50}},
                {"tool":"hole","args":{"sketch":"markers","diameter":4,"through":true}}
            ]}),
        )
        .unwrap()
        .unwrap_err();
    assert!(error.contains("step 2") && error.contains(&ids[0].to_string()), "{error}");
    same_archive(&archive(&mut r, &path), &before);
    call(&mut r, "hole", json!({"sketch":"markers","diameter":4,"through":true,"name":"drilled"}));
    let before = archive(&mut r, &path);
    for args in [json!({"name":"pitch","value":50}), json!({"name":"pitch","value":-8})] {
        assert!(r.call("param_set", args).unwrap().is_err());
        same_archive(&archive(&mut r, &path), &before);
    }
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "hole");
    volume(&mut r, 12000.);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn hole_sketch_real_stdio_batch_through_flip_and_undo() {
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
        writeln!(stdin, "{}", json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}})).unwrap();
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
        json!({"steps":[
            {"tool":"sketch_create","args":{"plane":"XY","name":"profile"}},
            {"tool":"sketch_add","args":{"sketch":"profile","entities":[{"type":"rect","w":40,"h":30}]}},
            {"tool":"extrude","args":{"sketch":"profile","height":10}},
            {"tool":"sketch_create","args":{"plane":"XY","name":"markers"}},
            {"tool":"sketch_add","args":{"sketch":"markers","entities":[{"type":"point","x":8,"y":6},{"type":"point","x":-8,"y":-6}]}},
            {"tool":"hole","args":{"sketch":"markers","flip":true,"diameter":4,"through":true,"name":"drilled"}}
        ]}),
    );
    assert_ne!(response["result"]["isError"], true, "{response}");
    let result = payload(&response);
    assert_eq!(result["steps"][5]["created"]["name"], "drilled");
    assert!((result["bodies"][0]["volume_mm3"].as_f64().unwrap() - (12000. - 80. * std::f64::consts::PI)).abs() < 1e-6);
    assert_eq!(request(3, "hole", json!({"sketch":"markers","offset":[0,0],"diameter":4,"depth":10}))["result"]["isError"], true);
    assert_eq!(payload(&request(4, "undo", json!({})))["undone"]["tool"], "batch");
    assert_eq!(payload(&request(5, "doc_info", json!({})))["bodies"], json!([]));
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
