//! H2 over real stdio: joint_limits (null clears, omitted keeps), clamps reported, batch with rollback, undo.
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
            json!({"protocolVersion":"2025-06-18", "capabilities":{}, "clientInfo":{"name":"h2-joint-limits-test", "version":"0"}}),
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
    let carcass = c.ok("doc_info", json!({}))["active_component"].as_u64().unwrap();
    c.ok("rename", json!({"target":{"part":carcass}, "name":"carcass"}));
    c.ok("sketch_create", json!({"plane":"XY", "name":"carcass-sk"}));
    c.ok("sketch_add", json!({"sketch":"carcass-sk", "entities":[{"type":"rect", "w":600, "h":800}]}));
    c.ok("extrude", json!({"sketch":"carcass-sk", "height":700, "name":"carcass-box"}));
    c.ok("part_new", json!({"name":"drawer"}));
    c.ok("sketch_create", json!({"plane":"XY", "name":"drawer-sk"}));
    c.ok("sketch_add", json!({"sketch":"drawer-sk", "entities":[{"type":"rect", "w":500, "h":600}]}));
    c.ok("extrude", json!({"sketch":"drawer-sk", "height":150, "name":"drawer-box"}));
    c.ok("part_place", json!({"part":"drawer", "translation_mm":[0, 0, 100]}));
    c.ok("ground", json!({"part":"carcass", "grounded":true}));
    let made = c.ok(
        "joint_create",
        json!({"name":"slide", "kind":"slider", "mode":"as_built", "axis":{"base_plane":"XZ"}, "offset":0,
               "limits":{"min":0, "max":300},
               "a":{"body":"carcass-box", "face":{"facing":[1, 0, 0]}},
               "b":{"body":"drawer-box", "face":{"facing":[-1, 0, 0]}}}),
    );
    assert_eq!(made["limit_max"], json!([null, 300.0, null]), "{made}");
    (c, made["joint"]["id"].as_u64().unwrap())
}

#[test]
fn joint_limits_over_stdio_registered_strict_batchable_undoable() {
    let (mut c, joint) = setup();
    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"joint_limits"), "{names:?}");
    let (error, text) = c.tool("joint_limits", json!({"joint":"slide", "max":10, "bogus":1}));
    assert!(error && text.as_str().unwrap().contains("bogus"), "{text}");

    let r = c.ok("joint_set", json!({"joint":"slide", "offset":400}));
    assert_eq!(r["clamped"], json!({"joint":joint, "slot":1, "requested":400.0, "applied":300.0}));
    assert_eq!(c.ok("assembly_info", json!({}))["joints"][0]["at_limit"], json!([{"joint":joint, "slot":"offset"}]));
    // Omitted keeps a bound; null ticks it off.
    let r = c.ok("joint_limits", json!({"joint":"slide", "max":null}));
    assert_eq!(r["limit_min"], json!([null, 0.0, null]));
    assert_eq!(r["limit_max"], json!([null, null, null]));
    let r = c.ok("joint_limits", json!({"joint":"slide", "slot":"offset", "max":"250"}));
    assert_eq!(r["limit_max"], json!([null, 250.0, null]));
    assert_eq!(r["clamped"], json!({"joint":joint, "slot":1, "requested":300.0, "applied":250.0}));
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "joint_limits");
    let (error, text) = c.tool("joint_limits", json!({"joint":"slide", "min":200, "max":100}));
    assert!(error && text.as_str().unwrap().contains("F-157"), "{text}");

    let batch = c.ok(
        "batch",
        json!({"steps":[
            {"tool":"joint_limits", "args":{"joint":"slide", "max":200}},
            {"tool":"joint_set", "args":{"joint":"slide", "offset":150}}
        ]}),
    );
    assert_eq!(batch["ok"], true, "{batch}");
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "batch");
    let (error, text) = c.tool(
        "batch",
        json!({"steps":[
            {"tool":"joint_limits", "args":{"joint":"slide", "max":200}},
            {"tool":"joint_limits", "args":{"joint":"slide", "min":300}}
        ]}),
    );
    assert!(error && text.to_string().contains("step 2"), "{text}");
    let info = c.ok("assembly_info", json!({}));
    assert_eq!(info["joints"][0]["limit_max"], json!([null, null, null]), "{info}");
}

/// A compact batch step carries the clamp of its own call, as the top-level result does; a step without a clamp has none.
#[test]
fn a_compact_batch_step_carries_its_clamp() {
    let (mut c, joint) = setup();
    let batch = c.ok(
        "batch",
        json!({"steps":[
            {"tool":"joint_set", "args":{"joint":"slide", "offset":100}},
            {"tool":"joint_set", "args":{"joint":"slide", "offset":400}},
            {"tool":"joint_limits", "args":{"joint":"slide", "max":250}}
        ]}),
    );
    assert_eq!(batch["ok"], true, "{batch}");
    assert!(batch["steps"][0].get("clamped").is_none(), "{batch}");
    assert_eq!(batch["steps"][1]["clamped"], json!({"joint":joint, "slot":1, "requested":400.0, "applied":300.0}), "{batch}");
    assert_eq!(batch["steps"][2]["clamped"], json!({"joint":joint, "slot":1, "requested":300.0, "applied":250.0}), "{batch}");
    // The assembly report of a batch uses the same at_limit format as assembly_info.
    assert_eq!(batch["assembly"]["at_limit"], json!([{"joint":joint, "slot":"offset"}]), "{batch}");
}
