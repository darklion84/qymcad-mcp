//! E4 acceptance through a real JSON-RPC stdio server, with independent 3MF inspection.
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
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
        let init =
            c.request("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"e4","version":"1"}}));
        assert!(init["result"].is_object(), "{init}");
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
    fn content(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let response = self.request("tools/call", json!({"name":name,"arguments":args}));
        if !response["error"].is_null() {
            return Err(response["error"].to_string());
        }
        let result = &response["result"];
        if result["isError"] == true {
            return Err(result["content"][0]["text"].as_str().unwrap().to_owned());
        }
        Ok(result["content"].clone())
    }
    fn ok(&mut self, name: &str, args: Value) -> Value {
        let content = self.content(name, args).unwrap_or_else(|e| panic!("{name}: {e}"));
        serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
    }
    fn snapshot(&mut self) -> Value {
        let doc = self.ok("doc_info", json!({}));
        let topologies: Vec<_> =
            doc["bodies"].as_array().unwrap().iter().map(|b| self.ok("topology", json!({"body":b["id"],"adjacency":true}))).collect();
        json!({"doc":doc,"topology":topologies})
    }
    fn refused(&mut self, name: &str, args: Value, hidden: Option<&Value>) {
        let before = self.snapshot();
        let error = self.content(name, args).expect_err("hidden-only output must be refused");
        assert!(error.to_lowercase().contains("hidden") || error.to_lowercase().contains("visible"), "{error}");
        if let Some(id) = hidden {
            assert!(error.contains(&id.to_string()) && error.contains("B"), "refusal must name hidden B: {error}");
        }
        assert_eq!(self.snapshot(), before, "refusal changed full doc_info/topology: {error}");
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("e4-stdio-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn entries(&self) -> Vec<String> {
        let mut entries: Vec<_> =
            std::fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().file_name().to_str().unwrap().to_owned()).collect();
        entries.sort();
        entries
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn fixture() -> (Client, Value, Value) {
    let mut c = Client::start();
    c.ok("doc_new", json!({}));
    let mut bodies = Vec::new();
    for (name, x) in [("A", 0), ("B", 30)] {
        let sketch = c.ok("sketch_create", json!({"plane":"XY","name":format!("{name} profile")}))["sketch"].clone();
        c.ok("sketch_add", json!({"sketch":sketch,"entities":[{"type":"rect","anchor":"corner","cx":x,"w":10,"h":20}]}));
        let body = c.ok("extrude", json!({"sketch":sketch,"height":4,"op":"new_body","name":name}))["body"].clone();
        // Body labels remain metadata, independent of generic flat export object names (ADR 0042).
        assert_eq!(c.ok("rename", json!({"target":{"body":body},"name":name}))["name"], name);
        bodies.push(body);
    }
    let info = c.ok("doc_info", json!({}));
    assert_eq!(info["bodies"].as_array().unwrap().len(), 2);
    for (i, b) in info["bodies"].as_array().unwrap().iter().enumerate() {
        assert_eq!(b["id"], bodies[i]);
        // Rectangle area × extrusion: 10 × 20 × 4 = 800 mm³ for each independent prism.
        assert!((b["volume_mm3"].as_f64().unwrap() - 10.0 * 20.0 * 4.0).abs() < 1e-6);
    }
    let a = c.ok("appearance", json!({"body":bodies[0],"color":[17,34,51]}));
    assert_eq!(a["color_rgb"], json!([17, 34, 51]));
    assert_eq!(a["color_source"], "manual");
    let b = c.ok("appearance", json!({"body":bodies[1],"visible":false}));
    assert_eq!(b["visible"], false);
    // Native palette's second lineage is brass: tree.rs mesh_color/part_color_index and curves.rs PALETTE[1].
    assert_eq!(b["color_rgb"], json!([200, 160, 110]));
    assert_eq!(b["color_source"], "palette");
    (c, bodies.remove(0), bodies.remove(0))
}
fn ids(report: &Value) -> Value {
    Value::Array(report["bodies"].as_array().unwrap().iter().map(|b| b["id"].clone()).collect())
}
fn xml(path: &Path) -> String {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut model = String::new();
    archive.by_name("3D/3dmodel.model").unwrap().read_to_string(&mut model).unwrap();
    model
}
fn attr<'a>(tag: &'a str, name: &str) -> &'a str {
    tag.split_once(&format!(" {name}=\"")).unwrap_or_else(|| panic!("missing {name} in {tag}")).1.split('"').next().unwrap()
}
fn assigned_colours(model: &str) -> Vec<(String, String)> {
    let palette: Vec<_> = model.split("<base ").skip(1).map(|s| attr(s.split('>').next().unwrap(), "displaycolor")).collect();
    model
        .split("<object ")
        .skip(1)
        .map(|s| {
            let tag = s.split('>').next().unwrap();
            assert_eq!(attr(tag, "pid"), "1");
            let index: usize = attr(tag, "pindex").parse().unwrap();
            (attr(tag, "name").to_owned(), palette[index].to_owned())
        })
        .collect()
}

