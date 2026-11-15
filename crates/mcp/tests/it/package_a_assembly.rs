//! Package A wire contracts, whole-document batch rollback, and typed assembly references.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

fn saved_breps(r: &mut Registry, path: &Path) -> BTreeMap<String, Vec<u8>> {
    call(r, "doc_save", json!({"path":path}));
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..archive.len())
        .filter_map(|i| {
            let mut entry = archive.by_index(i).unwrap();
            if !entry.name().starts_with("brep/") {
                return None;
            }
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            Some((name, bytes))
        })
        .collect()
}

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let result = r.call(tool, args).unwrap_or_else(|e| panic!("{tool}: {e}")).unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(result[0]["text"].as_str().unwrap()).unwrap()
}
fn request(id: usize, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn wire(name: &str, args: Value) -> Value {
    json!({"name":name,"arguments":args})
}
fn stdio(messages: Vec<Value>) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_qymcad-mcp"))
        .env("QYMCAD_MCP_LOG", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        for (id, params) in messages.into_iter().enumerate() {
            let (method, params) = if params == json!("list") { ("tools/list", json!({})) } else { ("tools/call", params) };
            writeln!(input, "{}", request(id, method, params)).unwrap();
        }
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

#[test]
fn a_stdio_discovery_strict_assembly_schemas_and_typed_refs() {
    let responses = stdio(vec![
        json!("list"),
        wire("doc_new", json!({})),
        wire("part_new", json!({"name":"drawer","typo":1})),
        wire("part_new", json!({"name":"drawer"})),
        wire("part_select", json!({"part":{"joint":"drawer"}})),
        wire("part_select", json!({"part":{"root":true,"extra":1}})),
        wire("part_place", json!({"part":"drawer","translation_mm":[0,0,1],"rotation":{"axis":[0,0,1],"angle_deg":90,"extra":1}})),
        wire(
            "joint_create",
            json!({"kind":"slider","a":{"body":3,"face":1,"extra":1},"b":{"body":4,"face":1},"mode":"as_built","axis":{"base_plane":"XZ"}}),
        ),
        wire("assembly_info", json!({"extra":true})),
        wire("part_new", json!({"name":"drawer"})),
        wire("part_select", json!({"part":"drawer"})),
        wire("rename", json!({"target":{"part":"drawer"},"name":"renamed"})),
        wire("joint_set", json!({"joint":{"part":"drawer"},"offset":0})),
        wire("joint_create", json!({"kind":"revolute","a":{"body":3,"face":1},"b":{"body":4,"face":1},"mode":"align"})),
    ]);
    let tools = responses[0]["result"]["tools"].as_array().unwrap();
    for name in ["part_new", "part_select", "part_place", "ground", "joint_create", "joint_set", "joint_delete", "assembly_info"] {
        let t = tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("missing assembly tool {name}"));
        assert_eq!(t["inputSchema"]["additionalProperties"], false, "{name}");
        for field in t["inputSchema"]["properties"].as_object().into_iter().flat_map(|properties| properties.values()) {
            assert!(field["description"].is_string(), "{name}: {field}");
        }
    }
    assert_ne!(responses[3]["result"]["isError"], true, "part creation: {}", responses[3]);
    for i in [2, 4, 5, 6, 7, 8, 10, 11, 12, 13] {
        assert_eq!(responses[i]["result"]["isError"], true, "request {i}: {}", responses[i]);
    }
    assert!(responses[10]["result"]["content"][0]["text"].as_str().unwrap().contains("several parts are named 'drawer'"));
}

fn plate_steps(name: &str) -> Vec<Value> {
    vec![
        json!({"tool":"sketch_create","args":{"plane":"XY","name":format!("{name}-sk")}}),
        json!({"tool":"sketch_add","args":{"sketch":format!("{name}-sk"),"entities":[{"type":"rect","w":500,"h":600}]}}),
        json!({"tool":"extrude","args":{"sketch":format!("{name}-sk"),"height":150,"op":"new_body","name":name}}),
    ]
}
fn fixture(r: &mut Registry) {
    call(r, "doc_new", json!({}));
    call(r, "param_set", json!({"name":"opening","value":300}));
    call(r, "part_new", json!({"name":"carcass"}));
    call(r, "batch", json!({"steps":plate_steps("fixed")}));
    call(r, "part_new", json!({"name":"drawer"}));
    call(r, "batch", json!({"steps":plate_steps("moving")}));
}
fn mechanism_steps() -> Vec<Value> {
    vec![
        json!({"tool":"part_new","args":{"name":"extra"}}),
        json!({"tool":"part_select","args":{"part":"carcass"}}),
        json!({"tool":"part_place","args":{"part":"drawer","translation_mm":[0,0,250]}}),
        json!({"tool":"ground","args":{"part":"carcass","grounded":true}}),
        json!({"tool":"joint_create","args":{"name":"slide","kind":"slider","a":{"body":"fixed","face":{"facing":"+x"}},"b":{"body":"moving","face":{"facing":"-x"}},"mode":"as_built","axis":{"base_plane":"XZ"},"offset":0}}),
        json!({"tool":"joint_set","args":{"joint":"slide","offset":"opening"}}),
    ]
}

#[test]
fn a_batch_restores_all_assembly_state_and_success_is_one_undo() {
    let mut r = Registry::new();
    fixture(&mut r);
    let path = std::env::temp_dir().join(format!("a-mcp-batch-{}.qcad", std::process::id()));
    let breps = saved_breps(&mut r, &path);
    assert_eq!(breps.len(), 2, "both Parts retain native B-reps");
    let before = call(&mut r, "assembly_info", json!({}));
    let before_doc = call(&mut r, "doc_info", json!({}));
    let mut failed = mechanism_steps();
    failed.push(json!({"tool":"joint_set","args":{"joint":"missing","offset":0}}));
    assert!(r.call("batch", json!({"steps":failed})).unwrap().unwrap_err().contains("step 7"));
    assert_eq!(call(&mut r, "assembly_info", json!({})), before, "placements, active part, ground, connectors, joints restored");
    assert_eq!(call(&mut r, "doc_info", json!({})), before_doc, "geometry/diagnostics restored");
    assert_eq!(saved_breps(&mut r, &path), breps, "failure retains exact local B-reps");
    let batch = call(&mut r, "batch", json!({"steps":mechanism_steps()}));
    assert_eq!(batch["steps"][0]["created"]["name"], "extra");
    assert!(batch["steps"][0]["created"]["id"].is_u64());
    assert_eq!(batch["steps"][4]["created"]["name"], "slide");
    assert!(batch["steps"][4]["created"]["id"].is_u64());
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(call(&mut r, "assembly_info", json!({})), before);
    assert_eq!(call(&mut r, "doc_info", json!({})), before_doc);
    assert_eq!(saved_breps(&mut r, &path), breps, "one undo restores exact local B-reps");
    std::fs::remove_file(&path).unwrap();
    let _ = std::fs::remove_file(path.with_extension("qcad.bak"));
}

#[test]
fn a_part_selection_noop_history_and_typed_rename_labels() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    let p = call(&mut r, "part_new", json!({"name":"drawer"}))["part"]["id"].clone();
    assert_eq!(call(&mut r, "part_select", json!({"part":p}))["changed"], false);
    let undo = call(&mut r, "undo", json!({}));
    assert_eq!(undo["undone"]["tool"], "part_new");
    assert_eq!(undo["undone"]["label"], "drawer");
    let p = call(&mut r, "part_new", json!({"name":"drawer"}))["part"]["id"].clone();
    call(&mut r, "part_select", json!({"part":{"root":true}}));
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "part_select");
    let renamed = call(&mut r, "rename", json!({"target":{"part":p},"name":"drawer renamed"}));
    assert_eq!(renamed["target"]["kind"], "part");
    call(&mut r, "part_select", json!({"part":"drawer renamed"}));
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "rename");
}

