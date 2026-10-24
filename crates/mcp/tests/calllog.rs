//! Persistent call logs exercised through the real stdio server.

use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "qymcad-mcp-calllog-test-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn command(home: &Path, log: Option<&std::ffi::OsStr>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_qymcad-mcp"));
    command.env("HOME", home).env_remove("XDG_STATE_HOME").env_remove("QYMCAD_MCP_LOG");
    if let Some(log) = log {
        command.env("QYMCAD_MCP_LOG", log);
    }
    command
}

fn run(mut command: Command, input: &str) -> Output {
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.as_bytes().to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success(), "server exited {}: {}", output.status, String::from_utf8_lossy(&output.stderr));
    output
}

fn request(id: Value, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

fn tool(id: u64, name: &str, args: Value) -> Value {
    request(json!(id), "tools/call", json!({ "name": name, "arguments": args }))
}

fn initialize() -> Value {
    request(
        json!(1),
        "initialize",
        json!({
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": { "name": "calllog-test", "version": "7" }
        }),
    )
}

fn lines(messages: &[Value]) -> String {
    messages.iter().map(|message| format!("{message}\n")).collect()
}

fn default_dir(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Logs/qymcad-mcp")
    } else {
        home.join(".local/state/qymcad-mcp")
    }
}

fn matches_name(name: &str, pid: u64) -> bool {
    let Some(time) = name.strip_prefix("qymcad-mcp-").and_then(|s| s.strip_suffix(&format!("-{pid}.jsonl"))) else {
        return false;
    };
    time.len() == 16
        && time.as_bytes()[8] == b'T'
        && time.as_bytes()[15] == b'Z'
        && time.as_bytes().iter().enumerate().all(|(i, byte)| i == 8 || i == 15 || byte.is_ascii_digit())
}

fn valid_timestamp(timestamp: &str) -> bool {
    let bytes = timestamp.as_bytes();
    bytes.len() == 24
        && [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':'), (19, b'.'), (23, b'Z')].iter().all(|(i, byte)| bytes[*i] == *byte)
        && bytes.iter().enumerate().all(|(i, byte)| [4, 7, 10, 13, 16, 19, 23].contains(&i) || byte.is_ascii_digit())
        && timestamp[5..7].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n))
        && timestamp[8..10].parse::<u8>().is_ok_and(|n| (1..=31).contains(&n))
        && timestamp[11..13].parse::<u8>().is_ok_and(|n| n < 24)
        && timestamp[14..16].parse::<u8>().is_ok_and(|n| n < 60)
        && timestamp[17..19].parse::<u8>().is_ok_and(|n| n < 60)
}

fn read_log(dir: &Path) -> (PathBuf, Vec<Value>) {
    let files: Vec<_> = fs::read_dir(dir).expect("call log directory must exist").map(|entry| entry.unwrap().path()).collect();
    assert_eq!(files.len(), 1, "one call log file per server process");
    let path = files[0].clone();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.ends_with('\n'), "each record includes its final newline");
    let records: Vec<Value> = text
        .lines()
        .map(|line| {
            assert!(line.len() < 256 * 1024, "record must stay below 256 KiB");
            serde_json::from_str(line).expect("every log line is valid JSON")
        })
        .collect();
    for record in &records {
        assert!(valid_timestamp(record["t"].as_str().expect("record has timestamp")), "invalid timestamp: {record}");
        assert!(record["kind"].is_string(), "record has kind: {record}");
    }
    let pid = records[0]["pid"].as_u64().expect("start includes pid");
    assert!(matches_name(path.file_name().unwrap().to_str().unwrap(), pid), "UTC timestamp and pid filename: {}", path.display());
    (path, records)
}

