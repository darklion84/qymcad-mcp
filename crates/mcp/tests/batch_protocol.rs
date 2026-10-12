//! Batch tool schema, stdio execution, and bounded per-call logging through the real binary.

use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "qymcad-mcp-batch-protocol-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})
}

fn tool(id: u64, name: &str, args: Value) -> Value {
    request(id, "tools/call", json!({"name":name, "arguments":args}))
}

fn initialized() -> Value {
    request(1, "initialize", json!({"protocolVersion":"2025-06-18", "capabilities":{}, "clientInfo":{"name":"batch-test", "version":"0"}}))
}

fn run(messages: &[Value], log_dir: Option<&Path>) -> Vec<Value> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_qymcad-mcp"));
    if let Some(path) = log_dir {
        command.env("QYMCAD_MCP_LOG", path);
    } else {
        command.env("QYMCAD_MCP_LOG", "off");
    }
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input: String = messages.iter().map(|message| format!("{message}\n")).collect();
    // Read output while writing: the 200-step verbose response can exceed pipe capacity.
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success(), "server exited {}: {}", output.status, String::from_utf8_lossy(&output.stderr));
    let responses: Vec<Value> = String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(responses.len(), messages.len(), "one response for each request");
    for (response, message) in responses.iter().zip(messages) {
        assert_eq!(response["id"], message["id"]);
    }
    responses
}

fn payload(response: &Value) -> Value {
    assert!(response["error"].is_null(), "tool must be registered: {response}");
    assert_ne!(response["result"]["isError"], true, "tool must succeed: {response}");
    let content = response["result"]["content"].as_array().expect("content array");
    assert_eq!(content.len(), 1, "batch and modelling responses contain one text item");
    assert_eq!(content[0]["type"], "text", "batch cannot produce image content");
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

fn resolve_schema<'a>(schema: &'a Value, root: &'a Value) -> &'a Value {
    match schema["$ref"].as_str() {
        Some(reference) => root.pointer(reference.strip_prefix('#').expect("local schema reference")).expect("valid schema reference"),
        None => schema,
    }
}

#[test]
fn tools_list_advertises_batch_with_a_valid_structured_schema() {
    let responses = run(&[initialized(), request(2, "tools/list", json!({}))], None);
    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    let batch = tools.iter().find(|tool| tool["name"] == "batch").expect("tools/list must advertise batch");
    assert!(!batch["description"].as_str().unwrap().is_empty());
    let schema = &batch["inputSchema"];
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["additionalProperties"], false, "reject misspelled batch arguments");
    assert_eq!(schema["required"], json!(["steps"]));
    assert_eq!(schema["properties"]["verbose"]["type"], "boolean");
    assert_eq!(schema["properties"]["verbose"]["default"], false);
    let steps = &schema["properties"]["steps"];
    assert_eq!(steps["type"], "array");
    let step = resolve_schema(&steps["items"], schema);
    assert_eq!(step["type"], "object");
    assert_eq!(step["additionalProperties"], false, "reject misspelled step fields");
    let required = step["required"].as_array().unwrap();
    assert_eq!(required.len(), 2);
    assert!(required.contains(&json!("tool")) && required.contains(&json!("args")));
    assert_eq!(step["properties"]["tool"]["type"], "string");
    assert!(step["properties"].get("args").is_some(), "every step carries arguments");
    for property in ["steps", "verbose"] {
        assert!(schema["properties"][property]["description"].is_string(), "batch field {property} is documented");
    }
}

