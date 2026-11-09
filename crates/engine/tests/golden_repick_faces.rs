mod common;
use common::*;
use qymcad_core::feature::FeatureKind;
use qymcad_engine::*;

fn cylinder_stock() -> (Session, Id) {
    let mut s = Session::new_part();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sketch, &0.0.into(), &0.0.into(), &20.0.into(), false).unwrap();
    let (body, report) = s
        .extrude(&Extrude {
            sketch,
            profiles: None,
            profiles_at: None,
            height: 10.0.into(),
            down: None,
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("cylinder".into()),
        })
        .unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_close(report.bodies[0].volume, std::f64::consts::PI * 10.0_f64.powi(2) * 10.0, 1e-5, "pi*r^2*h");
    (s, body)
}

fn box_stock() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("h", &10.0.into()).unwrap();
    s.param_set("t", &1.0.into()).unwrap();
    s.param_set("d", &5.0.into()).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let (body, report) = s
        .extrude(&Extrude {
            sketch,
            profiles: None,
            profiles_at: None,
            height: "h".into(),
            down: None,
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("box".into()),
        })
        .unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_close(report.bodies[0].volume, 4000.0, 1e-6, "20*20*10");
    (s, body)
}

fn face(s: &mut Session, body: Id, axis: usize, positive: bool) -> u32 {
    s.topology(Some(body), true)
        .unwrap()
        .faces
        .into_iter()
        .find(|f| f.normal.is_some_and(|n| if positive { n[axis] > 0.99 } else { n[axis] < -0.99 }))
        .unwrap()
        .id
}

fn volume(report: &Rebuild, expected: f64) {
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.bodies.len(), 1);
    assert_close(report.bodies[0].volume, expected, 1e-6, "analytic BREP volume");
}

#[test]
fn shell_repick_replaces_faces_preserves_formula_and_undoes_once() {
    let (mut s, body) = box_stock();
    let top = face(&mut s, body, 2, true);
    let bottom = face(&mut s, body, 2, false);
    let (feature, created) = s.shell(None, &Sel::Ids(vec![top]), &"t".into(), Side::Inward, Some("shell")).unwrap();
    volume(&created, 4000.0 - 18.0 * 18.0 * 9.0);
    let old_node = s.project().timeline.iter().find(|n| n.id == feature).unwrap().clone();
    let old_dims = s.project().feat_dims.clone();
    let (_, selected, report) = s.repick_shell(feature, &Sel::Ids(vec![top, bottom])).unwrap();
    let mut expected_faces = vec![top, bottom];
    expected_faces.sort_unstable();
    assert_eq!(selected, expected_faces);
    assert_eq!(s.project().timeline.iter().find(|n| n.id == feature).unwrap().id, old_node.id);
    assert_eq!(s.project().feat_dims, old_dims);
    volume(&report, 4000.0 - 18.0 * 18.0 * 10.0);
    volume(&s.param_set("t", &2.0.into()).unwrap(), 4000.0 - 16.0 * 16.0 * 10.0);
    let (mut undo_session, undo_body) = box_stock();
    let undo_top = face(&mut undo_session, undo_body, 2, true);
    let (undo_feature, _) = undo_session.shell(None, &Sel::Ids(vec![undo_top]), &"t".into(), Side::Inward, None).unwrap();
    let before_ref = match &undo_session.project().timeline.iter().find(|n| n.id == undo_feature).unwrap().kind {
        FeatureKind::Shell { faces, .. } => faces.clone(),
        _ => panic!("shell"),
    };
    undo_session.repick_shell(undo_feature, &Sel::Ids(vec![undo_top])).unwrap();
    let after_ref = match &undo_session.project().timeline.iter().find(|n| n.id == undo_feature).unwrap().kind {
        FeatureKind::Shell { faces, .. } => faces.clone(),
        _ => panic!("shell"),
    };
    assert_eq!(after_ref, before_ref);
}

