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
            json!({"protocolVersion":"2025-06-18", "capabilities":{}, "clientInfo":{"name":"d2-repick-test", "version":"0"}}),
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

fn setup() -> (Client, Value) {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("param_set", json!({"name":"h", "value":10}));
    c.ok("param_set", json!({"name":"t", "value":1}));
    c.ok("param_set", json!({"name":"d", "value":5}));
    c.ok("sketch_create", json!({"plane":"XY", "name":"profile"}));
    c.ok("sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":20, "h":20}]}));
    let b = c.ok("extrude", json!({"sketch":"profile", "height":"h", "name":"box"}));
    assert_eq!(b["rebuild"]["errors"], Value::Null);
    (c, b["body"].clone())
}

fn faces(c: &mut Client, body: &Value) -> (Value, Value, Value) {
    let top = c.ok("select", json!({"body":body, "faces":{"facing":"+z"}}))["faces"][0]["id"].clone();
    let bottom = c.ok("select", json!({"body":body, "faces":{"facing":"-z"}}))["faces"][0]["id"].clone();
    let xface = c.ok("select", json!({"body":body, "faces":{"facing":"+x"}}))["faces"][0]["id"].clone();
    (top, bottom, xface)
}

#[test]
fn shell_and_push_face_repick_stdio_ids_description_and_batch() {
    let (mut c, body) = setup();
    let (top, bottom, _) = faces(&mut c, &body);
    let shell = c.ok("shell", json!({"open_faces":{"facing":"+z"}, "thickness":"t", "name":"shell"}));
    assert!((shell["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - 1084.0).abs() < 1e-6);
    let repicked = c.ok("shell", json!({"feature":"shell", "open_faces":[top.clone(), bottom]}));
    for key in ["feature", "body", "name", "selected_faces", "rebuild"] {
        assert!(repicked.get(key).is_some(), "{key}");
    }
    assert_eq!(repicked["feature"], shell["body"]);
    assert_eq!(repicked["selected_faces"].as_array().unwrap().len(), 2);

    let (mut push_client, push_body) = setup();
    let (push_top, _, push_xface) = faces(&mut push_client, &push_body);
    let push = push_client.ok("push_face", json!({"face":push_top, "dist":"d", "name":"push"}));
    assert!((push["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - 6000.0).abs() < 1e-6);
    let result = push_client.ok("push_face", json!({"feature":"push", "face":push_xface}));
    assert_eq!(result["feature"], push["body"]);
    assert_eq!(result["selected_faces"], json!([push_xface]));
    assert!((result["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - 5000.0).abs() < 1e-6);

    let batch = push_client.ok(
        "batch",
        json!({"verbose":true, "steps":[
            {"tool":"shell", "args":{"open_faces":{"facing":"+z"}, "thickness":1, "name":"batch-shell"}},
            {"tool":"shell", "args":{"feature":"batch-shell", "open_faces":{"facing":"-z"}}}
        ]}),
    );
    assert_eq!(batch["ok"], true);
    assert_eq!(batch["steps"][1]["result"]["feature"], batch["steps"][0]["result"]["body"]);
}

#[test]
fn shell_repick_undo_restores_previous_volume_in_one_step() {
    let (mut c, body) = setup();
    let (top, bottom, _) = faces(&mut c, &body);
    let shell = c.ok("shell", json!({"open_faces":top, "thickness":1, "name":"shell"}));
    let repicked = c.ok("shell", json!({"feature":"shell", "open_faces":[top, bottom]}));
    assert!((repicked["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - 760.0).abs() < 1e-6);
    let undone = c.ok("undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "shell");
    assert!((undone["bodies"][0]["volume_mm3"].as_f64().unwrap() - 1084.0).abs() < 1e-6);
    let restored = c.ok("topology", json!({"body":undone["bodies"][0]["id"], "faces":true, "edges":false}));
    let restored_ids: Vec<_> = restored["faces"].as_array().unwrap().iter().map(|f| f["id"].clone()).collect();
    assert!(restored_ids.contains(&top));
    let _ = shell;
}

#[test]
fn feature_numeric_refusals_and_undo_are_stdio_atomic() {
    let (mut c, _) = setup();
    c.ok("shell", json!({"open_faces":{"facing":"+z"}, "thickness":1, "name":"shell"}));
    let (error, text) = c.tool("shell", json!({"feature":"shell", "open_faces":{"facing":"-z"}, "thickness":2}));
    assert!(error);
    assert!(text.as_str().unwrap().contains("with `feature`, `body`, `thickness`, `side` and `name` are not accepted"));

    let (mut push_client, push_body) = setup();
    let (_, _, push_xface) = faces(&mut push_client, &push_body);
    push_client.ok("push_face", json!({"face":{"facing":"+z"}, "dist":5, "name":"push"}));
    let (error, text) = push_client.tool("push_face", json!({"feature":"push", "face":push_xface, "dist":2}));
    assert!(error);
    assert!(text.as_str().unwrap().contains("with `feature`, `body`, `dist` and `name` are not accepted"));
    push_client.ok("push_face", json!({"feature":"push", "face":push_xface}));
    let undone = push_client.ok("undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "push_face");
    assert!((undone["bodies"][0]["volume_mm3"].as_f64().unwrap() - 6000.0).abs() < 1e-6);
}