fn payload(response: &Value) -> Value {
    assert!(response["error"].is_null(), "registered tool: {response}");
    assert_ne!(response["result"]["isError"], true, "successful tool: {response}");
    serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn a_stdio_mechanism_compact_batch_typed_joint_rename_and_undo() {
    let mut build =
        vec![json!({"tool":"param_set","args":{"name":"opening","value":300}}), json!({"tool":"part_new","args":{"name":"carcass"}})];
    build.extend(plate_steps("fixed"));
    build.push(json!({"tool":"part_new","args":{"name":"drawer"}}));
    build.extend(plate_steps("moving"));
    let responses = stdio(vec![
        wire("doc_new", json!({})),
        wire("batch", json!({"steps":build})),
        wire("assembly_info", json!({})),
        wire("batch", json!({"steps":mechanism_steps()})),
        wire("rename", json!({"target":{"joint":"slide"},"name":"moving"})),
        wire("joint_set", json!({"joint":"moving","offset":0})),
        wire("joint_set", json!({"joint":"moving","offset":0,"release":"offset"})),
        wire("joint_set", json!({"joint":"moving","release":"angle"})),
        wire("joint_set", json!({"joint":"moving","extra":true,"offset":0})),
        wire("undo", json!({})),
        wire("undo", json!({})),
        wire("undo", json!({})),
        wire("assembly_info", json!({})),
    ]);
    let initial = payload(&responses[2]);
    let mechanism = payload(&responses[3]);
    assert_eq!(mechanism["steps"][0]["created"]["name"], "extra");
    assert_eq!(mechanism["steps"][4]["created"]["name"], "slide");
    assert!(mechanism["steps"][4]["created"]["id"].is_u64());
    assert_eq!(payload(&responses[4])["target"]["kind"], "joint");
    payload(&responses[5]); // Body and joint share "moving": typed joint lookup remains unambiguous.
    for i in [6, 7, 8] {
        assert_eq!(responses[i]["result"]["isError"], true, "{i}: {}", responses[i]);
    }
    assert_eq!(payload(&responses[9])["undone"]["tool"], "joint_set");
    assert_eq!(payload(&responses[10])["undone"]["tool"], "rename");
    assert_eq!(payload(&responses[11])["undone"]["tool"], "batch");
    assert_eq!(payload(&responses[12]), initial);
}
