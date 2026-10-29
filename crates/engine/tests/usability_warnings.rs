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
        down: None,
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

/// A plate with a pocket on a datum plane at z = t (t: 6 → 10). QymCAD 0.1.0 could rebuild it on the old plane
/// and seal the pocket inside the stock (F-017, our #165); 0.1.1 cannot (fc1dedc), but files saved that way still
/// exist. The test makes one by storing the body of the same recipe with the plane left at z = 6.
fn sealed_plate(dist: Num) -> (Session, Id, Id) {
    let mut s = Session::new_part();
    s.param_set("t", &10.0.into()).unwrap();
    let stock = rect(&mut s, PlaneRef::Base(BaseName::XY), 60.0, 40.0);
    s.extrude(&Extrude {
        down: None,
        sketch: stock,
        profiles: None,
        height: Num::Expr("t".into()),
        op: Op::Add,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: Some("plate".into()),
    })
    .unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &dist, None).unwrap();
    let sketch = rect(&mut s, PlaneRef::Plane(plane), 30.0, 16.0);
    let (body, _) = extrude(&mut s, sketch, 3.0, Op::Cut, Direction::Reverse, "pocket");
    (s, sketch, body)
}

#[test]
fn opening_legacy_sealed_plate_reports_the_stored_shell_difference() {
    let (mut recipe, sketch, body) = sealed_plate(Num::Expr("t".into()));
    let (mut legacy, _, legacy_body) = sealed_plate(6.0.into());
    assert_eq!(legacy_body, body, "same recipe, same ids");
    let path = scratch("legacy_sealed_plate.qcad");
    recipe.save(Some(&path)).unwrap();
    let legacy_path = scratch("legacy_sealed_plate_body.qcad");
    legacy.save(Some(&legacy_path)).unwrap();
    let qymcad_io::LoadedProject { mut project, mut breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let old = qymcad_io::load_project_with_brep(legacy_path.to_str().unwrap()).unwrap();
    // A file saved by the 0.1.0 GUI has no server datum dependency (F-017); opening adds it and rebuilds.
    project.feat_dims.remove(&sketch);
    // Store the sealed body (mesh, faces and B-rep) under the open-pocket recipe.
    let sealed_brep = old.breps.iter().find(|(id, _)| *id == body).unwrap().1.clone();
    breps.iter_mut().find(|(id, _)| *id == body).unwrap().1 = sealed_brep.clone();
    let (i, j) = (project.mesh_index(body).unwrap(), old.project.mesh_index(body).unwrap());
    project.bodies[i] = old.project.bodies[j].clone();
    let stored = common::restore_shapes(vec![(body, sealed_brep)]).remove(&body).unwrap();
    // Sealed pocket has six stock faces plus six inner faces; open pocket has five inner faces.
    assert_eq!((stored.shell_count(), stored.solid_count()), (2, 1));
    assert_eq!(stored.face_kinds().unwrap().into_iter().sum::<u32>(), 6 + 6);
    let sealed_volume = 60.0 * 40.0 * 10.0 - 30.0 * 16.0 * (3.0 + 0.001);
    assert_close(stored.volume(), sealed_volume, 1e-6, "legacy sealed pocket volume");
    let stored_bbox = stored.bbox().unwrap();
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "repaired plate rebuild errors: {:?}", report.errors);
    let open_volume = 60.0 * 40.0 * 10.0 - 30.0 * 16.0 * 3.0;
    assert_close(report.bodies[0].volume, open_volume, 1e-6, "rebuilt open pocket volume");
    assert_close(open_volume - sealed_volume, 30.0 * 16.0 * 0.001, 1e-9, "entry clearance difference");
    let rebuilt = opened.shape(body).unwrap();
    assert_eq!((rebuilt.shell_count(), rebuilt.solid_count()), (1, 1));
    assert_eq!(rebuilt.face_kinds().unwrap().into_iter().sum::<u32>(), 6 + 5);
    for (old, new) in stored_bbox.into_iter().zip(rebuilt.bbox().unwrap()) {
        assert_close(new, old, 0.05, "plate bounds remain within F-016 tolerance");
    }
    let warning = report.warnings.iter().find(|w| w.node == body && w.message.contains("stored geometry")).unwrap();
    assert!(
        warning.message.contains("stored body had 2 shells (sealed void), the rebuild has 1"),
        "stored sealed void must name the shell difference: {}",
        warning.message
    );
    assert!(warning.message.contains("faces 12 → 11"), "stored face difference: {}", warning.message);
    assert!(opened.info().warnings.iter().any(|w| w.contains("sealed void") && w.contains("the rebuild has 1")));
}

