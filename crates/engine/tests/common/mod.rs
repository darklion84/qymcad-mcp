//! Shared helpers for engine integration tests.
#![allow(dead_code)]

use qymcad_core::model::Id;
use std::collections::HashMap;
use std::path::PathBuf;

/// A fresh scratch path under the system temp dir (no extra dependency for tempfiles).
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("qymcad-mcp-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

pub fn assert_close(actual: f64, expected: f64, tol: f64, what: &str) {
    assert!((actual - expected).abs() <= tol, "{what}: got {actual}, expected {expected} ± {tol}");
}

/// Reproduce what QymCAD.app (v0.1.1-dev.20261026) does when a person opens `path`, edits parameter `name` in
/// the Parameters window and the model rebuilds:
/// `finish_project_load` (`settle_params_seen`, faces back into `regen_faces`, io_jobs.rs:109-150) ->
/// `apply_param_edit` (solve the sketches that read names, params.rs:375-398) -> `rebuild_if_dirty`
/// (`mark_changed_params_dirty`, ui-state lib.rs:426-435, rebuild.rs:390-394). Returns the volume of the final body.
pub fn gui_edit_param(path: &std::path::Path, name: &str, expr: &str) -> f64 {
    gui_edit_param_body(path, name, expr).0
}

/// `gui_edit_param`, returning the final body's volume and bounding box (for edits that move or rotate geometry
/// without changing the volume).
pub fn gui_edit_param_body(path: &std::path::Path, name: &str, expr: &str) -> (f64, [f64; 6]) {
    let (volume, bbox, _) = gui_edit_param_faces(path, name, expr);
    (volume, bbox)
}

/// Native GUI rebuild geometry, without any engine propagation or repair.
pub fn gui_edit_param_faces(path: &std::path::Path, name: &str, expr: &str) -> (f64, [f64; 6], Vec<qymcad_core::geom::MeshFace>) {
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.ensure_document();
    // finish_project_load: faces stored in the bodies go back into `regen_faces` (edges are not restored).
    for i in 0..project.bodies.len() {
        if let (Some(body), false) = (project.mesh_id(i), project.bodies[i].faces.is_empty()) {
            let faces = project.bodies[i].faces.clone();
            project.regen_faces.insert(body, faces);
        }
    }
    let shapes = restore_shapes(breps);
    let seen = project.param_map();
    project.parameters.iter_mut().find(|q| q.name == name).expect("parameter").expr = expr.into();
    // apply_param_edit: solve, without marking, the sketches that read names; the scheduler decides what rebuilds.
    project.eval_parameters();
    for si in 0..project.sketches.len() {
        if project.sketch_reads_names(si) {
            project.solve_sketch(si);
        }
    }
    // mark_changed_params_dirty: names whose value changed, and names that are gone.
    let vars = project.param_map();
    let mut changed: Vec<String> =
        vars.iter().filter(|(k, v)| seen.get(*k).is_none_or(|o| (o - **v).abs() > 1e-12)).map(|(k, _)| k.clone()).collect();
    changed.extend(seen.keys().filter(|k| !vars.contains_key(*k)).cloned());
    for n in &changed {
        project.mark_param_dependents_dirty_for(n);
    }
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, shapes);
    assert!(report.errors.is_empty(), "GUI-path rebuild errors: {:?}", report.errors);
    let consumed = project.consumed_bodies();
    let last = project.timeline.iter().flat_map(|n| n.kind.bodies()).rfind(|b| !consumed.contains(b)).expect("a body");
    (shapes[&last].volume(), shapes[&last].bbox().unwrap_or([0.0; 6]), project.regen_faces[&last].clone())
}

/// Check an open rectangular top pocket, including placement that volume/bbox alone cannot establish.
#[allow(clippy::too_many_arguments)]
pub fn assert_top_pocket(
    s: &mut qymcad_engine::Session,
    w: f64,
    l: f64,
    t: f64,
    pw: f64,
    pl: f64,
    depth: f64,
    holes_area: f64,
    face_count: usize,
) {
    let topo = s.topology(None, false).unwrap();
    let faces: Vec<_> = topo.faces.iter().map(|f| (f.centroid[2], f.normal.unwrap_or([0.0; 3]), f.area)).collect();
    assert_pocket_faces(&faces, w, l, t, pw, pl, depth, holes_area, face_count);
}