#[test]
fn batch_builds_named_parametric_geometry_and_undoes_once_over_stdio() {
    let args = json!({"steps":[
        {"tool":"param_set", "args":{"name":"width", "value":20}},
        {"tool":"sketch_create", "args":{"plane":"XY", "name":"profile"}},
        {"tool":"sketch_add", "args":{"sketch":"profile", "entities":json!([{"type":"rect", "w":"width", "h":12}]).to_string()}},
        {"tool":"extrude", "args":{"sketch":"profile", "height":3, "name":"plate"}}
    ]});
    let responses = run(
        &[
            initialized(),
            tool(2, "doc_new", json!({})),
            tool(3, "batch", args.clone()),
            tool(4, "doc_info", json!({})),
            tool(5, "undo", json!({})),
            tool(6, "doc_info", json!({})),
        ],
        None,
    );
    let batch = payload(&responses[2]);
    assert_eq!(batch["ok"], true);
    let steps = batch["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 4);
    for (i, step) in steps.iter().enumerate() {
        assert_eq!(step["step"], i + 1);
        assert_eq!(step["tool"], args["steps"][i]["tool"]);
        assert!(step.get("result").is_none(), "verbose defaults to false");
    }
    assert_eq!(steps[1]["created"]["name"], "profile");
    assert_eq!(steps[3]["created"]["name"], "plate");
    let bodies = batch["bodies"].as_array().unwrap();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["name"], "plate");
    assert!((bodies[0]["volume_mm3"].as_f64().unwrap() - 720.0).abs() < 1e-6);
    assert_eq!(bodies[0]["bbox"], json!([-10.0, -6.0, 0.0, 10.0, 6.0, 3.0]));
    assert_eq!(batch["bodies"], payload(&responses[3])["bodies"]);
    let undo = payload(&responses[4]);
    assert_eq!(undo["undone"]["tool"], "batch");
    assert_eq!(undo["undone"]["label"], "Batch (4 steps)");
    assert_eq!(undo["undone"]["arguments"], args);
    let after = payload(&responses[5]);
    for field in ["params", "sketches", "timeline", "bodies"] {
        assert_eq!(after[field], json!([]), "undo clears the complete batch: {field}");
    }
}

#[test]
fn verbose_200_step_batch_is_one_bounded_call_log_pair() {
    const RECORD_LIMIT: usize = 256 * 1024;
    let temp = TempDir::new();
    let log_dir = temp.0.join("logs");
    let steps: Vec<Value> = (0..200)
        .map(|i| json!({"tool":"sketch_create", "args":{"plane":"XY", "name":format!("profile_{i}_{}", "x".repeat(2000))}}))
        .collect();
    let args = json!({"steps":steps, "verbose":true});
    assert!(args.to_string().len() > RECORD_LIMIT, "exercise truncation of the actual batch call");
    let responses = run(&[initialized(), tool(2, "doc_new", json!({})), tool(3, "batch", args)], Some(&log_dir));
    let batch = payload(&responses[2]);
    assert_eq!(batch["ok"], true);
    assert_eq!(batch["steps"].as_array().unwrap().len(), 200);
    for step in batch["steps"].as_array().unwrap() {
        assert_eq!(step["result"]["sketch"], step["created"]["id"], "verbose includes the normal handler result");
    }
    assert!(responses[2].to_string().len() > RECORD_LIMIT, "exercise truncation of the actual verbose result");
    let files: Vec<_> = fs::read_dir(&log_dir).unwrap().map(|entry| entry.unwrap().path()).collect();
    assert_eq!(files.len(), 1, "one process log");
    let bytes = fs::read(&files[0]).unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    let records: Vec<Value> = bytes
        .split_inclusive(|byte| *byte == b'\n')
        .map(|line| {
            assert!(line.len() < RECORD_LIMIT, "record including its newline must be below 256 KiB: {} bytes", line.len());
            serde_json::from_slice(line).expect("truncated log remains valid JSON")
        })
        .collect();
    let calls: Vec<_> = records.iter().filter(|record| record["kind"] == "call").collect();
    let results: Vec<_> = records.iter().filter(|record| record["kind"] == "result").collect();
    assert_eq!(calls.len(), 2, "doc_new plus one batch call; no internal step calls");
    assert_eq!(results.len(), 2, "doc_new plus one batch result; no internal step results");
    assert_eq!(calls[1]["tool"], "batch");
    assert_eq!(results[1]["tool"], "batch");
    assert_eq!(calls[1]["seq"], results[1]["seq"]);
    assert_eq!(results[1]["is_error"], false);
    for record in [calls[1], results[1]] {
        assert!(record.to_string().contains("[truncated "), "oversized batch log is marked as truncated");
    }
}
