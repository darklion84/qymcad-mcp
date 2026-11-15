//! P14 over the registry: `split_face` arguments are strict, the tool is batchable and undoable, refusals are tool errors.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}

fn fail(r: &mut Registry, tool: &str, args: Value) -> String {
    r.call(tool, args).unwrap().unwrap_err()
}

fn timeline(r: &mut Registry) -> Vec<String> {
    call(r, "doc_info", json!({}))["timeline"].as_array().unwrap().iter().map(|n| n["kind"].as_str().unwrap().to_string()).collect()
}

/// The block of the app's acceptance test: 40 x 30 x 10, x -20..20, y -15..15, z 0..10.
fn block() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "primitive", json!({"kind":"box","dx":40,"dy":30,"dz":10,"name":"block"}));
    r
}

#[test]
fn arguments_are_strict_and_nothing_is_created_by_a_bad_call() {
    let mut r = block();
    for (bad, word) in [
        (json!({}), "plane"),
        (json!({"plane":"AB"}), "plane must be one of"),
        (json!({"plane":"XY","faces":[1]}), "faces"),
        (json!({"plane":"XY","piece_names":["a","b"]}), "piece_names"),
        (json!({"plane":"XY","offset":"x","typo":1}), "typo"),
        (json!({"plane":{"plane":1,"face":2}}), "plane must be one of"),
    ] {
        let e = fail(&mut r, "split_face", bad.clone());
        assert!(e.contains("bad arguments for `split_face`") && e.contains(word), "{bad}: {e}");
    }
    let e = fail(&mut r, "split_face", json!({"plane":"XY","offset":5,"name":"7"}));
    assert!(e.contains("interpreted as an id"), "{e}");
    assert_eq!(timeline(&mut r), ["box3"], "nothing above created anything");
}

