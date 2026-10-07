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
fn concave_and_convex_select_l_profile_corners_along_y() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("sketch_create", json!({ "plane": "XZ", "name": "L" }));
    // A 20 × 15 rectangle minus the upper-right 16 × 11 rectangle: area 124 mm².
    // Six profile vertices extruded along −y produce six y-directed corners: one reflex, five convex.
    c.ok(
        "sketch_add",
        json!({ "sketch": "L", "entities": [{ "type": "polyline", "points":
        [[0,0], [20,0], [20,4], [4,4], [4,15], [0,15]], "closed": true }] }),
    );
    let r = c.ok("extrude", json!({ "sketch": "L", "height": 8 }));
    assert!((r["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - (20.0 * 15.0 - 16.0 * 11.0) * 8.0).abs() < 1e-6);
    let concave = c.ok("select", json!({ "edges": { "and": [{ "concave": true }, { "along": "y" }] } }));
    assert_eq!(concave["count"], 1, "L profile has exactly one concave y-directed corner: {concave}");
    assert_eq!(concave["edges"][0]["mid"], json!([4.0, -4.0, 4.0]));
    let convex = c.ok("select", json!({ "edges": { "and": [{ "convex": true }, { "along": "y" }] } }));
    assert_eq!(convex["count"], 5, "six profile corners minus the one reflex corner: {convex}");
    let inner = c.ok("select", json!({ "edges": { "concave": true } }));
    assert_eq!(inner["count"], 1, "all twelve cap junctions are convex, including the nonconvex cap's inner outline: {inner}");
    let outer = c.ok("select", json!({ "edges": { "convex": true } }));
    assert_eq!(outer["count"], 5 + 2 * 6, "five convex extrusion corners plus both six-edge outlines: {outer}");
    let all = c.ok("select", json!({ "edges": { "union": [{ "concave": true }, { "convex": true }] } }));
    assert_eq!(all["count"], 18, "six extrusion edges plus two six-edge cap outlines: {all}");
    let (bad, msg) = c.tool("select", json!({ "faces": { "concave": true } }));
    assert!(bad && msg.as_str().unwrap().contains("cannot select faces"), "{msg}");
    for value in [json!(false), json!(1), json!("true")] {
        let (bad, msg) = c.tool("select", json!({ "edges": { "convex": value } }));
        assert!(bad && msg.as_str().unwrap().contains("takes true"), "{msg}");
    }
}

fn cube() -> Client {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("sketch_create", json!({ "plane": "XY", "name": "square" }));
    c.ok("sketch_add", json!({ "sketch": "square", "entities": [{ "type": "rect", "w": 20, "h": 20 }] }));
    let r = c.ok("extrude", json!({ "sketch": "square", "height": 20 }));
    assert!((r["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - 20.0_f64.powi(3)).abs() < 1e-6);
    c
}

#[test]
fn concave_on_a_cube_selects_zero_edges() {
    let mut c = cube();
    // A cube's 12 edge dihedrals are all outward right angles: no inward corners.
    let r = c.ok("select", json!({ "edges": { "concave": true } }));
    assert_eq!(r["count"], 0);
    assert_eq!(r["edges"], json!([]));
    assert_eq!(r["hint"], "0 corner edges; for a junction between named features use {\"between\": [{\"of_feature\": \"X\", \"role\": \"wall\"}, {\"of_feature\": \"Y\", \"role\": \"cap_end\"}]}");
}

#[test]
fn empty_corner_filters_compose_as_empty_sets() {
    let mut c = cube();
    let empty = json!({ "concave": true });
    let y = json!({ "along": "y" });
    // A cube has four y-directed edges (one for each x/z corner).
    for (selection, count) in [
        (json!({ "and": [empty, y] }), 0),
        (json!({ "and": [y, empty] }), 0),
        (json!({ "minus": [empty, y] }), 0),
        (json!({ "minus": [y, empty] }), 4),
        (json!({ "union": [empty, y] }), 4),
        (json!({ "union": [empty, empty] }), 0),
    ] {
        let r = c.ok("select", json!({ "edges": selection }));
        assert_eq!(r["count"], count, "selection {selection}: {r}");
        assert_eq!(r.get("hint").is_some(), count == 0, "only empty corner compositions carry the hint: {r}");
    }
    let plain_empty = c.ok("select", json!({ "edges": [] }));
    assert_eq!(plain_empty["count"], 0);
    assert!(plain_empty.get("hint").is_none(), "ordinary empty selections do not need a corner hint");
}

#[test]
fn empty_final_corner_selection_refuses_fillet_and_chamfer() {
    let mut c = cube();
    let before = c.ok("doc_info", json!({}));
    for (tool, args) in
        [("fillet", json!({ "edges": { "concave": true }, "radius": 1 })), ("chamfer", json!({ "edges": { "concave": true }, "dist": 1 }))]
    {
        let (bad, msg) = c.tool(tool, args);
        assert!(bad, "{tool} must refuse empty final edges: {msg}");
        assert!(msg.as_str().unwrap().contains("edge selection matched no edge"), "{tool}: {msg}");
        assert_eq!(c.ok("doc_info", json!({})), before, "{tool} refusal leaves the cube unchanged");
    }
}

#[test]
fn convex_on_a_cylinder_selects_both_rims_without_the_seam() {
    let mut c = Client::start();
    c.init();
    c.ok("doc_new", json!({}));
    c.ok("sketch_create", json!({ "plane": "XY", "name": "circle" }));
    c.ok("sketch_add", json!({ "sketch": "circle", "entities": [{ "type": "circle", "d": 10 }] }));
    let r = c.ok("extrude", json!({ "sketch": "circle", "height": 20 }));
    // Tool volumes are rounded to four decimal places: at most 0.00005 mm³ rounding error.
    assert!((r["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - std::f64::consts::PI * 5.0_f64.powi(2) * 20.0).abs() < 0.00005);
    // Two circular cap/wall rims are convex; the closing seam is excluded.
    let r = c.ok("select", json!({ "edges": { "convex": true } }));
    assert_eq!(r["count"], 2, "a cylinder has two convex rims");
    assert!(r["edges"].as_array().unwrap().iter().all(|e| e["kind"] == "circle"));
}

#[test]
fn select_description_shows_a_concave_example() {
    let mut c = Client::start();
    c.init();
    let r = c.request("tools/list", json!({}));
    let select = r["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "select").unwrap();
    assert!(select["description"].as_str().unwrap().contains("{\"concave\": true}"), "{select}");
}
