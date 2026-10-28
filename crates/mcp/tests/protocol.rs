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
            .env("QYMCAD_MCP_LOG", "off")
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
fn json_encoded_structured_arguments_match_native_arguments() {
    let mut results = Vec::new();
    for encoded in [false, true] {
        let argument = |v: Value| if encoded { json!(format!("  {v}")) } else { v };
        let mut c = Client::start();
        c.init();
        c.ok("doc_new", json!({}));
        c.ok("param_set", json!({ "name": "t", "value": 10 }));
        c.ok("sketch_create", json!({ "plane": "XY", "name": "{plate" }));
        c.ok(
            "sketch_add",
            json!({ "sketch": "{plate", "entities": argument(json!([
            { "type": "rect", "w": 20, "h": 16 }
        ])) }),
        );
        let block = c.ok("extrude", json!({ "sketch": "{plate", "height": "t-2", "name": "[block" }));
        // A 20 × 16 rectangle extruded by t−2 = 8 has volume 2560 mm³.
        assert!((volume(&block) - 20.0 * 16.0 * 8.0).abs() < 1e-6);
        let faces = c.ok("select", json!({ "faces": argument(json!({ "facing": "+z" })) }));
        assert_eq!(faces["count"], 1);
        let selected = c.ok("select", json!({ "edges": { "edges_of": { "facing": "+z" } } }));
        let edge_ids = json!(selected["edges"].as_array().unwrap().iter().map(|edge| edge["id"].clone()).collect::<Vec<_>>());
        assert_eq!(edge_ids.as_array().unwrap().len(), 4, "rectangle outline has four edges");
        let edges = c.ok("select", json!({ "edges": argument(edge_ids) }));
        let nested = c.ok("select", json!({ "edges": argument(json!({ "edges_of": { "facing": [0, 0, 1] } })) }));
        assert_eq!(nested, c.ok("select", json!({ "edges": { "edges_of": { "facing": "+z" } } })));
        let largest = c.ok("select", json!({ "faces": "largest" }));
        let plane = c.ok("plane_offset", json!({ "base": "XY", "dist": 8, "name": "[datum" }))["plane"].clone();
        let sketch = c.ok("sketch_create", json!({ "plane": argument(json!({ "plane": plane })), "name": "on top" }));

        c.ok("doc_new", json!({}));
        c.ok("sketch_create", json!({ "plane": "XZ", "name": "ring" }));
        let added = c.ok(
            "sketch_add",
            json!({ "sketch": "ring", "entities": [
            argument(json!({ "type": "polyline", "closed": true,
                "points": argument(json!([argument(json!([4, 0])), [6, 0], [6, 8], [4, 8]])) })),
            { "type": "line", "x1": 0, "y1": -1, "x2": 0, "y2": 9, "construction": true }
        ] }),
        );
        let line = added["created"][1]["entities"][0].clone();
        let revolved = c.ok("revolve", json!({ "sketch": "ring", "axis": argument(json!({ "line": line })) }));
        // Revolving r∈[4,6], z∈[0,8] makes an annulus: π(6²−4²)·8 mm³.
        assert!((volume(&revolved) - std::f64::consts::PI * (36.0 - 16.0) * 8.0).abs() < 1e-6);
        let plain_axis = c.ok("revolve", json!({ "sketch": "ring", "axis": "sketch_y", "op": "new_body" }));
        results.push(json!([faces, edges, largest, sketch, revolved, plain_axis]));
    }
    assert_eq!(results[0], results[1], "encoded objects/arrays must have the same protocol results");
}

#[test]
fn malformed_json_encoded_arguments_report_the_argument_path() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    for faces in [json!("  {\"facing\": "), json!("[1,")] {
        let (is_err, message) = c.tool("select", json!({ "faces": faces }));
        let message = message.as_str().unwrap();
        assert!(is_err && message.contains("invalid JSON object/array string at arguments.faces"), "{message}");
    }
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
}

