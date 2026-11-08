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
            json!({"protocolVersion":"2025-06-18", "capabilities":{}, "clientInfo":{"name":"g12-draft-thicken-test", "version":"0"}}),
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

fn setup() -> Client {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("param_set", json!({"name":"a", "value":-5}));
    c.ok("param_set", json!({"name":"t", "value":3}));
    c.ok("sketch_create", json!({"plane":"XY", "name":"profile"}));
    c.ok("sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":20, "h":20}]}));
    let b = c.ok("extrude", json!({"sketch":"profile", "height":10, "name":"block"}));
    assert_eq!(b["rebuild"]["errors"], Value::Null);
    c
}

fn volume(v: &Value) -> f64 {
    v["rebuild"]["bodies"][0]["volume_mm3"].as_f64().or_else(|| v["bodies"][0]["volume_mm3"].as_f64()).unwrap()
}

const WALLS: &str = r#"{"union":[{"facing":"+x"},{"facing":"-x"},{"facing":"+y"},{"facing":"-y"}]}"#;

#[test]
fn draft_and_thicken_over_stdio_registered_batchable_undoable() {
    let mut c = setup();
    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"draft") && names.contains(&"thicken"), "{names:?}");
    let (error, text) = c.tool("draft", json!({"faces":{"facing":"+x"}, "neutral":{"facing":"-z"}, "angle":5, "bogus":1}));
    assert!(error && text.as_str().unwrap().contains("bogus"), "{text}");

    let walls: Value = serde_json::from_str(WALLS).unwrap();
    let draft = c.ok("draft", json!({"faces":walls, "neutral":{"facing":"-z"}, "angle":"a", "name":"taper"}));
    assert!((volume(&draft) - 3660.2510).abs() < 1e-3, "{draft}");
    let thick = c.ok("thicken", json!({"face":{"facing":"+z"}, "thickness":"t", "name":"plate"}));
    // The drafted top is 18.2502² before the plate grows 3 mm on it.
    let top = (20.0 - 20.0 * 5f64.to_radians().tan()).powi(2);
    assert!((volume(&thick) - (3660.2510 + top * 3.0)).abs() < 1e-2, "{thick}");

    let (error, text) = c.tool("param_set", json!({"name":"a", "value":70}));
    assert!(error && text.as_str().unwrap().contains("draft angle"), "{text}");
    let (error, text) = c.tool("param_set", json!({"name":"t", "value":-15}));
    assert!(error && text.as_str().unwrap().contains("thickness"), "{text}");
    let (error, text) = c.tool("thicken", json!({"feature":"plate", "face":{"facing":"+z"}, "thickness":1}));
    assert!(error && text.as_str().unwrap().contains("not accepted"), "{text}");

    let repicked = c.ok("draft", json!({"feature":"taper", "faces":{"facing":"+x"}}));
    assert_eq!(repicked["feature"], draft["body"]);
    assert_eq!(repicked["selected_faces"].as_array().unwrap().len(), 1);
    let undone = c.ok("undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "draft");

    let batch = c.ok(
        "batch",
        json!({"steps":[
            {"tool":"param_set", "args":{"name":"t", "value":1}},
            {"tool":"thicken", "args":{"feature":"plate", "face":{"facing":"+x"}}}
        ]}),
    );
    assert_eq!(batch["ok"], true, "{batch}");
    let undone = c.ok("undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "batch");
    assert!((volume(&undone) - (3660.2510 + top * 3.0)).abs() < 1e-2, "{undone}");

    let (error, text) = c.tool(
        "batch",
        json!({"steps":[
            {"tool":"thicken", "args":{"face":{"facing":"+z"}, "thickness":2}},
            {"tool":"draft", "args":{"faces":{"facing":"+x"}, "neutral":{"facing":"-z"}, "angle":61}}
        ]}),
    );
    assert!(error && text.to_string().contains("draft angle"), "{text}");
    let info = c.ok("doc_info", json!({}));
    assert!((info["bodies"][0]["volume_mm3"].as_f64().unwrap() - (3660.2510 + top * 3.0)).abs() < 1e-2, "{info}");
}
