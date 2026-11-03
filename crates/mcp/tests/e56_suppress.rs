//! E5/E6 acceptance over the real JSON-RPC stdio transport.
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
            c.request("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"e56","version":"1"}}));
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
    base: Value,
    cut: Value,
}

fn rect(c: &mut Client, name: &str, w: f64, h: f64, cx: f64, cy: f64) -> Value {
    let sketch = c.ok("sketch_create", json!({"plane":"XY","name":name}))["sketch"].clone();
    c.ok("sketch_add", json!({"sketch":sketch,"entities":[{"type":"rect","w":w,"h":h,"cx":cx,"cy":cy}]}));
    sketch
}

fn fixture() -> Fixture {
    let mut c = Client::start();
    c.ok("doc_new", json!({}));
    let sketch = rect(&mut c, "stock profile", 20.0, 20.0, 0.0, 0.0);
    let base = c.ok("extrude", json!({"sketch":sketch,"height":10,"name":"stock"}))["body"].clone();
    let cut_sketch = rect(&mut c, "cut profile", 5.0, 5.0, -4.0, 0.0);
    let cut = c.ok("extrude", json!({"sketch":cut_sketch,"height":10,"op":"cut","target":base,"name":"cut"}))["body"].clone();
    // Stock minus a rectangular through cut: 20×20×10 − 5×5×10 = 3750 mm³.
    volume(&c.ok("doc_info", json!({}))["bodies"][0], 20.0 * 20.0 * 10.0 - 5.0 * 5.0 * 10.0);
    Fixture { client: c, sketch, base, cut }
}

fn volume(body: &Value, expected: f64) {
    assert!((body["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6, "{body}; expected {expected}");
}
fn node<'a>(info: &'a Value, id: &Value) -> &'a Value {
    info["timeline"].as_array().unwrap().iter().find(|n| n["id"] == *id).unwrap()
}
fn keys(value: &Value) -> Vec<&str> {
    value.as_object().unwrap().keys().map(String::as_str).collect()
}
fn suppression(
    c: &mut Client,
    feature: &Value,
    name: &str,
    suppressed: bool,
    changed: bool,
    unbuilt: Value,
    bodies: &[(Value, f64)],
) -> Value {
    let result = c.ok("feature_suppress", json!({"feature":feature,"suppressed":suppressed}));
    assert_eq!(keys(&result), ["changed", "feature", "rebuild", "suppressed", "unbuilt"]);
    assert_eq!(result["feature"], json!({"id":feature,"name":name}));
    assert_eq!(result["suppressed"], suppressed);
    assert_eq!(result["changed"], changed);
    assert_eq!(result["unbuilt"], unbuilt);
    assert_eq!(keys(&result["rebuild"]), ["bodies", "errors", "warnings"]);
    assert_eq!(result["rebuild"]["errors"], json!([]));
    assert_eq!(result["rebuild"]["warnings"], json!([]));
    let actual = result["rebuild"]["bodies"].as_array().unwrap();
    assert_eq!(actual.len(), bodies.len());
    for (body, (id, expected)) in actual.iter().zip(bodies) {
        assert_eq!(body["id"], *id);
        volume(body, *expected);
    }
    let info = c.ok("doc_info", json!({}));
    assert_eq!(node(&info, feature)["suppressed"], suppressed);
    assert_eq!(info["bodies"], result["rebuild"]["bodies"]);
    result
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
            // ADR 0020 permits only the monotonic allocator to advance after failure.
            let bytes = if name == "document.ron" {
                std::str::from_utf8(&bytes)
                    .unwrap()
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("next_id:"))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into_bytes()
            } else {
                bytes
            };
            (name, bytes)
        })
        .collect()
}

#[test]
fn e56_stdio_suppress_unsuppress_exact_results_and_dirty_suffix() {
    let Fixture { mut client, base, cut, .. } = fixture();
    let initial = client.ok("doc_info", json!({}));
    assert_eq!(node(&initial, &base)["suppressed"], false);
    assert_eq!(node(&initial, &cut)["suppressed"], false);
    suppression(&mut client, &cut, "cut", true, true, json!([]), &[(cut.clone(), 20.0 * 20.0 * 10.0)]);
    suppression(&mut client, &cut, "cut", false, true, json!([]), &[(cut.clone(), 20.0 * 20.0 * 10.0 - 5.0 * 5.0 * 10.0)]);
    let next_sketch = rect(&mut client, "second cut profile", 2.0, 2.0, 4.0, 0.0);
    let second =
        client.ok("extrude", json!({"sketch":next_sketch,"height":10,"op":"cut","target":cut,"name":"second cut"}))["body"].clone();
    // Two disjoint cuts: 4000−250−40 = 3710; bypassing the first leaves 4000−40 = 3960.
    volume(&client.ok("doc_info", json!({}))["bodies"][0], 20.0 * 20.0 * 10.0 - 5.0 * 5.0 * 10.0 - 2.0 * 2.0 * 10.0);
    suppression(&mut client, &cut, "cut", true, true, json!([]), &[(second.clone(), 20.0 * 20.0 * 10.0 - 2.0 * 2.0 * 10.0)]);
    suppression(&mut client, &cut, "cut", false, true, json!([]), &[(second, 20.0 * 20.0 * 10.0 - 5.0 * 5.0 * 10.0 - 2.0 * 2.0 * 10.0)]);
}

