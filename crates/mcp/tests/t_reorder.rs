//! Checked feature reordering over the real JSON-RPC stdio transport.
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
        let initialized = c.request(
            "initialize",
            json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t-reorder","version":"1"}}),
        );
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
    stock: Value,
    hole: Value,
    array: Value,
}
fn fixture() -> Fixture {
    let mut c = Client::start();
    c.ok("doc_new", json!({}));
    let sketch = c.ok("sketch_create", json!({"plane":"XY","name":"profile"}))["sketch"].clone();
    c.ok("sketch_add", json!({"sketch":sketch,"entities":[{"type":"rect","w":10,"h":10}]}));
    let stock = c.ok("extrude", json!({"sketch":sketch,"height":10,"name":"stock"}))["body"].clone();
    // Store the picked original top face; a dynamic +Z query would match both array tops.
    let face = c.ok("select", json!({"body":stock,"faces":{"facing":"+z"}}))["faces"][0]["id"].clone();
    let hole = c.ok("hole", json!({"body":stock,"face":face,"at":[0,0,10],"diameter":2,"through":true,"name":"hole"}))["body"].clone();
    let array = c.ok("linear_array", json!({"body":hole,"dx":20,"count":2,"name":"array"}))["body"].clone();
    // Two disjoint 10³ prisms, each with a cylinder π·1²·10 removed.
    volume(&c.ok("doc_info", json!({}))["bodies"][0], 2000.0 - 20.0 * std::f64::consts::PI);
    Fixture { c, sketch, stock, hole, array }
}
fn volume(body: &Value, expected: f64) {
    assert!((body["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6, "{body}; expected {expected}");
}
fn path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("qymcad-t-reorder-{}-{label}.qcad", std::process::id()))
}
#[test]
fn t_stdio_reorder_strict_schema_refusals_and_ambiguous_names() {
    let Fixture { mut c, sketch, stock, hole, array } = fixture();
    let listed = c.request("tools/list", json!({}));
    let schema = &listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "feature_reorder")
        .expect("feature_reorder must be registered")["inputSchema"];
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["required"], json!(["feature", "direction"]));
    for key in ["feature", "direction"] {
        assert!(schema["properties"][key]["description"].is_string());
    }
    for args in [
        json!({}),
        json!({"feature":hole}),
        json!({"direction":"up"}),
        json!({"feature":null,"direction":"up"}),
        json!({"feature":hole,"direction":null}),
        json!({"feature":hole,"direction":4}),
        json!({"feature":hole,"direction":"up","before":stock}),
        json!({"feature":true,"direction":"up"}),
    ] {
        let error = c.refuse_unchanged("feature_reorder", args);
        assert!(error.starts_with("bad arguments for `feature_reorder`:"), "{error}");
    }
    assert_eq!(
        c.refuse_unchanged("feature_reorder", json!({"feature":hole,"direction":"UP"})),
        "invalid request: unknown reorder direction `UP`; use up or down"
    );
    assert_eq!(c.refuse_unchanged("feature_reorder", json!({"feature":999999,"direction":"up"})), "not found: no object with id 999999");
    assert_eq!(c.refuse_unchanged("feature_reorder", json!({"feature":"absent","direction":"up"})), "not found: no object named `absent`");
    assert_eq!(
        c.refuse_unchanged("feature_reorder", json!({"feature":sketch,"direction":"up"})),
        format!("invalid request: feature {sketch} is not a body-producing feature; feature_reorder moves body features only")
    );
    c.ok("rename", json!({"target":{"feature":hole},"name":"duplicate"}));
    c.ok("rename", json!({"target":{"feature":array},"name":"duplicate"}));
    let mut ids = vec![hole.as_u64().unwrap(), array.as_u64().unwrap()];
    ids.sort();
    assert_eq!(
        c.refuse_unchanged("feature_reorder", json!({"feature":"duplicate","direction":"up"})),
        format!("invalid request: several objects are named `duplicate` ({ids:?}); use the id")
    );
}
/// A described hole face is stored as the app's pick (`Ref::one`, 2026-11-09), so it is not re-resolved on the new
/// source: `facing +z` would match both array tops there, yet the move succeeds exactly as with an id pick.
#[test]
fn t_stdio_hole_face_given_by_a_description_moves_like_a_pick() {
    let mut c = Client::start();
    c.ok("doc_new", json!({}));
    let sketch = c.ok("sketch_create", json!({"plane":"XY"}))["sketch"].clone();
    c.ok("sketch_add", json!({"sketch":sketch,"entities":[{"type":"rect","w":10,"h":10}]}));
    let stock = c.ok("extrude", json!({"sketch":sketch,"height":10}))["body"].clone();
    let hole = c.ok("hole", json!({"body":stock,"face":{"facing":"+z"},"at":[0,0,10],"diameter":2,"through":true}))["body"].clone();
    c.ok("linear_array", json!({"body":hole,"dx":20,"count":2}));
    let moved = c.ok("feature_reorder", json!({"feature":hole,"direction":"down"}));
    assert_eq!(moved["changed"], true);
    let bodies = moved["rebuild"]["bodies"].as_array().unwrap();
    assert_eq!(bodies.len(), 1);
    // The single bore drills only the original copy: 2·1000 − π·1²·10.
    volume(&bodies[0], 2000.0 - 10.0 * std::f64::consts::PI);
}
#[test]
fn t_stdio_consumed_hole_down_and_array_up_rewire_geometry_one_undo() {
    for up in [true, false] {
        let Fixture { mut c, sketch, stock, hole, array } = fixture();
        let before = c.snapshot();
        let args = if up { json!({"feature":array,"direction":"up"}) } else { json!({"feature":hole,"direction":"down"}) };
        let moved = c.ok("feature_reorder", args.clone());
        assert_eq!(moved["changed"], true);
        assert_eq!(moved["feature"]["id"], args["feature"]);
        assert_eq!(moved["order"], json!([sketch, stock, array, hole]));
        assert_eq!(
            moved["rewired"],
            json!([
            {"feature":array,"before_inputs":[hole],"after_inputs":[stock]},
            {"feature":hole,"before_inputs":[stock],"after_inputs":[array]}])
        );
        let bodies = moved["rebuild"]["bodies"].as_array().unwrap();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0]["id"], hole);
        // Moving the single bore after the array drills only one copy: 2·1000−π·1²·10.
        volume(&bodies[0], 2000.0 - 10.0 * std::f64::consts::PI);
        let undo = c.ok("undo", json!({}));
        assert_eq!(undo["undone"]["tool"], "feature_reorder");
        assert_eq!(undo["undone"]["arguments"], args);
        assert!(!undo["undone"]["label"].as_str().unwrap().is_empty());
        assert_eq!(c.snapshot(), before);
        assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "linear_array", "one move consumes one entry");
    }
}
#[test]
fn t_stdio_boundary_noop_preserves_history_and_archive() {
    let Fixture { mut c, stock, array, .. } = fixture();
    let save = path("noop");
    let bytes = archive(&mut c, &save);
    let original = c.snapshot();
    for (id, direction) in [(stock, "up"), (array, "down")] {
        let result = c.ok("feature_reorder", json!({"feature":id,"direction":direction}));
        assert_eq!(result["changed"], false);
        assert_eq!(result["from_index"], result["to_index"]);
        assert_eq!(result["rewired"], json!([]));
        assert_eq!(result["lost_body_metadata"], json!([]));
        assert_eq!(c.snapshot(), original);
        assert_eq!(archive(&mut c, &save), bytes);
    }
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "linear_array");
}
#[test]
fn t_stdio_batch_direction_prevalidation_and_later_failure_restore_all_bytes() {
    let Fixture { mut c, stock, hole, array, .. } = fixture();
    let save = path("failed-batch");
    let bytes = archive(&mut c, &save);
    let before = c.snapshot();
    // The first step would fail at execution. Step 2 must still win at prevalidation.
    let invalid = json!({"steps":[{"tool":"rename","args":{"target":{"feature":999999},"name":"changed"}},{"tool":"feature_reorder","args":{"feature":hole,"direction":"sideways"}}]});
    let error = c.tool("batch", invalid).unwrap_err();
    assert_eq!(
        error,
        "batch step 2 (`feature_reorder`): invalid request: unknown reorder direction `sideways`; use up or down; nothing was changed"
    );
    assert_eq!(c.snapshot(), before);
    assert_eq!(archive(&mut c, &save), bytes);
    let invalid = json!({"steps":[{"tool":"feature_reorder","args":{"feature":array,"direction":"up"}},{"tool":"feature_reorder","args":{"feature":array,"direction":"up"}}]});
    let error = c.tool("batch", invalid).unwrap_err();
    assert_eq!(error,format!("batch step 2 (`feature_reorder`): invalid request: cannot move feature {array} up across feature {stock}: native QymCAD refuses this order or chain rewrite; nothing was changed"));
    assert_eq!(c.snapshot(), before);
    assert_eq!(archive(&mut c, &save), bytes);
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "linear_array", "failure adds no history");
}
#[test]
fn t_stdio_real_then_noop_batch_retains_one_entry_and_compact_metadata() {
    let Fixture { mut c, hole, array, .. } = fixture();
    let before = c.snapshot();
    let args = json!({"steps":[{"tool":"feature_reorder","args":{"feature":array,"direction":"up"}},{"tool":"feature_reorder","args":{"feature":hole,"direction":"down"}}]});
    let result = c.ok("batch", args.clone());
    for (i, changed) in [true, false].into_iter().enumerate() {
        let step = &result["steps"][i];
        assert_eq!(step["changed"], changed);
        assert!(step.get("created").is_none());
        assert_eq!(step["feature"]["id"], if i == 0 { array.clone() } else { hole.clone() });
        assert_eq!(step["direction"], if i == 0 { "up" } else { "down" });
        assert!(step["from_index"].is_number());
        assert!(step["to_index"].is_number());
    }
    volume(&result["bodies"][0], 2000.0 - 10.0 * std::f64::consts::PI);
    let undo = c.ok("undo", json!({}));
    assert_eq!(undo["undone"]["tool"], "batch");
    assert_eq!(undo["undone"]["arguments"], args);
    assert_eq!(c.snapshot(), before);
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "linear_array");
}
#[test]
fn t_stdio_all_noop_batch_keeps_existing_one_entry_policy() {
    let Fixture { mut c, stock, array, .. } = fixture();
    let before = c.snapshot();
    let result=c.ok("batch",json!({"steps":[{"tool":"feature_reorder","args":{"feature":stock,"direction":"up"}},{"tool":"feature_reorder","args":{"feature":array,"direction":"down"}}],"verbose":true}));
    assert_eq!(result["steps"][0]["result"]["changed"], false);
    assert_eq!(result["steps"][1]["result"]["changed"], false);
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(c.snapshot(), before);
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "linear_array");
}
