//! G3/G4 over real stdio: part_instance and part_copy registered, strict arguments, batch, undo, doc_info fields.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
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
            .expect("spawn qymcad-mcp");
        Self { stdin: child.stdin.take().unwrap(), stdout: BufReader::new(child.stdout.take().unwrap()), child, next: 1 }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        writeln!(self.stdin, "{}", json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})).unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], id);
        response
    }

    fn init(&mut self) {
        let response = self.request(
            "initialize",
            json!({"protocolVersion":"2025-06-18", "capabilities":{}, "clientInfo":{"name":"g34-instances-test", "version":"0"}}),
        );
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        writeln!(self.stdin, "{}", json!({"jsonrpc":"2.0", "method":"notifications/initialized"})).unwrap();
        self.stdin.flush().unwrap();
    }

    fn tool(&mut self, name: &str, args: Value) -> (bool, Value) {
        let response = self.request("tools/call", json!({"name":name, "arguments":args}));
        let result = &response["result"];
        let text = result["content"][0]["text"].as_str().unwrap();
        let payload = serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()));
        (result["isError"] == true, payload)
    }

    fn ok(&mut self, name: &str, args: Value) -> Value {
        let (is_error, payload) = self.tool(name, args);
        assert!(!is_error, "{name} failed: {payload}");
        payload
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn setup() -> (Client, u64) {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    let knob = c.ok("doc_info", json!({}))["active_component"].as_u64().unwrap();
    c.ok("rename", json!({"target":{"part":knob}, "name":"knob"}));
    c.ok("param_set", json!({"name":"kw", "value":40}));
    c.ok("sketch_create", json!({"plane":"XY", "name":"knob-sk"}));
    c.ok("sketch_add", json!({"sketch":"knob-sk", "entities":[{"type":"rect", "w":"kw", "h":30}]}));
    let b = c.ok("extrude", json!({"sketch":"knob-sk", "height":20, "name":"knob-body"}));
    assert_eq!(b["rebuild"]["errors"], Value::Null);
    (c, knob)
}

fn component(info: &Value, id: u64) -> Value {
    info["components"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap().clone()
}

#[test]
fn part_instance_and_part_copy_over_stdio_registered_batchable_undoable() {
    let (mut c, knob) = setup();
    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"part_instance") && names.contains(&"part_copy"), "{names:?}");
    for tool in ["part_instance", "part_copy"] {
        let (error, text) = c.tool(tool, json!({"source":"knob", "name":"x", "bogus":1}));
        assert!(error && text.as_str().unwrap().contains("bogus"), "{tool}: {text}");
        let (error, text) = c.tool(tool, json!({"source":"knob"}));
        assert!(error && text.as_str().unwrap().contains("name"), "{tool}: name is required: {text}");
    }

    let made = c.ok("part_instance", json!({"source":"knob", "name":"knob2", "translation_mm":[100, 0, 0]}));
    let clone = made["part"]["id"].as_u64().unwrap();
    assert_eq!(made["part"]["instance_of"], knob);
    let info = c.ok("doc_info", json!({}));
    assert_eq!(component(&info, clone)["instance_of"], knob);
    assert_eq!(component(&info, knob)["instances"], json!([clone]));
    let assembly = c.ok("assembly_info", json!({}));
    assert_eq!(component(&assembly, clone)["instance_of"], knob);
    let undone = c.ok("undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "part_instance");
    let info = c.ok("doc_info", json!({}));
    assert_eq!(info["components"].as_array().unwrap().len(), 2, "root + knob: {info}");
    assert!(component(&info, knob).get("instances").is_none());

    let batch = c.ok(
        "batch",
        json!({"steps":[
            {"tool":"part_instance", "args":{"source":"knob", "name":"knob2"}},
            {"tool":"part_copy", "args":{"source":"knob", "name":"knob-copy", "translation_mm":[0, 100, 0]}},
            {"tool":"param_set", "args":{"name":"kw", "value":50}}
        ]}),
    );
    assert_eq!(batch["ok"], true, "{batch}");
    assert_eq!(batch["steps"][0]["created"]["name"], "knob2", "{batch}");
    assert_eq!(batch["steps"][1]["created"]["name"], "knob-copy", "{batch}");
    let volumes: Vec<f64> = batch["bodies"].as_array().unwrap().iter().map(|b| b["volume_mm3"].as_f64().unwrap()).collect();
    assert_eq!(volumes.len(), 3, "{batch}");
    assert!(volumes.iter().all(|v| (v - 30000.0).abs() < 1e-6), "source, clone and copy follow kw: {volumes:?}");
    let undone = c.ok("undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "batch");
    assert_eq!(c.ok("doc_info", json!({}))["components"].as_array().unwrap().len(), 2);

    // A refused step rolls the whole batch back.
    let (error, text) = c.tool(
        "batch",
        json!({"steps":[
            {"tool":"part_instance", "args":{"source":"knob", "name":"knob2"}},
            {"tool":"sketch_create", "args":{"plane":"XY", "name":"extra-sk"}},
            {"tool":"sketch_add", "args":{"sketch":"extra-sk", "entities":[{"type":"rect", "cx":25, "w":10, "h":10}]}},
            {"tool":"extrude", "args":{"sketch":"extra-sk", "height":10, "op":"new_body"}}
        ]}),
    );
    assert!(error && text.to_string().contains("multi-body while instances"), "{text}");
    assert_eq!(c.ok("doc_info", json!({}))["components"].as_array().unwrap().len(), 2);
}
