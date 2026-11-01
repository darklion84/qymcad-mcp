mod common;
use common::*;
use qymcad_engine::*;
use std::f64::consts::PI;

fn stock(height: Num) -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("height", &height).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("stock sketch")).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &40.0.into(), &40.0.into(), false).unwrap();
    let (body, report) = s
        .extrude(&Extrude {
            down: None,
            sketch,
            profiles: None,
            profiles_at: None,
            height: Num::Expr("height".into()),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("stock".into()),
        })
        .unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    (s, body)
}

fn blind_hole(mut s: Session, body: Id) -> Session {
    let top = s.topology(Some(body), false).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    s.hole(&Hole {
        body: Some(body),
        face: Sel::Ids(vec![top]),
        at: None,
        offset: None,
        diameter: 10.0.into(),
        depth: Some(8.0.into()),
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: Some("hole".into()),
    })
    .unwrap();
    s
}

fn hole_faces(s: &mut Session) -> (u32, u32) {
    let t = s.topology(None, false).unwrap();
    let wall = t.faces.iter().find(|f| f.kind == FaceKind::Cylinder && (f.radius.unwrap() - 5.0).abs() < 1e-6).unwrap().id;
    let bottom = t.faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99) && (f.centroid[2] - 12.0).abs() < 1e-6).unwrap().id;
    (wall, bottom)
}

fn body_breps(s: &Session) -> Vec<(Id, Vec<u8>)> {
    s.project()
        .timeline
        .iter()
        .flat_map(|node| node.kind.bodies())
        .filter_map(|id| s.shape(id).map(|shape| (id, shape.to_brep_bytes().unwrap())))
        .collect()
}

#[test]
fn remove_face_heals_a_blind_hole_to_the_plain_box() {
    let (s, body) = stock(20.0.into());
    let mut s = blind_hole(s, body);
    let (wall, bottom) = hole_faces(&mut s);
    let (feature, selected, report) = s.remove_face(None, &Sel::Ids(vec![wall, bottom]), Some("healed")).unwrap();
    assert_eq!(selected, vec![wall, bottom]);
    assert_eq!(feature, report.bodies[0].id);
    assert_close(report.bodies[0].volume, 40.0 * 40.0 * 20.0, 1e-6, "40*40*20");
    assert_eq!(s.topology(Some(feature), false).unwrap().faces.len(), 6);
    assert_eq!(s.shape(feature).unwrap().solid_count(), 1);
    assert!(report.errors.is_empty(), "{report:?}");
}

#[test]
fn remove_face_wall_only_reports_native_failure_and_rolls_back() {
    let (s, body) = stock(20.0.into());
    let mut s = blind_hole(s, body);
    let (wall, _) = hole_faces(&mut s);
    let before = serde_json::to_vec(s.project()).unwrap();
    let volume = s.result_bodies()[0].volume;
    let error = s.remove_face(None, &Sel::Ids(vec![wall]), None).unwrap_err().to_string();
    assert!(error.contains("cad-neighbours-not-extendable"), "{error}");
    assert_eq!(serde_json::to_vec(s.project()).unwrap(), before);
    assert_close(s.result_bodies()[0].volume, volume, 1e-9, "rollback volume");
}

#[test]
fn remove_face_lost_upstream_face_is_a_clear_error_with_exact_rollback() {
    let assert_rollback = |s: &Session, before_project: &[u8], before_breps: &[(Id, Vec<u8>)], before_timeline: &[_], error: String| {
        assert!(error.contains("healed"), "{error}");
        assert_eq!(serde_json::to_vec(s.project()).unwrap(), before_project);
        assert_eq!(body_breps(s), before_breps);
        assert_close(s.result_bodies()[0].volume, 40.0 * 40.0 * 20.0, 1e-9, "rollback volume");
        assert_eq!(s.info().timeline, before_timeline);
    };

    let (s, body) = stock(20.0.into());
    let mut s = blind_hole(s, body);
    let (wall, bottom) = hole_faces(&mut s);
    let (_, _, _) = s.remove_face(None, &Sel::Ids(vec![wall, bottom]), Some("healed")).unwrap();
    let hole = s.project().timeline.iter().find(|node| node.name == "hole").unwrap().id;
    let before_project = serde_json::to_vec(s.project()).unwrap();
    let before_breps = body_breps(&s);
    let before_timeline = s.info().timeline;
    let error = s.feature_delete(hole, false).unwrap_err().to_string();
    assert_rollback(&s, &before_project, &before_breps, &before_timeline, error);

    for (name, value) in [("diameter", 12.0), ("depth", 4.0)] {
        let (s, body) = stock(20.0.into());
        let mut s = blind_hole(s, body);
        let (wall, bottom) = hole_faces(&mut s);
        let (remove_face, _, _) = s.remove_face(None, &Sel::Ids(vec![wall, bottom]), Some("healed")).unwrap();
        let before_project = serde_json::to_vec(s.project()).unwrap();
        let before_breps = body_breps(&s);
        let before_timeline = s.info().timeline;
        let result = s.param_set(name, &value.into());
        match result {
            Ok(report) => {
                assert!(report.errors.is_empty(), "{report:?}");
                assert_close(report.bodies[0].volume, 40.0 * 40.0 * 20.0, 1e-6, "40*40*20 after witness re-resolution");
                assert_eq!(s.topology(Some(remove_face), false).unwrap().faces.len(), 6);
                assert_eq!(s.shape(remove_face).unwrap().solid_count(), 1);
            }
            Err(error) => assert_rollback(&s, &before_project, &before_breps, &before_timeline, error.to_string()),
        }
    }
}