#[test]
fn opening_unchanged_sealed_geometry_checks_the_stored_body_for_voids() {
    let (mut s, body, _) = pocket(10.0);
    let path = scratch("unchanged_stored_sealed_pocket.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "stored sealed rebuild errors: {:?}", report.errors);
    assert_close(report.bodies[0].volume, 20.0_f64.powi(3) - 4.0 * 6.0 * 3.001, 1e-6, "stored sealed volume");
    assert!(
        report.warnings.iter().any(|w| w.node == body
            && w.message.contains("stored body has 2 shells and 1 solids")
            && w.message.contains("sealed internal void")),
        "clean open must inspect the stored body for a sealed void: {:?}",
        report.warnings
    );
    assert!(opened.info().warnings.iter().any(|w| w.contains("stored body has 2 shells")));
}

#[test]
fn opening_detects_a_face_count_difference_with_equal_volume_and_bounds() {
    let mut s = Session::new_part();
    let sk = rect(&mut s, PlaneRef::Base(BaseName::XY), 20.0, 20.0);
    let (body, _) = extrude(&mut s, sk, 20.0, Op::Add, Direction::Normal, "stock");
    let path = scratch("stale_face_count_only.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, mut breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let stored_bbox;
    // A plane at mid-height splits each of four walls into two faces, while preserving V=20³
    // and all six bounds. The cube recipe rebuilds to six faces, instead of 4*2+2=10.
    {
        use qymcad_core::feature::Kernel;
        let kernel =
            qymcad_kernel::OcctKernel { shapes: std::cell::RefCell::new(common::restore_shapes(breps.clone())), ..Default::default() };
        let _gate = qymcad_kernel::kernel_gate();
        kernel.split_faces(body + 1, body, [0.0, 0.0, 10.0], [0.0, 0.0, 1.0]).unwrap();
        let shapes = kernel.shapes.borrow();
        let stored = &shapes[&(body + 1)];
        assert_close(stored.volume(), 20.0_f64.powi(3), 1e-6, "split-face cube volume");
        assert_eq!(stored.face_kinds().unwrap().into_iter().sum::<u32>(), 4 * 2 + 2);
        assert_eq!((stored.shell_count(), stored.solid_count()), (1, 1));
        stored_bbox = stored.bbox().unwrap();
        breps.iter_mut().find(|(id, _)| *id == body).unwrap().1 = stored.to_brep_bytes().unwrap();
    }
    project.timeline.iter_mut().find(|n| n.id == body).unwrap().dirty = true;
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "split-face rebuild errors: {:?}", report.errors);
    assert_close(report.bodies[0].volume, 20.0_f64.powi(3), 1e-6, "rebuilt cube volume");
    let rebuilt = opened.shape(body).unwrap();
    assert_eq!((rebuilt.shell_count(), rebuilt.solid_count()), (1, 1));
    assert_eq!(rebuilt.face_kinds().unwrap().into_iter().sum::<u32>(), 6);
    for (old, new) in stored_bbox.into_iter().zip(rebuilt.bbox().unwrap()) {
        assert_close(new, old, 0.05, "split-face cube bounds remain within F-016 tolerance");
    }
    assert!(
        report.warnings.iter().any(|w| w.node == body && w.message.contains("faces 10 → 6")),
        "face-count-only change must warn despite equal metrics: {:?}",
        report.warnings
    );
}

#[test]
fn opening_stale_disjoint_array_reports_solid_counts_without_a_void_warning() {
    let mut s = Session::new_part();
    let sk = rect(&mut s, PlaneRef::Base(BaseName::XY), 2.0, 3.0);
    extrude(&mut s, sk, 4.0, Op::Add, Direction::Normal, "stock");
    let (body, _) = s
        .linear_array(None, &ArrayDir { dx: 10.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: 3.0.into() }, None, Some("copies"))
        .unwrap();
    let path = scratch("stale_disjoint_solids.qcad");
    assert_close(s.shape(body).unwrap().volume(), 3.0 * 2.0 * 3.0 * 4.0, 1e-6, "three disjoint stored prisms");
    assert_eq!((s.shape(body).unwrap().shell_count(), s.shape(body).unwrap().solid_count()), (3, 3));
    assert_eq!(s.shape(body).unwrap().face_kinds().unwrap().into_iter().sum::<u32>(), 3 * 6);
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.set_feat_dim(body, "count", "2".into());
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "array rebuild errors: {:?}", report.errors);
    assert_close(report.bodies[0].volume, 2.0 * 2.0 * 3.0 * 4.0, 1e-6, "two disjoint rebuilt prisms");
    assert_eq!((opened.shape(body).unwrap().shell_count(), opened.shape(body).unwrap().solid_count()), (2, 2));
    assert_eq!(opened.shape(body).unwrap().face_kinds().unwrap().into_iter().sum::<u32>(), 2 * 6);
    let warning = report.warnings.iter().find(|w| w.node == body && w.message.contains("stored geometry")).unwrap();
    assert!(warning.message.contains("solids 3 → 2"), "stored solid difference: {}", warning.message);
    assert!(warning.message.contains("faces 18 → 12"), "stored face difference: {}", warning.message);
    assert!(!report.warnings.iter().any(|w| w.message.contains("void")), "disconnected bodies have no inner shells: {:?}", report.warnings);
}

