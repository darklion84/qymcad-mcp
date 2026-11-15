//! Stage A2 wire contracts: tilted datum planes and native asymmetric chamfers.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Command, Stdio},
};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}
fn fresh() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    r
}
fn stock(r: &mut Registry) {
    call(r, "sketch_create", json!({"plane":"XY","name":"profile"}));
    call(r, "sketch_add", json!({"sketch":"profile","entities":[{"type":"rect","w":40,"h":30}]}));
    call(r, "extrude", json!({"sketch":"profile","height":10,"name":"stock"}));
}
fn edge() -> Value {
    json!({"between":[{"facing":"+x"},{"facing":"+y"}]})
}
fn volume(r: &mut Registry, expected: f64) {
    let b = call(r, "doc_info", json!({}))["bodies"][0].clone();
    assert!((b["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6, "{b}, expected {expected}");
}
fn chain_bbox(r: &mut Registry, degrees: f64) {
    let a = degrees.to_radians();
    let (s, c) = a.sin_cos();
    // Native sketch origin projects world zero onto the child plane. Its signed
    // distance is parent offset * cos(a) + child offset; local axes are +X,(0,c,s).
    let d = 7.0 * c + 2.0;
    let expected = [-5.0, -d * s - 3.0 * c - 4.0 * s, d * c - 3.0 * s, 5.0, -d * s + 3.0 * c, d * c + 3.0 * s + 4.0 * c];
    let b = call(r, "doc_info", json!({}))["bodies"][0].clone();
    assert!((b["volume_mm3"].as_f64().unwrap() - 240.0).abs() < 1e-6);
    for (actual, expected) in b["bbox"].as_array().unwrap().iter().zip(expected) {
        assert!((actual.as_f64().unwrap() - expected).abs() < 1e-4, "{b}, expected {expected}");
    }
}
fn archive(r: &mut Registry, path: &Path) -> BTreeMap<String, Vec<u8>> {
    call(r, "doc_save", json!({"path":path}));
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..z.len())
        .map(|i| {
            let mut e = z.by_index(i).unwrap();
            let n = e.name().to_owned();
            let mut b = Vec::new();
            e.read_to_end(&mut b).unwrap();
            (n, b)
        })
        .collect()
}
fn same_archive(actual: &BTreeMap<String, Vec<u8>>, expected: &BTreeMap<String, Vec<u8>>) {
    assert_eq!(actual.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
    for (name, bytes) in expected {
        assert!(actual[name] == *bytes, "archive entry {name} differs");
    }
}

#[test]
fn a2_schema_strict_fields_and_json_string_leniency() {
    let mut r = fresh();
    let list = r.list();
    for (tool, fields) in [("plane_offset", vec!["tilt_x", "tilt_y", "turn"]), ("chamfer", vec!["angle", "reference_face", "flip"])] {
        let schema = &list.iter().find(|t| t["name"] == tool).unwrap()["inputSchema"];
        assert_eq!(schema["additionalProperties"], false);
        for field in fields {
            assert!(schema["properties"][field].is_object(), "{tool}.{field}");
        }
    }
    let p =
        call(&mut r, "plane_offset", json!({"base":"XY","dist":7,"tilt_x":30,"tilt_y":20,"turn":15,"name":"tilted"}).to_string().into());
    assert!(p["plane"].is_number());
    assert_eq!(p.as_object().unwrap().len(), 2, "legacy result remains plane/rebuild");
    call(&mut r, "sketch_create", json!({"plane":{"plane":"tilted"},"name":"mixed"}));
    call(&mut r, "sketch_add", json!({"sketch":"mixed","entities":[{"type":"rect","w":10,"h":6}]}));
    call(&mut r, "extrude", json!({"sketch":"mixed","height":4}));
    volume(&mut r, 240.0);
    // Eight prism corners using n=(sin20*cos30,-sin30,cos20*cos30),
    // origin=(7*n.z)*n and drawing axes rotated 15 degrees around n.
    let mixed = call(&mut r, "doc_info", json!({}))["bodies"][0]["bbox"].clone();
    for (actual, expected) in
        mixed.as_array().unwrap().iter().zip([-3.80113256, -9.84909845, 2.19976691, 8.36056004, 2.15251468, 10.32715714])
    {
        assert!((actual.as_f64().unwrap() - expected).abs() < 1e-4, "{mixed}");
    }
    assert!(r.call("plane_offset", json!({"base":"XY","dist":0,"tilt_z":30})).unwrap().unwrap_err().contains("unknown field"));
    let mut r = fresh();
    stock(&mut r);
    assert!(r
        .call("chamfer", json!({"edges":edge(),"dist":2,"angle":30,"flip":false,"unknown":1}))
        .unwrap()
        .unwrap_err()
        .contains("unknown field"));
}

#[test]
fn tilted_datum_chain_global_named_id_name_batch_and_undo() {
    let mut r = fresh();
    call(&mut r, "param_set", json!({"name":"ax","value":30}));
    let parent = call(&mut r, "plane_offset", json!({"base":"XY","dist":7,"tilt_x":"ax","name":"ancestor"}))["plane"].clone();
    call(&mut r, "plane_offset", json!({"base":{"plane":parent},"dist":2,"name":"child"}));
    call(&mut r, "sketch_create", json!({"plane":{"plane":"child"},"name":"profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"profile","entities":[{"type":"rect","w":10,"h":6}]}));
    call(&mut r, "extrude", json!({"sketch":"profile","height":4}));
    chain_bbox(&mut r, 30.0);
    call(&mut r, "dimension_set", json!({"target":{"feature":parent,"key":"tilt_x"},"name":"lean"}));
    call(&mut r, "batch", json!({"steps":[{"tool":"param_set","args":{"name":"ax","value":40}}]}));
    chain_bbox(&mut r, 40.0);
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "batch");
    chain_bbox(&mut r, 30.0);
    call(&mut r, "dimension_set", json!({"target":{"feature":"ancestor","key":"tilt_x"},"value":45}));
    chain_bbox(&mut r, 45.0);
}

#[test]
fn chamfer_modes_reference_and_branch_aware_named_dimension() {
    for (extra, mode, expected) in [
        (json!({}), "symmetric", 11980.0),
        (json!({"d2":4}), "two_dist", 11960.0),
        (json!({"angle":30}), "dist_angle", 12000.0 - 20.0 * 30_f64.to_radians().tan()),
    ] {
        let mut r = fresh();
        stock(&mut r);
        let mut args = json!({"body":"stock","edges":edge(),"dist":2,"name":"bevel"});
        for (k, v) in extra.as_object().unwrap() {
            args[k] = v.clone();
        }
        if mode != "symmetric" {
            args["reference_face"] = json!({"facing":"+x"});
            args["flip"] = json!(true);
        }
        let result = call(&mut r, "chamfer", args);
        assert_eq!(result["chamfer"]["mode"], mode);
        assert_eq!(result["chamfer"]["flip"], mode != "symmetric");
        if mode == "symmetric" {
            assert!(result["chamfer"]["reference_face"].is_null());
        } else {
            assert!(result["chamfer"]["reference_face"].is_number());
        }
        volume(&mut r, expected);
        if mode != "symmetric" {
            let d = call(
                &mut r,
                "dimension_set",
                json!({"target":{"feature":"bevel","key":"d2"},"name":"other","value":if mode=="dist_angle" {45} else {5}}),
            );
            assert_eq!(d["dimension"]["mode"], mode);
            assert_eq!(d["dimension"]["unit"], if mode == "dist_angle" { "deg" } else { "mm" });
            volume(&mut r, if mode == "dist_angle" { 11980.0 } else { 11950.0 });
        }
    }
}

#[test]
fn angle_and_reference_refusals_restore_exact_archive_and_history() {
    let mut r = fresh();
    stock(&mut r);
    call(&mut r, "param_set", json!({"name":"angle_driver","value":30}));
    call(&mut r, "chamfer", json!({"edges":edge(),"dist":2,"angle":"angle_driver","reference_face":{"facing":"+x"},"name":"bevel"}));
    let path = std::env::temp_dir().join(format!("a2-refusals-{}.qcad", std::process::id()));
    let before = archive(&mut r, &path);
    for (tool, args) in [
        ("param_set", json!({"name":"angle_driver","value":181})),
        ("dimension_set", json!({"target":{"feature":"bevel","key":"d2"},"value":0})),
        ("dimension_set", json!({"target":{"feature":"bevel","key":"d2"},"value":90})),
        ("chamfer", json!({"edges":{"along":"z"},"dist":1,"angle":30,"d2":2})),
        ("chamfer", json!({"edges":{"along":"z"},"dist":1,"reference_face":{"facing":"+z"}})),
        ("chamfer", json!({"edges":{"along":"z"},"dist":1,"flip":true})),
        ("chamfer", json!({"edges":{"along":"z"},"dist":1,"angle":30,"reference_face":{"kind":"plane"}})),
        ("chamfer", json!({"edges":{"along":"z"},"dist":1,"angle":30,"reference_face":{"facing":"+z"}})),
        ("plane_offset", json!({"base":"XY","dist":100001})),
        ("plane_offset", json!({"base":"XY","dist":0,"tilt_x":361})),
        ("plane_offset", json!({"base":"XY","dist":0,"tilt_y":-361})),
        ("plane_offset", json!({"base":"XY","dist":0,"turn":361})),
        (
            "batch",
            json!({"steps":[{"tool":"param_set","args":{"name":"angle_driver","value":45}},{"tool":"dimension_set","args":{"target":{"feature":"bevel","key":"d2"},"value":181}}]}),
        ),
    ] {
        assert!(r.call(tool, args.clone()).unwrap().is_err(), "accepted {tool} {args}");
        same_archive(&archive(&mut r, &path), &before);
    }
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "chamfer");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn tilted_plane_evaluated_bounds_restore_exact_archive() {
    let mut r = fresh();
    call(&mut r, "param_set", json!({"name":"turn_driver","value":15}));
    call(&mut r, "plane_offset", json!({"base":"XY","dist":0,"turn":"turn_driver","name":"tilted"}));
    stock(&mut r);
    let path = std::env::temp_dir().join(format!("a2-plane-refusals-{}.qcad", std::process::id()));
    let before = archive(&mut r, &path);
    for (tool, args) in [
        ("param_set", json!({"name":"turn_driver","value":361})),
        ("dimension_set", json!({"target":{"feature":"tilted","key":"turn"},"value":-361})),
        ("dimension_set", json!({"target":{"feature":"tilted","key":"tilt_x"},"value":361})),
        ("dimension_set", json!({"target":{"feature":"tilted","key":"tilt_y"},"value":361})),
        ("dimension_set", json!({"target":{"feature":"tilted","key":"dist"},"value":100001})),
    ] {
        assert!(r.call(tool, args.clone()).unwrap().is_err(), "accepted {tool} {args}");
        same_archive(&archive(&mut r, &path), &before);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn stage_a2_real_stdio_batch_results_errors_and_undo() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_qymcad-mcp"))
        .env("QYMCAD_MCP_LOG", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut request = |id: u64, name: &str, args: Value| -> Value {
        let message = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}});
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], id);
        response
    };
    let payload = |v: &Value| -> Value { serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap() };
    assert_ne!(request(1, "doc_new", json!({}))["result"]["isError"], true);
    let response = request(
        2,
        "batch",
        json!({"verbose":true,"steps":[
            {"tool":"plane_offset","args":{"base":"XY","dist":5,"tilt_x":30,"tilt_y":20,"turn":15,"name":"tilted"}},
            {"tool":"sketch_create","args":{"plane":"XY","name":"wire"}},
            {"tool":"sketch_add","args":{"sketch":"wire","entities":[{"type":"rect","w":40,"h":30}]}},
            {"tool":"extrude","args":{"sketch":"wire","height":10,"name":"stock"}},
            {"tool":"chamfer","args":{"body":"stock","edges":edge().to_string(),"dist":2,"angle":30,"reference_face":"{\"facing\":\"+x\"}","flip":false,"name":"bevel"}}
        ]}),
    );
    assert_ne!(response["result"]["isError"], true, "{response}");
    let batch = payload(&response);
    assert!(batch["steps"][0]["result"]["plane"].is_number());
    assert_eq!(batch["steps"][4]["result"]["chamfer"]["mode"], "dist_angle");
    assert!((batch["bodies"][0]["volume_mm3"].as_f64().unwrap() - (12000.0 - 20.0 * 30_f64.to_radians().tan())).abs() < 1e-6);
    let refused = request(3, "dimension_set", json!({"target":{"feature":"bevel","key":"d2"},"value":181}));
    assert_eq!(refused["result"]["isError"], true);
    assert!(refused["error"].is_null());
    assert_eq!(payload(&request(4, "undo", json!({})))["undone"]["tool"], "batch");
    let empty = payload(&request(5, "doc_info", json!({})));
    assert_eq!(empty["bodies"], json!([]));
    assert_eq!(empty["sketches"], json!([]));
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