#[test]
fn remove_face_removes_a_rectangular_boss() {
    let (mut s, body) = stock(20.0.into());
    let top = s.topology(Some(body), false).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    let boss_sketch = s.sketch_create(&PlaneRef::Face { body, face: top }, None).unwrap();
    s.sketch_rect(boss_sketch, &0.0.into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    let boss = s
        .extrude(&Extrude {
            down: None,
            sketch: boss_sketch,
            profiles: None,
            profiles_at: None,
            height: 5.0.into(),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("boss".into()),
        })
        .unwrap()
        .0;
    let topo = s.topology(Some(boss), false).unwrap();
    let selected: Vec<_> = topo.faces.iter().filter(|f| f.centroid[2] > 20.0).map(|f| f.id).collect();
    assert_eq!(selected.len(), 5);
    let (_, _, report) = s.remove_face(None, &Sel::Ids(selected), None).unwrap();
    assert_close(report.bodies[0].volume, 40.0 * 40.0 * 20.0, 1e-6, "40*40*20 after boss removal");
    assert_eq!(s.topology(None, false).unwrap().faces.len(), 6);
    assert_eq!(s.shape(report.bodies[0].id).unwrap().solid_count(), 1);
    assert!(report.errors.is_empty(), "{report:?}");
}

#[test]
fn remove_face_repick_replaces_the_whole_set_and_undo_restores_it() {
    let (s, body) = stock(20.0.into());
    let mut s = blind_hole(s, body);
    let top = s.topology(None, false).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    s.hole(&Hole {
        body: None,
        face: Sel::Ids(vec![top]),
        at: Some([-10.0, 0.0, 20.0]),
        offset: None,
        diameter: 10.0.into(),
        depth: Some(8.0.into()),
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: Some("hole-a".into()),
    })
    .unwrap();
    let top = s.topology(None, false).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    let second_hole = s
        .hole(&Hole {
            body: None,
            face: Sel::Ids(vec![top]),
            at: Some([10.0, 0.0, 20.0]),
            offset: None,
            diameter: 10.0.into(),
            depth: Some(8.0.into()),
            kind: HoleKind::Plain,
            dia2: None,
            depth2: None,
            name: Some("hole-b".into()),
        })
        .unwrap()
        .0;
    let t = s.topology(None, false).unwrap();
    let a_bottom = t
        .faces
        .iter()
        .find(|f| f.normal.is_some_and(|n| n[2] > 0.99) && f.centroid[0] < -5.0 && (f.centroid[2] - 12.0).abs() < 1e-6)
        .unwrap()
        .id;
    // Hole A's wall is split by its seam into two cylinder faces; a heal needs both halves plus the bottom.
    let a_wall: Vec<_> = t.faces.iter().filter(|f| f.kind == FaceKind::Cylinder && f.centroid[0] < -5.0).map(|f| f.id).collect();
    assert_eq!(a_wall.len(), 2, "hole A wall halves");
    let (feature, _, _) = s.remove_face(None, &Sel::Ids(vec![a_bottom]), Some("repick")).unwrap();
    let before = s.result_bodies()[0].volume;
    let mut history = s.begin_tool_edit().unwrap();
    history.record_call("remove_face", serde_json::json!({"feature": feature}));
    let t = s.topology(None, false).unwrap();
    let b_wall = t.faces.iter().find(|f| f.kind == FaceKind::Cylinder && f.centroid[0] > 5.0).unwrap().id;
    let b_bottom = t
        .faces
        .iter()
        .find(|f| f.normal.is_some_and(|n| n[2] > 0.99) && f.centroid[0] > 5.0 && (f.centroid[2] - 12.0).abs() < 1e-6)
        .unwrap()
        .id;
    let (same, selected, report) = s.repick_remove_face(feature, &Sel::Ids(vec![b_wall, b_bottom])).unwrap();
    assert_eq!(same, feature);
    assert_eq!(selected, vec![b_wall, b_bottom]);
    assert_close(
        report.bodies[0].volume,
        40.0 * 40.0 * 20.0 - 2.0 * std::f64::consts::PI * 25.0 * 8.0,
        1e-3,
        "two blind-hole volumes remain after replacing the selected set",
    );
    assert!(report.errors.is_empty(), "{report:?}");
    assert!(before != report.bodies[0].volume);
    assert_eq!(second_hole, s.project().timeline.iter().find(|n| n.name == "hole-b").unwrap().id);
    // Re-pick ids are faces of the SOURCE body: hole A's bottom exists there but not in the current result,
    // where the first pick healed it away. Healing A instead of B leaves the centre hole and B.
    let mut a_set: Vec<_> = a_wall.iter().copied().chain([a_bottom]).collect();
    a_set.sort_unstable();
    let (_, selected, report) = s.repick_remove_face(feature, &Sel::Ids(a_set.clone())).unwrap();
    assert_eq!(selected, a_set);
    assert_close(
        report.bodies[0].volume,
        40.0 * 40.0 * 20.0 - 2.0 * std::f64::consts::PI * 25.0 * 8.0,
        1e-3,
        "healing hole A by source-body ids leaves the centre hole and hole B",
    );
    s.finish_tool_edit(history, true);
    s.undo().unwrap();
    assert_close(s.result_bodies()[0].volume, before, 1e-6, "undo restores previous re-pick volume");
}

#[test]
fn remove_face_imported_step_body_survives_save_and_reopen() {
    let source = scratch("remove_face_source.step");
    let (mut maker, body) = stock(20.0.into());
    let top = maker.topology(Some(body), false).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    let boss_sketch = maker.sketch_create(&PlaneRef::Face { body, face: top }, None).unwrap();
    maker.sketch_rect(boss_sketch, &0.0.into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    let _boss = maker
        .extrude(&Extrude {
            down: None,
            sketch: boss_sketch,
            profiles: None,
            profiles_at: None,
            height: 5.0.into(),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("boss".into()),
        })
        .unwrap()
        .0;
    maker.export(ExportFormat::Step, &source, Quality::Standard, None).unwrap();
    let mut s = Session::new_part();
    let imported = s.import(&source, 1.0, None, Some("imported")).unwrap().bodies[0].body;
    let imported_topology = s.topology(Some(imported), false).unwrap();
    let selected_faces: Vec<_> = imported_topology.faces.iter().filter(|f| f.centroid[2] > 20.0).map(|f| f.id).collect();
    assert_eq!(selected_faces.len(), 5);
    let (feature, selected, report) =
        s.remove_face(Some(imported), &Sel::Ids(selected_faces.clone()), Some("removed imported boss")).unwrap();
    assert_eq!(selected, selected_faces);
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(s.shape(feature).unwrap().solid_count(), 1);
    assert_eq!(s.topology(Some(feature), false).unwrap().faces.len(), 6);
    let path = scratch("remove_face_imported.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(opened.shape(feature).unwrap().solid_count(), 1);
    assert_eq!(opened.topology(Some(feature), false).unwrap().faces.len(), 6);
    let _ = std::fs::remove_file(source);
}

#[test]
fn remove_face_app_path_height_edit_is_the_full_box_volume() {
    let (s, body) = stock(20.0.into());
    let mut s = blind_hole(s, body);
    let (wall, bottom) = hole_faces(&mut s);
    let (_, _, _) = s.remove_face(None, &Sel::Ids(vec![wall, bottom]), Some("healed")).unwrap();
    let path = scratch("remove_face_app_path.qcad");
    s.save(Some(&path)).unwrap();
    let (volume, _, faces, _) = gui_edit_param_model(&path, "height", "25");
    assert_close(volume, 40.0 * 40.0 * 25.0, 1e-6, "40*40*25 app path");
    assert_eq!(faces.len(), 6);
    s.param_set("height", &25.0.into()).unwrap();
    assert_close(s.result_bodies()[0].volume, 40.0 * 40.0 * 25.0, 1e-6, "40*40*25 engine path");
    assert_eq!(PI * 5.0 * 5.0 * 8.0, 200.0 * PI);
}
