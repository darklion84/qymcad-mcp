//! TR (review round of package T): single `undo` of each surface tool, a STEP export of a sheet, and what the timeline tools do to a
//! face copy that a later stitch consumed.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}
fn refused(r: &mut Registry, tool: &str, args: Value) -> String {
    r.call(tool, args).unwrap().unwrap_err()
}
fn info(r: &mut Registry) -> Value {
    call(r, "doc_info", json!({}))
}
/// (id, sheet, area) of every current body.
fn bodies(r: &mut Registry) -> Vec<(u64, bool, f64)> {
    info(r)["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (b["id"].as_u64().unwrap(), b["sheet"].as_bool().unwrap(), b["area_mm2"].as_f64().unwrap()))
        .collect()
}
fn sheets(r: &mut Registry) -> usize {
    bodies(r).iter().filter(|b| b.1).count()
}

/// The 60 x 40 x 12 block `block` with the copies of its top (`top`) and its front (`front`).
fn plate() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "base"}));
    call(&mut r, "sketch_add", json!({"sketch": "base", "entities": [{"type": "rect", "w": 60, "h": 40}]}));
    call(&mut r, "extrude", json!({"sketch": "base", "height": 12, "name": "block"}));
    call(&mut r, "face_copy", json!({"faces": {"facing": "+z"}, "name": "top"}));
    call(&mut r, "face_copy", json!({"faces": {"facing": "-y"}, "name": "front"}));
    r
}

/// A single `undo` (not inside a batch) takes back a `stitch`, a `surface_replace`, a `trim` and a `patch`: the consumed bodies are back,
/// the new body is gone, and a second undo goes on to the call before.
#[test]
fn a_single_undo_takes_back_each_surface_tool() {
    // stitch
    let mut r = plate();
    let before = bodies(&mut r);
    assert_eq!(sheets(&mut r), 2);
    call(&mut r, "stitch", json!({"parts": ["top", "front"], "name": "skin"}));
    assert_eq!((bodies(&mut r).len(), sheets(&mut r)), (2, 1), "block and the stitched sheet; both copies consumed");
    let undone = call(&mut r, "undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "stitch");
    assert_eq!(bodies(&mut r), before, "the two copies are back, the stitch gone");
    // surface_replace
    call(&mut r, "surface_replace", json!({"faces": {"facing": "+z"}, "surface": "top", "name": "swap"}));
    assert_eq!(bodies(&mut r).iter().filter(|b| !b.1).count(), 1, "one solid");
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "surface_replace");
    assert_eq!(bodies(&mut r), before, "block and both copies again");
    // trim
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "box"}));
    call(&mut r, "sketch_add", json!({"sketch": "box", "entities": [{"type": "rect", "cx": 30, "w": 60, "h": 200}]}));
    call(&mut r, "extrude", json!({"sketch": "box", "height": 30, "op": "new_body", "name": "cutter"}));
    let with_tool = bodies(&mut r);
    call(&mut r, "trim", json!({"body": "top", "tool": "cutter", "keep": [-15, 0, 12], "name": "left"}));
    assert_eq!(sheets(&mut r), 2, "front and the trimmed top");
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "trim");
    assert_eq!(bodies(&mut r), with_tool, "the whole top is back");
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "extrude", "the next undo is the call before");
    // patch
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "base"}));
    call(&mut r, "sketch_add", json!({"sketch": "base", "entities": [{"type": "rect", "w": 40, "h": 30}]}));
    call(&mut r, "extrude", json!({"sketch": "base", "height": 20, "name": "block"}));
    call(&mut r, "shell", json!({"open_faces": {"facing": "+z"}, "thickness": 2, "name": "cup"}));
    let before = bodies(&mut r);
    let t = call(&mut r, "topology", json!({"body": "cup"}));
    let outer: Vec<u64> = t["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["a"][2] == 20.0
                && e["b"][2] == 20.0
                && (e["a"][0].as_f64().unwrap().abs() - 20.0).abs() < 1e-6
                && (e["b"][0].as_f64().unwrap().abs() - 20.0).abs() < 1e-6
                || e["a"][2] == 20.0
                    && e["b"][2] == 20.0
                    && (e["a"][1].as_f64().unwrap().abs() - 15.0).abs() < 1e-6
                    && (e["b"][1].as_f64().unwrap().abs() - 15.0).abs() < 1e-6
        })
        .map(|e| e["id"].as_u64().unwrap())
        .collect();
    assert_eq!(outer.len(), 4, "the outer outline");
    call(&mut r, "patch", json!({"edges": outer, "name": "lid"}));
    assert_eq!(sheets(&mut r), 1);
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "patch");
    assert_eq!(bodies(&mut r), before);
}

