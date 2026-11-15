//! Source-motion guarantees exercised through the production stdio executable.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Server {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next: u64,
}

impl Server {
    fn new() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_qymcad-mcp"))
            .env("QYMCAD_MCP_LOG", "off")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut server = Self { child, input, output, next: 0 };
        let initialized = server.request(
            "initialize",
            json!({"protocolVersion":"2025-06-18", "capabilities":{}, "clientInfo":{"name":"f5-pattern-source", "version":"0"}}),
        );
        assert!(initialized["result"].is_object(), "{initialized}");
        server
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        writeln!(self.input, "{}", json!({"jsonrpc":"2.0", "id":self.next, "method":method, "params":params})).unwrap();
        self.input.flush().unwrap();
        let mut line = String::new();
        assert_ne!(self.output.read_line(&mut line).unwrap(), 0, "server exited without a response");
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], self.next);
        assert!(response["error"].is_null(), "{response}");
        response
    }

    fn raw_tool(&mut self, name: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":args}))["result"].clone()
    }

    fn call(&mut self, name: &str, args: Value) -> Value {
        let result = self.raw_tool(name, args);
        assert_ne!(result["isError"], true, "{name} must succeed: {result}");
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    fn minimal() -> Self {
        let mut server = Self::new();
        server.call("doc_new", json!({}));
        for (name, value) in
            [("leg_span", json!(40)), ("leg_w", json!(6)), ("leg_n", json!(3)), ("leg_margin", json!(5)), ("step", json!("leg_w+4"))]
        {
            server.call("param_set", json!({"name":name,"value":value}));
        }
        server.call("sketch_create", json!({"plane":"XY","name":"minimal_hex"}));
        let added = server.call("sketch_add", json!({"sketch":"minimal_hex", "entities":[{"type":"polygon","cx":"leg_margin+leg_w/2","cy":"leg_span/2","r":"leg_w/1.7320508","sides":6}]}));
        server.call(
            "sketch_pattern",
            json!({"sketch":"minimal_hex","entities":added["created"][0]["entities"],"kind":"linear","count":"leg_n","dx":0,"dy":"step"}),
        );
        server.call("extrude", json!({"sketch":"minimal_hex","height":2,"op":"new_body","name":"hexes"}));
        server
    }

    fn geometry(&mut self) -> (Value, Value) {
        (self.call("sketch_info", json!({"sketch":"minimal_hex"})), self.call("doc_info", json!({})))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn edits() -> Vec<Value> {
    [("leg_span", json!(50)), ("leg_w", json!(8)), ("leg_n", json!(4)), ("leg_margin", json!("leg_span/5"))]
        .into_iter()
        .map(|(name, value)| json!({"tool":"param_set","args":{"name":name,"value":value}}))
        .collect()
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!((actual - expected).abs() <= tolerance, "actual {actual}, expected {expected}, tolerance {tolerance}");
}

fn archive(server: &mut Server, path: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
    server.call("doc_save", json!({"path":path}));
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|index| {
            let mut entry = zip.by_index(index).unwrap();
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}

fn entity_geometry(sketch: &Value) -> Vec<Value> {
    let points: BTreeMap<_, _> =
        sketch["points"].as_array().unwrap().iter().map(|p| (p["id"].as_u64().unwrap(), json!([p["x"], p["y"]]))).collect();
    sketch["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entity| {
            let coordinates: Vec<_> = entity["points"].as_array().unwrap().iter().map(|id| points[&id.as_u64().unwrap()].clone()).collect();
            json!({"type":entity["type"],"r":entity["r"],"construction":entity["construction"],"coordinates":coordinates})
        })
        .collect()
}

fn assert_hex_geometry(sketch: &Value, doc: &Value, cx: f64, cy: f64, width: f64, count: usize) {
    let entities = entity_geometry(sketch);
    assert_eq!(entities.len(), count * 7, "one construction circle and six polygon edges per copy");
    let pitch = width + 4.0;
    let radius = width / 1.7320508;
    for copy in 0..count {
        let group = &entities[copy * 7..(copy + 1) * 7];
        assert_eq!(group[0]["type"], "circle");
        close(group[0]["coordinates"][0][0].as_f64().unwrap(), cx, 1e-7);
        close(group[0]["coordinates"][0][1].as_f64().unwrap(), cy + copy as f64 * pitch, 1e-7);
        close(group[0]["r"].as_f64().unwrap(), radius, 1e-7);
        for (source, instance) in entities[..7].iter().zip(group) {
            for (source_point, instance_point) in
                source["coordinates"].as_array().unwrap().iter().zip(instance["coordinates"].as_array().unwrap())
            {
                close(instance_point[0].as_f64().unwrap(), source_point[0].as_f64().unwrap(), 1e-7);
                close(instance_point[1].as_f64().unwrap(), source_point[1].as_f64().unwrap() + copy as f64 * pitch, 1e-7);
            }
        }
        // Polygon vertices are at centre + R*(cos(j*pi/3), sin(j*pi/3)).
        // sketch_info rounds coordinates to four decimals; translated-point checks above remain 1e-7.
        for vertex in 0..6 {
            let angle = vertex as f64 * std::f64::consts::PI / 3.0;
            let point = &group[vertex + 1]["coordinates"][0];
            close(point[0].as_f64().unwrap(), cx + radius * angle.cos(), 0.000051);
            close(point[1].as_f64().unwrap(), cy + copy as f64 * pitch + radius * angle.sin(), 0.000051);
        }
    }
    // Extruded regular hexagons: x extrema cx +/- R, y extrema cy +/- R*sin(pi/3), z 0..2.
    let half_height = radius * (std::f64::consts::PI / 3.0).sin();
    let expected = [cx - radius, cy - half_height, 0.0, cx + radius, cy + (count - 1) as f64 * pitch + half_height, 2.0];
    let bbox = doc["bodies"][0]["bbox"].as_array().unwrap();
    for (actual, expected) in bbox.iter().zip(expected) {
        close(actual.as_f64().unwrap(), expected, 0.000051);
    }
}

#[test]
fn stdio_source_only_param_set_refreshes_instances_and_one_undo_restores_geometry() {
    let mut server = Server::minimal();
    let before = server.geometry();
    // Initially centre=(5+6/2,40/2)=(8,20), pitch=6+4=10, three copies.
    assert_hex_geometry(&before.0, &before.1, 8.0, 20.0, 6.0, 3);
    server.call("param_set", json!({"name":"leg_margin","value":10}));
    let after = server.geometry();
    // Only margin changes: centre=(10+6/2,40/2)=(13,20), copies at y=30,40.
    assert_hex_geometry(&after.0, &after.1, 13.0, 20.0, 6.0, 3);
    assert_eq!(server.call("undo", json!({}))["undone"]["tool"], "param_set");
    let restored = server.geometry();
    assert_eq!(restored, before, "one undo restores original ids, source and instance coordinates, contours and B-rep metrics");
}

#[test]
fn stdio_four_edit_batch_matches_singles_with_final_source_and_brep() {
    let mut batch = Server::minimal();
    let before = batch.geometry();
    let result = batch.call("batch", json!({"steps":edits()}));
    assert_eq!(result["ok"], true);
    let batched = batch.geometry();
    // margin=span/5=10; centre=(10+8/2,50/2)=(14,25); pitch=8+4=12.
    // Four centres: (14,25),(14,37),(14,49),(14,61).
    assert_hex_geometry(&batched.0, &batched.1, 14.0, 25.0, 8.0, 4);
    let mut single = Server::minimal();
    for step in edits() {
        single.call(step["tool"].as_str().unwrap(), step["args"].clone());
    }
    let singles = single.geometry();
    assert_hex_geometry(&singles.0, &singles.1, 14.0, 25.0, 8.0, 4);
    assert_eq!(entity_geometry(&batched.0), entity_geometry(&singles.0));
    assert_eq!(batched.1["bodies"], singles.1["bodies"]);
    assert_eq!(batch.call("undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(batch.geometry(), before, "the complete batch is one undo entry");
}

#[test]
fn stdio_later_batch_failure_restores_source_instances_and_brep() {
    let mut server = Server::minimal();
    let path = std::env::temp_dir().join(format!("qymcad-f5-mcp-rollback-{}.qcad", std::process::id()));
    let before_archive = archive(&mut server, &path);
    let before = server.geometry();
    let result = server.raw_tool(
        "batch",
        json!({"steps":[
            {"tool":"param_set","args":{"name":"leg_margin","value":10}},
            {"tool":"extrude","args":{"sketch":"missing","height":2}}
        ]}),
    );
    assert_eq!(result["isError"], true, "later step must refuse: {result}");
    assert!(result["content"][0]["text"].as_str().unwrap().contains("step 2"));
    let after = server.geometry();
    assert_eq!(after, before, "full doc_info and topology/coordinates snapshot precedes refusal");
    // ADR 0020/F-056: a failed batch keeps its allocated ids; every other archive byte must match.
    let mut after_archive = archive(&mut server, &path);
    let next_id = |bytes: &[u8]| -> (String, u64) {
        let text = std::str::from_utf8(bytes).unwrap();
        let lines: Vec<_> = text.lines().filter(|l| l.trim_start().starts_with("next_id:")).collect();
        assert_eq!(lines.len(), 1);
        (lines[0].to_owned(), lines[0].trim().trim_start_matches("next_id:").trim().trim_end_matches(',').parse().unwrap())
    };
    let (old_line, old_id) = next_id(&before_archive["document.ron"]);
    let (new_line, new_id) = next_id(&after_archive["document.ron"]);
    assert!(new_id >= old_id, "the allocator never moves backwards");
    let doc = std::str::from_utf8(&after_archive["document.ron"]).unwrap().replacen(&new_line, &old_line, 1);
    after_archive.insert("document.ron".into(), doc.into_bytes());
    assert_eq!(after_archive, before_archive, "all recipe and B-rep archive entry bytes except next_id");
    std::fs::remove_file(path).unwrap();
    assert_hex_geometry(&after.0, &after.1, 8.0, 20.0, 6.0, 3);
    assert_eq!(server.call("undo", json!({}))["undone"]["tool"], "extrude", "failed batch adds no undo entry");
}