#[test]
fn e4_stdio_3mf_exact_manual_palette_assignments_and_result_fields() {
    let (mut c, a, b) = fixture();
    let scratch = Scratch::new();
    let path = scratch.path("coloured.3mf");
    let report = c.ok("export", json!({"format":"3mf","path":path}));
    assert_eq!(report["colors"], true);
    assert_eq!(report["skipped_hidden"], json!([]));
    assert_eq!(ids(&report), json!([a, b]), "default includes both current bodies, even hidden B");
    for (i, (rgb, source)) in [([17, 34, 51], "manual"), ([200, 160, 110], "palette")].into_iter().enumerate() {
        assert_eq!(report["bodies"][i]["color_rgb"], json!(rgb));
        assert_eq!(report["bodies"][i]["color_source"], source);
        assert_eq!(report["bodies"][i]["mesh_volume_mm3"].as_f64().unwrap(), 10.0 * 20.0 * 4.0);
    }
    // A prism has six quadrilateral faces, split into two triangles per face.
    assert_eq!(report["triangles"], 2 * 6 * 2);
    assert_eq!(report["deflection_mm"], 0.05);
    assert_eq!(assigned_colours(&xml(&path)), vec![("body_1".into(), "#112233".into()), ("body_2".into(), "#C8A06E".into())]);
    assert_eq!(scratch.entries(), vec!["coloured.3mf"]);
}

#[test]
fn e4_stdio_export_visible_only_exact_membership_and_atomic_refusals() {
    let (mut c, a, b) = fixture();
    let scratch = Scratch::new();
    let report = c.ok("export", json!({"format":"3mf","path":scratch.path("visible.3mf"),"visible_only":true}));
    assert_eq!(ids(&report), json!([a]));
    assert_eq!(report["skipped_hidden"], json!([b]));
    assert_eq!(report["colors"], true);
    assert_eq!(report["triangles"], 6 * 2); // Six rectangular faces × two triangles.
    assert_eq!(report["bodies"][0]["mesh_volume_mm3"].as_f64().unwrap(), 10.0 * 20.0 * 4.0);
    assert_eq!(assigned_colours(&xml(&scratch.path("visible.3mf"))), vec![("body_1".into(), "#112233".into())]);
    let explicit = c.ok("export", json!({"format":"3mf","path":scratch.path("explicit.3mf"),"bodies":[a],"visible_only":true}));
    assert_eq!(ids(&explicit), json!([a]));
    assert_eq!(explicit["skipped_hidden"], json!([]));
    let before_entries = scratch.entries();
    let refused = scratch.path("hidden.3mf");
    c.refused("export", json!({"format":"3mf","path":refused,"bodies":[b],"visible_only":true}), Some(&b));
    assert!(!refused.exists());
    assert_eq!(scratch.entries(), before_entries, "no private staging leftovers");
    assert_eq!(c.ok("appearance", json!({"body":a,"visible":false}))["visible"], false);
    let refused = scratch.path("all-hidden.3mf");
    c.refused("export", json!({"format":"3mf","path":refused,"visible_only":true}), None);
    assert!(!refused.exists());
    assert_eq!(scratch.entries(), before_entries, "no private staging leftovers");
}

