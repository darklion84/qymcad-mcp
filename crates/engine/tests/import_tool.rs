mod common;
use qymcad_engine::*;
use qymcad_kernel::{ExactFormat, LengthUnit};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn box_body(session: &mut Session, center: [f64; 2], size: [f64; 3]) -> Id {
    let sketch = session.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    session.sketch_rect(sketch, &center[0].into(), &center[1].into(), &size[0].into(), &size[1].into(), false).unwrap();
    let (body, report) = session
        .extrude(&Extrude {
            sketch,
            profiles: None,
            height: size[2].into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    body
}

fn fixture(format: ExactFormat) -> (PathBuf, PathBuf) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("qymcad-import-tool-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    let extension = match format {
        ExactFormat::Step => "step",
        ExactFormat::Iges => "iges",
    };
    let path = dir.join(format!("fixture.{extension}"));
    let mut maker = Session::new_part();
    let body = box_body(&mut maker, [5.0, 5.0], [10.0, 10.0, 10.0]);
    match format {
        ExactFormat::Step => {
            maker.export(ExportFormat::Step, &path, Quality::Standard, None).unwrap();
        }
        // The server has no IGES export tool. Use the app's native writer on the server-built box.
        ExactFormat::Iges => {
            let _gate = qymcad_kernel::kernel_gate();
            qymcad_kernel::write_iges(
                &[(maker.shape(body).unwrap(), qymcad_core::feature::PLACE_IDENTITY)],
                path.to_str().unwrap(),
                LengthUnit::Millimetre,
            )
            .unwrap();
        }
    }
    (dir, path)
}

#[test]
fn step_and_iges_import_preserve_geometry_and_embedded_bytes() {
    for format in [ExactFormat::Step, ExactFormat::Iges] {
        let (dir, path) = fixture(format);
        let bytes = std::fs::read(&path).unwrap();
        let mut session = Session::new_part();
        let result = session.import(&path, 1.0, None, Some("Imported")).unwrap();
        assert_eq!(result.bodies.len(), 1);
        let body = &result.bodies[0];
        assert_eq!(body.name, "Imported");
        assert_eq!(body.solid, 0);
        assert!((body.volume - 1000.0).abs() < 1.0);
        for (actual, expected) in body.bbox.into_iter().zip([0.0, 0.0, 0.0, 10.0, 10.0, 10.0]) {
            assert!((actual - expected).abs() < 0.05, "{actual} != {expected}");
        }
        assert_eq!(session.project().sources.iter().find(|source| source.id == result.source).unwrap().data, bytes);
        assert!(session.shape(body.body).is_some());
        let topology = session.topology(Some(body.body), true).unwrap();
        assert!(!topology.faces.is_empty() && !topology.edges.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn persistent_ids(session: &mut Session, body: Id) -> (Vec<u32>, Vec<u32>) {
    let topology = session.topology(Some(body), true).unwrap();
    let mut faces: Vec<_> = topology.faces.iter().map(|face| face.id).collect();
    let mut edges: Vec<_> = topology.edges.iter().map(|edge| edge.id).collect();
    faces.sort_unstable();
    edges.sort_unstable();
    (faces, edges)
}

fn saved_import_restores_ids(format: ExactFormat) {
    let (dir, source) = fixture(format);
    let mut session = Session::new_part();
    let result = session.import(&source, 1.0, None, Some("Persistent cube")).unwrap();
    let body = result.bodies[0].body;
    let expected = persistent_ids(&mut session, body);
    assert_eq!((expected.0.len(), expected.1.len()), (6, 12));
    assert!(expected.0.iter().chain(&expected.1).all(|id| qymcad_core::names::NameTable::is_named(*id)));
    let saved = dir.join("stored.qcad");
    session.save(Some(&saved)).unwrap();
    let loaded = qymcad_io::load_project_with_brep(saved.to_str().unwrap()).unwrap();
    assert!(loaded.breps.iter().any(|(id, _)| *id == body));
    // Also exercise F-106: force restoration from embedded bytes, not a cached B-rep.
    let source_only = dir.join("source-only.qcad");
    qymcad_io::save_project_guarded_with_brep(&loaded.project, source_only.to_str().unwrap(), &[]).unwrap();
    assert!(qymcad_io::load_project_with_brep(source_only.to_str().unwrap()).unwrap().breps.is_empty());
    std::fs::remove_file(&source).unwrap();
    assert!(!source.exists());
    drop(session);
    for saved in [&saved, &source_only] {
        let (mut opened, report) = Session::open(saved).unwrap();
        assert!(report.errors.is_empty() && report.warnings.is_empty(), "{format:?} {saved:?}: {report:?}");
        common::assert_close(opened.shape(body).expect("restored exact solid").volume(), 10.0 * 10.0 * 10.0, 1e-4, "restored volume");
        assert_eq!(persistent_ids(&mut opened, body), expected, "{format:?} {saved:?} persistent names");
        for (element, ids) in [(Element::Faces, &expected.0), (Element::Edges, &expected.1)] {
            for id in ids {
                assert_eq!(
                    opened.select(Some(body), element, &Sel::Ids(vec![*id])).unwrap(),
                    (body, vec![*id]),
                    "{format:?} saved {element:?} selection"
                );
            }
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn saved_step_restores_volume_persistent_ids_and_selections_without_external_source() {
    saved_import_restores_ids(ExactFormat::Step);
}

#[test]
fn saved_iges_restores_volume_persistent_ids_and_selections_without_external_source() {
    saved_import_restores_ids(ExactFormat::Iges);
}

#[test]
fn import_scale_multiplies_bbox_and_cubically_scales_volume() {
    let (dir, path) = fixture(ExactFormat::Step);
    let mut session = Session::new_part();
    let imported = session.import(&path, 2.0, None, None).unwrap();
    assert!((imported.bodies[0].volume - 8000.0).abs() < 1.0);
    for (actual, expected) in imported.bodies[0].bbox.into_iter().zip([0.0, 0.0, 0.0, 20.0, 20.0, 20.0]) {
        assert!((actual - expected).abs() < 0.05, "{actual} != {expected}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn selected_source_index_is_preserved_and_bad_index_is_atomic() {
    let (dir, path) = fixture(ExactFormat::Step);
    let mut session = Session::new_part();
    let before = serde_json::to_vec(session.project()).unwrap();
    assert!(session.import(&path, 1.0, Some(&[7]), None).is_err());
    assert_eq!(serde_json::to_vec(session.project()).unwrap(), before);
    assert!(session.project().sources.is_empty());
    let imported = session.import(&path, 1.0, Some(&[0]), None).unwrap();
    assert_eq!(imported.bodies[0].solid, 0);
    assert_eq!(imported.bodies.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn importing_multiple_solids_preserves_names_indices_and_selection() {
    let (dir, path) = fixture(ExactFormat::Step);
    let mut maker = Session::new_part();
    box_body(&mut maker, [5.0, 5.0], [10.0, 10.0, 10.0]);
    box_body(&mut maker, [25.0, 5.0], [4.0, 6.0, 8.0]);
    maker.export(ExportFormat::Step, &path, Quality::Standard, None).unwrap();
    let mut session = Session::new_part();
    let all = session.import(&path, 1.0, None, None).unwrap();
    assert_eq!(all.bodies.len(), 2);
    assert_eq!(all.bodies.iter().map(|body| body.solid).collect::<Vec<_>>(), vec![0, 1]);
    assert_eq!(all.bodies[0].name, "fixture 1");
    assert_eq!(all.bodies[1].name, "fixture 2");
    let mut selected_session = Session::new_part();
    let selected = selected_session.import(&path, 1.0, Some(&[1]), None).unwrap();
    assert_eq!(selected.bodies.len(), 1);
    assert_eq!(selected.bodies[0].solid, 1);
    assert_eq!(selected.bodies[0].name, "fixture 2");
    for (body, expected) in
        [(&all.bodies[0], 10.0 * 10.0 * 10.0), (&all.bodies[1], 4.0 * 6.0 * 8.0), (&selected.bodies[0], 4.0 * 6.0 * 8.0)]
    {
        common::assert_close(body.volume, expected, 1e-4, "source solid volume");
    }
    for (actual, expected) in selected.bodies[0].bbox.into_iter().zip([23.0, 2.0, 0.0, 27.0, 8.0, 8.0]) {
        common::assert_close(actual, expected, 1e-4, "selected solid bounds");
    }
    assert_eq!(selected_session.info().bodies.len(), 1);
    for (doc, bodies) in [(&session, &all.bodies), (&selected_session, &selected.bodies)] {
        for body in bodies {
            let node = doc.project().timeline.iter().find(|node| node.id == body.body).unwrap();
            assert!(
                matches!(node.kind, qymcad_core::feature::FeatureKind::Import { solid, .. } if solid == body.solid),
                "native source index differs from the reported source index"
            );
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn invalid_import_preflight_changes_neither_document_nor_history() {
    let (dir, path) = fixture(ExactFormat::Step);
    let mut session = Session::new_part();
    let before = serde_json::to_vec(session.project()).unwrap();
    for scale in [0.0, -1.0, f64::INFINITY] {
        assert!(session.import(&path, scale, None, None).is_err());
        assert_eq!(serde_json::to_vec(session.project()).unwrap(), before);
        assert!(session.project().sources.is_empty());
    }
    let bad = dir.join("bad.stl");
    std::fs::write(&bad, b"not cad").unwrap();
    assert!(session.import(&bad, 1.0, None, None).is_err());
    assert_eq!(serde_json::to_vec(session.project()).unwrap(), before);
    assert!(session.project().sources.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

fn imported_modelling(operation: &str) {
    for format in [ExactFormat::Step, ExactFormat::Iges] {
        let (dir, source) = fixture(format);
        let mut session = Session::new_part();
        let imported = session.import(&source, 1.0, None, None).unwrap().bodies[0].body;
        let active_before = session.project().active_component;
        let topology = session.topology(Some(imported), true).unwrap();
        let top = topology.faces.iter().find(|face| face.normal.is_some_and(|n| n[2] > 0.999)).unwrap().id;
        let (body, report, expected) = match operation {
            "push" => {
                let (body, report) = session.push_face(Some(imported), &Sel::Ids(vec![top]), &2.0.into(), None).unwrap();
                // 10×10 top moves outward by 2 mm: +200 mm³.
                (body, report, 10.0 * 10.0 * (10.0 + 2.0))
            }
            "fillet" => {
                let edge = topology.edges.iter().find(|edge| (edge.a[2] - edge.b[2]).abs() > 9.99).unwrap().id;
                let (body, report) = session.fillet(Some(imported), &Sel::Ids(vec![edge]), &1.0.into(), None).unwrap();
                // One vertical R1: remove (square - quarter disk) × edge length.
                (body, report, 1000.0 - (1.0 - std::f64::consts::PI / 4.0) * 1.0_f64.powi(2) * 10.0)
            }
            "cut" => {
                let sketch = session.sketch_create(&PlaneRef::Face { body: imported, face: top }, None).unwrap();
                let frame = session.sketch_info(sketch).unwrap().world_frame.unwrap();
                // Centre at world (5,5,10), independent of the native face frame's origin/rotation.
                let delta = [5.0 - frame.origin[0], 5.0 - frame.origin[1], 10.0 - frame.origin[2]];
                let x: f64 = delta.iter().zip(frame.x_axis).map(|(d, a)| d * a).sum();
                let y: f64 = delta.iter().zip(frame.y_axis).map(|(d, a)| d * a).sum();
                session.sketch_rect(sketch, &x.into(), &y.into(), &2.0.into(), &3.0.into(), false).unwrap();
                let (body, report) = session
                    .extrude(&Extrude {
                        sketch,
                        profiles: None,
                        height: 4.0.into(),
                        op: Op::Cut,
                        direction: Direction::Reverse,
                        through: false,
                        target: Some(imported),
                        name: None,
                    })
                    .unwrap();
                (body, report, 10.0 * 10.0 * 10.0 - 2.0 * 3.0 * 4.0)
            }
            _ => unreachable!(),
        };
        assert!(report.errors.is_empty(), "{format:?} {operation}: {report:?}");
        assert_eq!(session.project().active_component, active_before, "explicit edits preserve the active part");
        assert_eq!(session.project().body_owner(body), session.project().body_owner(imported), "result belongs to the imported part");
        common::assert_close(session.shape(body).unwrap().volume(), expected, 1e-4, operation);
        assert_eq!(session.result_bodies().iter().map(|b| b.id).collect::<Vec<_>>(), vec![body]);
        let saved = dir.join("modelled.qcad");
        session.save(Some(&saved)).unwrap();
        // Drop all exact caches: reopening must actually rebuild the downstream feature from the source.
        let project = qymcad_io::load_project(saved.to_str().unwrap()).unwrap();
        qymcad_io::save_project_guarded_with_brep(&project, saved.to_str().unwrap(), &[]).unwrap();
        std::fs::remove_file(source).unwrap();
        drop(session);
        let (opened, report) = Session::open(&saved).unwrap();
        assert!(report.errors.is_empty(), "{format:?} {operation} reopen: {report:?}");
        assert_eq!(report.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![body]);
        common::assert_close(opened.shape(body).expect("rebuilt feature").volume(), expected, 1e-4, "rebuilt imported-body feature");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn imported_body_push_face_changes_volume_and_rebuilds_after_open() {
    imported_modelling("push");
}
#[test]
fn imported_body_single_edge_fillet_changes_volume_and_rebuilds_after_open() {
    imported_modelling("fillet");
}
#[test]
fn imported_body_face_sketch_cut_changes_volume_and_rebuilds_after_open() {
    imported_modelling("cut");
}
