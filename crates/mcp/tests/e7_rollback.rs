//! E7 rollback acceptance over the real JSON-RPC stdio transport.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next: u64,
}
impl Client {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_qymcad-mcp"))
            .env("QYMCAD_MCP_LOG", "off")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut c = Self { child, input, output, next: 1 };
        let initialized =
            c.request("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"e7","version":"1"}}));
        assert!(initialized["result"].is_object(), "{initialized}");
        writeln!(c.input, "{}", json!({"jsonrpc":"2.0","method":"notifications/initialized"})).unwrap();
        c.input.flush().unwrap();
        c
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        writeln!(self.input, "{}", json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})).unwrap();
        self.input.flush().unwrap();
        let mut line = String::new();
        assert_ne!(self.output.read_line(&mut line).unwrap(), 0, "server closed stdout");
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], id);
        response
    }
    fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let response = self.request("tools/call", json!({"name":name,"arguments":args}));
        if !response["error"].is_null() {
            return Err(response["error"].to_string());
        }
        let result = &response["result"];
        let text = result["content"][0]["text"].as_str().unwrap_or_default();
        if result["isError"] == true {
            return Err(text.to_owned());
        }
        serde_json::from_str(text).map_err(|e| format!("{name}: invalid JSON payload: {e}: {result}"))
    }
    fn ok(&mut self, name: &str, args: Value) -> Value {
        self.tool(name, args).unwrap_or_else(|e| panic!("{name} failed: {e}"))
    }
    fn snapshot(&mut self) -> Value {
        json!({"doc":self.ok("doc_info",json!({})),"topology":self.ok("topology",json!({"adjacency":true}))})
    }
    fn refuse_unchanged(&mut self, name: &str, args: Value) -> String {
        let before = self.snapshot();
        let error = self.tool(name, args).expect_err("request must be refused");
        assert_eq!(self.snapshot(), before, "{name} refusal changed document/topology: {error}");
        error
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn archive(c: &mut Client, path: &Path) -> BTreeMap<String, Vec<u8>> {
    c.ok("doc_save", json!({"path":path}));
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut entry = zip.by_index(i).unwrap();
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            // ADR 0020 permits only the monotonic allocator to advance after failure.
            let bytes = if name == "document.ron" {
                std::str::from_utf8(&bytes)
                    .unwrap()
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("next_id:"))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into_bytes()
            } else {
                bytes
            };
            (name, bytes)
        })
        .collect()
}