#[test]
fn e56_stdio_final_move_pass_through_remains_current() {
    let mut c = Client::start();
    c.ok("doc_new", json!({}));
    let sketch = rect(&mut c, "profile", 10.0, 20.0, 0.0, 0.0);
    let base = c.ok("extrude", json!({"sketch":sketch,"height":20,"name":"stock"}))["body"].clone();
    let moved = c.ok("move", json!({"body":base,"dx":5,"name":"shift"}))["body"].clone();
    suppression(&mut c, &moved, "shift", true, true, json!([]), &[(moved.clone(), 10.0 * 20.0 * 20.0)]);
    let info = c.ok("doc_info", json!({}));
    // Centered 10×20 rectangle extruded +20: x=[−5,5], y=[−10,10], z=[0,20].
    let bbox = info["bodies"][0]["bbox"].as_array().unwrap();
    for (actual, expected) in bbox.iter().zip([-5.0, -10.0, 0.0, 5.0, 10.0, 20.0]) {
        assert!((actual.as_f64().unwrap() - expected).abs() < 1e-6);
    }
    assert_eq!(c.ok("topology", json!({}))["body"], moved);
    let selected = c.ok("select", json!({"faces":{"facing":"+z"}}));
    assert_eq!(selected["body"], moved);
    assert_eq!(selected["count"], 1);
}

#[test]
fn e56_stdio_base_cascade_and_undo_noop() {
    let Fixture { mut client, base, cut, .. } = fixture();
    let before = client.snapshot();
    suppression(&mut client, &cut, "cut", true, true, json!([]), &[(cut.clone(), 20.0 * 20.0 * 10.0)]);
    let suppressed = client.snapshot();
    suppression(&mut client, &cut, "cut", true, false, json!([]), &[(cut.clone(), 20.0 * 20.0 * 10.0)]);
    assert_eq!(client.snapshot(), suppressed);
    assert_eq!(client.ok("undo", json!({}))["undone"]["tool"], "feature_suppress");
    assert_eq!(client.snapshot(), before, "one undo skips the same-state request and restores exact topology");
    suppression(&mut client, &base, "stock", true, true, json!([base, cut]), &[]);
    let info = client.ok("doc_info", json!({}));
    assert_eq!(info["errors"], json!([]));
    assert_eq!(info["bodies"], json!([]));
    suppression(&mut client, &base, "stock", false, true, json!([]), &[(cut, 20.0 * 20.0 * 10.0 - 5.0 * 5.0 * 10.0)]);
}

#[test]
fn e56_stdio_refusals_strict_schema_and_exact_snapshots() {
    let Fixture { mut client, sketch, base, cut } = fixture();
    client.ok("param_set", json!({"name":"height_param","value":10}));
    let datum = client.ok("plane_offset", json!({"base":"XY","dist":2,"name":"datum"}))["plane"].clone();
    client.ok("rename", json!({"target":{"feature":base},"name":"same"}));
    client.ok("rename", json!({"target":{"feature":cut},"name":"same"}));
    let tools = client.request("tools/list", json!({}));
    let tool =
        tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "feature_suppress").expect("feature_suppress registered");
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    assert_eq!(tool["inputSchema"]["required"], json!(["feature", "suppressed"]));
    assert_eq!(tool["inputSchema"]["properties"]["suppressed"]["type"], "boolean");
    for args in [
        json!({"feature":sketch,"suppressed":true}),
        json!({"feature":datum,"suppressed":true}),
        json!({"feature":u64::MAX,"suppressed":true}),
        json!({"feature":"same","suppressed":true}),
        json!({"feature":"height_param","suppressed":true}),
        json!({"feature":"unknown feature","suppressed":true}),
        json!({"feature":cut,"suppressed":true,"unknown":true}),
        json!({"feature":cut}),
        json!({"suppressed":true}),
        json!({"feature":cut,"suppressed":"true"}),
    ] {
        client.refuse_unchanged("feature_suppress", args);
    }
    volume(&client.ok("doc_info", json!({}))["bodies"][0], 20.0 * 20.0 * 10.0 - 5.0 * 5.0 * 10.0);
}

