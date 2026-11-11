//! H1 over real stdio: part_delete registered, strict arguments, batch with rollback, undo.
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
            json!({"protocolVersion":"2025-06-18", "capabilities":{}, "clientInfo":{"name":"h1-part-delete-test", "version":"0"}}),
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

fn components(c: &mut Client) -> Vec<u64> {
    let info = c.ok("doc_info", json!({}));
    info["components"].as_array().unwrap().iter().map(|c| c["id"].as_u64().unwrap()).collect()
}

#[test]
fn part_delete_over_stdio_registered_strict_batchable_undoable() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    let knob = c.ok("doc_info", json!({}))["active_component"].as_u64().unwrap();
    c.ok("rename", json!({"target":{"part":knob}, "name":"knob"}));
    c.ok("sketch_create", json!({"plane":"XY", "name":"knob-sk"}));
    c.ok("sketch_add", json!({"sketch":"knob-sk", "entities":[{"type":"rect", "w":40, "h":30}]}));
    c.ok("extrude", json!({"sketch":"knob-sk", "height":20, "name":"knob-body"}));
    let spare = c.ok("part_new", json!({"name":"spare"}))["part"]["id"].as_u64().unwrap();
    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"part_delete"), "{names:?}");
    let (error, text) = c.tool("part_delete", json!({"part":"spare", "bogus":1}));
    assert!(error && text.as_str().unwrap().contains("bogus"), "{text}");
    let (error, text) = c.tool("part_delete", json!({"part":{"root":true}}));
    assert!(error && text.as_str().unwrap().contains("the root assembly cannot be deleted"), "{text}");

    let deleted = c.ok("part_delete", json!({"part":"spare"}));
    assert_eq!(deleted["removed"]["part"], spare);
    assert!(!components(&mut c).contains(&spare));
    let undone = c.ok("undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "part_delete");
    assert!(components(&mut c).contains(&spare));

    // Batch: a clone is made and deleted in one undo entry.
    let batch = c.ok(
        "batch",
        json!({"steps":[
            {"tool":"part_instance", "args":{"source":"knob", "name":"knob2"}},
            {"tool":"part_delete", "args":{"part":"knob2"}},
            {"tool":"part_delete", "args":{"part":"spare"}}
        ]}),
    );
    assert_eq!(batch["ok"], true, "{batch}");
    assert_eq!(components(&mut c).len(), 2, "root + knob");
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(components(&mut c).len(), 3, "root + knob + spare");

    // A refused later step rolls the delete back.
    let (error, text) = c.tool(
        "batch",
        json!({"steps":[
            {"tool":"part_delete", "args":{"part":"spare"}},
            {"tool":"part_delete", "args":{"part":{"root":true}}}
        ]}),
    );
    assert!(error && text.to_string().contains("step 2"), "{text}");
    assert!(components(&mut c).contains(&spare));
}