#[test]
fn json_looking_plain_string_arguments_are_preserved() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("sketch_create", json!({ "plane": "XY", "name": "{\"custom\":[]}" }));
    let sketch = c.ok("sketch_info", json!({ "sketch": "{\"custom\":[]}" }));
    assert_eq!(sketch["name"], "{\"custom\":[]}");
    let (is_err, message) = c.tool("doc_open", json!({ "path": "[missing.qcad" }));
    assert!(is_err && message.as_str().unwrap().contains("[missing.qcad"), "{message}");
    assert!(!message.as_str().unwrap().contains("JSON object/array string"), "{message}");
}

#[test]
fn opaque_selection_names_are_literal_in_native_and_encoded_arguments() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("sketch_create", json!({"plane":"XY", "name":"profile"}));
    c.ok("sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect","w":20,"h":16}]}));
    // Include malformed and valid object/array JSON names: all remain object references.
    for name in ["{x}", "[x]", "{\"x\":1}", "[1]"] {
        c.ok("extrude", json!({"sketch":"profile", "height":8, "name":name, "op":"new_body"}));
        for encoded in [false, true] {
            let selection = json!({"of_feature":name, "role":"cap_end"});
            let selection = if encoded { json!(selection.to_string()) } else { selection };
            let picked = c.ok("select", json!({"body":name, "faces":selection}));
            assert_eq!(picked["count"], 1, "one extrude end cap for {name}");
        }
    }
}

#[test]
fn decoder_depth_cap_reports_the_path_and_keeps_the_server_alive() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    let mut selection = json!({"facing":"+z"});
    for _ in 0..40 {
        selection = json!({"union":[selection]});
    }
    let (is_err, message) = c.tool("select", json!({"faces":selection.to_string()}));
    assert!(is_err, "deep encoded argument must be refused");
    let message = message.as_str().unwrap();
    assert!(message.contains("structured argument nesting exceeds 32 at arguments.faces"), "{message}");
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
}

#[test]
fn initialize_documents_encoded_structured_arguments_without_widening_schemas() {
    let mut c = Client::start();
    let initialized = c.init();
    assert!(initialized["result"]["instructions"].as_str().unwrap().contains("Objects and arrays may also be sent as JSON strings"));
    let tools = c.request("tools/list", json!({}));
    let add = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "sketch_add").unwrap();
    assert_eq!(add["inputSchema"]["properties"]["entities"]["type"], "array", "keep the advertised structured schema");
}

