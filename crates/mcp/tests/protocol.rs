//! End-to-end: spawn the real binary and talk MCP over stdio.

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
    fn start() -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_qymcad-mcp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn qymcad-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Client { child, stdin, stdout, next: 1 }
    }

    fn send(&mut self, msg: Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad response {line:?}: {e}"));
        assert_eq!(v["id"], json!(id), "response id");
        v
    }

    /// Call a tool; returns (isError, parsed JSON payload or the error text as a string).
    fn tool(&mut self, name: &str, args: Value) -> (bool, Value) {
        let v = self.request("tools/call", json!({ "name": name, "arguments": args }));
        let r = &v["result"];
        assert!(r.is_object(), "tools/call {name}: {v}");
        let text = r["content"][0]["text"].as_str().unwrap_or_default().to_string();
        let payload = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (r["isError"] == json!(true), payload)
    }

    fn ok(&mut self, name: &str, args: Value) -> Value {
        let (is_err, v) = self.tool(name, args);
        assert!(!is_err, "{name} failed: {v}");
        v
    }

    fn init(&mut self) -> Value {
        let r = self.request(
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }),
        );
        self.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        r
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn initialize_negotiates_the_protocol() {
    let mut c = Client::start();
    let r = c.init();
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    assert!(r["result"]["serverInfo"]["version"].as_str().unwrap().contains(qymcad_engine::QYMCAD_VERSION));
    assert!(r["result"]["capabilities"]["tools"].is_object());
    let r = c.request("initialize", json!({ "protocolVersion": "1999-01-01" }));
    assert_eq!(r["result"]["protocolVersion"], qymcad_mcp::transport::PROTOCOL_VERSIONS[0]);
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
}

#[test]
fn lists_tools_with_object_schemas() {
    let mut c = Client::start();
    c.init();
    let tools = c.request("tools/list", json!({}))["result"]["tools"].as_array().unwrap().clone();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for want in [
        "doc_new",
        "doc_open",
        "doc_save",
        "doc_info",
        "param_set",
        "sketch_create",
        "sketch_add",
        "extrude",
        "plane_offset",
        "topology",
        "select",
        "revolve",
        "fillet",
        "chamfer",
        "hole",
        "shell",
        "push_face",
        "linear_array",
        "circular_array",
        "mirror",
    ] {
        assert!(names.contains(&want), "missing {want} in {names:?}");
    }
    for t in &tools {
        assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        assert!(!t["description"].as_str().unwrap().is_empty());
    }
}

#[test]
fn errors_are_reported_the_right_way() {
    let mut c = Client::start();
    c.init();
    assert_eq!(c.request("nope", json!({}))["error"]["code"], -32601);
    assert!(c.request("tools/call", json!({ "name": "no_such_tool" }))["error"].is_object());
    let (is_err, msg) = c.tool("doc_info", json!({}));
    assert!(is_err && msg.as_str().unwrap().contains("doc_new"), "{msg}");
    c.ok("doc_new", json!({}));
    let (is_err, msg) = c.tool("param_set", json!({ "name": "x" }));
    assert!(is_err && msg.as_str().unwrap().contains("bad arguments"), "{msg}");
    let (is_err, msg) = c.tool("sketch_create", json!({ "plane": "XY", "typo": 1 }));
    assert!(is_err, "unknown fields are rejected: {msg}");
}

