//! Exact-state and extended scheduling acceptance at the native/session boundary.
use crate::*;
use qymcad_core::{
    feature::{FaceKey, FeatureKind},
    model::{DimTarget, NamedDim},
};
use std::collections::HashMap;

fn stock() -> (Session, Id) {
    let mut s = Session::new_part();
    let stock = s.p.add_box(10., 10., 10.);
    assert!(s.rebuild().errors.is_empty());
    (s, stock)
}
fn hole(s: &mut Session, src: Id) -> Id {
    let f = s.p.regen_faces[&src].iter().find(|f| f.normal[2] > 0.9).unwrap();
    let key = FaceKey { index: 0, centroid: [f.centroid.x, f.centroid.y, f.centroid.z], normal: f.normal, id: f.id };
    let id = s.p.add_hole(src, key, 2., 10.);
    assert!(s.rebuild().errors.is_empty());
    id
}
fn shapes(s: &Session) -> HashMap<Id, Vec<u8>> {
    let _gate = qymcad_kernel::kernel_gate();
    s.shapes.iter().map(|(id, sh)| (*id, sh.to_brep_bytes().unwrap())).collect()
}
#[test]
fn failed_reorder_restores_original_brep_absence_dirty_flags_and_diagnostics() {
    let (mut s, base) = stock();
    let h = hole(&mut s, base);
    let arr = s.p.add_linear_array(h, 20., 0., 0., 2);
    assert!(s.rebuild().errors.is_empty());
    // An unrelated active error must not be exempted by reorder's whole-document check.
    let broken = s.p.add_box(-1., 2., 3.);
    s.p.regen_errors.insert(broken, qymcad_core::errors::CoreError::EdgesNotFound { asked: 1 });
    assert!(!s.shapes.contains_key(&broken));
    // Embedded import payloads are private rebuild inputs and must survive failed edits too.
    s.p.sources.push(qymcad_core::model::SourceFile { id: 700, name: "fixture.dxf".into(), ext: "dxf".into(), data: vec![1, 4, 9, 16] });
    let project = serde_json::to_value(&s.p).unwrap();
    let breps = shapes(&s);
    let diagnostics = s.current_rebuild();
    let error = s.feature_reorder(arr, "up").unwrap_err().to_string();
    assert!(error.starts_with("the model did not rebuild (operation rolled back):"), "{error}");
    assert_eq!(serde_json::to_value(&s.p).unwrap(), project);
    assert_eq!(s.p.sources[0].data, vec![1, 4, 9, 16]);
    assert_eq!(s.current_rebuild(), diagnostics);
    assert_eq!(shapes(&s), breps);
    assert!(!s.shapes.contains_key(&broken));
    assert!(s.undo().is_err(), "a failed edit adds no entry");
}
#[test]
fn harmless_forward_driving_name_does_not_block_native_move() {
    let (mut s, base) = stock();
    let h = hole(&mut s, base);
    let arr = s.p.add_linear_array(h, 20., 0., 0., 2);
    s.p.set_feat_dim(h, "diameter", "later_d".into());
    s.p.set_feat_dim(arr, "dy", "2".into());
    s.p.named_dims.push(NamedDim { name: "later_d".into(), target: DimTarget::Feature { node: arr, key: "dy".into() } });
    // The name is a driving value, available without building its owning body's geometry.
    assert!(s.rebuild().errors.is_empty());
    let result = s.feature_reorder(arr, "up").unwrap();
    assert!(result.changed);
    assert!(matches!(s.p.timeline.last().unwrap().kind, FeatureKind::Hole { .. }));
    // 2 * (10*10*10) - pi * (2/2)^2 * 10: one drilled original after duplication.
    assert!((result.rebuild.bodies[0].volume - (2000. - 10. * std::f64::consts::PI)).abs() < 1e-6);
}

#[test]
fn native_accepted_move_cannot_cross_a_measured_value_producer() {
    use qymcad_core::model::Constraint;
    let (mut s, base) = stock();
    let independent = s.p.add_box(2., 2., 2.);
    let measured = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("measured diameter")).unwrap();
    let si = s.p.sketch_index(measured).unwrap();
    let a = s.p.sketch_point_at(si, 0., 0., 1e-9);
    let b = s.p.sketch_point_at(si, 2., 0., 1e-9);
    s.p.sketches[si].constraints.push(Constraint::Distance { a, b, d: 2., off: 0., expr: String::new(), driven: true, axis: 1, at: None });
    assert!(s.p.add_named_dim("measured_d".into(), measured, vec![a, b]));
    s.p.solve_sketch(si);
    let h = hole(&mut s, base);
    s.p.set_feat_dim(h, "diameter", "measured_d".into());
    assert!(s.rebuild().errors.is_empty());
    let mut candidate = s.p.clone_without_source_data();
    assert!(candidate.reorder_feature(candidate.timeline_index(h).unwrap(), candidate.timeline_index(independent).unwrap()));
    let before = serde_json::to_value(&s.p).unwrap();
    let breps = shapes(&s);
    assert_eq!(
        s.feature_reorder(h, "up").unwrap_err().to_string(),
        format!("invalid request: cannot reorder: feature {h} would precede required object {measured}")
    );
    assert_eq!(serde_json::to_value(&s.p).unwrap(), before);
    assert_eq!(shapes(&s), breps);
    // A measurement elsewhere does not prevent an independent base exchange.
    assert!(s.feature_reorder(independent, "up").unwrap().changed);
}

#[test]
fn native_chain_rewrite_preserves_independent_face_datum_source() {
    use qymcad_core::model::PlaneDef;
    let (mut s, base) = stock();
    let h = hole(&mut s, base);
    let arr = s.p.add_linear_array(h, 20., 0., 0., 2);
    assert!(s.rebuild().errors.is_empty());
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &1.0.into(), None).unwrap();
    // A datum on the final array is valid after the native modifier exchange and remains on arr;
    // native rehosting applies only to Face sketches, never to datum definitions.
    let face = s.p.regen_faces[&arr].iter().find(|f| f.normal[2] > 0.9).unwrap();
    let key = FaceKey { index: 0, centroid: [face.centroid.x, face.centroid.y, face.centroid.z], normal: face.normal, id: face.id };
    s.p.planes.iter_mut().find(|pl| pl.id == plane).unwrap().def = PlaneDef::OffsetFace { body: arr, face: key, dist: 1. };
    assert!(s.rebuild().errors.is_empty());
    assert!(s.feature_reorder(arr, "up").unwrap().changed);
    assert!(matches!(s.p.planes.iter().find(|pl|pl.id==plane).unwrap().def,PlaneDef::OffsetFace {body,..} if body==arr));
}