struct Fixture {
    c: Client,
    sketch: Value,
    m1: Value,
    m2: Value,
}
fn fixture() -> Fixture {
    let mut c = Client::start();
    c.ok("doc_new", json!({}));
    let sketch = c.ok("sketch_create", json!({"plane":"XY","name":"profile"}))["sketch"].clone();
    // The stdio fixture uses a pretranslated sketch and an extrusion M1 so creating a Move
    // at the marker does not invoke ADR 0033's required accumulation onto an existing Move.
    c.ok("sketch_add", json!({"sketch":sketch,"entities":[{"type":"rect","w":10,"h":20,"cx":5}]}));
    let m1 = c.ok("extrude", json!({"sketch":sketch,"height":20,"name":"M1"}))["body"].clone();
    c.ok("appearance", json!({"body":m1,"color":[10,20,30]}));
    let m2 = c.ok("move", json!({"body":m1,"dy":7,"name":"M2"}))["body"].clone();
    Fixture { c, sketch, m1, m2 }
}
fn marker(position: &str, after: Value, next: Value, active: usize, total: usize) -> Value {
    json!({"position":position,"after":after,"next_feature":next,"active_nodes":active,"total_nodes":total})
}
fn named(id: &Value, name: &str) -> Value {
    json!({"id":id,"name":name})
}
fn body(body: &Value, id: &Value, shift: [f64; 2], height: f64) {
    assert_eq!(body["id"], *id);
    // Centered rectangle 10×20 extruded +h: V=10·20·h; translate both XY corners.
    assert!((body["volume_mm3"].as_f64().unwrap() - 10.0 * 20.0 * height).abs() < 1e-6);
    let [x, y] = shift;
    assert_eq!(body["bbox"], json!([-5.0 + x, -10.0 + y, 0.0, 5.0 + x, 10.0 + y, height]));
}
fn rollback(c: &mut Client, args: Value, expected: Value, changed: bool, bodies: &[(Value, &str, [f64; 2], f64)]) -> Value {
    let result = c.ok("history_rollback", args);
    let mut fields = result.clone();
    let rebuild = fields.as_object_mut().unwrap().remove("rebuild").unwrap();
    assert_eq!(fields.as_object_mut().unwrap().remove("changed"), Some(json!(changed)));
    assert_eq!(fields, expected);
    assert_eq!(rebuild.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["bodies", "errors", "warnings"]);
    assert_eq!(rebuild["errors"], json!([]));
    assert_eq!(rebuild["warnings"], json!([]));
    let actual = rebuild["bodies"].as_array().unwrap();
    assert_eq!(actual.len(), bodies.len());
    for (value, (id, name, shift, height)) in actual.iter().zip(bodies) {
        body(value, id, *shift, *height);
        // Native set_body_mesh creates the first body as name-body#1 ("Body 1" in en/main.ftl)
        // and carried_body_name preserves it through Moves (core/model/features.rs:7-22).
        // The explicit manual RGB in fixture() follows its root through the same lineage.
        let mut metadata = value.clone();
        metadata.as_object_mut().unwrap().remove("volume_mm3");
        metadata.as_object_mut().unwrap().remove("bbox");
        assert_eq!(
            metadata,
            json!({"id":id,"name":name,"body_name":"Body 1","visible":true,"color_rgb":[10,20,30],"color_source":"manual"})
        );
    }
    let info = c.ok("doc_info", json!({}));
    assert_eq!(info["rollback"], expected);
    assert_eq!(info["bodies"], rebuild["bodies"]);
    result
}
fn path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("qymcad-e7-mcp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn e7_stdio_selectors_exact_json_prefix_export_and_last_node_normalization() {
    let Fixture { mut c, sketch, m1, m2 } = fixture();
    let listed = c.request("tools/list", json!({}));
    let schema =
        &listed["result"]["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "history_rollback").unwrap()["inputSchema"];
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"].as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["after", "position"]);
    rollback(
        &mut c,
        json!({"after":m1}),
        marker("after", named(&m1, "M1"), named(&m2, "M2"), 2, 3),
        true,
        &[(m1.clone(), "M1", [5.0, 0.0], 20.0)],
    );
    assert_eq!(c.ok("topology", json!({}))["body"], m1);
    let selected = c.ok("select", json!({"faces":{"facing":"+z"}}));
    assert_eq!(selected["body"], m1);
    assert_eq!(selected["count"], 1);
    assert_eq!(selected["faces"].as_array().unwrap().len(), 1);
    let top = &selected["faces"][0];
    // Top plane of a 10×20 prism translated +5X: centre (5,0,20), area 200.
    assert_eq!(top["kind"], "plane");
    assert_eq!(top["centroid"], json!([5.0, 0.0, 20.0]));
    assert_eq!(top["normal"], json!([0.0, 0.0, 1.0]));
    assert!((top["area"].as_f64().unwrap() - 10.0 * 20.0).abs() < 1e-6);
    let measured = c.ok("measure", json!({"a":{"point":[5,0,25]},"b":{"body":m1,"face":{"facing":"+z"}}}));
    // Point z=25 to support plane z=20 has distance |25−20|=5.
    assert_eq!(
        measured,
        json!({"distance_mm":25.0-20.0,"distance_kind":"point_support_plane","resolved":[{},{"body":m1,"face":top["id"]}],"coordinate_frame":"world"})
    );
    let exported = c.ok("export", json!({"path":path("prefix.step"),"format":"step"}));
    assert_eq!(exported["bodies"].as_array().unwrap().len(), 1);
    assert_eq!(exported["bodies"][0]["id"], m1);
    rollback(
        &mut c,
        json!({"position":"end"}),
        marker("end", Value::Null, Value::Null, 3, 3),
        true,
        &[(m2.clone(), "M2", [5.0, 7.0], 20.0)],
    );
    rollback(&mut c, json!({"after":m2}), marker("end", Value::Null, Value::Null, 3, 3), false, &[(m2, "M2", [5.0, 7.0], 20.0)]);
    rollback(&mut c, json!({"position":"start"}), marker("start", Value::Null, named(&sketch, "profile"), 0, 3), true, &[]);
    assert_eq!(c.ok("doc_info", json!({}))["errors"], json!([]));
    // The prefix is empty, so topology cannot be part of a successful snapshot; use every
    // archive entry instead, including live B-reps, as well as the complete document info.
    let empty_path = path("empty-prefix.qcad");
    let empty_archive = archive(&mut c, &empty_path);
    let empty_info = c.ok("doc_info", json!({}));
    assert!(c.tool("topology", json!({})).unwrap_err().contains("body"));
    assert_eq!(c.ok("doc_info", json!({})), empty_info);
    assert_eq!(archive(&mut c, &empty_path), empty_archive);
    assert!(c.tool("export", json!({"path":path("empty.step"),"format":"step"})).unwrap_err().contains("bod"));
    assert_eq!(c.ok("doc_info", json!({})), empty_info);
    assert_eq!(archive(&mut c, &empty_path), empty_archive);
}

#[test]
fn e7_stdio_noop_undo_and_sketch_node_marker() {
    let Fixture { mut c, sketch, m1, m2 } = fixture();
    let original = c.snapshot();
    rollback(
        &mut c,
        json!({"after":m1}),
        marker("after", named(&m1, "M1"), named(&m2, "M2"), 2, 3),
        true,
        &[(m1.clone(), "M1", [5.0, 0.0], 20.0)],
    );
    let marked = c.snapshot();
    rollback(
        &mut c,
        json!({"after":"M1"}),
        marker("after", named(&m1, "M1"), named(&m2, "M2"), 2, 3),
        false,
        &[(m1, "M1", [5.0, 0.0], 20.0)],
    );
    assert_eq!(c.snapshot(), marked);
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "history_rollback");
    assert_eq!(c.snapshot(), original, "no-op must not consume the undo entry");
    // Native markers count every timeline node; a sketch is a valid predecessor.
    let m1 = original["doc"]["timeline"][1]["id"].clone();
    rollback(&mut c, json!({"after":sketch}), marker("after", named(&sketch, "profile"), named(&m1, "M1"), 1, 3), true, &[]);
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "history_rollback");
    assert_eq!(c.snapshot(), original);
}