#[test]
fn shell_descriptive_repick_stores_app_picks_and_app_edit_survives() {
    let (mut s, body) = box_stock();
    let (feature, first) =
        s.shell(None, &Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }, &"t".into(), Side::Inward, Some("shell")).unwrap();
    volume(&first, 4000.0 - 18.0 * 18.0 * 9.0);
    let (_, selected, report) = s.repick_shell(feature, &Sel::Facing { dir: [0.0, 0.0, -1.0], tol_deg: 1.0 }).unwrap();
    volume(&report, 4000.0 - 18.0 * 18.0 * 9.0);
    assert_eq!(selected, vec![face(&mut s, body, 2, false)]);
    let stored = match &s.project().timeline.iter().find(|n| n.id == feature).unwrap().kind {
        FeatureKind::Shell { faces, .. } => faces.clone(),
        _ => panic!("shell"),
    };
    let (mut fresh, _) = box_stock();
    let (fresh_feature, _) =
        fresh.shell(None, &Sel::Facing { dir: [0.0, 0.0, -1.0], tol_deg: 1.0 }, &"t".into(), Side::Inward, Some("shell")).unwrap();
    let fresh_ref = match &fresh.project().timeline.iter().find(|n| n.id == fresh_feature).unwrap().kind {
        FeatureKind::Shell { faces, .. } => faces,
        _ => panic!("shell"),
    };
    assert_eq!(&stored, fresh_ref);
    // A description is stored as the app stores a hand pick (2026-11-09): the app's edit mode reopens only picked ids.
    assert_eq!(stored, qymcad_core::refs::Ref::picks(&selected), "descriptive shell re-pick must store the app's picks");
    let path = scratch("d2-shell-repick.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, opened_report) = Session::open(&path).unwrap();
    volume(&opened_report, 4000.0 - 18.0 * 18.0 * 9.0);
    let engine = opened.param_set("h", &15.0.into()).unwrap();
    let expected = 20.0 * 20.0 * 15.0 - 18.0 * 18.0 * 14.0;
    volume(&engine, expected);
    assert_close(gui_edit_param(&path, "h", "15"), expected, 1e-6, "app shell height edit");
}

#[test]
fn push_face_repick_updates_hint_preserves_formula_and_app_edit() {
    let (mut s, body) = box_stock();
    let top = face(&mut s, body, 2, true);
    let xface = face(&mut s, body, 0, true);
    let (feature, created) = s.push_face(None, &Sel::Ids(vec![top]), &"d".into(), Some("push")).unwrap();
    volume(&created, 20.0 * 20.0 * 15.0);
    let old_hint = match &s.project().timeline.iter().find(|n| n.id == feature).unwrap().kind {
        FeatureKind::PushFace { face, .. } => face.hint,
        _ => panic!("push_face"),
    };
    let (_, selected, report) = s.repick_push_face(feature, &Sel::Ids(vec![xface])).unwrap();
    assert_eq!(selected, vec![xface]);
    let new_hint = match &s.project().timeline.iter().find(|n| n.id == feature).unwrap().kind {
        FeatureKind::PushFace { face, .. } => face.hint,
        _ => panic!("push_face"),
    };
    assert_ne!(old_hint, new_hint);
    assert_eq!(new_hint.centroid[0], 10.0);
    assert_close(new_hint.centroid[2], 5.0, 1e-12, "x-face centroid z");
    assert_eq!(new_hint.normal, [1.0, 0.0, 0.0]);
    volume(&report, 25.0 * 20.0 * 10.0);
    assert_eq!(s.project().feat_dims[&feature]["dist"], "d");
    volume(&s.param_set("d", &2.0.into()).unwrap(), 22.0 * 20.0 * 10.0);
    let path = scratch("d2-push-repick.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    volume(&report, 22.0 * 20.0 * 10.0);
    let engine = opened.param_set("h", &15.0.into()).unwrap();
    let expected = 22.0 * 20.0 * 15.0;
    volume(&engine, expected);
    assert_close(gui_edit_param(&path, "h", "15"), expected, 1e-6, "app push height edit");
}

type Snapshot = (String, Vec<(Id, Vec<u8>)>, DocInfo);

fn snapshot(s: &Session) -> Snapshot {
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        s.project().bodies.iter().filter_map(|b| s.shape(b.id).map(|shape| (b.id, shape.to_brep_bytes().unwrap()))).collect()
    };
    (ron::ser::to_string(s.project()).unwrap(), breps, s.info())
}

