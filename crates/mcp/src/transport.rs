//! MCP over stdio: newline-delimited JSON-RPC 2.0, one request at a time (ADR 0002).
//!
//! Implemented: `initialize`, `notifications/initialized` (and any other notification: ignored), `ping`,
//! `tools/list`, `tools/call`. Everything else answers `-32601 method not found`.

use crate::tools::Registry;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// Protocol revisions this server speaks, newest first. The client's requested version is echoed back when we
/// support it; otherwise we answer with our newest and the client decides.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

pub struct Server {
    registry: Registry,
    instructions: String,
}

impl Server {
    pub fn new(registry: Registry, instructions: String) -> Server {
        Server { registry, instructions }
    }

    /// Serve until stdin closes.
    pub fn run(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
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
    }

    /// Handle one message. Returns the response line, or `None` for notifications.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error(Value::Null, PARSE_ERROR, &format!("parse error: {e}")).to_string()),
        };
        if msg.is_array() {
            return Some(error(Value::Null, INVALID_REQUEST, "batches are not supported").to_string());
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
                Err(msg) => error(id, INVALID_PARAMS, &msg),
            },
            "" => error(id, INVALID_REQUEST, "missing method"),
            other => error(id, METHOD_NOT_FOUND, &format!("method not found: {other}")),
        };
        Some(reply.to_string())
    }

    fn initialize(&self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
        let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(PROTOCOL_VERSIONS[0]);
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
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        let out = self.registry.call(name, args)?;
        Ok(match out {
            Ok(content) => json!({ "content": content, "isError": false }),
            Err(message) => json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
        })
    }
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
