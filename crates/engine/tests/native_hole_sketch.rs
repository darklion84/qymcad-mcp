//! Pin-specific characterization: raw native scheduling, never engine propagation/repair.
mod common;
use common::{assert_close, scratch};
use qymcad_core::model::{Constraint, HoleTool, Id, Param, Project};
use qymcad_core::names::Role;
use std::collections::HashMap;

fn fixture(named: bool, bridge: bool) -> (Project, HashMap<Id, qymcad_kernel::Shape>, Id, Id, Id) {
    let mut p = Project::default();
    p.new_document();
    p.parameters.push(Param { name: "pitch".into(), expr: "8".into(), value: 8. });
    assert!(p.eval_parameters().is_empty());
    let stock = p.add_box(40., 30., 10.);
    let si = p.new_sketch("marks");
    let sk = p.sketches[si].id;
    p.add_sketch_node(sk, "marks");
    let point = p.sketch_point_at(si, 8., 6., 1e-6);
    let (xa, xb) = p.ensure_axis(si, 0);
    let (ya, yb) = p.ensure_axis(si, 1);
    for (a, b, d, expr) in [(yb, ya, 8., "pitch"), (xa, xb, 6., "")] {
        assert!(p.add_constraint_if_independent(
            si,
            Constraint::DistancePL { p: point, a, b, d, off: 0., expr: expr.into(), driven: false, at: None }
        ));
    }
    if named {
        assert!(p.add_named_dim("point_x".into(), sk, vec![point, ya, yb]));
    }
    assert!(p.solve_sketch(si) < 1e-6);
    let h = p.add_hole_from_sketch(stock, sk, HoleTool { kind: 0, diameter: 4., depth: 12., dia2: 0., depth2: 0. }, true);
    if bridge {
        p.set_feat_dim(h, "sketch_dep_coord", "pitch".into());
        if named {
            p.set_feat_dim(h, "sketch_dep_driver", "point_x".into());
        }
    }
    let (r, shapes) = qymcad_testkit::regenerate(&mut p);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_close(shapes[&h].volume(), 12000. - 40. * std::f64::consts::PI, 1e-6, "one through hole");
    (p, shapes, sk, point, h)
}

fn save(p: &mut Project, shapes: &HashMap<Id, qymcad_kernel::Shape>, name: &str) -> std::path::PathBuf {
    for (id, faces) in p.regen_faces.clone() {
        if let Some(i) = p.mesh_index(id) {
            p.bodies[i].faces = faces;
        }
    }
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(id, shape)| (*id, shape.to_brep_bytes().unwrap())).collect::<Vec<_>>()
    };
    let path = scratch(name);
    qymcad_io::save_project_with_brep(p, path.to_str().unwrap(), &breps).unwrap();
    path
}

fn wall_x(p: &Project, h: Id, point: Id) -> f64 {
    let f = p.regen_faces[&h]
        .iter()
        .find(|f| p.names.get(f.id).is_some_and(|n| n.feature == h && n.role == Role::Hole && n.src == point) && f.area > 0.)
        .expect("point-named bore face");
    assert_close(f.centroid.y, 6., 1e-6, "bore y");
    assert_close(f.centroid.z, 5., 1e-6, "bore z");
    f.centroid.x
}

#[test]
fn raw_global_app_path_needs_persisted_sketch_guard() {
    for bridge in [false, true] {
        let (mut p, shapes, sk, point, h) = fixture(false, bridge);
        assert!(!p.timeline.iter().find(|n| n.id == h).unwrap().kind.inputs().contains(&sk));
        let path = save(&mut p, &shapes, &format!("native-hole-global-{bridge}.qcad"));
        let (volume, _, _, p) = common::gui_edit_param_model(&path, "pitch", "12");
        assert_close(p.sketch_isolated_points(sk)[0][0], 12., 1e-6, "native sketch moved");
        assert_close(wall_x(&p, h, point), if bridge { 12. } else { 8. }, 1e-6, "native bore location");
        assert_close(volume, 12000. - 40. * std::f64::consts::PI, 1e-6, "position-independent analytic volume");
        println!("RAW_GLOBAL bridge={bridge} sketch_x=12 wall_x={}", wall_x(&p, h, point));
    }
}

#[test]
fn raw_named_app_path_needs_driver_name_guard() {
    for bridge in [false, true] {
        let (mut p, shapes, sk, point, h) = fixture(true, bridge);
        let path = save(&mut p, &shapes, &format!("native-hole-driver-{bridge}.qcad"));
        let (volume, _, _, p) = common::gui_edit_driver_model(&path, "point_x", "15");
        assert_close(p.sketch_isolated_points(sk)[0][0], 15., 1e-6, "native driver moved point");
        assert_close(wall_x(&p, h, point), if bridge { 15. } else { 8. }, 1e-6, "native bore location");
        assert_close(volume, 12000. - 40. * std::f64::consts::PI, 1e-6, "analytic volume");
        println!("RAW_NAMED bridge={bridge} point_x=15 wall_x={}", wall_x(&p, h, point));
    }
}

#[test]
fn persisted_bridge_cannot_observe_unnamed_literal_app_edit() {
    let (mut p, shapes, sk, point, h) = fixture(false, true);
    let path = save(&mut p, &shapes, "native-hole-unnamed.qcad");
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.ensure_document();
    for i in 0..project.bodies.len() {
        if let Some(id) = project.mesh_id(i) {
            project.regen_faces.insert(id, project.bodies[i].faces.clone());
        }
    }
    let shapes = common::restore_shapes(breps);
    let si = project.sketch_index(sk).unwrap();
    for c in &mut project.sketches[si].constraints {
        if let Constraint::DistancePL { d, expr, .. } = c {
            if expr == "pitch" {
                *d = 13.;
                expr.clear();
            }
        }
    }
    assert!(project.solve_sketch(si) < 1e-6);
    project.mark_sketch_dirty(sk);
    let (r, _) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, shapes);
    assert!(r.errors.is_empty());
    assert!(!r.built.iter().any(|(id, _)| *id == h));
    assert_close(project.sketch_isolated_points(sk)[0][0], 13., 1e-6, "unnamed native dimension moved point");
    assert_close(wall_x(&project, h, point), 8., 1e-6, "hole remains stale");
    println!("RAW_UNNAMED_LITERAL bridge=true point_x=13 wall_x=8 rebuilt={:?}", r.built);
}