#[test]
fn logs_session_in_order_with_errors_and_redacted_images() {
    let temp = TempDir::new();
    let log_dir = temp.0.join("nested/logs");
    let calls = [
        ("doc_new", json!({})),
        ("param_set", json!({ "name": "Width", "value": 20 })),
        ("extrude", json!({ "sketch": "missing", "height": 5 })),
        ("sketch_create", json!({ "plane": "XY", "name": "s" })),
        ("sketch_add", json!({ "sketch": "s", "entities": [{ "type": "rect", "w": "Width", "h": 10 }] })),
        ("extrude", json!({ "sketch": "s", "height": 5, "name": "box" })),
        ("render", json!({ "view": "top", "width": 64, "height": 64 })),
    ];
    let mut messages = vec![
        initialize(),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        request(json!(90), "ping", json!({})),
        request(json!(91), "tools/list", json!({})),
        json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 100 } }),
    ];
    messages.extend(calls.iter().enumerate().map(|(i, (name, args))| tool(i as u64 + 2, name, args.clone())));
    messages.push(tool(100, "unknown-tool", json!({})));
    let input = format!("{}not JSON\n", lines(&messages));
    let output = run(command(&temp.0, Some(log_dir.as_os_str())), &input);
    let replies: Vec<Value> = String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let (path, records) = read_log(&log_dir);
    let kinds: Vec<_> = records.iter().map(|record| record["kind"].as_str().unwrap()).collect();
    let mut expected = vec!["start", "initialize"];
    for _ in &calls {
        expected.extend(["call", "result"]);
    }
    expected.extend(["rpc_error", "rpc_error", "stop"]);
    assert_eq!(kinds, expected, "calls precede results; notifications, ping and list create no records");
    let start = &records[0];
    assert_eq!(start["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(start["qymcad"], qymcad_engine::QYMCAD_VERSION);
    assert_eq!(Path::new(start["cwd"].as_str().unwrap()).canonicalize().unwrap(), std::env::current_dir().unwrap().canonicalize().unwrap());
    assert!(!start["app_note"].as_str().unwrap().is_empty());
    assert_eq!(start["log_path"], path.to_str().unwrap());
    assert_eq!(records[1]["clientInfo"], initialize()["params"]["clientInfo"]);
    assert_eq!(records[1]["protocolVersion"], "2024-11-05");
    assert_eq!(records[1]["answered_protocolVersion"], replies[0]["result"]["protocolVersion"]);
    for (i, (name, args)) in calls.iter().enumerate() {
        let call = &records[2 + 2 * i];
        let result = &records[3 + 2 * i];
        assert_eq!(call["seq"], i + 1);
        assert_eq!(result["seq"], i + 1);
        assert_eq!(call["tool"], *name);
        assert_eq!(result["tool"], *name);
        assert_eq!(call["args"], *args, "arguments recorded as received, before normalization");
        assert!(result["ms"].as_f64().is_some_and(|ms| ms >= 0.0));
        assert_eq!(result["is_error"], i == 2);
        let reply = replies.iter().find(|reply| reply["id"] == json!(i + 2)).unwrap();
        if *name != "render" {
            assert_eq!(result["content"], reply["result"]["content"], "text content preserved");
        } else {
            let image = &result["content"][0];
            assert_eq!(image["type"], "image");
            assert_eq!(image["mimeType"], "image/png");
            assert_eq!(
                image["base64_bytes"],
                reply["result"]["content"][0]["data"].as_str().unwrap().len(),
                "base64_bytes counts the original base64 string"
            );
            assert!(image.get("data").is_none(), "image base64 must never enter call log");
            assert_eq!(image.as_object().unwrap().len(), 3, "image contains type, mimeType and base64_bytes only");
            assert_eq!(result["content"][1], reply["result"]["content"][1], "render caption preserved");
        }
    }
    assert_eq!(records[16]["code"], -32602);
    assert_eq!(records[16]["method"], "tools/call");
    assert_eq!(records[16]["id"], 100);
    assert_eq!(records[17]["code"], -32700);
    assert_eq!(records[17]["id"], Value::Null);
    assert!(records[17].get("method").is_none());
    for error in &records[16..18] {
        assert!(!error["message"].as_str().unwrap().is_empty());
    }
    assert_eq!(records[18]["reason"], "eof");
    assert!(String::from_utf8(output.stderr).unwrap().lines().any(|line| line == format!("qymcad-mcp: call log: {}", path.display())));
}

#[test]
fn logs_every_protocol_error_with_the_answered_id() {
    let temp = TempDir::new();
    let dir = temp.0.join("logs");
    let input = format!(
        "{}malformed\n",
        lines(&[
            json!([]),
            json!({ "jsonrpc": "2.0", "id": "missing" }),
            request(json!(22), "unknown-method", json!({})),
            request(json!(23), "tools/call", json!({})),
            tool(24, "unknown-tool", json!({})),
        ])
    );
    let output = run(command(&temp.0, Some(dir.as_os_str())), &input);
    let replies: Vec<Value> = String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let (_, records) = read_log(&dir);
    assert_eq!(records.len(), replies.len() + 2);
    for (record, reply) in records[1..records.len() - 1].iter().zip(replies) {
        assert_eq!(record["kind"], "rpc_error");
        assert_eq!(record["code"], reply["error"]["code"]);
        assert_eq!(record["message"], reply["error"]["message"]);
        assert_eq!(record["id"], reply["id"]);
    }
    assert!(records[1].get("method").is_none());
    assert!(records[2].get("method").is_none());
    assert_eq!(records[3]["method"], "unknown-method");
    assert_eq!(records[4]["method"], "tools/call");
}

#[test]
fn logging_only_changes_initialize_instructions() {
    let temp = TempDir::new();
    let dir = temp.0.join("logs");
    let input = lines(&[
        initialize(),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        tool(2, "doc_new", json!({})),
        tool(3, "param_set", json!({ "name": "w", "value": 20 })),
        tool(4, "extrude", json!({ "sketch": "missing", "height": 5 })),
        tool(5, "sketch_create", json!({ "plane": "XY", "name": "s" })),
        tool(6, "sketch_add", json!({ "sketch": "s", "entities": [{ "type": "rect", "w": "w", "h": 10 }] })),
        tool(7, "extrude", json!({ "sketch": "s", "height": 5, "name": "box" })),
        tool(8, "render", json!({ "view": "top", "width": 64, "height": 64 })),
        request(json!(9), "ping", json!({})),
        request(json!(10), "tools/list", json!({})),
        tool(11, "unknown-tool", json!({})),
    ]);
    let input = format!("{input}not JSON\n");
    let logged = run(command(&temp.0, Some(dir.as_os_str())), &input);
    let off = run(command(&temp.0, Some(std::ffi::OsStr::new("off"))), &input);
    let logged_end = logged.stdout.iter().position(|byte| *byte == b'\n').expect("logged initialize response ends with a newline") + 1;
    let off_end = off.stdout.iter().position(|byte| *byte == b'\n').expect("off initialize response ends with a newline") + 1;
    let logged_initialize: Value = serde_json::from_slice(&logged.stdout[..logged_end]).unwrap();
    let mut expected_initialize: Value = serde_json::from_slice(&off.stdout[..off_end]).unwrap();
    let off_instructions = expected_initialize["result"]["instructions"].as_str().unwrap();
    assert!(!off_instructions.contains("Call log of this server process"), "off adds no log sentence");
    let (path, _) = read_log(&dir);
    expected_initialize["result"]["instructions"] =
        json!(format!("{off_instructions}\nCall log of this server process (JSONL, for diagnosing problems): {}.", path.display()));
    assert_eq!(
        logged_initialize["result"]["instructions"], expected_initialize["result"]["instructions"],
        "initialize advertises exactly the log file found on disk"
    );
    assert_eq!(logged_initialize, expected_initialize, "initialize differs only in result.instructions");
    assert_eq!(&logged.stdout[logged_end..], &off.stdout[off_end..], "every response after initialize is byte-identical");
}

#[test]
fn initialize_logs_the_requested_and_negotiated_protocol_versions() {
    let temp = TempDir::new();
    let dir = temp.0.join("logs");
    let output = run(
        command(&temp.0, Some(dir.as_os_str())),
        &lines(&[request(
            json!(1),
            "initialize",
            json!({ "protocolVersion": "1999-01-01", "clientInfo": { "name": "old-client", "version": "1" } }),
        )]),
    );
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    let (_, records) = read_log(&dir);
    assert_eq!(records.len(), 3);
    assert_eq!(records[1]["kind"], "initialize");
    assert_eq!(records[1]["protocolVersion"], "1999-01-01");
    assert_eq!(records[1]["answered_protocolVersion"], qymcad_mcp::transport::PROTOCOL_VERSIONS[0]);
    assert_eq!(records[1]["answered_protocolVersion"], reply["result"]["protocolVersion"]);
    assert_eq!(records[1]["clientInfo"], json!({ "name": "old-client", "version": "1" }));
}

#[test]
fn off_aliases_create_no_default_directory() {
    for value in ["off", "OFF", "Off", "0", "false", "FALSE", "no", "No"] {
        let temp = TempDir::new();
        let output = run(command(&temp.0, Some(std::ffi::OsStr::new(value))), &lines(&[initialize(), tool(2, "doc_new", json!({}))]));
        assert!(!default_dir(&temp.0).exists(), "{value} must not create the default log directory");
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 0, "off creates no files or directories");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.lines().any(|line| line == "qymcad-mcp: call log: off"), "off announced: {stderr}");
        let reply: Value = serde_json::from_str(String::from_utf8(output.stdout).unwrap().lines().next().unwrap()).unwrap();
        assert!(!reply["result"]["instructions"].as_str().unwrap().contains("Call log of this server process"));
    }
}