/// The golden plate (see crates/engine/tests/golden_plate.rs), built purely through MCP calls.
#[test]
fn builds_saves_and_reopens_the_plate() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    for (k, v) in [("w", 60.0), ("l", 40.0), ("t", 6.0), ("d", 4.5), ("hx", 22.0), ("hy", 12.0), ("pw", 30.0), ("pl", 16.0), ("pd", 3.0)] {
        c.ok("param_set", json!({ "name": k, "value": v }));
    }
    c.ok("sketch_create", json!({ "plane": "XY", "name": "plate sketch" }));
    let r = c.ok(
        "sketch_add",
        json!({ "sketch": "plate sketch", "entities": [
            { "type": "rect", "w": "w", "h": "l" },
            { "type": "circle", "cx": "hx", "cy": "hy", "d": "d" },
            { "type": "circle", "cx": "-hx", "cy": "hy", "d": "d" },
            { "type": "circle", "cx": "-hx", "cy": "-hy", "d": "d" },
            { "type": "circle", "cx": "hx", "cy": "-hy", "d": "d" },
        ]}),
    );
    assert_eq!(r["sketch"]["dof"], json!([0, 0]), "fully defined: {r}");
    assert_eq!(r["sketch"]["contours"].as_array().unwrap().len(), 5);
    c.ok("extrude", json!({ "sketch": "plate sketch", "height": "t", "name": "plate" }));
    let p = c.ok("plane_offset", json!({ "base": "XY", "dist": "t", "name": "top" }));
    assert!(p["plane"].is_u64());
    c.ok("sketch_create", json!({ "plane": { "plane": "top" }, "name": "pocket sketch" }));
    c.ok("sketch_add", json!({ "sketch": "pocket sketch", "entities": [{ "type": "rect", "w": "pw", "h": "pl" }] }));
    let r = c.ok("extrude", json!({ "sketch": "pocket sketch", "height": "pd", "op": "cut", "direction": "reverse", "name": "pocket" }));
    let v = r["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap();
    assert!((v - 12578.2965).abs() < 1e-3, "volume {v}");

    let path = std::env::temp_dir().join(format!("qymcad-mcp-protocol-{}.qcad", std::process::id()));
    let saved = c.ok("doc_save", json!({ "path": path.to_str().unwrap() }));
    assert!(saved["saved"].as_str().unwrap().ends_with(".qcad"));

    let mut c2 = Client::start();
    c2.init();
    let o = c2.ok("doc_open", json!({ "path": path.to_str().unwrap() }));
    let v2 = o["doc"]["bodies"][0]["volume"].as_f64().unwrap();
    assert!((v2 - 12578.2965).abs() < 1e-3, "reopened volume {v2}");
    let r = c2.ok("param_set", json!({ "name": "t", "value": 10 }));
    let v3 = r["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap();
    assert!((v3 - 21923.8275).abs() < 1.0, "after t=10: {v3}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_failed_feature_is_an_error_result_and_changes_nothing() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("sketch_create", json!({ "plane": "XY", "name": "s" }));
    c.ok("sketch_add", json!({ "sketch": "s", "entities": [{ "type": "rect", "w": 10, "h": 10 }] }));
    let (is_err, msg) = c.tool("extrude", json!({ "sketch": "s", "height": 5, "op": "cut" }));
    assert!(is_err, "cut without a body: {msg}");
    c.ok("extrude", json!({ "sketch": "s", "height": 5 }));
    let info = c.ok("doc_info", json!({}));
    assert_eq!(info["bodies"].as_array().unwrap().len(), 1);
}

#[test]
fn paths_are_restricted_to_cad_files() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    let home = std::env::var("HOME").unwrap();
    let (is_err, msg) = c.tool("doc_save", json!({ "path": format!("{home}/.zshrc") }));
    assert!(is_err && msg.as_str().unwrap().contains(".qcad"), "{msg}");
    let (is_err, _) = c.tool("doc_open", json!({ "path": "/etc/passwd" }));
    assert!(is_err);
    let (is_err, msg) = c.tool("doc_save", json!({ "path": "/no/such/dir/x.qcad" }));
    assert!(is_err, "{msg}");
}

#[test]
fn symlinks_are_not_followed() {
    let dir = std::env::temp_dir().join(format!("qymcad-mcp-symlink-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let victim = dir.join("victim.txt");
    std::fs::write(&victim, "keep me").unwrap();
    let link = dir.join("evil.qcad");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&victim, &link).unwrap();
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    let (is_err, msg) = c.tool("doc_save", json!({ "path": link.to_str().unwrap() }));
    assert!(is_err && msg.as_str().unwrap().contains("symbolic link"), "{msg}");
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me");
    let _ = std::fs::remove_dir_all(&dir);
}

fn volume(r: &Value) -> f64 {
    r["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap()
}

/// The agent's loop: read the topology, pick faces/edges, apply features; descriptive selections and errors.
#[test]
fn topology_pick_then_hole_and_fillet() {
    use std::f64::consts::PI;
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("param_set", json!({ "name": "d", "value": 6 }));
    c.ok("sketch_create", json!({ "plane": "XY", "name": "s" }));
    c.ok("sketch_add", json!({ "sketch": "s", "entities": [{ "type": "rect", "w": 40, "h": 30 }] }));
    c.ok("extrude", json!({ "sketch": "s", "height": 10, "name": "block" }));

    let t = c.ok("topology", json!({ "face_kind": ["plane"], "facing": "+z", "edges": false }));
    assert_eq!(t["faces_total"], 6);
    let faces = t["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 1, "{t}");
    assert_eq!(faces[0]["normal"], json!([0.0, 0.0, 1.0]));
    let top = faces[0]["id"].clone();

    let r = c.ok("hole", json!({ "face": top, "diameter": "d", "through": true, "name": "bore" }));
    let v1 = volume(&r);
    assert!((v1 - (12000.0 - PI * 9.0 * 10.0)).abs() < 1e-3, "through hole: {v1}");

    // Ids are re-read after the feature; vertical edges picked by filter, rounded by id.
    let t = c.ok("topology", json!({ "faces": false, "edge_kind": ["line"], "along": "z" }));
    // Five: the four corners plus the seam line of the hole's cylinder, flagged `seam`.
    assert_eq!(t["edges"].as_array().unwrap().len(), 5, "{t}");
    let ids: Vec<Value> = t["edges"].as_array().unwrap().iter().filter(|e| e["seam"] != json!(true)).map(|e| e["id"].clone()).collect();
    assert_eq!(ids.len(), 4, "{t}");
    let r = c.ok("fillet", json!({ "edges": ids, "radius": 3 }));
    let v2 = volume(&r);
    assert!((v1 - v2 - (4.0 - PI) * 9.0 * 10.0).abs() < 1e-3, "fillet: {v2}");

    // A description: the top outline is 4 lines + 4 arcs + the hole's circle.
    let sel = c.ok("select", json!({ "edges": { "edges_of": { "facing": "+z" } } }));
    assert_eq!(sel["count"], 9, "{sel}");
    let r = c.ok("chamfer", json!({ "edges": { "edges_of": { "of_feature": "block", "role": "cap_end" } }, "dist": 0.5 }));
    let v3 = volume(&r);
    assert!(v3 < v2);

    // The hole follows its parameter: d6 → d8 removes π(4² − 3²)·10 more, and the 0.5 mm chamfer ring on its
    // rim (2π(r + d/3)·d²/2) grows with the radius by 2π·1·0.125.
    let r = c.ok("param_set", json!({ "name": "d", "value": 8 }));
    let v4 = r["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap();
    let want = PI * 7.0 * 10.0 + 2.0 * PI * 0.125;
    assert!((v3 - v4 - want).abs() < 0.05, "d=8: removed {}, expected {want}", v3 - v4);

    // Clear errors for the model.
    let (is_err, msg) = c.tool("fillet", json!({ "edges": { "facing": "+z" }, "radius": 1 }));
    assert!(is_err && msg.as_str().unwrap().contains("along"), "{msg}");
    let (is_err, msg) = c.tool("fillet", json!({ "edges": { "top": true }, "radius": 1 }));
    assert!(is_err && msg.as_str().unwrap().contains("bad arguments"), "{msg}");
    let (is_err, msg) = c.tool("fillet", json!({ "edges": [123456], "radius": 1 }));
    assert!(is_err && msg.as_str().unwrap().contains("topology"), "{msg}");
    let (is_err, msg) = c.tool("fillet", json!({ "edges": { "along": "z" }, "radius": 50 }));
    assert!(is_err && msg.as_str().unwrap().contains("rolled back"), "{msg}");
    let (is_err, msg) = c.tool("hole", json!({ "face": { "facing": "+z" }, "diameter": 3 }));
    assert!(is_err && msg.as_str().unwrap().contains("through"), "{msg}");
}

/// Revolve, shell, arrays and mirror through MCP.
#[test]
fn revolve_shell_array_mirror() {
    use std::f64::consts::PI;
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("sketch_create", json!({ "plane": "XY", "name": "ring" }));
    c.ok("sketch_add", json!({ "sketch": "ring", "entities": [{ "type": "rect", "cx": 15, "cy": 15, "w": 10, "h": 30 }] }));
    let r = c.ok("revolve", json!({ "sketch": "ring", "axis": "sketch_y" }));
    assert!((volume(&r) - PI * 300.0 * 30.0).abs() < 0.5, "tube {}", volume(&r));

    c.ok("doc_new", json!({}));
    c.ok("sketch_create", json!({ "plane": "XY", "name": "s" }));
    c.ok("sketch_add", json!({ "sketch": "s", "entities": [{ "type": "rect", "cx": 15, "w": 10, "h": 10 }] }));
    c.ok("extrude", json!({ "sketch": "s", "height": 10 }));
    let r = c.ok("shell", json!({ "open_faces": { "facing": "+z" }, "thickness": 1 }));
    assert!((volume(&r) - (1000.0 - 8.0 * 8.0 * 9.0)).abs() < 1e-3, "cup {}", volume(&r));
    let r = c.ok("circular_array", json!({ "count": 3 }));
    assert!((volume(&r) - 3.0 * 424.0).abs() < 1e-3, "3 cups {}", volume(&r));
    let r = c.ok("mirror", json!({ "plane": "XY" }));
    assert!((volume(&r) - 6.0 * 424.0).abs() < 1e-3, "mirrored {}", volume(&r));
    let r = c.ok("linear_array", json!({ "dz": 30, "count": 2 }));
    assert!((volume(&r) - 12.0 * 424.0).abs() < 1e-3, "stacked {}", volume(&r));
    let (is_err, msg) = c.tool("revolve", json!({ "sketch": "s", "axis": "W" }));
    assert!(is_err && msg.as_str().unwrap().contains("unknown axis"), "{msg}");
}