#[test]
fn raw_partial_cut_succeeds_without_every_requested_site() {
    let (mut p, _, sk, inside, h) = fixture(false, false);
    let si = p.sketch_index(sk).unwrap();
    let outside = p.sketch_point_at(si, 80., 6., 1e-6);
    let (r, shapes) = qymcad_testkit::regenerate(&mut p);
    assert!(r.errors.is_empty(), "native aggregate cut reports success: {:?}", r.errors);
    assert_eq!(p.sketch_hole_point_ids(sk), vec![inside, outside], "native always drills all isolated points");
    assert_close(shapes[&h].volume(), 12000. - 40. * std::f64::consts::PI, 1e-6, "only inside site removes stock");
    assert!(p.regen_faces[&h]
        .iter()
        .any(|f| p.names.get(f.id).is_some_and(|n| n.feature == h && n.role == Role::Hole && n.src == inside) && f.area > 0.));
    assert!(!p.regen_faces[&h].iter().any(|f| p
        .names
        .get(f.id)
        .is_some_and(|n| n.feature == h && n.role == Role::Hole && n.src == outside)
        && f.area > 0.));
    println!("RAW_PARTIAL_SITE inside={inside} outside={outside} native_errors=0 outside_bore=false");
}

#[test]
fn raw_partial_cut_also_ignores_site_over_existing_void() {
    let (mut p, _, _, _, source) = fixture(false, false);
    let si = p.new_sketch("second marks");
    let sk = p.sketches[si].id;
    p.add_sketch_node(sk, "second marks");
    let good = p.sketch_point_at(si, -8., -6., 1e-6);
    let void = p.sketch_point_at(si, 8., 6., 1e-6);
    let h = p.add_hole_from_sketch(source, sk, HoleTool { kind: 0, diameter: 4., depth: 12., dia2: 0., depth2: 0. }, true);
    let (r, shapes) = qymcad_testkit::regenerate(&mut p);
    assert!(r.errors.is_empty(), "native aggregate cut reports success: {:?}", r.errors);
    assert_close(shapes[&h].volume(), 12000. - 80. * std::f64::consts::PI, 1e-6, "only one new hole removes stock");
    for (point, expected) in [(good, true), (void, false)] {
        assert_eq!(
            p.regen_faces[&h]
                .iter()
                .any(|f| p.names.get(f.id).is_some_and(|n| n.feature == h && n.role == Role::Hole && n.src == point) && f.area > 0.),
            expected
        );
    }
    println!("RAW_VOID_SITE good={good} void={void} native_errors=0 void_bore=false");
}

#[test]
fn raw_datum_guard_does_not_add_native_wave_ordering() {
    use qymcad_core::feature::{BasePlane, FeatureKind, SketchPlane};
    let (mut p, _, sk, _, h) = fixture(false, true);
    p.parameters.push(Param { name: "height".into(), expr: "10".into(), value: 10. });
    assert!(p.eval_parameters().is_empty());
    let plane = p.add_offset_plane(BasePlane::XY, 10.);
    p.set_feat_dim(plane, "dist", "height".into());
    // Construct the same valid order as the app: stock, datum, markers, hole.
    let node = p.timeline.pop().unwrap();
    assert_eq!(node.id, plane);
    p.timeline.insert(1, node);
    let si = p.sketch_index(sk).unwrap();
    p.sketches[si].plane = SketchPlane::Datum(plane);
    let FeatureKind::Hole { depth, flip, .. } = &mut p.timeline.iter_mut().find(|n| n.id == h).unwrap().kind else { panic!() };
    *depth = 4.;
    *flip = false;
    p.set_feat_dim(h, "sketch_dep_support", "height".into());
    let (r, shapes) = qymcad_testkit::regenerate(&mut p);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_close(shapes[&h].volume(), 12000., 1e-6, "first native wave captures unresolved datum z0");
    let (r, shapes) = qymcad_testkit::regenerate_with_shapes(&mut p, shapes);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_close(shapes[&h].volume(), 12000. - 16. * std::f64::consts::PI, 1e-6, "blind hole at z10");
    let path = save(&mut p, &shapes, "native-hole-datum-guard.qcad");
    let (v, _, _, mut p) = common::gui_edit_param_model(&path, "height", "11");
    let origin = p.planes.iter().find(|pl| pl.id == plane).unwrap().origin;
    assert_close(origin[2], 11., 1e-6, "native plane updated");
    p.mark_param_dependents_dirty_for("height");
    let waves = p.rebuild_waves();
    assert!(waves.iter().any(|wave| wave.contains(&plane) && wave.contains(&h)), "hole and datum share a wave: {waves:?}");
    println!(
        "RAW_DATUM_GUARD volume={v} expected_new={} old={} plane_z={} waves={:?}",
        12000. - 12. * std::f64::consts::PI,
        12000. - 16. * std::f64::consts::PI,
        origin[2],
        waves
    );
    // Persisted formula reads dirty the hole, but do not make it wait for the datum.
    assert_close(v, 12000. - 16. * std::f64::consts::PI, 1e-6, "native hole captured old datum placement");
}
