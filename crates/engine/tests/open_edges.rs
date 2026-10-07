//! Opening and later edits cannot rebuild stored edge queries over unnamed live edges.
mod common;
use common::*;
use qymcad_core::refs::{Query, Ref};
use qymcad_engine::*;

fn unnamed_box(path: &std::path::Path, dirty: bool, missing: bool) -> Id {
    edge_query_box(path, dirty, missing, false).0
}

fn edge_query_box(path: &std::path::Path, dirty: bool, missing: bool, named: bool) -> (Id, Id) {
    edge_query_box_with_array(path, dirty, missing, named, false)
}

fn edge_query_box_with_array(path: &std::path::Path, dirty: bool, missing: bool, named: bool, array: bool) -> (Id, Id) {
    let mut s = Session::new_part();
    s.param_set("r", &2.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let (body, _) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: 20.0.into(),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("unnamed box".into()),
        })
        .unwrap();
    if array {
        s.linear_array(None, &ArrayDir { dx: 30.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: 2.0.into() }, None, None).unwrap();
    }
    let (fillet, _) = s.fillet(None, &Sel::Along { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }, &Num::Expr("r".into()), None).unwrap();
    // Four vertical corner cuts replace squares r² by quarter circles πr²/4 over height h.
    let expected = 20.0_f64.powi(3) - 4.0 * (1.0 - std::f64::consts::PI / 4.0) * 2.0_f64.powi(2) * 20.0;
    assert_close(s.shape(fillet).unwrap().volume(), expected * if array { 2.0 } else { 1.0 }, 1e-6, "vertical-edge fillet fixture");
    s.save(Some(path)).unwrap();
    let qymcad_io::LoadedProject { mut project, mut breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let node = project.timeline.iter_mut().find(|n| n.id == fillet).unwrap();
    let qymcad_core::feature::FeatureKind::Fillet { edges, .. } = &mut node.kind else { panic!("fillet") };
    *edges = Ref::many(Query::Oriented { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 });
    node.dirty = dirty;
    if !named {
        let _gate = qymcad_kernel::kernel_gate();
        let bytes = &mut breps.iter_mut().find(|(id, _)| *id == body).unwrap().1;
        let shape = qymcad_kernel::Shape::from_brep_bytes(bytes).unwrap();
        shape.rename_edges(&shape.edges_info().iter().map(|e| (e.id, 0)).collect::<Vec<_>>());
        assert!(shape.edges_info().iter().all(|e| e.id == 0));
        *bytes = shape.to_brep_bytes().unwrap();
    }
    if missing {
        breps.retain(|(id, _)| *id != fillet);
    }
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    (body, fillet)
}

fn brep_bytes(s: &Session) -> Vec<(Id, Vec<u8>)> {
    let _gate = qymcad_kernel::kernel_gate();
    s.project()
        .timeline
        .iter()
        .flat_map(|n| n.kind.bodies())
        .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
        .collect()
}

#[test]
fn clean_open_refuses_radius_edit_over_unnamed_edges_without_changing_state() {
    let path = scratch("unnamed_clean_radius_edit.qcad");
    let body = unnamed_box(&path, false, false);
    let (mut opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    let before = serde_json::to_vec(opened.project()).unwrap();
    let shapes = brep_bytes(&opened);
    let error = match opened.param_set("r", &3.0.into()) {
        Err(error) => error,
        Ok(_) => panic!("radius edit accepted a stored edge query over unnamed body {body}"),
    };
    assert!(error.to_string().contains(&format!("body {body}")), "{error}");
    assert!(error.to_string().contains("stored edge query"), "{error}");
    assert!(serde_json::to_vec(opened.project()).unwrap() == before, "radius refusal changed Project bytes");
    assert!(brep_bytes(&opened) == shapes, "radius refusal changed live B-rep bytes");
    assert!(opened.rebuild().errors.is_empty(), "a later clean rebuild remains usable");
    assert!(brep_bytes(&opened) == shapes);
}

#[test]
fn clean_open_radius_edit_with_named_edges_rounds_only_vertical_corners() {
    let path = scratch("named_clean_radius_edit.qcad");
    let (_, fillet) = edge_query_box(&path, false, false, true);
    let (mut opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    let report = opened.param_set("r", &3.0.into()).unwrap();
    assert!(report.errors.is_empty());
    // Each of four vertical corners loses (r² - πr²/4)*height; top and bottom edges stay sharp.
    let expected = 20.0_f64.powi(3) - 4.0 * (1.0 - std::f64::consts::PI / 4.0) * 3.0_f64.powi(2) * 20.0;
    assert_close(opened.shape(fillet).unwrap().volume(), expected, 1e-6, "named-query radius edit");
}

#[test]
fn clean_open_refuses_sketch_edit_over_unnamed_edges_without_changing_state() {
    let path = scratch("unnamed_clean_sketch_edit.qcad");
    let body = unnamed_box(&path, false, false);
    let (mut opened, _) = Session::open(&path).unwrap();
    let sketch = opened.sketches()[0].id;
    let before = serde_json::to_vec(opened.project()).unwrap();
    let shapes = brep_bytes(&opened);
    let result = opened.sketch_edit(sketch, |s| s.sketch_circle(sketch, &0.0.into(), &0.0.into(), &4.0.into(), true));
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("sketch edit accepted a stored edge query over unnamed body {body}"),
    };
    assert!(error.to_string().contains(&format!("body {body}")), "{error}");
    assert!(serde_json::to_vec(opened.project()).unwrap() == before, "sketch refusal changed Project bytes");
    assert!(brep_bytes(&opened) == shapes, "sketch refusal changed live B-rep bytes");
}

#[test]
fn clean_open_refuses_atomic_dirty_baseline_over_unnamed_edges_without_changing_state() {
    let path = scratch("unnamed_clean_atomic_baseline.qcad");
    let body = unnamed_box(&path, false, false);
    let (mut opened, _) = Session::open(&path).unwrap();
    let sketch = opened.sketches()[0].id;
    // Low-level sketch geometry leaves a pending edit. atomic must refuse before creating a datum node.
    opened.sketch_circle(sketch, &0.0.into(), &0.0.into(), &4.0.into(), true).unwrap();
    let before = serde_json::to_vec(opened.project()).unwrap();
    let shapes = brep_bytes(&opened);
    let error = match opened.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), None) {
        Err(error) => error,
        Ok(_) => panic!("atomic baseline accepted a stored edge query over unnamed body {body}"),
    };
    assert!(error.to_string().contains(&format!("body {body}")), "{error}");
    assert!(serde_json::to_vec(opened.project()).unwrap() == before, "atomic refusal changed Project bytes");
    assert!(brep_bytes(&opened) == shapes, "atomic refusal changed live B-rep bytes");
}

