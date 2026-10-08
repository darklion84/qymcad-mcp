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
    let expected = [
        "doc_new",
        "doc_open",
        "doc_save",
        "doc_info",
        "param_set",
        "param_delete",
        "sketch_create",
        "sketch_add",
        "sketch_info",
        "sketch_constrain",
        "sketch_remove",
        "extrude",
        "plane_offset",
        "export",
        "render",
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
        "feature_delete",
        "undo",
    ];
    for want in expected {
        assert!(names.contains(&want), "missing {want} in {names:?}");
    }
    assert_eq!(names.len(), expected.len(), "every registered tool must be covered");
    for t in &tools {
        assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        assert!(!t["description"].as_str().unwrap().is_empty());
    }
}

#[test]
fn output_descriptions_explain_document_wide_refusal() {
    let mut c = Client::start();
    c.init();
    let tools = c.request("tools/list", json!({}))["result"]["tools"].as_array().unwrap().clone();
    for name in ["export", "render"] {
        let description = tools.iter().find(|t| t["name"] == name).unwrap()["description"].as_str().unwrap();
        assert!(
            description.contains("any timeline node has a regeneration error, even when the selected bodies are clean"),
            "{name} must describe document-wide refusal: {description}"
        );
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
    let expected = |t: f64| 60.0 * 40.0 * t - 4.0 * std::f64::consts::PI * (4.5_f64 / 2.0).powi(2) * t - 30.0 * 16.0 * 3.0;
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
    // Literal pocket dimensions expose F-017; expression-driven pockets are covered by golden_plate::build().
    c.ok("sketch_add", json!({ "sketch": "pocket sketch", "entities": [{ "type": "rect", "w": 30, "h": 16 }] }));
    let r = c.ok("extrude", json!({ "sketch": "pocket sketch", "height": "pd", "op": "cut", "direction": "reverse", "name": "pocket" }));
    let v = r["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap();
    assert!((v - expected(6.0)).abs() < 1e-3, "volume {v}");

    let path = std::env::temp_dir().join(format!("qymcad-mcp-protocol-{}.qcad", std::process::id()));
    let saved = c.ok("doc_save", json!({ "path": path.to_str().unwrap() }));
    assert!(saved["saved"].as_str().unwrap().ends_with(".qcad"));

    let mut c2 = Client::start();
    c2.init();
    let o = c2.ok("doc_open", json!({ "path": path.to_str().unwrap() }));
    let v2 = o["doc"]["bodies"][0]["volume_mm3"].as_f64().unwrap();
    assert!((v2 - expected(6.0)).abs() < 1e-3, "reopened volume {v2}");
    let r = c2.ok("param_set", json!({ "name": "t", "value": 10 }));
    let v3 = r["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap();
    let topo = c2.ok("topology", json!({ "edges": false }));
    let faces = topo["faces"].as_array().unwrap();
    let floor = faces
        .iter()
        .find(|f| (f["area"].as_f64().unwrap() - 30.0 * 16.0).abs() < 1e-3 && f["normal"][2].as_f64().unwrap_or(0.0) > 0.99)
        .unwrap();
    let z = floor["centroid"][2].as_f64().unwrap();
    assert!((z - (10.0 - 3.0)).abs() < 1e-6, "MCP pocket floor z: got {z}, expected 7");
    assert_eq!(faces.len(), 15, "open pocket must have no ceiling");
    assert!((v3 - expected(10.0)).abs() < 1e-3, "after t=10: {v3}");
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
    // A 0.5 mm chamfer removes a triangle of 0.5²/2 = 0.125 mm² along each edge: straight edges 2·34 + 2·24 =
    // 116 mm → 14.5; the four convex r 3 quarter arcs by Pappus at the triangle's centroid radius 3 − 0.5/3 →
    // 4·(π/2)(17/6)·0.125 = 17π/24; the hole rim at 3 + 0.5/3 → 2π(19/6)·0.125 = 19π/24. Total 14.5 + 1.5π
    // (tangent joins, no corner patches).
    let want = 14.5 + 1.5 * PI;
    assert!((v2 - v3 - want).abs() < 1e-3, "chamfer removed {}, expected {want}", v2 - v3);

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
    // QymCAD has no closed hollow shell: omitting the openings is refused by name (F-035).
    let (is_err, msg) = c.tool("shell", json!({ "thickness": 1 }));
    assert!(is_err && msg.as_str().unwrap().contains("open_faces"), "{msg}");
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

/// Review #27: an expression deep enough to overflow QymCAD's recursive parser must be refused, not crash the
/// server: the process answers a ping afterwards.
#[test]
fn a_pathological_expression_does_not_kill_the_server() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    let deep = format!("{}1{}", "(".repeat(100_000), ")".repeat(100_000));
    let (is_err, msg) = c.tool("param_set", json!({ "name": "x", "value": deep }));
    assert!(is_err && msg.as_str().unwrap().contains("expression error"), "{msg}");
    let (is_err, msg) = c.tool("plane_offset", json!({ "base": "XY", "dist": format!("{}1", "-".repeat(100_000)) }));
    assert!(is_err, "{msg}");
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
}

/// Review #2 and #16: vectors are exactly three finite numbers (a stray string was dropped silently, turning
/// [1, "x", 0, 0] into [1, 0, 0]); tolerances are degrees in [0, 90).
#[test]
fn numbers_are_parsed_strictly() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("sketch_create", json!({ "plane": "XY", "name": "s" }));
    c.ok("sketch_add", json!({ "sketch": "s", "entities": [{ "type": "circle", "cx": 30, "d": 10 }] }));
    c.ok("extrude", json!({ "sketch": "s", "height": 5 }));
    let bad = [
        ("select", json!({ "faces": { "facing": [1, "x", 0, 0] } })),
        ("select", json!({ "faces": { "facing": [0, 0] } })),
        ("select", json!({ "edges": { "along": [0, 0, 1, 5] } })),
        ("circular_array", json!({ "count": 3, "axis": { "origin": [0, 0, "z"], "dir": [0, 0, 1] } })),
        ("circular_array", json!({ "count": 3, "axis": { "origin": [0, 0, 0], "dir": [0, 0, 1, 0] } })),
        ("topology", json!({ "facing": [0, 0, 1], "tol_deg": 120 })),
        ("topology", json!({ "along": "z", "tol_deg": -1 })),
        ("topology", json!({ "facing": [0, "up", 1] })),
    ];
    for (tool, args) in bad {
        let (is_err, msg) = c.tool(tool, args.clone());
        assert!(is_err, "{tool} {args} was accepted: {msg}");
    }
    // Well-formed ones still work.
    c.ok("select", json!({ "faces": { "facing": [0, 0, 1] } }));
    c.ok("topology", json!({ "facing": "+z", "tol_deg": 10 }));
}

/// A 20 × 10 × 5 box, built through MCP calls.
fn build_box(c: &mut Client) {
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("sketch_create", json!({ "plane": "XY", "name": "s" }));
    c.ok("sketch_add", json!({ "sketch": "s", "entities": [{ "type": "rect", "w": 20, "h": 10 }] }));
    c.ok("extrude", json!({ "sketch": "s", "height": 5, "name": "box" }));
}

#[test]
fn exports_through_mcp() {
    let dir = std::env::temp_dir().join(format!("qymcad-mcp-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut c = Client::start();
    build_box(&mut c);
    let stl = dir.join("box.stl");
    let r = c.ok("export", json!({ "format": "stl", "path": stl.to_str().unwrap(), "quality": "draft" }));
    assert_eq!(r["triangles"], json!(12), "a box is 12 triangles: {r}");
    assert_eq!(r["bodies"][0]["name"], "box");
    assert!((r["bodies"][0]["mesh_volume"].as_f64().unwrap() - 1000.0).abs() < 1e-6, "{r}");
    assert_eq!(std::fs::metadata(&stl).unwrap().len(), 84 + 12 * 50);
    let step = dir.join("box.stp");
    let r = c.ok("export", json!({ "format": "step", "path": step.to_str().unwrap(), "bodies": ["box"] }));
    assert!(r["bytes"].as_u64().unwrap() > 1000, "{r}");
    assert!(std::fs::read_to_string(&step).unwrap().starts_with("ISO-10303-21;"));
    // OCCT prints transfer statistics to fd 1 on every STEP write; the protocol stream must stay clean (F-019)
    c.ok("export", json!({ "format": "step", "path": step.to_str().unwrap() }));
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
    // the extension must be the format's
    let (is_err, msg) = c.tool("export", json!({ "format": "stl", "path": dir.join("box.step").to_str().unwrap() }));
    assert!(is_err && msg.as_str().unwrap().contains(".stl"), "{msg}");
    let (is_err, msg) = c.tool("export", json!({ "format": "png", "path": dir.join("x.png").to_str().unwrap() }));
    assert!(is_err && msg.as_str().unwrap().contains("bad arguments"), "{msg}");
    let (is_err, msg) = c.tool("export", json!({ "format": "stl", "path": stl.to_str().unwrap(), "bodies": ["nope"] }));
    assert!(is_err && msg.as_str().unwrap().contains("nope"), "{msg}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_paths_are_restricted() {
    let mut c = Client::start();
    build_box(&mut c);
    let home = std::env::var("HOME").unwrap();
    for (format, path) in
        [("stl", format!("{home}/.zshrc")), ("step", format!("{home}/.ssh/authorized_keys")), ("obj", "/etc/hosts".into())]
    {
        let (is_err, msg) = c.tool("export", json!({ "format": format, "path": path }));
        assert!(is_err && msg.as_str().unwrap().contains("only"), "{format} {path}: {msg}");
    }
    let dir = std::env::temp_dir().join(format!("qymcad-mcp-export-link-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let victim = dir.join("victim.txt");
    std::fs::write(&victim, "keep me").unwrap();
    let link = dir.join("evil.stl");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&victim, &link).unwrap();
    let (is_err, msg) = c.tool("export", json!({ "format": "stl", "path": link.to_str().unwrap() }));
    assert!(is_err && msg.as_str().unwrap().contains("symbolic link"), "{msg}");
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Decode standard base64 (test-side, independent of the server's encoder).
fn unbase64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("not base64: {c}"),
    } as u32;
    assert_eq!(s.len() % 4, 0, "padded base64");
    let mut out = Vec::new();
    for q in s.as_bytes().chunks(4) {
        let pad = q.iter().filter(|c| **c == b'=').count();
        let n = q.iter().take(4 - pad).enumerate().fold(0u32, |n, (k, c)| n | val(*c) << (18 - 6 * k));
        out.extend_from_slice(&n.to_be_bytes()[1..4 - pad]);
    }
    out
}

#[test]
fn render_returns_an_image() {
    let mut c = Client::start();
    build_box(&mut c);
    let v = c.request("tools/call", json!({ "name": "render", "arguments": { "view": "top", "width": 200, "height": 100 } }));
    let content = v["result"]["content"].as_array().unwrap_or_else(|| panic!("{v}"));
    assert_ne!(v["result"]["isError"], json!(true), "{v}");
    assert_eq!(content[0]["type"], "image");
    assert_eq!(content[0]["mimeType"], "image/png");
    let png = unbase64(content[0]["data"].as_str().unwrap());
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 200, "IHDR width");
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 100, "IHDR height");
    assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    let text = content[1]["text"].as_str().unwrap();
    assert!(text.starts_with("top view") && text.contains("-10..10"), "{text}");
    let (is_err, msg) = c.tool("render", json!({ "width": 4 }));
    assert!(is_err && msg.as_str().unwrap().contains("width"), "{msg}");
    let (is_err, _) = c.tool("render", json!({ "view": "sideways" }));
    assert!(is_err);
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