#[allow(clippy::too_many_arguments)]
pub fn assert_pocket_faces(
    faces: &[(f64, [f64; 3], f64)],
    w: f64,
    l: f64,
    t: f64,
    pw: f64,
    pl: f64,
    depth: f64,
    holes_area: f64,
    face_count: usize,
) {
    let up: Vec<_> = faces.iter().filter(|(_, n, _)| n[2] > 1.0 - 1e-9).collect();
    let floor = up.iter().find(|(_, _, area)| (*area - pw * pl).abs() < 1e-3).expect("rectangular pocket floor");
    assert_close(floor.0, t - depth, 1e-6, "pocket floor z = thickness - depth");
    assert_eq!(faces.len(), face_count, "an open pocket has four walls and a floor, no ceiling");
    let top = up.iter().find(|(z, _, _)| (*z - t).abs() < 1e-6).expect("top face at stock thickness");
    // The plate's 1.42 mm³ mesh-volume error at t=6 implies ≈ 1.42/6 = 0.237 mm² hole-area error (F-021).
    // Inscribed hole polygons only inflate the top area; allow 2 mm² above analytic area and roundoff below.
    let expected = w * l - holes_area - pw * pl;
    let roundoff = 1e-6;
    assert!(
        top.2 >= expected - roundoff && top.2 <= expected + 2.0,
        "top face area excludes the pocket opening: got {}, expected {expected} - {roundoff} .. + 2 mm²",
        top.2
    );
}

/// Restore saved test geometry without holding the non-reentrant kernel gate.
pub fn restore_shapes(breps: Vec<(Id, Vec<u8>)>) -> HashMap<Id, qymcad_kernel::Shape> {
    qymcad_kernel::imports::restore(breps.into_iter().map(|(body, bytes)| qymcad_kernel::imports::SavedBody { body, bytes }).collect())
        .into_iter()
        .map(|read| (read.body, read.shape))
        .collect()
}

#[path = "../../src/test_support.rs"]
/// Exact native fixtures shared by engine integration tests.
pub mod native;

/// Emulate QymCAD 0.1.1 `Act::SetDriver` (params.rs:260–286), then `apply_param_edit`
/// and changed-name rebuild scheduling. A literal driver explicitly solves and dirties its own sketch.
pub fn gui_edit_driver(path: &std::path::Path, name: &str, text: &str) -> f64 {
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.ensure_document();
    for i in 0..project.bodies.len() {
        if let (Some(body), false) = (project.mesh_id(i), project.bodies[i].faces.is_empty()) {
            project.regen_faces.insert(body, project.bodies[i].faces.clone());
        }
    }
    let shapes = restore_shapes(breps);
    let seen = project.param_map();
    let target = project.named_dims.iter().find(|d| d.name == name).expect("named dimension").target.clone();
    project.check_dim_target_text(&target, text).unwrap();
    assert!(project.set_dim_target_text(&target, text).unwrap());
    if let qymcad_core::model::DimTarget::Sketch { sketch, .. } = target {
        let si = project.sketch_index(sketch).unwrap();
        assert!(project.solve_sketch(si) <= 1e-6);
        project.mark_sketch_dirty(sketch);
    }
    project.eval_parameters();
    for si in 0..project.sketches.len() {
        if project.sketch_reads_names(si) {
            assert!(project.solve_sketch(si) <= 1e-6);
        }
    }
    let vars = project.param_map();
    let mut changed: Vec<String> =
        vars.iter().filter(|(k, v)| seen.get(*k).is_none_or(|o| (o - **v).abs() > 1e-12)).map(|(k, _)| k.clone()).collect();
    changed.extend(seen.keys().filter(|k| !vars.contains_key(*k)).cloned());
    for name in changed {
        project.mark_param_dependents_dirty_for(&name);
    }
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, shapes);
    assert!(report.errors.is_empty(), "GUI SetDriver rebuild errors: {:?}", report.errors);
    let consumed = project.consumed_bodies();
    let last = project.timeline.iter().flat_map(|n| n.kind.bodies()).rfind(|b| !consumed.contains(b)).expect("a body");
    shapes[&last].volume()
}