#[test]
fn e56_stdio_lost_cut_rim_pick_refuses_and_restores_every_archive_entry() {
    let mut c = Client::start();
    c.ok("doc_new", json!({}));
    let stock_profile = rect(&mut c, "stock profile", 10.0, 20.0, 0.0, 0.0);
    let base = c.ok("extrude", json!({"sketch":stock_profile,"height":20,"name":"stock"}))["body"].clone();
    let circle = c.ok("sketch_create", json!({"plane":"XY","name":"circular cut"}))["sketch"].clone();
    c.ok("sketch_add", json!({"sketch":circle,"entities":[{"type":"circle","d":4}]}));
    let cut = c.ok("extrude", json!({"sketch":circle,"height":20,"op":"cut","target":base,"name":"cut"}))["body"].clone();
    // Cylinder radius 2 through depth 20: 10×20×20 − π×2²×20.
    volume(&c.ok("doc_info", json!({}))["bodies"][0], 10.0 * 20.0 * 20.0 - std::f64::consts::PI * 2.0_f64.powi(2) * 20.0);
    let topology = c.ok("topology", json!({"body":cut}));
    let rims: Vec<_> = topology["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["kind"] == "circle"
                && (e["radius"].as_f64().unwrap() - 2.0).abs() < 1e-6
                && (e["center"][2].as_f64().unwrap() - 20.0).abs() < 1e-6
        })
        .collect();
    assert_eq!(rims.len(), 1);
    let rim = rims[0]["id"].clone();
    let fillet = c.ok("fillet", json!({"body":cut,"edges":[rim],"radius":0.5,"name":"rim fillet"}))["body"].clone();
    // The concave top rim removes the integral π[(R+f−sqrt(f²−u²))²−R²], u=0..f.
    let (r, f) = (2.0, 0.5_f64);
    let loss = std::f64::consts::PI * (2.0 * r * f * f + 5.0 / 3.0 * f.powi(3) - std::f64::consts::PI / 2.0 * (r + f) * f * f);
    volume(&c.ok("doc_info", json!({}))["bodies"][0], 10.0 * 20.0 * 20.0 - std::f64::consts::PI * r * r * 20.0 - loss);
    let path = std::env::temp_dir().join(format!("e56-lost-pick-{}.qcad", std::process::id()));
    let before_archive = archive(&mut c, &path);
    let before = c.snapshot();
    let error = c.tool("feature_suppress", json!({"feature":cut,"suppressed":true})).expect_err("lost native rim pick must be refused");
    assert!(error.contains("edge") || error.contains("Edge"), "{error}");
    assert_eq!(c.snapshot(), before);
    assert_eq!(archive(&mut c, &path), before_archive, "recipe, diagnostics and every live B-rep archive entry restore exactly");
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "fillet", "refusal added no history");
    assert_eq!(c.ok("doc_info", json!({}))["bodies"][0]["id"], cut);
    assert_ne!(fillet, cut);
    volume(&c.ok("doc_info", json!({}))["bodies"][0], 10.0 * 20.0 * 20.0 - std::f64::consts::PI * 2.0_f64.powi(2) * 20.0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn e56_stdio_batch_suppression_failure_restores_and_success_is_one_undo() {
    let Fixture { mut client, sketch, cut, .. } = fixture();
    let path = std::env::temp_dir().join(format!("e56-batch-{}.qcad", std::process::id()));
    let before_archive = archive(&mut client, &path);
    let before = client.snapshot();
    let error = client
        .tool(
            "batch",
            json!({"steps":[
                {"tool":"feature_suppress","args":{"feature":cut,"suppressed":true}},
                {"tool":"extrude","args":{"sketch":"missing","height":1}}
            ]}),
        )
        .expect_err("second step must fail after suppression");
    assert!(error.contains("step 2") && error.contains("extrude"), "{error}");
    assert_eq!(client.snapshot(), before);
    assert_eq!(archive(&mut client, &path), before_archive);
    let batch = client.ok(
        "batch",
        json!({"verbose":true,"steps":[
            {"tool":"feature_suppress","args":{"feature":cut,"suppressed":true}},
            {"tool":"extrude","args":{"sketch":sketch,"height":2,"op":"new_body","name":"independent"}}
        ]}),
    );
    assert_eq!(batch["ok"], true);
    assert_eq!(batch["steps"].as_array().unwrap().len(), 2);
    assert_eq!(batch["steps"][0]["result"]["feature"], json!({"id":cut,"name":"cut"}));
    assert_eq!(batch["steps"][0]["result"]["changed"], true);
    assert_eq!(batch["steps"][0]["result"]["suppressed"], true);
    assert_eq!(batch["bodies"].as_array().unwrap().len(), 2);
    let added = &batch["steps"][1]["result"]["body"];
    for (id, expected) in [(&cut, 20.0 * 20.0 * 10.0), (added, 20.0 * 20.0 * 2.0)] {
        volume(batch["bodies"].as_array().unwrap().iter().find(|b| b["id"] == *id).unwrap(), expected);
    }
    assert_eq!(client.ok("undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(client.snapshot(), before);
    std::fs::remove_file(path).unwrap();
}
