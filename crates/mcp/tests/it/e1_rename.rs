//! E1 acceptance over the real JSON-RPC stdio transport.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next: u64,
}
impl Client {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_qymcad-mcp"))
            .env("QYMCAD_MCP_LOG", "off")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut c = Self { child, input, output, next: 1 };
        let initialized =
            c.request("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"e1","version":"1"}}));
        assert!(initialized["result"].is_object(), "{initialized}");
        writeln!(c.input, "{}", json!({"jsonrpc":"2.0","method":"notifications/initialized"})).unwrap();
        c.input.flush().unwrap();
        c
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        writeln!(self.input, "{}", json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})).unwrap();
        self.input.flush().unwrap();
        let mut line = String::new();
        assert_ne!(self.output.read_line(&mut line).unwrap(), 0, "server closed stdout");
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], id);
        response
    }
    fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let response = self.request("tools/call", json!({"name":name,"arguments":args}));
        if !response["error"].is_null() {
            return Err(response["error"].to_string());
        }
        let result = &response["result"];
        let text = result["content"][0]["text"].as_str().unwrap_or_default();
        if result["isError"] == true {
            return Err(text.to_owned());
        }
        serde_json::from_str(text).map_err(|e| format!("{name}: invalid JSON payload: {e}: {result}"))
    }
    fn ok(&mut self, name: &str, args: Value) -> Value {
        self.tool(name, args).unwrap_or_else(|e| panic!("{name} failed: {e}"))
    }
    fn rename(&mut self, kind: &str, id: Value, name: &str) -> Value {
        self.ok("rename", json!({"target":{kind:id},"name":name}))
    }
    fn snapshot(&mut self) -> Value {
        json!({"doc":self.ok("doc_info",json!({})),"topology":self.ok("topology",json!({"adjacency":true}))})
    }
    fn refuse_unchanged(&mut self, name: &str, args: Value) -> String {
        let before = self.snapshot();
        let error = self.tool(name, args).expect_err("request must be refused");
        assert_eq!(self.snapshot(), before, "{name} refusal changed document/topology: {error}");
        error
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn fixture() -> (Client, Value, Value) {
    let mut c = Client::start();
    c.ok("doc_new", json!({}));
    c.ok("param_set", json!({"name":"h","value":5}));
    let sketch = c.ok("sketch_create", json!({"plane":"XY","name":"profile"}))["sketch"].clone();
    c.ok("sketch_add", json!({"sketch":sketch,"entities":[{"type":"rect","w":10,"h":6}]}));
    let body = c.ok("extrude", json!({"sketch":sketch,"height":"h","name":"plate"}))["body"].clone();
    volume(&c.ok("doc_info", json!({}))["bodies"][0], 10.0 * 6.0 * 5.0);
    (c, sketch, body)
}
fn volume(body: &Value, expected: f64) {
    assert!((body["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6, "{body}; expected {expected}");
}
fn result(actual: Value, kind: &str, id: &Value, names: [&str; 3], changed: bool, duplicates: Value) {
    let [previous, name, display] = names;
    assert_eq!(
        actual,
        json!({"target":{"kind":kind,"id":id},"previous_name":previous,"name":name,"display_name":display,"changed":changed,"duplicates":duplicates})
    );
}
fn node<'a>(info: &'a Value, id: &Value) -> &'a Value {
    info["timeline"].as_array().unwrap().iter().find(|n| n["id"] == *id).unwrap()
}
fn archive(c: &mut Client, path: &Path) -> BTreeMap<String, Vec<u8>> {
    c.ok("doc_save", json!({"path":path}));
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
fn same_archive(actual: &BTreeMap<String, Vec<u8>>, expected: &BTreeMap<String, Vec<u8>>) {
    assert_eq!(actual.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
    for (name, bytes) in expected {
        assert!(actual[name] == *bytes, "archive entry {name} differs");
    }
}

#[test]
fn e1_stdio_three_targets_exact_results_and_body_name_compatibility() {
    let (mut c, sketch, body) = fixture();
    result(
        c.rename("feature", body.clone(), "Плита α # / :"),
        "feature",
        &body,
        ["plate", "Плита α # / :", "Плита α # / :"],
        true,
        json!([]),
    );
    let feature = c.ok("doc_info", json!({}));
    assert_eq!(node(&feature, &body)["name"], "Плита α # / :");
    assert_eq!(node(&feature, &sketch)["name"], "profile");
    assert_eq!(feature["sketches"][0]["name"], "profile");
    assert_eq!(c.ok("sketch_info", json!({"sketch":"profile"}))["id"], sketch, "identical timeline/record aliases identify one object");
    assert_eq!(feature["bodies"][0]["name"], "Плита α # / :");
    assert_eq!(feature["bodies"][0]["body_name"], "Body 1");
    result(c.rename("sketch", sketch.clone(), "drawing"), "sketch", &sketch, ["profile", "drawing", "drawing"], true, json!([]));
    let drawing = c.ok("doc_info", json!({}));
    assert_eq!(drawing["sketches"][0]["name"], "drawing");
    assert_eq!(node(&drawing, &sketch)["name"], "profile");
    assert_eq!(drawing["bodies"], feature["bodies"]);
    result(c.rename("body", body.clone(), "Housing"), "body", &body, ["name-body#1", "Housing", "Housing"], true, json!([]));
    let housing = c.ok("doc_info", json!({}));
    assert_eq!(housing["bodies"][0]["body_name"], "Housing");
    assert_eq!(c.ok("topology", json!({"body":"Housing"}))["body_name"], "Housing");
    assert_eq!(housing["bodies"][0]["name"], "Плита α # / :");
    assert_eq!(housing["timeline"], drawing["timeline"]);
    assert_eq!(housing["sketches"], drawing["sketches"]);
    assert_eq!(housing["params"], feature["params"]);
    assert_eq!(housing["named_dimensions"], feature["named_dimensions"]);
    volume(&housing["bodies"][0], 10.0 * 6.0 * 5.0);
}

#[test]
fn e1_stdio_policy_refusals_strict_target_and_exact_rollback() {
    let (mut c, _sketch, body) = fixture();
    result(c.rename("feature", body.clone(), "  Base  "), "feature", &body, ["plate", "Base", "Base"], true, json!([]));
    let list = c.request("tools/list", json!({}));
    let rename = list["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "rename").expect("rename registered");
    assert_eq!(rename["inputSchema"]["additionalProperties"], false);
    let mut required = rename["inputSchema"]["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>();
    required.sort_unstable();
    assert_eq!(required, vec!["name", "target"]);
    for name in ["", "   ", "a\nb", "a\tb", "\u{0085}Base", "123", "+1", "007"] {
        c.refuse_unchanged("rename", json!({"target":{"feature":body},"name":name}));
    }
    for args in [
        json!({"name":"valid"}),
        json!({"target":{"feature":body}}),
        json!({"target":{},"name":"valid"}),
        json!({"target":{"unknown":body},"name":"valid"}),
        json!({"target":{"feature":body,"body":body},"name":"valid"}),
        json!({"target":{"feature":body,"unknown":true},"name":"valid"}),
        json!({"target":{"feature":body},"name":"valid","unknown":true}),
        json!({"target":{"feature":u64::MAX},"name":"valid"}),
        json!({"target":{"sketch":body},"name":"valid"}),
    ] {
        c.refuse_unchanged("rename", args);
    }
    let info = c.ok("doc_info", json!({}));
    assert_eq!(node(&info, &body)["name"], "Base");
    volume(&info["bodies"][0], 10.0 * 6.0 * 5.0);
}

#[test]
fn e1_stdio_duplicates_localisation_and_ambiguous_target() {
    let (mut c, sketch, body) = fixture();
    result(c.rename("feature", body.clone(), "same"), "feature", &body, ["plate", "same", "same"], true, json!([]));
    let datum = c.ok("plane_offset", json!({"base":"XY","dist":9,"name":"second"}))["plane"].clone();
    result(c.rename("feature", datum.clone(), "same"), "feature", &datum, ["second", "same", "same"], true, json!([body]));
    let error = c.refuse_unchanged("rename", json!({"target":{"feature":"same"},"name":"other"}));
    assert!(error.contains("several objects") && error.contains("use the id"), "{error}");
    result(c.rename("feature", datum.clone(), "feat-name-shell"), "feature", &datum, ["same", "feat-name-shell", "Shell"], true, json!([]));
    assert_eq!(node(&c.ok("doc_info", json!({})), &datum)["name"], "Shell");
    result(
        c.rename("feature", json!("Shell"), "finished"),
        "feature",
        &datum,
        ["feat-name-shell", "finished", "finished"],
        true,
        json!([]),
    );
    result(
        c.rename("sketch", sketch.clone(), "feat-name-shell"),
        "sketch",
        &sketch,
        ["profile", "feat-name-shell", "Shell"],
        true,
        json!([]),
    );
    for alias in ["feat-name-shell", "Shell"] {
        let detail = c.ok("sketch_info", json!({"sketch":alias}));
        assert_eq!(detail["id"], sketch);
        assert_eq!(detail["name"], "Shell");
    }
}

#[test]
fn e1_stdio_same_name_skips_history_and_undo_restores_exact_label() {
    let (mut c, _sketch, body) = fixture();
    let before = c.snapshot();
    result(
        c.rename("feature", body.clone(), "Плита α # / :"),
        "feature",
        &body,
        ["plate", "Плита α # / :", "Плита α # / :"],
        true,
        json!([]),
    );
    let renamed = c.snapshot();
    result(
        c.rename("feature", body.clone(), "Плита α # / :"),
        "feature",
        &body,
        ["Плита α # / :", "Плита α # / :", "Плита α # / :"],
        false,
        json!([]),
    );
    assert_eq!(c.snapshot(), renamed);
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "rename");
    assert_eq!(c.snapshot(), before, "same-name rename adds no undo entry");
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "extrude");
    assert_eq!(c.ok("doc_info", json!({}))["bodies"], json!([]));
}

#[test]
fn e1_stdio_renamed_aliases_work_immediately_and_after_open() {
    let (mut c, sketch, body) = fixture();
    result(c.rename("sketch", sketch.clone(), "drawing"), "sketch", &sketch, ["profile", "drawing", "drawing"], true, json!([]));
    c.rename("feature", sketch.clone(), "sketch timeline");
    c.rename("feature", body.clone(), "Плита α # / :");
    c.rename("body", body.clone(), "Housing");
    let path = std::env::temp_dir().join(format!("e1-stdio-aliases-{}.qcad", std::process::id()));
    for reopened in [false, true] {
        if reopened {
            c.ok("doc_save", json!({"path":path}));
            c.ok("doc_open", json!({"path":path}));
        }
        assert_eq!(c.ok("sketch_info", json!({"sketch":"drawing"}))["id"], sketch);
        assert_eq!(c.ok("topology", json!({"body":"Housing"}))["body"], body);
        result(
            c.rename("feature", json!("Плита α # / :"), "Плита α # / :"),
            "feature",
            &body,
            ["Плита α # / :", "Плита α # / :", "Плита α # / :"],
            false,
            json!([]),
        );
        c.refuse_unchanged("sketch_info", json!({"sketch":"profile"}));
        let added = c
            .ok("sketch_add", json!({"sketch":"drawing","entities":[{"type":"line","x1":20,"y1":20,"x2":21,"y2":20,"construction":true}]}));
        assert_eq!(added["created"].as_array().unwrap().len(), 1);
        assert_eq!(added["created"][0]["entities"].as_array().unwrap().len(), 1);
        let extruded =
            c.ok("extrude", json!({"sketch":"drawing","height":2,"op":"new_body","name":if reopened {"after open"} else {"before open"}}));
        let retained = extruded["rebuild"]["bodies"].as_array().unwrap().iter().find(|b| b["id"] == body).unwrap();
        assert_eq!(retained["body_name"], "Housing");
        assert_eq!(retained["name"], "Плита α # / :");
        let new_body = extruded["body"].clone();
        let info = c.ok("doc_info", json!({}));
        volume(info["bodies"].as_array().unwrap().iter().find(|b| b["id"] == new_body).unwrap(), 10.0 * 6.0 * 2.0);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn e1_stdio_batch_uses_new_name_and_late_failure_restores_archive() {
    let (mut c, sketch, body) = fixture();
    result(c.rename("feature", body.clone(), "Base"), "feature", &body, ["plate", "Base", "Base"], true, json!([]));
    let path = std::env::temp_dir().join(format!("e1-stdio-batch-{}.qcad", std::process::id()));
    let before = archive(&mut c, &path);
    let state = c.snapshot();
    let error = c
        .tool(
            "batch",
            json!({"steps":[
                {"tool":"rename","args":{"target":{"sketch":sketch},"name":"s2"}},
                {"tool":"sketch_add","args":{"sketch":"s2","entities":[{"type":"circle","cx":20,"cy":0,"d":2}]}},
                {"tool":"extrude","args":{"sketch":"missing","height":1}}
            ]}),
        )
        .expect_err("late batch step must fail");
    assert!(error.contains("step 3") && error.contains("extrude"), "{error}");
    assert_eq!(c.snapshot(), state);
    // ADR 0020/F-056: a failed batch keeps its allocated ids; every other byte must match.
    let mut after = archive(&mut c, &path);
    let next_id = |bytes: &[u8]| -> (String, u64) {
        let text = std::str::from_utf8(bytes).unwrap();
        let lines: Vec<_> = text.lines().filter(|l| l.trim_start().starts_with("next_id:")).collect();
        assert_eq!(lines.len(), 1);
        (lines[0].to_owned(), lines[0].trim().trim_start_matches("next_id:").trim().trim_end_matches(',').parse().unwrap())
    };
    let (old_line, old_id) = next_id(&before["document.ron"]);
    let (new_line, new_id) = next_id(&after["document.ron"]);
    assert!(new_id > old_id, "a failed allocating batch keeps the monotonic allocator");
    let doc = std::str::from_utf8(&after["document.ron"]).unwrap().replacen(&new_line, &old_line, 1);
    after.insert("document.ron".into(), doc.into_bytes());
    same_archive(&after, &before);
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "rename", "failed batch consumes no undo entry");
    let batch = c.ok(
        "batch",
        json!({"verbose":true,"steps":[
            {"tool":"rename","args":{"target":{"sketch":sketch},"name":"s2"}},
            {"tool":"extrude","args":{"sketch":"s2","height":2,"op":"new_body","name":"second extrusion"}}
        ]}),
    );
    assert_eq!(batch["ok"], true);
    assert_eq!(batch["steps"].as_array().unwrap().len(), 2);
    result(batch["steps"][0]["result"].clone(), "sketch", &sketch, ["profile", "s2", "s2"], true, json!([]));
    let extruded = &batch["steps"][1]["result"]["body"];
    volume(batch["bodies"].as_array().unwrap().iter().find(|b| b["id"] == *extruded).unwrap(), 10.0 * 6.0 * 2.0);
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(c.ok("doc_info", json!({}))["sketches"][0]["name"], "profile");
    std::fs::remove_file(path).unwrap();
}
