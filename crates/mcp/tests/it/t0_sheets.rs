//! Sheet bodies through the tools (T0): reports, default bodies, export, thicken both ways, refusals.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}
fn close(actual: f64, expected: f64, what: &str) {
    assert!((actual - expected).abs() < 1e-3, "{what}: got {actual}, expected {expected}");
}

/// A 40×40×10 block named `block`, then a 20×16 tube sheet 10 high named `tube` (a surface loft, new body).
fn solid_then_sheet() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "base"}));
    call(&mut r, "sketch_add", json!({"sketch": "base", "entities": [{"type": "rect", "w": 40, "h": 40}]}));
    call(&mut r, "extrude", json!({"sketch": "base", "height": 10, "name": "block"}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "low"}));
    call(&mut r, "sketch_add", json!({"sketch": "low", "entities": [{"type": "rect", "w": 20, "h": 16}]}));
    let plane = call(&mut r, "plane_offset", json!({"base": "XY", "dist": 10}));
    call(&mut r, "sketch_create", json!({"plane": {"plane": plane["plane"]}, "name": "high"}));
    call(&mut r, "sketch_add", json!({"sketch": "high", "entities": [{"type": "rect", "w": 20, "h": 16}]}));
    call(
        &mut r,
        "loft",
        json!({"sections": [{"sketch": "low"}, {"sketch": "high"}], "ruled": true, "surface": true, "op": "new_body", "name": "tube"}),
    );
    r
}

fn body<'a>(info: &'a Value, volume_zero: bool) -> &'a Value {
    info["bodies"].as_array().unwrap().iter().find(|b| (b["volume_mm3"].as_f64().unwrap() == 0.0) == volume_zero).unwrap()
}

// ---- (a) reports ----

#[test]
fn doc_info_and_the_loft_result_say_which_body_is_a_sheet_and_its_area() {
    let mut r = solid_then_sheet();
    let info = call(&mut r, "doc_info", json!({}));
    let (sheet, solid) = (body(&info, true), body(&info, false));
    assert_eq!(sheet["sheet"], true);
    close(sheet["area_mm2"].as_f64().unwrap(), 720., "tube area");
    assert_eq!(solid["sheet"], false);
    close(solid["area_mm2"].as_f64().unwrap(), 2. * 1600. + 4. * 400., "block area");
    // The rebuild entry of a tool call is the same record.
    let moved = call(&mut r, "move", json!({"body": "tube", "dz": 1}));
    let entry = body(&moved["rebuild"], true);
    assert_eq!(entry["sheet"], true);
    close(entry["area_mm2"].as_f64().unwrap(), 720., "moved tube area");
}

#[test]
fn export_names_a_sheet_and_leaves_out_its_mesh_volume() {
    let mut r = solid_then_sheet();
    let path = std::env::temp_dir().join(format!("t0-sheet-{}.stl", std::process::id()));
    let out = call(&mut r, "export", json!({"path": path.to_str().unwrap(), "format": "stl"}));
    let bodies = out["bodies"].as_array().unwrap();
    let sheet = bodies.iter().find(|b| b["sheet"] == true).expect("the sheet");
    assert!(sheet.get("mesh_volume_mm3").is_none(), "{sheet}");
    assert!(sheet["note"].as_str().unwrap().contains("open surface"), "{sheet}");
    close(sheet["area_mm2"].as_f64().unwrap(), 720., "exported area");
    let solid = bodies.iter().find(|b| b["sheet"] == false).expect("the solid");
    assert!(solid["mesh_volume_mm3"].as_f64().unwrap() > 15000.);
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_assembly_metrics_of_a_part_carry_the_sheet_flag_and_area() {
    let mut r = solid_then_sheet();
    let info = call(&mut r, "doc_info", json!({}));
    let part = info["components"].as_array().unwrap().iter().find(|c| c["kind"] == "part").unwrap()["name"].as_str().unwrap().to_string();
    let placed = call(&mut r, "part_place", json!({"part": part, "translation_mm": [0, 0, 5]}));
    let metrics = placed["local_metrics"].as_array().unwrap();
    let sheet = metrics.iter().find(|m| m["sheet"] == true).unwrap_or_else(|| panic!("{placed}"));
    close(sheet["area_mm2"].as_f64().unwrap(), 720., "tube area");
    assert!(metrics.iter().any(|m| m["sheet"] == false));
}