#[test]
fn unset_and_empty_log_use_the_platform_default_under_home() {
    for value in [None, Some(std::ffi::OsStr::new(""))] {
        let temp = TempDir::new();
        run(command(&temp.0, value), &lines(&[initialize()]));
        read_log(&default_dir(&temp.0));
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn xdg_state_home_selects_the_default_location() {
    let temp = TempDir::new();
    let xdg = temp.0.join("custom-state");
    let mut cmd = command(&temp.0, None);
    cmd.env("XDG_STATE_HOME", &xdg);
    run(cmd, &lines(&[initialize()]));
    read_log(&xdg.join("qymcad-mcp"));
    assert!(!default_dir(&temp.0).exists());
}

#[test]
fn a_regular_file_log_location_disables_logging_once_and_keeps_serving() {
    let temp = TempDir::new();
    let path = temp.0.join("not-a-directory");
    fs::write(&path, "preserve").unwrap();
    let output = run(
        command(&temp.0, Some(path.as_os_str())),
        &lines(&[initialize(), tool(2, "doc_new", json!({})), tool(3, "param_set", json!({"name":"w","value":20}))]),
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "preserve");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().filter(|line| line.starts_with("qymcad-mcp: call log disabled: ")).count(), 1, "one disable note: {stderr}");
    for reply in String::from_utf8(output.stdout).unwrap().lines() {
        let reply: Value = serde_json::from_str(reply).unwrap();
        assert!(reply["result"].is_object(), "still serving normally: {reply}");
        assert_ne!(reply["result"]["isError"], true);
    }
}

#[test]
fn cli_modes_never_create_a_log_directory() {
    for mode in ["--version", "--dump-tools"] {
        let temp = TempDir::new();
        let dir = temp.0.join("logs");
        let mut cmd = command(&temp.0, Some(dir.as_os_str()));
        cmd.arg(mode);
        let output = run(cmd, "");
        assert!(!dir.exists(), "{mode} must not create a log directory");
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 0);
        assert!(!String::from_utf8(output.stderr).unwrap().contains("call log"));
        assert!(!output.stdout.is_empty());
    }
}