/// F-075: QymCAD 0.1.1 drops the spline records of a 0.1.0 document on load and keeps only their points. The
/// fixture was written by the 0.1.0 release (`Project::add_spline` + `save_project`: one open spline through
/// (0,0) (10,5) (20,0) (30,5), no other entity). The loss is said on open and stays in doc_info after rebuilds.
#[test]
fn opening_a_0_1_0_spline_says_it_is_lost() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy_spline_0.1.0.qcad");
    let copy = scratch("legacy_spline_0.1.0.qcad");
    std::fs::copy(&path, &copy).unwrap();
    let (mut s, report) = Session::open(&copy).unwrap();
    let sketch = s.sketches()[0].id;
    let detail = s.sketch_detail(sketch).unwrap();
    assert_eq!(detail.entities.len(), 0, "QymCAD 0.1.1 kept no spline entity");
    let mut points: Vec<(f64, f64)> = detail.points.iter().filter(|p| p.role.is_none()).map(|p| (p.x, p.y)).collect();
    points.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(points, [(0.0, 0.0), (10.0, 5.0), (20.0, 0.0), (30.0, 5.0)], "the spline's points stay");
    let lost = |warnings: Vec<String>| warnings.into_iter().filter(|w| w.contains("1 spline saved by QymCAD 0.1.0 is lost")).count();
    assert_eq!(
        report.warnings.iter().filter(|w| w.node == sketch && w.message.contains("1 spline saved by QymCAD 0.1.0 is lost")).count(),
        1
    );
    assert_eq!(lost(s.info().warnings), 1);
    s.param_set("w", &Num::Value(5.0)).unwrap();
    s.rebuild();
    assert_eq!(lost(s.info().warnings), 1, "the loss outlives rebuilds and is not repeated");
}
