//! Undo retains the exact committed Project and original B-rep handles.
use qymcad_engine::*;

fn ex(sketch: Id, height: Num) -> Extrude {
    Extrude { sketch, profiles: None, height, op: Op::Add, direction: Direction::Normal, through: false, target: None, name: None }
}
fn stock() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("h", &20.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    s.extrude(&ex(sk, Num::Expr("h".into()))).unwrap();
    (s, sk)
}
fn bytes(s: &Session) -> Vec<(Id, Vec<u8>)> {
    s.project()
        .timeline
        .iter()
        .flat_map(|n| n.kind.bodies())
        .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
        .collect()
}
fn capture(s: &Session) -> (serde_json::Value, Vec<(Id, Vec<u8>)>, DocInfo) {
    (serde_json::to_value(s.project()).unwrap(), bytes(s), s.info())
}
fn assert_restored(s: &Session, before: &(serde_json::Value, Vec<(Id, Vec<u8>)>, DocInfo)) {
    assert_eq!(serde_json::to_value(s.project()).unwrap(), before.0, "exact Project restoration");
    assert_eq!(bytes(s), before.1, "exact B-rep byte restoration");
    assert_eq!(s.info(), before.2, "exact document info restoration");
}
#[test]
fn undo_after_extrude_fillet_and_parameter_edit_restores_exact_breps() {
    let (mut s, sk) = stock();
    for operation in 0..3 {
        let before = capture(&s);
        let snapshot = s.begin_tool_edit().unwrap();
        match operation {
            0 => {
                s.extrude(&ex(sk, 22.0.into())).unwrap();
            }
            1 => {
                s.fillet(None, &Sel::Along { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }, &2.0.into(), None).unwrap();
            }
            _ => {
                s.param_set("h", &25.0.into()).unwrap();
            }
        }
        s.finish_tool_edit(snapshot, true);
        assert_ne!(bytes(&s), before.1);
        s.undo().unwrap();
        assert_restored(&s, &before);
    }
}
#[test]
fn failed_call_retains_history_and_restores_exact_geometry() {
    let (mut s, _) = stock();
    let before = capture(&s);
    let snapshot = s.begin_tool_edit().unwrap();
    s.param_set("h", &25.0.into()).unwrap();
    s.finish_tool_edit(snapshot, true);
    let after = capture(&s);
    let snapshot = s.begin_tool_edit().unwrap();
    assert!(s.fillet(None, &Sel::Along { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }, &100.0.into(), None).is_err());
    s.finish_tool_edit(snapshot, false);
    assert_restored(&s, &after);
    s.undo().unwrap();
    assert_restored(&s, &before);
}
#[test]
fn undo_is_bounded_and_empty_history_is_an_error() {
    let mut s = Session::new_part();
    assert!(s.undo().is_err());
    for value in 0..UNDO_LIMIT + 3 {
        let snapshot = s.begin_tool_edit().unwrap();
        s.param_set("n", &(value as f64).into()).unwrap();
        s.finish_tool_edit(snapshot, true);
    }
    for _ in 0..UNDO_LIMIT {
        s.undo().unwrap();
    }
    assert!(s.undo().is_err());
    assert_eq!(s.params()[0].value, 2.0);
}
#[test]
fn deleting_a_modifier_restores_consumed_source_without_rebuilding_it() {
    let (mut s, _) = stock();
    let source = s.result_bodies()[0].id;
    let source_bytes = s.shape(source).unwrap().to_brep_bytes().unwrap();
    let (modifier, _) = s.fillet(None, &Sel::Along { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }, &2.0.into(), None).unwrap();
    let report = s.feature_delete(modifier, false).unwrap();
    assert_eq!(report.bodies[0].id, source);
    // Cube stock volume is 20³ mm³.
    assert!((report.bodies[0].volume - 20.0_f64.powi(3)).abs() < 1e-6);
    assert_eq!(s.shape(source).unwrap().to_brep_bytes().unwrap(), source_bytes);
    assert!(s.shape(modifier).is_none());
}
#[test]
fn cascade_reaches_literal_chained_datums_and_their_sketches() {
    let mut s = Session::new_part();
    let (parent, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &3.0.into(), None).unwrap();
    let (child, _) = s.plane_offset(&PlaneRef::Plane(parent), &2.0.into(), None).unwrap();
    let sk = s.sketch_create(&PlaneRef::Plane(child), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &4.0.into(), &6.0.into(), false).unwrap();
    s.extrude(&ex(sk, 2.0.into())).unwrap();
    let before = capture(&s);
    assert!(s.feature_delete(parent, false).unwrap_err().to_string().contains("dependents"));
    assert_restored(&s, &before);
    assert!(s.feature_delete(parent, true).unwrap().bodies.is_empty());
    assert!(s.project().planes.is_empty());
    assert!(s.project().sketches.is_empty());
    assert!(s.project().timeline.is_empty());
}

#[test]
fn revolve_axis_deletion_refuses_dependents_and_cascades_them() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XZ), None).unwrap();
    s.sketch_rect(sk, &5.0.into(), &0.0.into(), &2.0.into(), &3.0.into(), false).unwrap();
    let (body, _) = s
        .revolve(&Revolve {
            sketch: sk,
            profiles: None,
            axis: AxisRef::World(Axis::Z),
            angle: 360.0.into(),
            direction: Direction::Normal,
            op: Op::NewBody,
            target: None,
            name: None,
        })
        .unwrap();
    // Annular cylinder: pi*(6²-4²)*3 = 60*pi mm³.
    assert!((s.result_bodies()[0].volume - 60.0 * std::f64::consts::PI).abs() < 1e-6);
    let axis = s.project().datum_axes[0].id;
    let before = capture(&s);
    let error = s.feature_delete(axis, false).unwrap_err();
    assert!(error.to_string().contains("dependents"), "{error}");
    assert_restored(&s, &before);
    assert!(s.feature_delete(axis, true).unwrap().bodies.is_empty());
    assert!(s.project().datum_axes.is_empty());
    assert!(!s.project().timeline.iter().any(|n| n.id == body));
    assert!(s.shape(body).is_none());
    assert_eq!(s.project().sketches.len(), 1, "the revolve's independent sketch survives");
}

#[test]
fn undo_preserves_the_latest_save_path() {
    let (mut s, _) = stock();
    let snapshot = s.begin_tool_edit().unwrap();
    s.param_set("h", &25.0.into()).unwrap();
    s.finish_tool_edit(snapshot, true);
    let dir = std::env::temp_dir().join(format!("qymcad-undo-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("latest.qcad");
    s.save(Some(&path)).unwrap();
    let saved_bytes = std::fs::read(&path).unwrap();
    s.undo().unwrap();
    assert_eq!(s.path(), Some(path.as_path()), "modelling undo must preserve the latest save association");
    assert_eq!(std::fs::read(&path).unwrap(), saved_bytes, "undo does not alter saved files");
    assert!((s.result_bodies()[0].volume - 20.0_f64.powi(3)).abs() < 1e-6);
    assert_eq!(s.save(None).unwrap(), path);
}