fn assert_image(content: &Value, bodies: &[&Value], skipped: &[&Value], xmax: u32) {
    assert_eq!(content.as_array().unwrap().len(), 2);
    assert_eq!(content[0]["type"], "image");
    assert_eq!(content[0]["mimeType"], "image/png");
    // PNG signature base64; IHDR encodes width=64, height=64 in this exact prefix.
    assert!(content[0]["data"].as_str().unwrap().starts_with("iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIA"));
    assert_eq!(content[1]["type"], "text");
    let ids = bodies.iter().map(|id| id.as_u64().unwrap()).collect::<Vec<_>>();
    let skipped = skipped.iter().map(|id| id.as_u64().unwrap()).collect::<Vec<_>>();
    // Corner-anchored A is [0,10]×[0,20]×[0,4]; B shifts X by 30, so union xmax=40.
    assert_eq!(
        content[1]["text"],
        format!("top view, from above: +X right, +Y up: bodies {ids:?}, bbox x 0..{xmax} y 0..20 z 0..4 mm\nskipped_hidden: {skipped:?}")
    );
}

#[test]
fn e4_stdio_render_visible_only_image_membership_and_atomic_refusals() {
    let (mut c, a, b) = fixture();
    let default = c.content("render", json!({"view":"top","width":64,"height":64})).unwrap();
    assert_image(&default, &[&a, &b], &[], 30 + 10);
    let visible = c.content("render", json!({"view":"top","width":64,"height":64,"visible_only":true})).unwrap();
    assert_image(&visible, &[&a], &[&b], 10);
    let explicit = c.content("render", json!({"view":"top","width":64,"height":64,"visible_only":true,"bodies":[a]})).unwrap();
    assert_image(&explicit, &[&a], &[], 10);
    assert_eq!(visible[0], explicit[0], "same selected A produces byte-identical PNG");
    c.refused("render", json!({"width":64,"height":64,"visible_only":true,"bodies":[b]}), Some(&b));
    assert_eq!(c.ok("appearance", json!({"body":a,"visible":false}))["visible"], false);
    c.refused("render", json!({"width":64,"height":64,"visible_only":true}), None);
}

#[test]
fn e4_stdio_colourless_stl_obj_bytes_ignore_manual_colour_and_schema_defaults() {
    let (mut c, a, _) = fixture();
    let scratch = Scratch::new();
    let listed = c.request("tools/list", json!({}));
    for name in ["export", "render"] {
        let tool = listed["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(tool["inputSchema"]["properties"]["visible_only"]["type"], "boolean");
        assert_eq!(tool["inputSchema"]["properties"]["visible_only"]["default"], false);
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    }
    for format in ["stl", "obj"] {
        let coloured = scratch.path(&format!("coloured.{format}"));
        let report = c.ok("export", json!({"format":format,"path":coloured,"bodies":[a]}));
        assert_eq!(report["colors"], false);
        assert_eq!(report["skipped_hidden"], json!([]));
        assert_eq!(report["bodies"][0]["color_rgb"], json!([17, 34, 51]));
        assert_eq!(report["bodies"][0]["color_source"], "manual");
        assert_eq!(report["triangles"], 6 * 2); // Six prism faces × two triangles.
        assert_eq!(report["bodies"][0]["mesh_volume_mm3"].as_f64().unwrap(), 10.0 * 20.0 * 4.0);
        let reset = c.ok("appearance", json!({"body":a,"color":null}));
        assert_eq!(reset["color_rgb"], json!([150, 170, 190]));
        assert_eq!(reset["color_source"], "palette");
        let plain = scratch.path(&format!("plain.{format}"));
        let report = c.ok("export", json!({"format":format,"path":plain,"bodies":[a]}));
        assert_eq!(report["colors"], false);
        assert_eq!(report["bodies"][0]["color_source"], "palette");
        assert_eq!(std::fs::read(coloured).unwrap(), std::fs::read(plain).unwrap());
        assert_eq!(c.ok("appearance", json!({"body":a,"color":[17,34,51]}))["color_rgb"], json!([17, 34, 51]));
    }
}