#[test]
fn e7_stdio_refusals_preserve_document_topology_and_archive() {
    let Fixture { mut c, m1, m2, .. } = fixture();
    // Require successful setup through the new tool before testing its refusal branches.
    rollback(&mut c, json!({"position":"end"}), marker("end", Value::Null, Value::Null, 3, 3), false, &[(m2, "M2", [5.0, 7.0], 20.0)]);
    let save = path("refusals.qcad");
    for args in [
        json!({}),
        json!({"after":m1,"position":"end"}),
        json!({"position":"middle"}),
        json!({"after":"missing"}),
        json!({"after":999999}),
        json!({"position":"start","index":0}),
        json!({"after":null,"position":"end"}),
        json!({"after":m1,"position":null}),
        json!({"after":null}),
        json!({"position":null}),
    ] {
        let before_archive = archive(&mut c, &save);
        c.refuse_unchanged("history_rollback", args);
        assert_eq!(archive(&mut c, &save), before_archive);
    }
    c.ok("rename", json!({"target":{"feature":"M1"},"name":"duplicate"}));
    c.ok("rename", json!({"target":{"feature":"M2"},"name":"duplicate"}));
    let before_archive = archive(&mut c, &save);
    assert!(c.refuse_unchanged("history_rollback", json!({"after":"duplicate"})).contains("several"));
    assert_eq!(archive(&mut c, &save), before_archive);
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "rename", "refusals add no undo entry");
}

