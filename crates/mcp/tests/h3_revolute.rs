//! H3 over real stdio: a Revolute door on edge anchors, angle drive and limits, grounding refusal, batch, undo.
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
            json!({"protocolVersion":"2025-06-18", "capabilities":{}, "clientInfo":{"name":"h3-revolute-test", "version":"0"}}),
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

fn z_edge(c: &mut Client, body: &str, x: f64, y: f64) -> u64 {
    let topo = c.ok("topology", json!({"body":body}));
    let edges = topo["edges"].as_array().unwrap();
    let e = edges
        .iter()
        .find(|e| {
            let m = &e["mid"];
            e["kind"] == "line" && (m[0].as_f64().unwrap() - x).abs() < 1e-6 && (m[1].as_f64().unwrap() - y).abs() < 1e-6
        })
        .unwrap_or_else(|| panic!("no edge at ({x}, {y}): {topo}"));
    e["id"].as_u64().unwrap()
}

fn door_box(c: &mut Client) -> Vec<f64> {
    let info = c.ok("doc_info", json!({}));
    let b = info["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "leaf").unwrap().clone();
    b["bbox_world"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect()
}

#[test]
fn revolute_door_over_stdio() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    let carcass = c.ok("doc_info", json!({}))["active_component"].as_u64().unwrap();
    c.ok("rename", json!({"target":{"part":carcass}, "name":"carcass"}));
    c.ok("sketch_create", json!({"plane":"XY", "name":"side-sk"}));
    c.ok("sketch_add", json!({"sketch":"side-sk", "entities":[{"type":"rect", "w":20, "h":400}]}));
    c.ok("extrude", json!({"sketch":"side-sk", "height":700, "name":"side"}));
    c.ok("part_new", json!({"name":"door"}));
    c.ok("sketch_create", json!({"plane":"XY", "name":"leaf-sk"}));
    c.ok("sketch_add", json!({"sketch":"leaf-sk", "entities":[{"type":"rect", "w":400, "h":18}]}));
    c.ok("extrude", json!({"sketch":"leaf-sk", "height":700, "name":"leaf"}));
    c.ok("part_place", json!({"part":"door", "translation_mm":[210, -209, 0]}));
    c.ok("ground", json!({"part":"carcass", "grounded":true}));
    let ea = z_edge(&mut c, "side", 10.0, -200.0);
    let eb = z_edge(&mut c, "leaf", -200.0, 9.0);
    let (error, text) = c.tool(
        "joint_create",
        json!({"kind":"revolute", "mode":"as_built", "a":{"body":"side", "edge":[ea], "face":[1]}, "b":{"body":"leaf", "edge":[eb]}}),
    );
    assert!(error && text.as_str().unwrap().contains("exactly one of face or edge"), "{text}");
    let made = c.ok(
        "joint_create",
        json!({"name":"hinge", "kind":"revolute", "mode":"as_built", "angle":-90, "limits":{"min":-110, "max":0},
               "a":{"body":"side", "edge":[ea]}, "b":{"body":"leaf", "edge":[eb]}}),
    );
    assert_eq!(made["joint"]["kind"], "revolute", "{made}");
    let b = door_box(&mut c);
    for (got, want) in b.iter().zip([-8.0, -600.0, 0.0, 10.0, -200.0, 700.0]) {
        assert!((got - want).abs() < 1e-4, "{b:?}");
    }
    let r = c.ok("joint_set", json!({"joint":"hinge", "angle":-150}));
    assert_eq!(r["clamped"]["applied"], -110.0, "{r}");
    let (error, text) = c.tool("joint_set", json!({"joint":"hinge", "offset":10}));
    assert!(error && text.as_str().unwrap().contains("no editable offset slot"), "{text}");
    let (error, text) = c.tool("ground", json!({"part":"door", "grounded":true}));
    assert!(error && text.as_str().unwrap().contains("F-159"), "{text}");
    let (error, text) = c.tool("joint_limits", json!({"joint":"hinge", "min":-270}));
    assert!(error && text.as_str().unwrap().contains("F-158"), "{text}");

    let batch = c.ok(
        "batch",
        json!({"steps":[
            {"tool":"param_set", "args":{"name":"swing", "value":45}},
            {"tool":"joint_set", "args":{"joint":"hinge", "angle":"-swing"}},
            {"tool":"joint_limits", "args":{"joint":"hinge", "slot":"angle", "min":-60}}
        ]}),
    );
    assert_eq!(batch["ok"], true, "{batch}");
    assert_eq!(c.ok("assembly_info", json!({}))["joints"][0]["drive"][0], -45.0);
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(c.ok("assembly_info", json!({}))["joints"][0]["drive"][0], -110.0);
    let r = c.ok("joint_set", json!({"joint":"hinge", "release":"angle"}));
    assert_eq!(r["drive"], json!([null, null, null]), "{r}");
}
