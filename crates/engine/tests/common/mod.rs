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

/// Reproduce what QymCAD.app (v0.1.0-dev.20261001) does when a person opens `path`, edits parameter `name` in
/// the Parameters window and the model rebuilds:
/// `spawn_project_load` -> `finish_project_load` (`settle_params_seen`, faces back into `regen_faces`) -> `apply_param_edit` -> `spawn_regen`
/// (`mark_changed_params_dirty`). Returns the volume of the final body. See FINDINGS F-001.
pub fn gui_edit_param(path: &std::path::Path, name: &str, expr: &str) -> f64 {
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.ensure_document();
    // finish_project_load: faces stored in the bodies go back into `regen_faces` (edges are not restored).
    for i in 0..project.bodies.len() {
        if let (Some(body), false) = (project.mesh_id(i), project.bodies[i].faces.is_empty()) {
            let faces = project.bodies[i].faces.clone();
            project.regen_faces.insert(body, faces);
        }
    }
    let shapes: HashMap<Id, qymcad_kernel::Shape> = {
        let _g = qymcad_kernel::kernel_gate();
        breps.into_iter().filter_map(|(id, b)| qymcad_kernel::Shape::from_brep_bytes(&b).map(|s| (id, s))).collect()
    };
    let seen = project.param_map();
    project.parameters.iter_mut().find(|q| q.name == name).expect("parameter").expr = expr.into();
    // apply_param_edit
    project.eval_parameters();
    for si in 0..project.sketches.len() {
        if project.sketches[si].constraints.iter().any(|c| c.expr().is_some()) {
            project.solve_sketch(si);
            let sid = project.sketches[si].id;
            project.mark_sketch_dirty(sid);
        }
    }
    // mark_changed_params_dirty
    let vars = project.param_map();
    let changed: Vec<String> =
        vars.iter().filter(|(k, v)| seen.get(*k).is_none_or(|o| (o - **v).abs() > 1e-12)).map(|(k, _)| k.clone()).collect();
    for n in &changed {
        project.mark_param_dependents_dirty_for(n);
    }
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, shapes);
    assert!(report.errors.is_empty(), "GUI-path rebuild errors: {:?}", report.errors);
    let consumed = project.consumed_bodies();
    let last = project.timeline.iter().flat_map(|n| n.kind.bodies()).rfind(|b| !consumed.contains(b)).expect("a body");
    shapes[&last].volume()
}
