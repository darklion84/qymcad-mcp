//! MCP over stdio: newline-delimited JSON-RPC 2.0, one request at a time (ADR 0002).
//!
//! Implemented: `initialize`, `notifications/initialized` (and any other notification: ignored), `ping`,
//! `tools/list`, `tools/call`. Everything else answers `-32601 method not found`.

use crate::calllog::{redact_images, CallLog};
use crate::tools::Registry;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::time::Instant;

/// Protocol revisions this server speaks, newest first. The client's requested version is echoed back when we
/// support it; otherwise we answer with our newest and the client decides.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Synchronous JSON-RPC server with optional local diagnostics.
pub struct Server {
    registry: Registry,
    instructions: String,
    log: Option<CallLog>,
    seq: u64,
}

impl Server {
    /// Construct a server without persistent diagnostics (also used by existing tests).
    pub fn new(registry: Registry, instructions: String) -> Server {
        Self::with_log(registry, instructions, None)
    }

    /// Construct a server sharing an optional process log with the startup panic hook.
    pub fn with_log(registry: Registry, instructions: String, log: Option<CallLog>) -> Server {
        Server { registry, instructions, log, seq: 0 }
    }

    /// Serve until stdin closes, recording EOF or any transport read/write/flush failure.
    pub fn run(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        let result = (|| {
            for line in input.lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                if let Some(reply) = self.handle_line(&line) {
                    writeln!(output, "{reply}")?;
                    output.flush()?;
                }
            }
            Ok(())
        })();
        self.record(match &result {
            Ok(()) => json!({"kind":"stop", "reason":"eof"}),
            Err(error) => json!({"kind":"stop", "reason":"io_error", "error":format!("{error}")}),
        });
        result
    }

    /// Handle one message. Returns the response line, or `None` for notifications.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(self.rpc_error(Value::Null, PARSE_ERROR, &format!("parse error: {e}"), None).to_string()),
        };
        if msg.is_array() {
            return Some(self.rpc_error(Value::Null, INVALID_REQUEST, "batches are not supported", None).to_string());
        }
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let Some(id) = id else {
            return None; // a notification (initialized, cancelled, ...): nothing to answer
        };
        let reply = match method {
            "initialize" => ok(id, self.initialize(&params)),
            "ping" => ok(id, json!({})),
            "tools/list" => ok(id, json!({ "tools": self.registry.list() })),
            "tools/call" => match self.call(&params) {
                Ok(result) => ok(id, result),
                Err(msg) => self.rpc_error(id, INVALID_PARAMS, &msg, Some(method)),
            },
            "" => self.rpc_error(id, INVALID_REQUEST, "missing method", None),
            other => self.rpc_error(id, METHOD_NOT_FOUND, &format!("method not found: {other}"), Some(other)),
        };
        Some(reply.to_string())
    }

    fn record(&self, record: Value) {
        if let Some(log) = &self.log {
            log.record(record);
        }
    }

    fn rpc_error(&self, id: Value, code: i64, message: &str, method: Option<&str>) -> Value {
        let mut record = json!({"kind":"rpc_error", "id":id, "code":code, "message":message});
        if let Some(method) = method {
            record["method"] = json!(method);
        }
        self.record(record);
        error(id, code, message)
    }

    fn initialize(&self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
        let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(PROTOCOL_VERSIONS[0]);
        self.record(json!({
            "kind":"initialize", "clientInfo":params.get("clientInfo"),
            "protocolVersion":params.get("protocolVersion"), "answered_protocolVersion":version,
        }));
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": {
                "name": "qymcad-mcp",
                "version": format!("{} (QymCAD {})", env!("CARGO_PKG_VERSION"), qymcad_engine::QYMCAD_VERSION),
            },
            "instructions": self.instructions,
        })
    }

    /// `tools/call`. Protocol-level problems (unknown tool, missing name) are JSON-RPC errors; a tool that runs
    /// and fails returns a normal result with `isError: true`, so the model sees the message.
    fn call(&mut self, params: &Value) -> Result<Value, String> {
        let name = params.get("name").and_then(Value::as_str).ok_or("tools/call without a tool name")?;
        if !self.registry.contains(name) {
            return Err(format!("unknown tool: {name}"));
        }
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        self.seq += 1;
        let seq = self.seq;
        self.record(json!({"kind":"call", "seq":seq, "tool":name, "args":args}));
        let began = Instant::now();
        let out = self.registry.call(name, args)?;
        let ms = began.elapsed().as_millis();
        let result = match out {
            Ok(content) => json!({ "content": content, "isError": false }),
            Err(message) => json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
        };
        self.record(json!({
            "kind":"result", "seq":seq, "tool":name, "ms":ms,
            "is_error":result["isError"], "content":redact_images(&result["content"]),
        }));
        Ok(result)
    }
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