#[test]
fn repeated_renders_after_parameter_edits_match_image_bytes_and_captions() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("param_set", json!({"name":"w", "value":20}));
    c.ok("sketch_create", json!({"plane":"XY", "name":"profile"}));
    c.ok("sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect","w":"w","h":16}]}));
    c.ok("extrude", json!({"sketch":"profile", "height":8}));
    let mut frames = Vec::new();
    for width in [20, 32, 20] {
        c.ok("param_set", json!({"name":"w", "value":width}));
        let args = json!({"name":"render", "arguments":{"view":"top", "width":200, "height":160}});
        let first = c.request("tools/call", args.clone());
        let second = c.request("tools/call", args);
        assert_ne!(first["result"]["isError"], true, "{first}");
        assert_eq!(first["result"], second["result"], "same current document must render identically twice");
        let content = first["result"]["content"].as_array().unwrap();
        let image = unbase64(content[0]["data"].as_str().unwrap());
        assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
        let caption = content[1]["text"].as_str().unwrap();
        // Centered rectangle: x=±w/2, y=±16/2; extrusion z=[0,8].
        assert!(caption.contains(&format!("bbox x {}..{} y -8..8 z 0..8 mm", -width / 2, width / 2)), "{caption}");
        frames.push((image, caption.to_string()));
    }
    assert_ne!(frames[0].0, frames[1].0, "changing rectangle aspect changes pixels as well as the caption");
    assert_ne!(frames[0].1, frames[1].1);
    assert_eq!(frames[0], frames[2], "returning to the original width restores both PNG bytes and caption");
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
        "dimension_set",
        "param_delete",
        "sketch_create",
        "sketch_add",
        "sketch_project",
        "sketch_info",
        "sketch_constrain",
        "sketch_remove",
        "sketch_edit",
        "extrude",
        "plane_offset",
        "export",
        "render",
        "topology",
        "select",
        "revolve",
        "sweep",
        "loft",
        "fillet",
        "chamfer",
        "hole",
        "shell",
        "push_face",
        "linear_array",
        "circular_array",
        "mirror",
        "thread",
        "body_boolean",
        "split",
        "import",
        "feature_delete",
        "undo",
        "batch",
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
fn imports_step_through_mcp_and_batch_with_embedded_source() {
    let dir = std::env::temp_dir().join(format!("qymcad-mcp-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("box.step");
    let mut maker = Client::start();
    build_box(&mut maker);
    maker.ok("export", json!({"format":"step","path":source.to_str().unwrap()}));

    let mut client = Client::start();
    client.init();
    client.ok("doc_new", json!({}));
    let imported = client.ok("import", json!({"path":source.to_str().unwrap(),"name":"Imported"}));
    assert_eq!(imported["bodies"].as_array().unwrap().len(), 1);
    let body = imported["bodies"][0]["body"].clone();
    assert!((imported["bodies"][0]["volume_mm3"].as_f64().unwrap() - 1000.0).abs() < 1.0);
    assert_eq!(imported["bodies"][0]["name"], "Imported");
    let topology = client.ok("topology", json!({"body":body,"edges":false}));
    assert_eq!(topology["faces_total"], 6);
    let face = topology["faces"][0]["id"].clone();
    let sketch = client.ok("sketch_create", json!({"plane":{"body":body,"face":face},"name":"on-import"}));
    assert!(sketch["sketch"].as_u64().is_some());

    let batch = client.ok("batch", json!({"steps":[{"tool":"import","args":{"path":source.to_str().unwrap(),"scale":2.0}}]}));
    assert_eq!(batch["steps"][0]["tool"], "import");
    assert!((batch["bodies"][1]["volume_mm3"].as_f64().unwrap() - 8000.0).abs() < 1.0);

    let saved = dir.join("imported.qcad");
    client.ok("doc_save", json!({"path":saved.to_str().unwrap()}));
    std::fs::remove_file(&source).unwrap();
    let mut reopened = Client::start();
    reopened.init();
    let opened = reopened.ok("doc_open", json!({"path":saved.to_str().unwrap()}));
    assert_eq!(opened["doc"]["bodies"].as_array().unwrap().len(), 2);
    assert!((opened["doc"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - 1000.0).abs() < 1.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn imports_iges_through_mcp() {
    let dir = std::env::temp_dir().join(format!("qymcad-mcp-iges-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("box.iges");
    // Server-built 10×10×5 mm box, written by the native IGES writer in inches;
    // extracted from the verified research probe's embedded-igs.qcad (sources/22.igs).
    // The server's export tool supports STEP but not IGES. No kernel dependency in the MCP layer.
    std::fs::write(&source, include_bytes!("../../engine/tests/fixtures/server-box-10x10x5-inch.igs")).unwrap();
    let mut client = Client::start();
    client.init();
    client.ok("doc_new", json!({}));
    let imported = client.ok("import", json!({"path":source.to_str().unwrap(), "name":"Inch box"}));
    assert_eq!(imported["bodies"].as_array().unwrap().len(), 1);
    assert_eq!(imported["bodies"][0]["name"], "Inch box");
    assert_eq!(imported["bodies"][0]["solid"], 0);
    assert!((imported["bodies"][0]["volume_mm3"].as_f64().unwrap() - 10.0 * 10.0 * 5.0).abs() < 1e-4);
    for (actual, expected) in imported["bodies"][0]["bbox"].as_array().unwrap().iter().zip([-5.0, -5.0, 0.0, 5.0, 5.0, 5.0]) {
        assert!((actual.as_f64().unwrap() - expected).abs() < 1e-4);
    }
    let body = imported["bodies"][0]["body"].clone();
    assert_eq!(client.ok("topology", json!({"body":body}))["faces_total"], 6);
    // Also verify IGES uses the normal batch handler, source embedding and literal scaling.
    let batch =
        client.ok("batch", json!({"verbose":true, "steps":[{"tool":"import", "args":{"path":source.to_str().unwrap(), "scale":2.0}}]}));
    assert_eq!(batch["steps"][0]["tool"], "import");
    assert_eq!(batch["bodies"].as_array().unwrap().len(), 2);
    let mut volumes: Vec<_> = batch["bodies"].as_array().unwrap().iter().map(|b| b["volume_mm3"].as_f64().unwrap()).collect();
    volumes.sort_by(f64::total_cmp);
    assert!((volumes[0] - 500.0).abs() < 1e-4);
    assert!((volumes[1] - 500.0 * 2.0_f64.powi(3)).abs() < 1e-4);
    let saved = dir.join("iges.qcad");
    client.ok("doc_save", json!({"path":saved.to_str().unwrap()}));
    std::fs::remove_file(&source).unwrap();
    let mut reopened = Client::start();
    reopened.init();
    let opened = reopened.ok("doc_open", json!({"path":saved.to_str().unwrap()}));
    let bodies = opened["doc"]["bodies"].as_array().unwrap();
    assert_eq!(bodies.len(), 2);
    let mut reopened_volumes: Vec<_> = bodies.iter().map(|b| b["volume_mm3"].as_f64().unwrap()).collect();
    reopened_volumes.sort_by(f64::total_cmp);
    for (actual, expected) in reopened_volumes.into_iter().zip([500.0, 4000.0]) {
        assert!((actual - expected).abs() < 1e-4);
    }
    let _ = std::fs::remove_dir_all(&dir);
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
    assert!((r["bodies"][0]["mesh_volume_mm3"].as_f64().unwrap() - 1000.0).abs() < 1e-6, "{r}");
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

#[test]
fn sketch_project_uses_topology_and_returns_constrainable_geometry_over_stdio() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("sketch_create", json!({"plane":"XY","name":"profile"}));
    c.ok("sketch_add", json!({"sketch":"profile","entities":[{"type":"rect","w":40,"h":30}]}));
    c.ok("extrude", json!({"sketch":"profile","height":10,"name":"body"}));
    c.ok("sketch_create", json!({"plane":"XY","name":"reference"}));
    let projected = c.ok("sketch_project", json!({"sketch":"reference","body":"body","edges":{"edges_of":{"facing":"+z"}}}));
    let entities = projected["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 4);
    assert!(entities.iter().all(|e| e["driven"] == true && e["projection_state"] == "attached"));
    assert!(entities.iter().all(|e| e["geometry"].is_object()));
    let info = c.ok("sketch_info", json!({"sketch":"reference"}));
    assert_eq!(info["entities"].as_array().unwrap().len(), 4);
    assert!(info["entities"].as_array().unwrap().iter().all(|e| e["driven"] == true));
    let (is_err, _) = c.tool("sketch_project", json!({"sketch":"reference","body":"body","edges":[2147483647]}));
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

#[test]
fn projected_edge_drives_slot_and_cut_after_parameter_edit_over_stdio() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("param_set", json!({"name":"w","value":40}));
    c.ok("sketch_create", json!({"plane":"XY","name":"profile"}));
    c.ok("sketch_add", json!({"sketch":"profile","entities":[{"type":"rect","w":"w","h":30}]}));
    c.ok("extrude", json!({"sketch":"profile","height":10,"op":"new_body","name":"source"}));
    c.ok("sketch_create", json!({"plane":"XY","name":"stock"}));
    c.ok("sketch_add", json!({"sketch":"stock","entities":[{"type":"rect","w":80,"h":30}]}));
    c.ok("extrude", json!({"sketch":"stock","height":10,"op":"new_body","name":"stock body"}));
    let plane = c.ok("plane_offset", json!({"base":"XY","dist":10}))["plane"].clone();
    c.ok("sketch_create", json!({"plane":{"plane":plane},"name":"pocket"}));
    let topology = c.ok("topology", json!({"body":"source"}));
    let edge = topology["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| {
            let a = e["a"].as_array().unwrap();
            let b = e["b"].as_array().unwrap();
            a[0].as_f64() == Some(20.0) && b[0].as_f64() == Some(20.0) && a[2].as_f64() == Some(10.0) && b[2].as_f64() == Some(10.0)
        })
        .expect("top Y edge at x=w/2")["id"]
        .clone();
    let projected = c.ok("sketch_project", json!({"sketch":"pocket","body":"source","edges":edge}));
    assert_eq!(projected["entities"].as_array().unwrap().len(), 1);
    let corner = projected["points"]
        .as_array()
        .unwrap()
        .iter()
        .min_by(|a, b| a["y"].as_f64().unwrap().total_cmp(&b["y"].as_f64().unwrap()))
        .unwrap()["id"]
        .clone();
    let slot = c.ok(
        "sketch_add",
        json!({"sketch":"pocket","entities":[
            {"type":"slot","x1":17,"y1":-10,"x2":17,"y2":0,"width":2,"dimensioned":false}
        ]}),
    );
    let centers = slot["created"][0]["points"].as_array().unwrap();
    for (center, dy) in centers.iter().zip([5, 15]) {
        c.ok("sketch_constrain", json!({"sketch":"pocket","kind":"distance","refs":[corner,center],"axis":"x","value":3}));
        c.ok("sketch_constrain", json!({"sketch":"pocket","kind":"distance","refs":[corner,center],"axis":"y","value":dy}));
    }
    c.ok("sketch_constrain", json!({"sketch":"pocket","kind":"radius","refs":[slot["created"][0]["entities"][1]],"value":1}));
    let cut = c.ok("extrude", json!({"sketch":"pocket","target":"stock body","height":2,"direction":"reverse","op":"cut","name":"cut"}));
    let expected = 24000.0 - 2.0 * (20.0 + std::f64::consts::PI);
    let body = &cut["rebuild"]["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "cut").unwrap();
    assert!((body["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6);
    c.ok("param_set", json!({"name":"w","value":60}));
    let info = c.ok("sketch_info", json!({"sketch":"pocket"}));
    for point in projected["points"].as_array().unwrap() {
        let p = info["points"].as_array().unwrap().iter().find(|p| p["id"] == point["id"]).unwrap();
        assert_eq!(p["x"], 30.0, "projection at w/2");
    }
    for id in centers {
        let p = info["points"].as_array().unwrap().iter().find(|p| p["id"] == *id).unwrap();
        assert_eq!(p["x"], 27.0, "slot at w/2-3");
    }
    let topo = c.ok("topology", json!({"body":"cut"}));
    let floor = topo["faces"].as_array().unwrap().iter().find(|f| f["centroid"][2] == 8.0).expect("2 mm pocket floor");
    assert!((floor["centroid"][0].as_f64().unwrap() - 27.0).abs() < 1e-5);
}

#[test]
fn sketch_info_reports_actual_hole_edge_detachment_over_stdio() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("sketch_create", json!({"plane":"XY","name":"profile"}));
    let added = c.ok("sketch_add", json!({"sketch":"profile","entities":[{"type":"rect","w":40,"h":30},{"type":"circle","d":10}]}));
    c.ok("extrude", json!({"sketch":"profile","height":5,"name":"body"}));
    c.ok("sketch_create", json!({"plane":"XY","name":"projection"}));
    let topo = c.ok("topology", json!({"body":"body"}));
    let edge = topo["edges"].as_array().unwrap().iter().find(|e| e["kind"] == "circle").unwrap()["id"].clone();
    let projected = c.ok("sketch_project", json!({"sketch":"projection","body":"body","edges":edge}));
    assert_eq!(projected["entities"][0]["r"], 5.0);
    c.ok("sketch_remove", json!({"sketch":"profile","entity":added["created"][1]["circle"]}));
    let info = c.ok("sketch_info", json!({"sketch":"projection"}));
    let entities = info["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0]["projection_state"], "detached");
    assert_eq!(entities[0]["driven"], false);
    assert_eq!(entities[0]["projection"], projected["projections"][0]["id"]);
    assert_eq!(entities[0]["geometry"], projected["entities"][0]["geometry"]);
    c.ok("undo", json!({}));
    assert_eq!(c.ok("sketch_info", json!({"sketch":"projection"}))["entities"][0]["projection_state"], "attached");
}