#[test]
fn clean_open_refuses_query_with_transitive_unnamed_ancestor() {
    let path = scratch("unnamed_clean_transitive_query.qcad");
    let (body, fillet) = edge_query_box_with_array(&path, false, false, false, true);
    let (mut opened, _) = Session::open(&path).unwrap();
    let qymcad_core::feature::FeatureKind::Fillet { src, .. } = opened.project().timeline.iter().find(|n| n.id == fillet).unwrap().kind
    else {
        panic!("fillet")
    };
    assert_ne!(src, body, "the query reads an array body, not the unnamed extrusion");
    let _gate = qymcad_kernel::kernel_gate();
    assert!(opened.shape(src).unwrap().edges_info().iter().any(|e| e.id != 0 && e.poly.len() >= 2), "immediate source has named edges");
    drop(_gate);
    let before = serde_json::to_vec(opened.project()).unwrap();
    let shapes = brep_bytes(&opened);
    let error = match opened.param_set("r", &3.0.into()) {
        Err(error) => error,
        Ok(_) => panic!("radius edit accepted a stored edge query with unnamed ancestor body {body}"),
    };
    assert!(error.to_string().contains(&format!("body {body}")), "{error}");
    assert!(serde_json::to_vec(opened.project()).unwrap() == before);
    assert!(brep_bytes(&opened) == shapes);
}

#[test]
fn clean_open_with_unnamed_edges_allows_an_unrelated_feature() {
    let path = scratch("unnamed_clean_unrelated_feature.qcad");
    unnamed_box(&path, false, false);
    let (mut opened, _) = Session::open(&path).unwrap();
    let shapes = brep_bytes(&opened);
    let (_, report) = opened.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), None).unwrap();
    assert!(report.errors.is_empty());
    assert!(brep_bytes(&opened) == shapes, "unrelated feature preserves live B-reps");
}

#[test]
fn unsafe_full_retry_restores_the_first_pass_project_and_shapes() {
    let path = scratch("unnamed_clean_unsafe_retry.qcad");
    let body = unnamed_box(&path, false, false);
    let (mut opened, _) = Session::open(&path).unwrap();
    let sketch = opened.sketch_create(&PlaneRef::Base(BaseName::XY), Some("independent box")).unwrap();
    let lines = opened.sketch_rect(sketch, &40.0.into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    opened
        .extrude(&Extrude {
            sketch,
            profiles: None,
            height: 10.0.into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    // The first pass touches only an independent box with a broken contour. Retrying the whole timeline
    // would reach the original unnamed box's query; refusal must also undo the safe first pass.
    opened.sketch_remove_entity(sketch, lines[0]).unwrap();
    let before = serde_json::to_vec(opened.project()).unwrap();
    let shapes = brep_bytes(&opened);
    let report = opened.rebuild();
    assert!(report.errors.iter().any(|e| e.message.contains(&format!("body {body}")) && e.message.contains("stored edge query")));
    assert!(serde_json::to_vec(opened.project()).unwrap() == before, "unsafe retry refusal changed first-pass Project bytes");
    assert!(brep_bytes(&opened) == shapes, "unsafe retry refusal changed first-pass B-rep bytes");
}

#[test]
fn open_refuses_unnamed_edges_before_a_dirty_stored_query_rebuild() {
    for (dirty, missing) in [(true, false), (false, true)] {
        let path = scratch(&format!("unnamed_dirty_{dirty}_missing_{missing}.qcad"));
        let body = unnamed_box(&path, dirty, missing);
        let before = std::fs::read(&path).unwrap();
        let result = Session::open(&path);
        assert!(std::fs::read(&path).unwrap() == before, "open must leave the file byte-identical");
        let error = match result {
            Err(e) => e,
            Ok(_) => panic!("open accepted unnamed source edges before a stored-query rebuild"),
        };
        assert!(error.to_string().contains(&format!("body {body}")), "{error}");
        assert!(error.to_string().contains("open") && error.to_string().contains("rebuild"), "{error}");
    }
}

#[test]
fn open_keeps_a_clean_document_with_unnamed_edges_usable() {
    let path = scratch("unnamed_clean.qcad");
    let body = unnamed_box(&path, false, false);
    let before = std::fs::read(&path).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    assert!(!opened.project().timeline.iter().any(|n| n.dirty));
    let error = opened.topology(None, true).unwrap_err();
    assert!(error.to_string().contains(&format!("body {body} has live edges but no usable named edges")), "{error}");
    assert!(std::fs::read(&path).unwrap() == before, "clean open and topology refusal do not write the file");
}
