//! E2 measurement acceptance over the real JSON-RPC stdio transport.
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
            c.request("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"e2","version":"1"}}));
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

fn fixture() -> (Client, Value, Value) {
    let mut c = Client::start();
    c.ok("doc_new", json!({}));
    c.ok("param_set", json!({"name":"h","value":5}));
    let sketch = c.ok("sketch_create", json!({"plane":"XY","name":"profile"}))["sketch"].clone();
    c.ok("sketch_add", json!({"sketch":sketch,"entities":[{"type":"rect","w":20,"h":10}]}));
    let body = c.ok("extrude", json!({"sketch":sketch,"height":"h","name":"plate"}))["body"].clone();
    volume(&c.ok("doc_info", json!({}))["bodies"][0], 20.0 * 10.0 * 5.0);
    (c, sketch, body)
}
fn volume(body: &Value, expected: f64) {
    assert!((body["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6, "{body}; expected {expected}");
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
            (name, bytes)
        })
        .collect()
}
fn same_archive(actual: &BTreeMap<String, Vec<u8>>, expected: &BTreeMap<String, Vec<u8>>) {
    assert_eq!(actual.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
    for (name, bytes) in expected {
        assert!(actual[name] == *bytes, "archive entry {name} differs");
    }
}

fn face(body: &Value, selection: Value) -> Value {
    json!({"body":body,"face":selection})
}
fn edge(body: &Value, selection: Value) -> Value {
    json!({"body":body,"edge":selection})
}
fn measure(c: &mut Client, a: Value, b: Option<Value>) -> Value {
    let mut args = json!({"a":a});
    if let Some(b) = b {
        args["b"] = b;
    }
    c.ok("measure", args)
}
fn near(actual: &Value, expected: f64) {
    assert!((actual.as_f64().expect("number") - expected).abs() < 1e-8, "{actual}, expected {expected}");
}
fn selected(c: &mut Client, body: &Value, kind: &str, selection: Value) -> Value {
    let v = c.ok("select", json!({"body":body,kind:selection}));
    let items = v[kind].as_array().expect("selected rows");
    assert_eq!(items.len(), 1, "{v}");
    items[0]["id"].clone()
}

#[test]
fn e2_stdio_points_native_support_semantics_and_exact_fields() {
    let (mut c, _, _body) = fixture();
    // Pythagoras: sqrt(3²+4²)=5; native delta is b-a.
    assert_eq!(
        measure(&mut c, json!({"point":[0,0,0]}), Some(json!({"point":[3,4,0]}))),
        json!({"distance_mm":5.0,"distance_kind":"point_point","delta_mm":[3.0,4.0,0.0],"resolved":[{},{}],"coordinate_frame":"world"})
    );
    assert_eq!(
        measure(&mut c, json!({"point":[3,4,0]}), Some(json!({"point":[0,0,0]}))),
        json!({"distance_mm":5.0,"distance_kind":"point_point","delta_mm":[-3.0,-4.0,0.0],"resolved":[{},{}],"coordinate_frame":"world"})
    );
    // A separate 1×1×7 block provides a true x=0..1 finite control and skew lines.
    let sketch = c.ok("sketch_create", json!({"plane":"XY","name":"unit"}))["sketch"].clone();
    c.ok("sketch_add", json!({"sketch":sketch,"entities":[{"type":"rect","cx":0.5,"cy":0.5,"w":1,"h":1}]}));
    let unit = c.ok("extrude", json!({"sketch":sketch,"height":7,"op":"new_body","name":"unit block"}))["body"].clone();
    let bottom_x = json!({"and":[{"along":"x"},{"extreme":"-z"},{"extreme":"-y"}]});
    let id = selected(&mut c, &unit, "edges", bottom_x.clone());
    let r = measure(&mut c, json!({"point":[100,0,0]}), Some(edge(&unit, bottom_x.clone())));
    // The supporting x-axis contains (100,0,0), beyond the finite edge x=0..1.
    assert_eq!(
        r,
        json!({"distance_mm":0.0,"distance_kind":"point_support_line","resolved":[{},{"body":unit,"edge":id}],"coordinate_frame":"world"})
    );
    assert_ne!(r["distance_mm"], 99.0, "finite segment distance is 100-1=99");
    let top_y = json!({"and":[{"along":"y"},{"extreme":"+z"},{"extreme":"-x"}]});
    let top_y_id = selected(&mut c, &unit, "edges", top_y.clone());
    // The supporting x/y lines are perpendicular, separated only by z=7.
    assert_eq!(
        measure(&mut c, edge(&unit, bottom_x), Some(edge(&unit, top_y))),
        json!({"distance_mm":7.0,"distance_kind":"support_lines","angle_deg":90.0,"angle_kind":"acute_unsigned","resolved":[{"body":unit,"edge":id},{"body":unit,"edge":top_y_id}],"coordinate_frame":"world"})
    );
}

#[test]
fn e2_stdio_block_single_pair_and_open() {
    let (mut c, _, body) = fixture();
    let top = face(&body, json!({"facing":"+z"}));
    let bottom = face(&body, json!({"facing":"-z"}));
    let top_id = selected(&mut c, &body, "faces", json!({"facing":"+z"}));
    let bottom_id = selected(&mut c, &body, "faces", json!({"facing":"-z"}));
    let gap = json!({"distance_mm":5.0,"distance_kind":"support_plane_gap","angle_deg":0.0,"angle_kind":"acute_unsigned","resolved":[{"body":body,"face":top_id},{"body":body,"face":bottom_id}],"coordinate_frame":"world"});
    assert_eq!(measure(&mut c, top.clone(), Some(bottom.clone())), gap);
    let side_id = selected(&mut c, &body, "faces", json!({"facing":"+x"}));
    assert_eq!(
        measure(&mut c, top.clone(), Some(face(&body, json!({"facing":"+x"})))),
        json!({"angle_deg":90.0,"angle_kind":"acute_unsigned","resolved":[{"body":body,"face":top_id},{"body":body,"face":side_id}],"coordinate_frame":"world"})
    );
    let long_top = json!({"and":[{"along":"x"},{"extreme":"+z"},{"extreme":"-y"}]});
    let long_id = selected(&mut c, &body, "edges", long_top.clone());
    assert_eq!(
        measure(&mut c, edge(&body, long_top), None),
        json!({"length_mm":20.0,"resolved":[{"body":body,"edge":long_id}],"coordinate_frame":"world"})
    );
    // Plane z=h=5; point z=12: perpendicular gap 12-5=7.
    near(&measure(&mut c, json!({"point":[0,0,12]}), Some(top.clone()))["distance_mm"], 12.0 - 5.0);
    let path = std::env::temp_dir().join(format!("qymcad-e2-open-{}.qcad", std::process::id()));
    c.ok("doc_save", json!({"path":path}));
    c.ok("doc_open", json!({"path":path}));
    assert_eq!(measure(&mut c, top.clone(), Some(bottom.clone())), gap);
    c.ok("param_set", json!({"name":"h","value":8}));
    near(&measure(&mut c, top.clone(), Some(bottom))["distance_mm"], 8.0);
    near(&measure(&mut c, json!({"point":[0,0,12]}), Some(top))["distance_mm"], 12.0 - 8.0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn e2_stdio_cylinder_exact_fields_and_fallback_refusal() {
    let mut c = Client::start();
    c.ok("doc_new", json!({}));
    let sketch = c.ok("sketch_create", json!({"plane":"XY"}))["sketch"].clone();
    c.ok("sketch_add", json!({"sketch":sketch,"entities":[{"type":"circle","d":8}]}));
    let body = c.ok("extrude", json!({"sketch":sketch,"height":10}))["body"].clone();
    let cylinder = face(&body, json!({"kind":"cylinder"}));
    let cylinder_id = selected(&mut c, &body, "faces", json!({"kind":"cylinder"}));
    assert_eq!(
        measure(&mut c, cylinder.clone(), None),
        json!({"radius_mm":4.0,"diameter_mm":8.0,"resolved":[{"body":body,"face":cylinder_id}],"coordinate_frame":"world"})
    );
    let rim_sel = json!({"and":[{"kind":"circle"},{"extreme":"+z"}]});
    let rim = edge(&body, rim_sel.clone());
    let rim_id = selected(&mut c, &body, "edges", rim_sel);
    let result = measure(&mut c, rim.clone(), None);
    // Circle circumference: 2πr, r=4.
    assert_eq!(
        result,
        json!({"radius_mm":4.0,"diameter_mm":8.0,"length_mm":2.0*std::f64::consts::PI*4.0,"resolved":[{"body":body,"edge":rim_id}],"coordinate_frame":"world"})
    );
    assert_eq!(
        measure(&mut c, json!({"point":[10,0,5]}), Some(cylinder)),
        json!({"distance_mm":6.0,"distance_kind":"point_cylinder_surface","resolved":[{},{"body":body,"face":cylinder_id}],"coordinate_frame":"world"})
    );
    let error = c.refuse_unchanged("measure", json!({"a":rim,"b":face(&body,json!({"facing":"+z"}))}));
    assert!(error.contains("at"), "{error}");
    // Refusal creates no undo step: the last model operation is still extrude.
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "extrude");
}

#[test]
fn e2_stdio_strict_refusals_preserve_state_archive_and_history() {
    let (mut c, _, body) = fixture();
    let topology = c.ok("topology", json!({"body":body}));
    let edge_id = topology["edges"][0]["id"].clone();
    let face_id = topology["faces"][0]["id"].clone();
    let path = std::env::temp_dir().join(format!("qymcad-e2-readonly-{}.qcad", std::process::id()));
    let before = archive(&mut c, &path);
    let failures = vec![
        json!({"a":{"point":[0,0,0]}}),
        json!({"a":face(&body,json!({"facing":"+z"}))}),
        json!({"a":face(&body,json!([])),"b":{"point":[0,0,0]}}),
        json!({"a":face(&body,json!({"union":[{"facing":"+z"},{"facing":"-z"}]})),"b":{"point":[0,0,0]}}),
        json!({"a":face(&body,edge_id.clone()),"b":{"point":[0,0,0]}}),
        json!({"a":edge(&body,face_id),"b":{"point":[0,0,0]}}),
        json!({"a":{"body":body,"edge":edge_id,"at":"center"},"b":{"point":[0,0,0]}}),
        json!({"a":{"point":[null,0,0]},"b":{"point":[0,0,0]}}),
        json!({"a":{"body":body}}),
        json!({"a":{"body":body,"point":[0,0,0]}}),
        json!({"a":{"body":body,"edge":edge_id,"face":edge_id}}),
        json!({"a":{"point":[0,0,0],"at":"mid"}}),
        json!({"a":{"point":[0,0,0]},"b":{"point":[1,0,0]},"unexpected":true}),
        json!({"a":{"point":[0,0,0]},"b":{"point":[1,0,0],"unexpected":true}}),
    ];
    for args in failures {
        c.refuse_unchanged("measure", args);
    }
    let batch_error =
        c.refuse_unchanged("batch", json!({"steps":[{"tool":"measure","args":{"a":{"point":[0,0,0]},"b":{"point":[3,4,0]}}}]}));
    assert!(batch_error.contains("measure") && batch_error.contains("not allowed"), "{batch_error}");
    assert_eq!(measure(&mut c, json!({"point":[0,0,0]}), Some(json!({"point":[3,4,0]})))["distance_mm"], 5.0);
    same_archive(&archive(&mut c, &path), &before);
    let schema = c.request("tools/list", json!({}));
    let measure_schema = &schema["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "measure").unwrap()["inputSchema"];
    assert_eq!(measure_schema["additionalProperties"], false);
    assert_eq!(measure_schema["required"], json!(["a"]));
    // Every refusal and the successful read leave the previous modelling call on top.
    assert_eq!(c.ok("undo", json!({}))["undone"]["tool"], "extrude");
    std::fs::remove_file(path).unwrap();
}
