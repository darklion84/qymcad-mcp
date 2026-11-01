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
            json!({"protocolVersion":"2025-06-18", "capabilities":{}, "clientInfo":{"name":"d1-repick-test", "version":"0"}}),
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

fn build_box(client: &mut Client) -> Value {
    client.init();
    client.ok("doc_new", json!({}));
    client.ok("sketch_create", json!({"plane":"XY", "name":"profile"}));
    client.ok("sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":20, "h":20}]}));
    let extrude = client.ok("extrude", json!({"sketch":"profile", "height":10, "name":"box"}));
    assert!(extrude["body"].is_number());
    assert_eq!(extrude["rebuild"]["errors"], Value::Null);
    extrude["body"].clone()
}

fn selected_ids(result: &Value) -> Vec<Value> {
    assert_eq!(result["count"], result["edges"].as_array().unwrap().len());
    result["edges"].as_array().unwrap().iter().map(|edge| edge["id"].clone()).collect()
}

#[test]
fn fillet_repick_over_stdio_returns_exact_selection_and_feature_fields() {
    let mut client = Client::start();
    let box_body = build_box(&mut client);
    let first = client.ok("fillet", json!({"edges":{"along":"z"}, "radius":1, "name":"round"}));
    assert_ne!(first["body"], box_body);
    assert_eq!(first["rebuild"]["errors"], Value::Null);

    let selection = json!({"edges_of":{"of_feature":"box", "role":"cap_end"}});
    let preview = client.ok("select", json!({"body":box_body, "edges":selection.clone()}));
    let ids = selected_ids(&preview);
    let repicked = client.ok("fillet", json!({"feature":"round", "edges":{"edges_of":{"of_feature":"box", "role":"cap_end"}}}));
    assert_eq!(repicked["feature"], first["body"]);
    assert_ne!(repicked["body"], box_body);
    assert_eq!(repicked["name"], "round");
    assert_eq!(repicked["selected_edges"], json!(ids));
    assert_eq!(repicked["rebuild"]["errors"], Value::Null);
}

#[test]
fn fillet_repick_with_feature_runs_inside_batch() {
    let mut client = Client::start();
    let box_body = build_box(&mut client);
    let batch = client.ok(
        "batch",
        json!({"verbose":true, "steps":[
            {"tool":"fillet", "args":{"edges":{"along":"z"}, "radius":1, "name":"round"}},
            {"tool":"fillet", "args":{"feature":"round", "edges":{"edges_of":{"of_feature":"box", "role":"cap_end"}}}}
        ]}),
    );
    assert_eq!(batch["ok"], true);
    assert_eq!(batch["steps"][0]["tool"], "fillet");
    let feature = batch["steps"][0]["result"]["body"].clone();
    assert_ne!(feature, box_body);
    assert_eq!(batch["steps"][1]["tool"], "fillet");
    assert_eq!(batch["steps"][1]["result"]["feature"], feature);
    assert_eq!(batch["steps"][1]["result"]["name"], "round");
    assert!(!batch["steps"][1]["result"]["selected_edges"].as_array().unwrap().is_empty());
    assert_eq!(batch["steps"][1]["result"]["rebuild"]["errors"], Value::Null);
}

#[test]
fn repick_refuses_radius_with_feature_over_stdio() {
    let mut client = Client::start();
    let _box_body = build_box(&mut client);
    client.ok("fillet", json!({"edges":{"along":"z"}, "radius":1, "name":"round"}));
    let (is_error, message) = client.tool("fillet", json!({"feature":"round", "edges":{"along":"z"}, "radius":2}));
    assert!(is_error);
    assert!(message.as_str().unwrap().contains("`body`, `radius` and `name` are not accepted"), "{message}");
}

#[test]
fn chamfer_repick_returns_chamfer_json_fields_over_stdio() {
    let mut client = Client::start();
    let box_body = build_box(&mut client);
    let first = client.ok("chamfer", json!({"edges":{"edges_of":{"facing":"+z"}}, "dist":1, "name":"bevel"}));
    assert_ne!(first["body"], box_body);
    assert_eq!(first["chamfer"]["mode"], "symmetric");
    assert_eq!(first["chamfer"]["reference_face"], Value::Null);
    assert_eq!(first["chamfer"]["flip"], false);
    let repicked = client.ok("chamfer", json!({"feature":"bevel", "edges":{"extreme":"+z"}}));
    assert_eq!(repicked["feature"], first["body"]);
    assert_ne!(repicked["body"], box_body);
    assert_eq!(repicked["name"], "bevel");
    assert!(!repicked["selected_edges"].as_array().unwrap().is_empty());
    assert_eq!(repicked["rebuild"]["errors"], Value::Null);
    assert_eq!(repicked["chamfer"]["mode"], "symmetric");
    assert_eq!(repicked["chamfer"]["reference_face"], Value::Null);
    assert_eq!(repicked["chamfer"]["flip"], false);
}