#[test]
fn the_reply_names_the_node_the_faces_and_the_new_ids_and_the_action_is_one_undo_step() {
    let mut r = block();
    let a = call(&mut r, "split_face", json!({"plane":"XY","offset":5,"name":"mid"}));
    assert_eq!(a["feature"], a["body"], "the node is the body");
    assert_eq!((a["name"].as_str(), a["offset"].as_f64()), (Some("mid"), Some(5.0)));
    assert_eq!(a["faces"], json!({"before":6,"after":10}));
    assert_eq!(a["added_faces"].as_array().unwrap().len(), 4);
    assert!(a["rebuild"].get("errors").is_none(), "{a}");
    assert_eq!(timeline(&mut r), ["box3", "splitface"]);
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(info["bodies"].as_array().unwrap().len(), 1);
    assert!((info["bodies"][0]["volume_mm3"].as_f64().unwrap() - 12000.0).abs() < 1e-6);
    // The new ids are real faces of the body: topology lists all ten, each added id once.
    let topology = call(&mut r, "topology", json!({"body": a["body"]}));
    let listed: Vec<u64> = topology["faces"].as_array().unwrap().iter().map(|f| f["id"].as_u64().unwrap()).collect();
    assert_eq!(listed.len(), 10);
    for id in a["added_faces"].as_array().unwrap() {
        assert_eq!(listed.iter().filter(|l| Some(**l) == id.as_u64()).count(), 1);
    }
    // The offset is editable by dimension_set on the key `offset`.
    call(&mut r, "dimension_set", json!({"target":{"feature":a["feature"],"key":"offset"},"value":3}));
    let undone = call(&mut r, "undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "dimension_set");
    let undone = call(&mut r, "undo", json!({}));
    assert_eq!((undone["undone"]["tool"].as_str(), undone["undone"]["label"].as_str()), (Some("split_face"), Some("mid")), "{undone}");
    assert_eq!(timeline(&mut r), ["box3"], "one step took the whole split");
}

#[test]
fn a_plane_from_a_datum_or_a_face_and_a_json_string_plane() {
    let mut r = block();
    call(&mut r, "plane_offset", json!({"base":"XY","dist":4,"name":"cutter"}));
    let a = call(&mut r, "split_face", json!({"plane":{"plane":"cutter"}}));
    assert_eq!(a["faces"]["after"], 10);
    let b = call(&mut r, "split_face", json!({"plane":"{\"plane\":\"cutter\"}","offset":2}));
    assert_eq!(b["faces"], json!({"before":10,"after":14}));
    let mut r = block();
    let c = call(&mut r, "split_face", json!({"plane":{"body":"block","face":{"facing":"+z"}},"offset":-5}));
    assert_eq!(c["faces"], json!({"before":6,"after":10}), "the app's acceptance case: the top face, offset -5");
}

#[test]
fn refusals_are_tool_errors_that_say_what_to_do_and_leave_the_document() {
    let mut r = block();
    let before = call(&mut r, "doc_info", json!({}));
    for (args, word) in [
        (json!({"plane":"XY","offset":50}), "splits nothing"),
        (json!({"plane":"XY","offset":10}), "splits nothing"),
        (json!({"plane":"XY","offset":9.999999}), "divided no face"),
    ] {
        let e = fail(&mut r, "split_face", args.clone());
        assert!(e.contains(word), "{args}: {e}");
    }
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
    // A second split below the first: duplicate face ids (F-189).
    call(&mut r, "split_face", json!({"plane":"XY","offset":6}));
    let before = call(&mut r, "doc_info", json!({}));
    let e = fail(&mut r, "split_face", json!({"plane":"XY","offset":3}));
    assert!(e.contains("F-189") && e.contains("distinct ids"), "{e}");
    assert_eq!(call(&mut r, "doc_info", json!({})), before);
}

#[test]
fn a_batch_with_a_refused_second_step_changes_nothing_and_a_good_one_is_one_undo_step() {
    let mut r = block();
    let steps = |second: Value| json!({"steps":[{"tool":"split_face","args":{"plane":"XY","offset":6,"name":"upper"}},{"tool":"split_face","args":second}]});
    let e = fail(&mut r, "batch", steps(json!({"plane":"XY","offset":3})));
    assert!(e.contains("step 2") && e.contains("F-189"), "{e}");
    assert_eq!(timeline(&mut r), ["box3"], "the first step was rolled back");
    let ok = call(&mut r, "batch", steps(json!({"plane":"XY","offset":8,"name":"higher"})));
    assert_eq!(ok["steps"][0]["created"]["name"], "upper");
    assert_eq!(ok["steps"][1]["created"]["name"], "higher");
    assert_eq!(timeline(&mut r), ["box3", "splitface", "splitface"]);
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(timeline(&mut r), ["box3"]);
}

#[test]
fn the_description_says_what_the_tool_does_and_what_it_does_not() {
    let r = Registry::new();
    let listed = r.list();
    let tool = listed.iter().find(|t| t["name"] == "split_face").expect("split_face is registered");
    let description = tool["description"].as_str().unwrap();
    for phrase in
        ["ONE piece", "EVERY face", "not `split`", "local frame", "added_faces", "WHOLE face", "whole edge chain", "recreate the split"]
    {
        assert!(description.contains(phrase), "the description lacks `{phrase}`");
    }
}

#[test]
fn a_batch_error_does_not_name_a_node_the_batch_made_and_rolled_back() {
    let mut r = block();
    call(&mut r, "param_set", json!({"name":"cut","value":5}));
    let e = fail(
        &mut r,
        "batch",
        json!({"steps":[{"tool":"split_face","args":{"plane":"XY","offset":"cut","name":"a"}},{"tool":"param_set","args":{"name":"cut","value":50}}]}),
    );
    assert!(e.contains("step 2") && e.contains("a: operation failed"), "{e}");
    assert!(!e.contains("Undo this edit") && e.contains("not applied"), "{e}");
}
