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
    s.extrude(&Extrude { sketch, profiles: None, height: height.into(), op, direction, through: false, target: None, name: Some(name.into()) }).unwrap()
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
    assert_close(report.bodies[0].volume, 20.0_f64.powi(3) - 4.0*6.0*3.001, 1e-3, "sealed pocket volume");
    assert!(report.warnings.iter().any(|w| w.node == id && w.name == "pocket" && w.message.contains("shell")),
        "sealed pocket must warn with the feature name: {:?}", report.warnings);
    assert!(s.info().warnings.iter().any(|w| w.contains("pocket") && w.contains("shell")), "doc_info retains cavity warning");
}
#[test]
fn a_normal_open_pocket_does_not_warn() {
    let (_, _, report) = pocket(20.0);
    // Entry clearance lies outside stock, so V=20³−4*6*3.
    assert_close(report.bodies[0].volume, 20.0_f64.powi(3) - 4.0*6.0*3.0, 1e-3, "open pocket volume");
    assert!(report.warnings.is_empty(), "open pocket warnings: {:?}", report.warnings);
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
    assert_close(report.bodies[0].volume, 20.0*30.0*10.0, 1e-6, "rebuilt volume");
    assert!(report.warnings.iter().any(|w| w.node == body && w.message.contains("stored geometry") && w.message.contains("3600") && w.message.contains("6000")),
        "stale stored geometry must warn about 3600 -> 6000: {:?}", report.warnings);
    assert!(opened.info().warnings.iter().any(|w| w.contains("stored geometry")));
}
#[test]
fn opening_unchanged_geometry_does_not_warn() {
    let (mut s, _, _) = pocket(20.0);
    let path = scratch("unchanged_geometry_warning.qcad");
    s.save(Some(&path)).unwrap();
    let (_, report) = Session::open(&path).unwrap();
    assert!(report.warnings.is_empty(), "unchanged open: {:?}", report.warnings);
}