/// What the timeline tools do to a face copy that a later stitch consumed (the app's own behaviour, recorded; nothing is refused or
/// written wrong): a suppress cascades the stitch to unbuilt and says so, a delete without `cascade` is refused naming the stitch, a
/// cascading delete removes both and frees the other copy, and each is one undo. At no point is a rebuild error left behind.
#[test]
fn suppress_and_delete_of_a_copy_that_a_stitch_consumed() {
    let mut r = plate();
    call(&mut r, "stitch", json!({"parts": ["top", "front"], "name": "skin"}));
    let stitched = bodies(&mut r);
    assert_eq!(stitched.len(), 2, "block and skin");
    // Suppress: the stitch has no input and goes unbuilt with it, reported; the document has no errors. (The other copy, `front`, is
    // still consumed by the unbuilt node in the core's reading and does not show; undo brings everything back.)
    let out = call(&mut r, "feature_suppress", json!({"feature": "top", "suppressed": true}));
    assert_eq!(out["rebuild"]["errors"], json!([]));
    assert_eq!(out["unbuilt"].as_array().unwrap().len(), 2, "the copy and the stitch: {out}");
    assert_eq!(bodies(&mut r).iter().map(|b| b.1).collect::<Vec<_>>(), vec![false], "only the block is built");
    assert_eq!(info(&mut r)["errors"], json!([]));
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "feature_suppress");
    assert_eq!(bodies(&mut r), stitched);
    // Delete: refused while the stitch depends on it, document unchanged.
    let e = refused(&mut r, "feature_delete", json!({"feature": "top"}));
    assert!(e.contains("has dependents") && e.contains("skin") && e.contains("cascade"), "{e}");
    assert_eq!(bodies(&mut r), stitched);
    // Cascade: both go, the other copy is a body again with its own area.
    let out = call(&mut r, "feature_delete", json!({"feature": "top", "cascade": true}));
    assert_eq!(out["deleted"].as_array().unwrap().len(), 2);
    assert_eq!(bodies(&mut r), vec![(21, false, 7200.0), (23, true, 720.0)]);
    assert_eq!(info(&mut r)["errors"], json!([]));
    assert_eq!(call(&mut r, "undo", json!({}))["undone"]["tool"], "feature_delete");
    assert_eq!(bodies(&mut r), stitched);
}

/// STEP export of a sheet writes its faces as an open shell, not a solid (ADR 0059: "STEP writes the faces"), and says so in `note`.
#[test]
fn a_sheet_exports_to_step_as_an_open_shell() {
    let mut r = plate();
    call(&mut r, "stitch", json!({"parts": ["top", "front"], "name": "skin"}));
    let path = std::env::temp_dir().join(format!("tr-sheet-{}.step", std::process::id()));
    let out = call(&mut r, "export", json!({"path": path.to_str().unwrap(), "format": "step", "bodies": ["skin"]}));
    let body = &out["bodies"][0];
    assert_eq!((body["sheet"].clone(), body["area_mm2"].as_f64().unwrap().round()), (json!(true), 3120.));
    assert!(body["note"].as_str().unwrap().contains("not a printable solid") && body.get("mesh_volume_mm3").is_none(), "{out}");
    let step = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(step.matches("ADVANCED_FACE").count(), 2, "the two faces of the stitched sheet");
    assert_eq!((step.matches("OPEN_SHELL").count(), step.matches("MANIFOLD_SOLID_BREP").count()), (1, 0), "an open shell, no solid");
}