#[test]
fn e7_stdio_native_insert_persistence_suppress_tail_and_batch_atomicity() {
    let Fixture { mut c, sketch, m1, m2 } = fixture();
    rollback(
        &mut c,
        json!({"after":m1}),
        marker("after", named(&m1, "M1"), named(&m2, "M2"), 2, 3),
        true,
        &[(m1.clone(), "M1", [5.0, 0.0], 20.0)],
    );
    let before_tail = c.ok("doc_info", json!({}))["bodies"].clone();
    let suppression = c.ok("feature_suppress", json!({"feature":m2,"suppressed":true}));
    assert_eq!(suppression["changed"], true);
    assert_eq!(suppression["rebuild"]["bodies"], before_tail);
    c.ok("feature_suppress", json!({"feature":m2,"suppressed":false}));
    let m3 = c.ok("move", json!({"body":m1,"dx":3,"name":"M3"}))["body"].clone();
    let info = c.ok("doc_info", json!({}));
    assert_eq!(info["rollback"], marker("after", named(&m3, "M3"), named(&m2, "M2"), 3, 4));
    assert_eq!(
        info["timeline"].as_array().unwrap().iter().map(|n| n["name"].as_str().unwrap()).collect::<Vec<_>>(),
        ["profile", "M1", "M3", "M2"]
    );
    body(&info["bodies"][0], &m3, [8.0, 0.0], 20.0);
    let saved = path("insert.qcad");
    c.ok("doc_save", json!({"path":saved}));
    assert_eq!(c.ok("doc_open", json!({"path":saved}))["doc"]["rollback"], info["rollback"]);
    rollback(
        &mut c,
        json!({"position":"end"}),
        marker("end", Value::Null, Value::Null, 4, 4),
        true,
        &[(m2.clone(), "M2", [8.0, 7.0], 20.0)],
    );
    let before_archive = archive(&mut c, &saved);
    let before = c.snapshot();
    let failure = c
        .tool(
            "batch",
            json!({"steps":[{"tool":"history_rollback","args":{"after":m1}},{"tool":"extrude","args":{"sketch":sketch,"height":-1}}]}),
        )
        .unwrap_err();
    assert!(failure.contains("step 2") && failure.contains("positive"), "{failure}");
    assert_eq!(c.snapshot(), before);
    assert_eq!(archive(&mut c, &saved), before_archive);
    // A failed batch must leave the preceding successful resume as the next undo entry.
    let undone = c.ok("undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "history_rollback");
    body(&undone["bodies"][0], &m3, [8.0, 0.0], 20.0);
    rollback(
        &mut c,
        json!({"position":"end"}),
        marker("end", Value::Null, Value::Null, 4, 4),
        true,
        &[(m2.clone(), "M2", [8.0, 7.0], 20.0)],
    );
    assert_eq!(c.snapshot(), before);
    let result = c.ok("batch",json!({"verbose":true,"steps":[{"tool":"history_rollback","args":{"after":m1}},{"tool":"extrude","args":{"sketch":sketch,"height":2,"op":"new_body","name":"inserted stock"}}]}));
    let added = result["steps"][1]["result"]["body"].clone();
    assert!(added.is_u64());
    let info = c.ok("doc_info", json!({}));
    assert_eq!(info["rollback"], marker("after", named(&added, "inserted stock"), named(&m3, "M3"), 3, 5));
    assert_eq!(info["timeline"][2]["id"], added);
    let bodies = info["bodies"].as_array().unwrap();
    assert_eq!(bodies.len(), 2);
    body(bodies.iter().find(|b| b["id"] == m1).unwrap(), &m1, [5.0, 0.0], 20.0);
    body(bodies.iter().find(|b| b["id"] == added).unwrap(), &added, [5.0, 0.0], 2.0);
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(c.snapshot(), before);
}

#[test]
fn e7_stdio_cut_creation_inserts_and_remaps_tail() {
    let Fixture { mut c, m1, m2, .. } = fixture();
    rollback(
        &mut c,
        json!({"after":m1}),
        marker("after", named(&m1, "M1"), named(&m2, "M2"), 2, 3),
        true,
        &[(m1.clone(), "M1", [5., 0.], 20.)],
    );
    let top = c.ok("plane_offset", json!({"base":"XY","dist":20,"name":"top"}))["plane"].clone();
    assert!(top.is_u64());
    let sk = c.ok("sketch_create", json!({"plane":{"plane":top},"name":"cut profile"}))["sketch"].clone();
    c.ok("sketch_add", json!({"sketch":sk,"entities":[{"type":"rect","w":2,"h":2,"cx":5}]}));
    let created = c.ok("extrude", json!({"sketch":sk,"height":1,"op":"cut","through":true,"direction":"reverse","target":m1,"name":"C"}));
    let cut = created["body"].clone();
    assert!(cut.is_u64());
    let info = c.ok("doc_info", json!({}));
    assert_eq!(info["rollback"], marker("after", named(&cut, "C"), named(&m2, "M2"), 5, 6));
    assert_eq!(
        info["timeline"].as_array().unwrap().iter().map(|n| n["name"].as_str().unwrap()).collect::<Vec<_>>(),
        ["profile", "M1", "top", "cut profile", "C", "M2"]
    );
    let check_cut = |bodies: &Value, id: &Value, y: f64| {
        assert_eq!(bodies.as_array().unwrap().len(), 1);
        assert_eq!(bodies[0]["id"], *id);
        // Through interior 2*2 rectangle: V=(10*20-2*2)*20=3920; outer bbox unchanged.
        assert!((bodies[0]["volume_mm3"].as_f64().unwrap() - (10. * 20. - 2. * 2.) * 20.).abs() < 1e-6);
        assert_eq!(bodies[0]["bbox"], json!([0., -10. + y, 0., 10., 10. + y, 20.]));
    };
    check_cut(&created["rebuild"]["bodies"], &cut, 0.);
    check_cut(&info["bodies"], &cut, 0.);
    let saved = path("cut-prefix.qcad");
    c.ok("doc_save", json!({"path":saved}));
    assert_eq!(c.ok("doc_open", json!({"path":saved}))["doc"]["rollback"], info["rollback"]);
    let saved_info = c.ok("doc_info", json!({}));
    let resumed = c.ok("history_rollback", json!({"position":"end"}));
    let mut fields = resumed.clone();
    fields.as_object_mut().unwrap().remove("rebuild");
    fields.as_object_mut().unwrap().remove("changed");
    assert_eq!(fields, marker("end", Value::Null, Value::Null, 6, 6));
    assert_eq!(resumed["changed"], true);
    assert_eq!(resumed["rebuild"]["errors"], json!([]));
    assert_eq!(resumed["rebuild"]["warnings"], json!([]));
    check_cut(&resumed["rebuild"]["bodies"], &m2, 7.);
    // Undo proves one marker boundary and restores the exact active cut result.
    let undone = c.ok("undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "history_rollback");
    check_cut(&undone["bodies"], &cut, 0.);
    assert_eq!(c.ok("doc_info", json!({})), saved_info);
}
