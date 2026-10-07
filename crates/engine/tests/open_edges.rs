//! Opening cannot rebuild stored edge queries when their source has unnamed live edges.
mod common;
use common::*;
use qymcad_core::refs::{Query, Ref};
use qymcad_engine::*;

fn unnamed_box(path: &std::path::Path, dirty: bool, missing: bool) -> Id {
    let mut s = Session::new_part();
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
    let (fillet, _) = s.fillet(None, &Sel::Along { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }, &2.0.into(), None).unwrap();
    // Four vertical corner cuts replace squares r² by quarter circles πr²/4 over height h.
    let expected = 20.0_f64.powi(3) - 4.0 * (1.0 - std::f64::consts::PI / 4.0) * 2.0_f64.powi(2) * 20.0;
    assert_close(s.shape(fillet).unwrap().volume(), expected, 1e-6, "vertical-edge fillet fixture");
    s.save(Some(path)).unwrap();
    let qymcad_io::LoadedProject { mut project, mut breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let node = project.timeline.iter_mut().find(|n| n.id == fillet).unwrap();
    let qymcad_core::feature::FeatureKind::Fillet { edges, .. } = &mut node.kind else { panic!("fillet") };
    *edges = Ref::many(Query::Oriented { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 });
    node.dirty = dirty;
    let _gate = qymcad_kernel::kernel_gate();
    let bytes = &mut breps.iter_mut().find(|(id, _)| *id == body).unwrap().1;
    let shape = qymcad_kernel::Shape::from_brep_bytes(bytes).unwrap();
    shape.rename_edges(&shape.edges_info().iter().map(|e| (e.id, 0)).collect::<Vec<_>>());
    assert!(shape.edges_info().iter().all(|e| e.id == 0));
    *bytes = shape.to_brep_bytes().unwrap();
    if missing {
        breps.retain(|(id, _)| *id != fillet);
    }
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    body
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
