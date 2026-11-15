//! E3 acceptance over the real JSON-RPC stdio transport.
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
            c.request("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"e3","version":"1"}}));
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

struct Fixture {
    client: Client,
    sketch: Value,
    source: Value,
    body: Value,
}
fn fixture() -> Fixture {
    let mut c = Client::start();
    c.ok("doc_new", json!({}));
    c.ok("param_set", json!({"name":"h","value":4}));
    let sketch = c.ok("sketch_create", json!({"plane":"XY","name":"profile"}))["sketch"].clone();
    c.ok("sketch_add", json!({"sketch":sketch,"entities":[{"type":"rect","w":10,"h":20}]}));
    let source = c.ok("extrude", json!({"sketch":sketch,"height":"h","name":"A"}))["body"].clone();
    let moved = c.ok("move", json!({"body":source,"dx":5,"name":"B"}));
    assert_eq!(moved["source_body"], source);
    assert_eq!(moved["reused"], false);
    let body = moved["body"].clone();
    assert_ne!(body, source);
    let info = c.ok("doc_info", json!({}));
    volume(&info["bodies"][0], 10.0 * 20.0 * 4.0); // Rectangle area × extrusion height = 800 mm³; translation preserves it.
    Fixture { client: c, sketch, source, body }
}
fn volume(body: &Value, expected: f64) {
    assert!((body["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6, "{body}; expected {expected}");
}
fn assert_appearance(actual: Value, identity: &Value, source: &Value, visible: bool, rgb: [u8; 3], color_source: &str, changed: bool) {
    assert_eq!(
        actual,
        json!({
            "body":{"id":identity["id"],"name":identity["name"],"body_name":identity["body_name"]},
            "lineage_root":source,"visible":visible,"color_rgb":rgb,"color_source":color_source,
            "scope":{"visibility":"body","color":"lineage"},"changed":changed
        })
    );
}
fn identity(c: &mut Client, body: &Value) -> Value {
    let info = c.ok("doc_info", json!({}));
    info["bodies"].as_array().unwrap().iter().find(|b| b["id"] == *body).unwrap().clone()
}
fn assert_undo_sequence(c: &mut Client, expected: &[&str]) {
    for name in expected {
        assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], *name);
    }
    assert!(c.tool("undo", json!({})).is_err(), "there must be no extra undo entry");
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
fn next_id(bytes: &[u8]) -> (String, u64) {
    let text = std::str::from_utf8(bytes).unwrap();
    let lines: Vec<_> = text.lines().filter(|l| l.trim_start().starts_with("next_id:")).collect();
    assert_eq!(lines.len(), 1);
    (lines[0].to_owned(), lines[0].trim().trim_start_matches("next_id:").trim().trim_end_matches(',').parse().unwrap())
}

#[test]
fn e3_stdio_set_reset_manual_palette_and_doc_info_exact() {
    let Fixture { mut client, source, body, .. } = fixture();
    let original = identity(&mut client, &body);
    assert_eq!(original["name"], "B");
    assert_appearance(
        client.ok("appearance", json!({"body":body,"visible":false,"color":[17,34,51]})),
        &original,
        &source,
        false,
        [17, 34, 51],
        "manual",
        true,
    );
    let after = identity(&mut client, &body);
    let mut expected = original.clone();
    expected["visible"] = json!(false);
    expected["color_rgb"] = json!([17, 34, 51]);
    expected["color_source"] = json!("manual");
    assert_eq!(after, expected, "all existing body fields keep their values and meanings");
    volume(&after, 10.0 * 20.0 * 4.0); // Prism = 800 mm³; appearance has no geometry effect.
                                       // Native palette at this body's index is [150,170,190], independently verified by
                                       // tasks/research-medE/appearance/src/main.rs and its retained rustc-run.log reset probe.
    assert_appearance(
        client.ok("appearance", json!({"body":"B","color":null})),
        &original,
        &source,
        false,
        [150, 170, 190],
        "palette",
        true,
    );
    assert_appearance(
        client.ok("appearance", json!({"body":body,"color":[150,170,190]})),
        &original,
        &source,
        false,
        [150, 170, 190],
        "manual",
        true,
    );
    assert_eq!(identity(&mut client, &body)["color_source"], "manual", "map membership, not palette inequality");
    assert_appearance(
        client.ok("appearance", json!({"body":body,"color":[0,255,0]})),
        &original,
        &source,
        false,
        [0, 255, 0],
        "manual",
        true,
    );
    assert_eq!(identity(&mut client, &body)["color_rgb"], json!([0, 255, 0]));
    assert_appearance(client.ok("appearance", json!({"body":"B","visible":true})), &original, &source, true, [0, 255, 0], "manual", true);
}

#[test]
fn e3_stdio_refusals_preserve_full_doc_topology_and_undo_depth() {
    // The real transport has no undo-depth reader. After each before/after snapshot,
    // draining the known history proves its exact depth and every entry's identity.
    for case in 0..17 {
        let Fixture { mut client, sketch, source, body } = fixture();
        let listed = client.request("tools/list", json!({}));
        let tools = listed["result"]["tools"].as_array().unwrap();
        let appearance = tools.iter().find(|t| t["name"] == "appearance").expect("appearance registered");
        assert_eq!(appearance["inputSchema"]["additionalProperties"], false);
        assert_eq!(appearance["inputSchema"]["required"], json!(["body"]));
        assert!(appearance["inputSchema"]["properties"]["color"].get("default").is_none(), "omission must not advertise a reset default");
        assert!(appearance["inputSchema"]["properties"]["visible"].get("default").is_none());
        assert_eq!(appearance["inputSchema"]["properties"]["visible"]["type"], "boolean");
        let mut extra = Vec::new();
        let args = match case {
            0 => json!({"body":body}),
            1 => json!({"body":body,"color":[-1,2,3]}),
            2 => json!({"body":body,"color":[256,2,3]}),
            3 => json!({"body":body,"color":[1.5,2,3]}),
            4 => json!({"body":body,"color":[1,2]}),
            5 => json!({"body":body,"color":"red"}),
            6 => json!({"body":source,"visible":false}),
            7 => json!({"body":sketch,"visible":false}),
            8 => json!({"body":u64::MAX,"visible":false}),
            9 => json!({"body":body,"visible":false,"unknown":true}),
            10 => json!({"visible":false}),
            11 => {
                let datum = client.ok("plane_offset", json!({"base":"XY","dist":9,"name":"datum"}))["plane"].clone();
                extra.push("plane_offset");
                json!({"body":datum,"visible":false})
            }
            12 => {
                client.ok("rename", json!({"target":{"feature":body},"name":"duplicate"}));
                let datum = client.ok("plane_offset", json!({"base":"XY","dist":9,"name":"duplicate"}))["plane"].clone();
                assert!(datum.is_u64());
                extra.extend(["plane_offset", "rename"]);
                json!({"body":"duplicate","visible":false})
            }
            13 => json!({"body":body,"color":[1,2,3,4]}),
            14 => json!({"body":body,"color":["1",2,3]}),
            15 => json!({"body":body,"visible":null}),
            16 => json!({"body":"missing","visible":false}),
            _ => unreachable!(),
        };
        client.refuse_unchanged("appearance", args);
        extra.extend(["move", "extrude", "sketch_add", "sketch_create", "param_set"]);
        assert_undo_sequence(&mut client, &extra);
    }
}

#[test]
fn e3_stdio_noop_adds_no_history_and_undo_restores_exact_archive() {
    let Fixture { mut client, source, body, .. } = fixture();
    let path = std::env::temp_dir().join(format!("e3-stdio-undo-{}.qcad", std::process::id()));
    let entries = archive(&mut client, &path);
    let before = client.snapshot();
    let original = identity(&mut client, &body);
    assert_appearance(
        client.ok("appearance", json!({"body":body,"visible":false,"color":[17,34,51]})),
        &original,
        &source,
        false,
        [17, 34, 51],
        "manual",
        true,
    );
    let edited = client.snapshot();
    assert_appearance(
        client.ok("appearance", json!({"body":body,"visible":false,"color":[17,34,51]})),
        &original,
        &source,
        false,
        [17, 34, 51],
        "manual",
        false,
    );
    assert_eq!(client.snapshot(), edited);
    assert_eq!(client.ok("undo", json!({}))["undone"]["tool"], "appearance");
    assert_eq!(client.snapshot(), before);
    assert_eq!(archive(&mut client, &path), entries, "undo restores every archive entry, including flag and colour map");
    assert_undo_sequence(&mut client, &["move", "extrude", "sketch_add", "sketch_create", "param_set"]);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn e3_stdio_batch_late_failure_archive_rollback_and_successful_extrude() {
    let Fixture { mut client, sketch, source, body } = fixture();
    let path = std::env::temp_dir().join(format!("e3-stdio-batch-{}.qcad", std::process::id()));
    let entries = archive(&mut client, &path);
    let before = client.snapshot();
    let error = client
        .tool(
            "batch",
            json!({"steps":[
                {"tool":"appearance","args":{"body":body,"visible":false,"color":[17,34,51]}},
                {"tool":"sketch_create","args":{"plane":"XY","name":"temporary"}},
                {"tool":"extrude","args":{"sketch":"missing","height":1}}
            ]}),
        )
        .expect_err("late batch failure required");
    assert!(error.contains("step 3") && error.contains("extrude"), "{error}");
    assert_eq!(client.snapshot(), before);
    let mut after = archive(&mut client, &path);
    // ADR 0020: allocated ids survive failed batches; every other archive byte matches.
    let (old_line, old_id) = next_id(&entries["document.ron"]);
    let (new_line, new_id) = next_id(&after["document.ron"]);
    assert!(new_id > old_id);
    let restored = std::str::from_utf8(&after["document.ron"]).unwrap().replacen(&new_line, &old_line, 1);
    after.insert("document.ron".into(), restored.into_bytes());
    assert_eq!(after, entries);
    let original = identity(&mut client, &body);
    let batch = client.ok(
        "batch",
        json!({"verbose":true,"steps":[
            {"tool":"appearance","args":{"body":body,"visible":false,"color":[17,34,51]}},
            {"tool":"extrude","args":{"sketch":sketch,"height":2,"op":"new_body","name":"second"}}
        ]}),
    );
    assert_eq!(batch["ok"], true);
    assert_eq!(batch["steps"].as_array().unwrap().len(), 2);
    assert_appearance(batch["steps"][0]["result"].clone(), &original, &source, false, [17, 34, 51], "manual", true);
    let second = batch["steps"][1]["result"]["body"].clone();
    let second_info = identity(&mut client, &second);
    volume(&second_info, 10.0 * 20.0 * 2.0); // New extrusion area × height = 400 mm³.
    assert_eq!(second_info["visible"], true);
    let first_info = identity(&mut client, &body);
    assert_eq!(first_info["visible"], false);
    assert_eq!(first_info["color_rgb"], json!([17, 34, 51]));
    assert_eq!(first_info["color_source"], "manual");
    volume(&first_info, 10.0 * 20.0 * 4.0); // Original prism remains 800 mm³.
    assert_eq!(client.ok("undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(client.snapshot(), before);
    assert_undo_sequence(&mut client, &["move", "extrude", "sketch_add", "sketch_create", "param_set"]);
    std::fs::remove_file(path).unwrap();
}