fn unchanged(s: &Session, before: &Snapshot) {
    assert_eq!(ron::ser::to_string(s.project()).unwrap(), before.0);
    assert_eq!(snapshot(s).1, before.1);
    assert_eq!(s.info(), before.2);
}

#[test]
fn push_face_refusals_are_exact_and_use_planar_message() {
    let (mut s, body) = cylinder_stock();
    let cylindrical = s.topology(Some(body), true).unwrap().faces.into_iter().find(|f| f.kind == FaceKind::Cylinder).unwrap().id;
    let error = s.push_face(None, &Sel::Ids(vec![cylindrical]), &1.0.into(), None).unwrap_err().to_string();
    assert!(error.contains(&format!("face {cylindrical} of body {body} is not planar")), "{error}");

    let top = s.topology(Some(body), true).unwrap().faces.into_iter().find(|f| f.normal.is_some()).unwrap().id;
    let (feature, report) = s.push_face(None, &Sel::Ids(vec![top]), &1.0.into(), None).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let before = snapshot(&s);
    let error = s.repick_push_face(feature, &Sel::Ids(vec![cylindrical])).unwrap_err().to_string();
    assert!(error.contains("is not planar"), "{error}");
    unchanged(&s, &before);

    let (mut box_session, box_body) = box_stock();
    let top = face(&mut box_session, box_body, 2, true);
    let xface = face(&mut box_session, box_body, 0, true);
    let (feature, report) = box_session.push_face(None, &Sel::Ids(vec![top]), &"d".into(), None).unwrap();
    volume(&report, 20.0 * 20.0 * 15.0);
    let before = snapshot(&box_session);
    let error = box_session.repick_push_face(feature, &Sel::Ids(vec![top, xface])).unwrap_err().to_string();
    assert!(error.contains("must match exactly one face"), "{error}");
    unchanged(&box_session, &before);
    let error = box_session.repick_push_face(feature, &Sel::Ids(vec![])).unwrap_err().to_string();
    assert!(error.contains("must match exactly one face"), "{error}");
    unchanged(&box_session, &before);
}

#[test]
fn repicks_rebuild_downstream_and_rollback_lost_face_refs() {
    let (mut s, body) = box_stock();
    let top = face(&mut s, body, 2, true);
    let xface = face(&mut s, body, 0, true);
    let (shell, created) = s.shell(None, &Sel::Ids(vec![top]), &1.0.into(), Side::Inward, None).unwrap();
    volume(&created, 4000.0 - 18.0 * 18.0 * 9.0);
    let (downstream, pushed) = s.push_face(Some(shell), &Sel::Ids(vec![xface]), &2.0.into(), Some("downstream")).unwrap();
    volume(&pushed, (20.0 + 2.0) * 20.0 * 10.0 - 18.0 * 18.0 * 9.0);
    let bottom = face(&mut s, body, 2, false);
    let (_, _, rebuilt) = s.repick_shell(shell, &Sel::Ids(vec![top, bottom])).unwrap();
    volume(&rebuilt, 4000.0 - 18.0 * 18.0 * 10.0 + 2.0 * 20.0 * 10.0);
    assert_eq!(rebuilt.bodies[0].id, downstream);

    let (mut failed, body) = box_stock();
    let top = face(&mut failed, body, 2, true);
    let (shell, report) = failed.shell(None, &Sel::Ids(vec![top]), &1.0.into(), Side::Inward, None).unwrap();
    volume(&report, 4000.0 - 18.0 * 18.0 * 9.0);
    let bottom = face(&mut failed, body, 2, false);
    let inner_bottom = failed
        .topology(Some(shell), true)
        .unwrap()
        .faces
        .into_iter()
        .find(|f| f.normal.is_some_and(|n| n[2] > 0.99) && f.centroid[2] > 0.9 && f.centroid[2] < 2.0)
        .unwrap()
        .id;
    let (downstream, report) = failed.push_face(Some(shell), &Sel::Ids(vec![inner_bottom]), &1.0.into(), None).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let before = snapshot(&failed);
    let top_again = face(&mut failed, body, 2, true);
    let result = failed.repick_shell(shell, &Sel::Ids(vec![top_again, bottom]));
    let error = result.unwrap_err().to_string();
    assert!(error.contains("face") || error.contains("Face"), "{error}; downstream {downstream}");
    unchanged(&failed, &before);
}

