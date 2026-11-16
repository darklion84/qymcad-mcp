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

fn body(info: &Value, volume_zero: bool) -> &Value {
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

// ---- (b) the false warning ----

#[test]
fn a_sheet_in_doc_info_and_in_a_reopened_file_raises_no_sealed_void_warning() {
    let mut r = solid_then_sheet();
    let sealed = |v: &Value| v["warnings"].as_array().unwrap().iter().any(|w| w.to_string().contains("sealed"));
    assert!(!sealed(&call(&mut r, "doc_info", json!({}))));
    let path = std::env::temp_dir().join(format!("t0-sheet-warn-{}.qcad", std::process::id()));
    call(&mut r, "doc_save", json!({"path": path.to_str().unwrap()}));
    let opened = call(&mut r, "doc_open", json!({"path": path.to_str().unwrap()}));
    assert!(!opened.to_string().contains("sealed"), "{opened}");
    let _ = std::fs::remove_file(path);
}

// ---- (c) defaults skip sheets ----

#[test]
fn extrude_without_a_target_lands_on_the_solid_and_sketch_project_reads_the_solid() {
    let mut r = solid_then_sheet();
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "boss"}));
    call(&mut r, "sketch_add", json!({"sketch": "boss", "entities": [{"type": "rect", "w": 10, "h": 10}]}));
    call(&mut r, "sketch_create", json!({"plane": "XY", "name": "proj"}));
    let projected = call(&mut r, "sketch_project", json!({"sketch": "proj", "edges": {"edges_of": {"facing": "+z"}}}));
    assert_eq!(projected["projections"].as_array().unwrap().len(), 4, "the four top edges of the block, not the sheet: {projected}");
    let boss = call(&mut r, "extrude", json!({"sketch": "boss", "height": 15, "op": "add"}));
    assert!(boss["rebuild"].get("errors").is_none(), "{boss}");
    let info = call(&mut r, "doc_info", json!({}));
    assert_eq!(info["bodies"].as_array().unwrap().len(), 2, "the sheet stays beside the solid");
    close(body(&info, false)["volume_mm3"].as_f64().unwrap(), 16000. + 10. * 10. * 5., "boss on the block");
}

// ---- (d) refusals ----

fn refused(r: &mut Registry, tool: &str, args: Value) -> String {
    r.call(tool, args).unwrap().unwrap_err()
}

#[test]
fn shell_and_a_chamfer_with_boundary_edges_are_refused_on_a_sheet() {
    let mut r = solid_then_sheet();
    let e = refused(&mut r, "shell", json!({"body": "tube", "open_faces": {"facing": "+x"}, "thickness": 1}));
    assert!(e.contains("is a sheet") && e.contains("F-196"), "{e}");
    let e = refused(&mut r, "chamfer", json!({"body": "tube", "edges": {"kind": "line"}, "dist": 1}));
    assert!(e.contains("is a sheet") && e.contains("F-197"), "{e}");
}

// ---- (e) thicken of a sheet ----

#[test]
fn thicken_of_a_sheet_takes_either_sign_and_a_solid_stays_positive() {
    let mut r = solid_then_sheet();
    for (thickness, volume) in [(-1.0, 680.0), (1.0, 760.0 - 10.0 * (4.0 - std::f64::consts::PI))] {
        let mut r = solid_then_sheet();
        let out = call(&mut r, "thicken", json!({"body": "tube", "face": {"facing": "+x"}, "thickness": thickness}));
        assert!(out["rebuild"].get("errors").is_none(), "{out}");
        let plate = out["rebuild"]["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["sheet"] == false && b["volume_mm3"].as_f64().unwrap() < 2000.)
            .unwrap()
            .clone();
        close(plate["volume_mm3"].as_f64().unwrap(), volume, "plate");
    }
    // The solid keeps F-150.
    let e = refused(&mut r, "thicken", json!({"body": "block", "face": {"facing": "+z"}, "thickness": -1}));
    assert!(e.contains("F-150"), "{e}");
}
