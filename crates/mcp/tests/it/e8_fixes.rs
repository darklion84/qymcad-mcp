//! E8 review fixes over the real JSON-RPC stdio transport.
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
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
            c.request("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"e8","version":"1"}}));
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
        let doc = self.ok("doc_info", json!({}));
        let topology: Vec<_> =
            doc["bodies"].as_array().unwrap().iter().map(|body| self.ok("topology", json!({"body":body["id"],"adjacency":true}))).collect();
        json!({"doc":doc,"topology":topology})
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn stock(c: &mut Client) -> Value {
    c.ok("doc_new", json!({}));
    let sk = c.ok("sketch_create", json!({"plane":"XY","name":"profile"}))["sketch"].clone();
    c.ok("sketch_add", json!({"sketch":sk,"entities":[{"type":"rect","w":10,"h":20}]}));
    let base = c.ok("extrude", json!({"sketch":sk,"height":20,"name":"stock"}))["body"].clone();
    assert_prism(&c.ok("doc_info", json!({}))["bodies"][0], &base, 0.0);
    base
}

fn assert_prism(body: &Value, id: &Value, dx: f64) {
    assert_eq!(body["id"], *id);
    // Centered 10×20 rectangle extruded +20: V=10×20×20; translation adds dx to X bounds.
    assert!((body["volume_mm3"].as_f64().unwrap() - 10.0 * 20.0 * 20.0).abs() < 1e-6);
    assert_eq!(body["bbox"], json!([-5.0 + dx, -10.0, 0.0, 5.0 + dx, 10.0, 20.0]));
}

fn clean_rebuild(result: &Value) {
    assert_eq!(result["rebuild"]["errors"], json!([]));
    assert_eq!(result["rebuild"]["warnings"], json!([]));
}

#[test]
fn e8_stdio_multi_piece_cut_refuses_before_mutation() {
    let mut c = Client::start();
    let base = stock(&mut c);
    let sk = c.ok("sketch_create", json!({"plane":"XY","name":"slot"}))["sketch"].clone();
    c.ok("sketch_add", json!({"sketch":sk,"entities":[{"type":"rect","w":2,"h":30}]}));
    let cut = c.ok("extrude", json!({"sketch":sk,"height":20,"op":"cut","target":base,"name":"separating cut"}));
    assert_eq!(cut["rebuild"].as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["bodies"]);
    let feature = cut["body"].clone();
    let info = c.ok("doc_info", json!({}));
    let pieces = info["bodies"].as_array().unwrap();
    assert_eq!(pieces.len(), 2);
    for piece in pieces {
        // A width-2 slot through centered width-10 stock leaves two width-(10-2)/2 pieces.
        let expected = ((10.0 - 2.0) / 2.0) * 20.0 * 20.0;
        assert!((piece["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6);
    }
    let before = c.snapshot();
    let error = c.tool("feature_suppress", json!({"feature":feature,"suppressed":true})).expect_err("multi-output must be refused");
    assert!(error.contains("separating cut"), "{error}");
    assert!(
        error.contains("pinned") && (error.contains("stale") || error.contains("overlapping") || error.contains("identity guard")),
        "{error}"
    );
    assert_eq!(c.snapshot(), before, "refusal must preserve full doc_info and every result topology");
}

#[test]
fn e8_stdio_rollback_reports_lost_body_metadata_and_keeps_lineage_colour() {
    let mut c = Client::start();
    let base = stock(&mut c);
    let moved = c.ok("move", json!({"body":base,"dx":5,"name":"tail"}))["body"].clone();
    c.ok("rename", json!({"target":{"body":moved},"name":"custom-body"}));
    c.ok("appearance", json!({"body":moved,"visible":false,"color":[12,34,56]}));
    let before = c.ok("doc_info", json!({}));
    assert_prism(&before["bodies"][0], &moved, 5.0);
    assert_eq!(before["bodies"][0]["body_name"], "custom-body");
    assert_eq!(before["bodies"][0]["visible"], false);
    let noop = c.ok("history_rollback", json!({"position":"end"}));
    assert_eq!(noop["changed"], false);
    assert_eq!(noop["lost_body_metadata"], json!([]));
    let rollback = c.ok("history_rollback", json!({"after":base}));
    assert_eq!(rollback["changed"], true);
    assert_eq!(rollback["lost_body_metadata"], json!([{"body":moved,"body_name":"custom-body","visible":false}]));
    clean_rebuild(&rollback);
    assert_eq!(rollback["rebuild"]["bodies"].as_array().unwrap().len(), 1);
    assert_prism(&rollback["rebuild"]["bodies"][0], &base, 0.0);
    let resumed = c.ok("history_rollback", json!({"position":"end"}));
    assert_eq!(resumed["changed"], true);
    assert_eq!(resumed["lost_body_metadata"], json!([]));
    clean_rebuild(&resumed);
    assert_eq!(resumed["rebuild"]["bodies"].as_array().unwrap().len(), 1);
    assert_prism(&resumed["rebuild"]["bodies"][0], &moved, 5.0);
    let body = &resumed["rebuild"]["bodies"][0];
    // Native regenerated body records use Body 1/default visibility; lineage colour lives separately.
    assert_eq!(body["body_name"], "Body 1");
    assert_eq!(body["visible"], true);
    assert_eq!(body["color_rgb"], json!([12, 34, 56]));
    assert_eq!(body["color_source"], "manual");
}

#[test]
fn e8_stdio_suppression_reports_lost_body_metadata_and_plain_empty_list() {
    let mut c = Client::start();
    let base = stock(&mut c);
    let moved = c.ok("move", json!({"body":base,"dx":5,"name":"tail"}))["body"].clone();
    c.ok("rename", json!({"target":{"body":moved},"name":"custom-body"}));
    c.ok("appearance", json!({"body":moved,"visible":false,"color":[12,34,56]}));
    let suppressed = c.ok("feature_suppress", json!({"feature":base,"suppressed":true}));
    assert_eq!(suppressed["changed"], true);
    assert_eq!(suppressed["feature"], json!({"id":base,"name":"stock"}));
    assert_eq!(suppressed["suppressed"], true);
    assert_eq!(suppressed["unbuilt"], json!([base, moved]));
    assert_eq!(suppressed["lost_body_metadata"], json!([{"body":moved,"body_name":"custom-body","visible":false}]));
    clean_rebuild(&suppressed);
    assert_eq!(suppressed["rebuild"]["bodies"], json!([]));
    let noop = c.ok("feature_suppress", json!({"feature":base,"suppressed":true}));
    assert_eq!(noop["changed"], false);
    assert_eq!(noop["lost_body_metadata"], json!([]));
    let enabled = c.ok("feature_suppress", json!({"feature":base,"suppressed":false}));
    assert_eq!(enabled["changed"], true);
    assert_eq!(enabled["suppressed"], false);
    assert_eq!(enabled["unbuilt"], json!([]));
    assert_eq!(enabled["lost_body_metadata"], json!([]));
    clean_rebuild(&enabled);
    assert_eq!(enabled["rebuild"]["bodies"].as_array().unwrap().len(), 1);
    assert_prism(&enabled["rebuild"]["bodies"][0], &moved, 5.0);
    let body = &enabled["rebuild"]["bodies"][0];
    assert_eq!(body["body_name"], "Body 1");
    assert_eq!(body["visible"], true);
    assert_eq!(body["color_rgb"], json!([12, 34, 56]));
    assert_eq!(body["color_source"], "manual");
    // With native defaults only, removing these records has no non-default metadata to report.
    let plain = c.ok("feature_suppress", json!({"feature":base,"suppressed":true}));
    assert_eq!(plain["lost_body_metadata"], json!([]));
    assert_eq!(plain["rebuild"]["bodies"], json!([]));
    clean_rebuild(&plain);
}
