//! Advisory diagnostics; all geometry expectations are derived from rectangular prisms.
mod common;
use common::*;
use qymcad_engine::*;

fn rect(s: &mut Session, plane: PlaneRef, w: f64, l: f64) -> Id {
    let sk = s.sketch_create(&plane, None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &w.into(), &l.into(), false).unwrap();
    sk
}
fn extrude(s: &mut Session, sketch: Id, height: f64, op: Op, direction: Direction, name: &str) -> (Id, Rebuild) {
    s.extrude(&Extrude {
        sketch,
        profiles: None,
        height: height.into(),
        op,
        direction,
        through: false,
        target: None,
        name: Some(name.into()),
    })
    .unwrap()
}
fn pocket(z: f64) -> (Session, Id, Rebuild) {
    let mut s = Session::new_part();
    let sk = rect(&mut s, PlaneRef::Base(BaseName::XY), 20.0, 20.0);
    extrude(&mut s, sk, 20.0, Op::Add, Direction::Normal, "stock");
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &z.into(), None).unwrap();
    let sk = rect(&mut s, PlaneRef::Plane(plane), 4.0, 6.0);
    let (id, report) = extrude(&mut s, sk, 3.0, Op::Cut, Direction::Reverse, "pocket");
    (s, id, report)
}
#[test]
fn a_pocket_inside_stock_warns_and_names_the_feature() {
    let (s, id, report) = pocket(10.0);
    assert!(report.errors.is_empty());
    // Interior cuts include a 0.001 mm entry clearance (F-003/F-017): V=20³−4*6*(3+0.001).
    assert_close(report.bodies[0].volume, 20.0_f64.powi(3) - 4.0 * 6.0 * 3.001, 1e-3, "sealed pocket volume");
    assert!(
        report.warnings.iter().any(|w| w.node == id && w.name == "pocket" && w.message.contains("shell")),
        "sealed pocket must warn with the feature name: {:?}",
        report.warnings
    );
    assert!(s.info().warnings.iter().any(|w| w.contains("pocket") && w.contains("shell")), "doc_info retains cavity warning");
}
#[test]
fn a_normal_open_pocket_does_not_warn() {
    let (_, _, report) = pocket(20.0);
    // Entry clearance lies outside stock, so V=20³−4*6*3.
    assert_close(report.bodies[0].volume, 20.0_f64.powi(3) - 4.0 * 6.0 * 3.0, 1e-3, "open pocket volume");
    assert!(report.warnings.is_empty(), "open pocket warnings: {:?}", report.warnings);
}
#[test]
fn disjoint_array_copies_do_not_warn_about_a_void() {
    let mut s = Session::new_part();
    let sk = rect(&mut s, PlaneRef::Base(BaseName::XY), 2.0, 3.0);
    extrude(&mut s, sk, 4.0, Op::Add, Direction::Normal, "stock");
    let (body, report) = s
        .linear_array(None, &ArrayDir { dx: 10.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: 3.0.into() }, None, Some("copies"))
        .unwrap();
    // Three disjoint 2×3×4 prisms: V=3*2*3*4, with one shell per solid.
    assert_close(report.bodies[0].volume, 3.0 * 2.0 * 3.0 * 4.0, 1e-6, "array volume");
    assert_eq!(s.shape(body).unwrap().solid_count(), 3);
    assert_eq!(s.shape(body).unwrap().shell_count(), 3);
    assert!(report.warnings.is_empty(), "disconnected solids are not sealed voids: {:?}", report.warnings);
}
#[test]
fn opening_stale_stored_geometry_warns_about_the_rebuilt_body() {
    let mut s = Session::new_part();
    let sk = rect(&mut s, PlaneRef::Base(BaseName::XY), 20.0, 30.0);
    let (body, _) = extrude(&mut s, sk, 6.0, Op::Add, Direction::Normal, "stock");
    let path = scratch("stale_geometry_warning.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    // Store the old V=20*30*6 B-rep alongside a dirty recipe h=10, expected V=20*30*10.
    project.set_feat_dim(body, "height", "10".into());
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert_close(report.bodies[0].volume, 20.0 * 30.0 * 10.0, 1e-6, "rebuilt volume");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.node == body && w.message.contains("stored geometry") && w.message.contains("3600") && w.message.contains("6000")),
        "stale stored geometry must warn about 3600 -> 6000: {:?}",
        report.warnings
    );
    assert!(opened.info().warnings.iter().any(|w| w.contains("stored geometry")));
    let warning = report.warnings.iter().find(|w| w.node == body && w.message.contains("stored geometry")).unwrap();
    assert!(warning.message.contains("3600.0000 → 6000.0000 mm³"), "readable volume: {}", warning.message);
    assert!(!warning.message.contains("Some("), "human-readable bbox: {}", warning.message);
    assert!(warning.message.contains("bbox"), "a 4 mm height change exceeds F-016 tolerance: {}", warning.message);
    assert!(warning.message.contains("the rebuilt geometry is now used; save to update the file"), "current geometry: {}", warning.message);
}
#[test]
fn opening_unchanged_geometry_does_not_warn() {
    let (mut s, _, _) = pocket(20.0);
    let path = scratch("unchanged_geometry_warning.qcad");
    s.save(Some(&path)).unwrap();
    let (_, report) = Session::open(&path).unwrap();
    assert!(report.warnings.is_empty(), "unchanged open: {:?}", report.warnings);
}

#[test]
fn failed_sketch_rebuild_keeps_existing_advisory_warnings() {
    let (mut s, _, _) = pocket(10.0);
    let before = s.info();
    let sk = s.project().sketches.last().unwrap().id;
    let edge = s.sketch_detail(sk).unwrap().entities[0].id;
    assert!(s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, edge)).is_err());
    assert_eq!(s.info(), before, "failed edits retain cavity diagnostics and document state");
}

#[test]
fn opening_stale_pocket_warns_even_when_the_bbox_is_unchanged() {
    let (mut s, body, _) = pocket(20.0);
    let path = scratch("stale_pocket_volume_warning.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    // Same stock bbox, one extra millimetre of pocket depth: delta V=4*6*1=24 mm³.
    project.set_feat_dim(body, "height", "4".into());
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (_, report) = Session::open(&path).unwrap();
    assert_close(report.bodies[0].volume, 20.0_f64.powi(3) - 4.0 * 6.0 * 4.0, 1e-6, "deeper rebuilt pocket");
    assert!(
        report.warnings.iter().any(|w| w.node == body && w.message.contains("stored geometry")),
        "same-bbox stale pocket: {:?}",
        report.warnings
    );
    let warning = report.warnings.iter().find(|w| w.node == body && w.message.contains("stored geometry")).unwrap();
    assert!(!warning.message.contains("bbox"), "bounds within F-016 tolerance must not be described as changed: {}", warning.message);
}

#[test]
fn opening_unchanged_dirty_boolean_geometry_does_not_warn() {
    let (mut s, body, _) = pocket(20.0);
    let path = scratch("unchanged_dirty_boolean.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.timeline.iter_mut().find(|n| n.id == body).unwrap().dirty = true;
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (_, report) = Session::open(&path).unwrap();
    assert!(report.warnings.is_empty(), "unchanged dirty geometry: {:?}", report.warnings);
}
