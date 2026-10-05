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
        "sketch_info",
        "sketch_constrain",
        "sketch_remove",
        "extrude",
        "plane_offset",
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

/// New entity types and sketch_constrain end to end: a tombstone (polyline + arc), a hexagon pocket and a slot
/// hole, all parametric; then a free triangle dimensioned with sketch_constrain; a conflict and a bad reference.
#[test]
fn sketch_entities_and_constraints_end_to_end() {
    use std::f64::consts::PI;
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    for (k, v) in [("w", 40.0), ("hh", 30.0), ("t", 5.0), ("r", 6.0), ("l", 10.0), ("sw", 6.0), ("th", 20.0)] {
        c.ok("param_set", json!({ "name": k, "value": v }));
    }
    c.ok("sketch_create", json!({ "plane": "XY", "name": "s" }));
    let r = c.ok(
        "sketch_add",
        json!({ "sketch": "s", "entities": [
            { "type": "polyline", "points": [["-w/2", "hh"], ["-w/2", 0], ["w/2", 0], ["w/2", "hh"]] },
            { "type": "arc", "cy": "hh", "r": "w/2", "start_angle": 0, "end_angle": 180 },
            { "type": "polygon", "cy": 12, "sides": 6, "r": "r" },
            { "type": "slot", "x1": "-l/2", "y1": 32, "x2": "l/2", "y2": 32, "width": "sw" },
        ]}),
    );
    assert_eq!(r["sketch"]["dof"], json!([0, 0]), "fully defined: {}", r["sketch"]["constraints"]);
    assert_eq!(r["created"].as_array().unwrap().len(), 4);
    assert_eq!(r["created"][1]["type"], "arc");
    assert_eq!(r["sketch"]["contours"].as_array().unwrap().len(), 3, "outline + two holes");
    let e = c.ok("extrude", json!({ "sketch": "s", "height": "t" }));
    let sw: f64 = 6.0; // the `sw` parameter: slot width
    let area = |w: f64, hh: f64, r: f64, l: f64| {
        w * hh + PI * (w / 2.0).powi(2) / 2.0 - 1.5 * 3f64.sqrt() * r * r - (PI * (sw / 2.0).powi(2) + sw * l)
    };
    let vol = |r: &Value| r["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap();
    assert!((vol(&e) - area(40.0, 30.0, 6.0, 10.0) * 5.0).abs() < 1e-2, "volume {}", vol(&e));
    let p = c.ok("param_set", json!({ "name": "r", "value": 7 }));
    assert!((vol(&p) - area(40.0, 30.0, 7.0, 10.0) * 5.0).abs() < 1e-2, "r=7: {}", vol(&p));
    let p = c.ok("param_set", json!({ "name": "l", "value": 14 }));
    assert!((vol(&p) - area(40.0, 30.0, 7.0, 14.0) * 5.0).abs() < 1e-2, "l=14: {}", vol(&p));

    // A free triangle below the x axis, dimensioned with sketch_constrain: base `w`, height `th` from the axis.
    c.ok("sketch_create", json!({ "plane": "XY", "name": "tri" }));
    let r = c.ok(
        "sketch_add",
        json!({ "sketch": "tri", "entities": [{ "type": "polyline", "points": [[0, 0], [30, 0], [0, -12]], "closed": true, "dimensioned": false }] }),
    );
    assert_eq!(r["sketch"]["dof"], json!([6, 0]));
    let lines = r["created"][0]["entities"].clone();
    let pts = r["created"][0]["points"].clone();
    c.ok("sketch_constrain", json!({ "sketch": "tri", "kind": "coincident", "refs": [pts[0], "origin"] }));
    c.ok("sketch_constrain", json!({ "sketch": "tri", "kind": "horizontal", "refs": [lines[0]] }));
    c.ok("sketch_constrain", json!({ "sketch": "tri", "kind": "vertical", "refs": [lines[2]] }));
    c.ok("sketch_constrain", json!({ "sketch": "tri", "kind": "distance", "refs": [lines[0]], "value": "w" }));
    let r = c.ok("sketch_constrain", json!({ "sketch": "tri", "kind": "distance", "refs": [pts[2], "x_axis"], "value": "th" }));
    assert_eq!(r["dof"], json!([0, 0]), "{r}");
    let i = r["index"].as_u64().unwrap() as usize;
    // Checks the triangle (0,0) (w,0) (0,−th): coordinates, the stored (signed: below the axis) distance, the area.
    let check = |sk: &Value, w: f64, th: f64| {
        let pt = |k: usize| {
            let p = sk["points"].as_array().unwrap().iter().find(|p| p["id"] == pts[k]).unwrap().clone();
            (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap())
        };
        for (k, want) in [(0, (0.0, 0.0)), (1, (w, 0.0)), (2, (0.0, -th))] {
            let got = pt(k);
            assert!((got.0 - want.0).abs() < 1e-6 && (got.1 - want.1).abs() < 1e-6, "point {k}: {got:?} vs {want:?}");
        }
        assert_eq!(sk["constraints"][i]["kind"], "distance_point_line");
        assert_eq!(sk["constraints"][i]["expr"], "th");
        assert!((sk["constraints"][i]["value"].as_f64().unwrap() + th).abs() < 1e-9, "stored signed −th: {}", sk["constraints"][i]);
        let a = sk["contours"][0]["area"].as_f64().unwrap();
        assert!((a - w * th / 2.0).abs() < 1e-6, "area {a}");
    };
    check(&r["sketch"], 40.0, 20.0);
    c.ok("param_set", json!({ "name": "th", "value": 25 }));
    let info = c.ok("sketch_info", json!({ "sketch": "tri" }));
    check(&info, 40.0, 25.0);

    let (is_err, msg) = c.tool("sketch_constrain", json!({ "sketch": "tri", "kind": "distance", "refs": [pts[1], pts[2]], "value": 99 }));
    assert!(is_err && msg.as_str().unwrap().contains("over-constrains"), "{msg}");
    let (is_err, msg) = c.tool("sketch_constrain", json!({ "sketch": "tri", "kind": "horizontal", "refs": [123456] }));
    assert!(is_err && msg.as_str().unwrap().contains("not found"), "{msg}");
    let (is_err, msg) =
        c.tool("sketch_add", json!({ "sketch": "tri", "entities": [{ "type": "line", "x1": 0, "y1": 0, "x2": 1, "y2": 1, "bogus": 1 }] }));
    assert!(is_err && msg.as_str().unwrap().contains("bogus"), "unknown fields are rejected: {msg}");
    let info = c.ok("sketch_info", json!({ "sketch": "tri" }));
    assert_eq!(info["dof"], json!([0, 0]), "failed calls changed nothing");
    let n = info["constraints"].as_array().unwrap().len();
    let r = c.ok("sketch_remove", json!({ "sketch": "tri", "constraint": n - 1 }));
    assert_eq!(r["sketch"]["dof"], json!([1, 0]));
    let (is_err, _) = c.tool("sketch_remove", json!({ "sketch": "tri" }));
    assert!(is_err);
    let (is_err, msg) = c.tool("sketch_add", json!({ "sketch": "tri", "entities": [] }));
    assert!(is_err && msg.as_str().unwrap().contains("empty"), "{msg}");
}