#[test]
fn foreign_body_face_selections_refuse_with_exact_rollback() {
    let (mut s, body) = box_stock();
    let top = face(&mut s, body, 2, true);
    let other_sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("other")).unwrap();
    s.sketch_circle(other_sketch, &30.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
    let (other, report) = s
        .extrude(&Extrude {
            sketch: other_sketch,
            profiles: None,
            profiles_at: None,
            height: 10.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("other".into()),
        })
        .unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let foreign = s.topology(Some(other), true).unwrap().faces[0].id;
    let (shell, report) = s.shell(Some(body), &Sel::Ids(vec![top]), &1.0.into(), Side::Inward, None).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let before = snapshot(&s);
    let error = s.repick_shell(shell, &Sel::Ids(vec![foreign])).unwrap_err().to_string();
    assert!(error.contains("not on body") || error.contains("source body"), "{error}");
    unchanged(&s, &before);

    let source = s.result_bodies().iter().find(|b| b.id != other).map(|b| b.id).unwrap();
    let source_face = face(&mut s, source, 2, true);
    let (push, report) = s.push_face(Some(source), &Sel::Ids(vec![source_face]), &1.0.into(), None).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let before = snapshot(&s);
    let error = s.repick_push_face(push, &Sel::Ids(vec![foreign])).unwrap_err().to_string();
    assert!(error.contains("not on body") || error.contains("source body"), "{error}");
    unchanged(&s, &before);
}

#[test]
fn wrong_kind_and_unknown_features_refuse() {
    let (mut s, body) = box_stock();
    let top = face(&mut s, body, 2, true);
    let (shell, report) = s.shell(None, &Sel::Ids(vec![top]), &1.0.into(), Side::Inward, None).unwrap();
    volume(&report, 4000.0 - 18.0 * 18.0 * 9.0);
    let before = snapshot(&s);
    assert!(s.repick_push_face(shell, &Sel::Ids(vec![top])).unwrap_err().to_string().contains("not a push_face feature"));
    unchanged(&s, &before);
    assert!(s.repick_shell(u64::MAX, &Sel::Ids(vec![top])).unwrap_err().to_string().contains("not found"));
    unchanged(&s, &before);
    let push_source = s.result_bodies()[0].id;
    let shell_top = face(&mut s, push_source, 2, true);
    let (push, report) = s.push_face(Some(push_source), &Sel::Ids(vec![shell_top]), &1.0.into(), None).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let before = snapshot(&s);
    assert!(s.repick_shell(push, &Sel::Ids(vec![shell_top])).unwrap_err().to_string().contains("not a shell feature"));
    unchanged(&s, &before);
    assert!(s.repick_push_face(u64::MAX, &Sel::Ids(vec![shell_top])).unwrap_err().to_string().contains("not found"));
    unchanged(&s, &before);
}

#[test]
fn empty_shell_repick_rolls_back_exact_state() {
    let (mut s, body) = box_stock();
    let top = face(&mut s, body, 2, true);
    let (shell, report) = s.shell(None, &Sel::Ids(vec![top]), &1.0.into(), Side::Inward, None).unwrap();
    volume(&report, 4000.0 - 18.0 * 18.0 * 9.0);
    let before = snapshot(&s);
    let error = s.repick_shell(shell, &Sel::Ids(vec![])).unwrap_err().to_string();
    assert!(error.contains("matched no face"), "{error}");
    unchanged(&s, &before);
}
