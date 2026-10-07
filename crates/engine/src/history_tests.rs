//! Deletion safeguards for structural definitions produced by the native GUI.
use crate::*;
use qymcad_core::model::HoleTool;

fn stock() -> Session {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch: sk,
        profiles: None,
        height: 20.0.into(),
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap();
    s
}

#[test]
fn deleting_a_native_point_removes_its_pool_entry() {
    let mut s = Session::new_part();
    let point = s.p.add_point_at([1.0, 2.0, 3.0]);
    s.feature_delete(point, false).unwrap();
    assert!(s.p.datum_points.is_empty(), "deleted datum points must not remain in the pool");
    assert!(s.p.timeline.is_empty());
}

#[test]
fn point_axis_dependencies_are_refused_and_cascaded() {
    let mut s = Session::new_part();
    let a = s.p.add_point_at([0.0, 0.0, 0.0]);
    let b = s.p.add_point_at([0.0, 0.0, 10.0]);
    let axis = s.p.add_axis_two_points(a, b);
    assert!(s.rebuild().errors.is_empty());
    let error = s.feature_delete(a, false).unwrap_err();
    assert!(error.to_string().contains("dependents"), "{error}");
    s.feature_delete(a, true).unwrap();
    assert!(!s.p.datum_axes.iter().any(|d| d.id == axis));
    assert_eq!(s.p.datum_points.iter().map(|d| d.id).collect::<Vec<_>>(), vec![b]);
}

#[test]
fn native_vertex_point_depends_on_its_body() {
    let mut s = stock();
    let body = s.result_bodies()[0].id;
    let topology = s.topology(None, false).unwrap();
    let point = s.p.add_point_at_vertex([0.0; 3], body, topology.edges[0].id, false);
    let error = s.feature_delete(body, false).unwrap_err();
    assert!(error.to_string().contains("dependents"), "{error}");
    assert!(s.feature_delete(body, true).unwrap().bodies.is_empty());
    assert!(!s.p.datum_points.iter().any(|d| d.id == point));
}

#[test]
fn native_sketch_driven_hole_depends_on_its_sketch() {
    let mut s = stock();
    let body = s.result_bodies()[0].id;
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    // Native unfinished GUI hole: the dependency must be protected before its first successful build.
    let hole = s.p.add_hole_from_sketch(body, sketch, HoleTool { kind: 0, diameter: 2.0, depth: 20.0, dia2: 0.0, depth2: 0.0 }, false);
    let error = s.feature_delete(sketch, false).unwrap_err();
    assert!(error.to_string().contains("dependents"), "{error}");
    s.feature_delete(sketch, true).unwrap();
    assert!(!s.p.timeline.iter().any(|n| n.id == hole));
    assert_eq!(s.result_bodies()[0].id, body);
    assert!((s.result_bodies()[0].volume - 20.0_f64.powi(3)).abs() < 1e-6);
}

#[test]
fn deleting_before_the_rollback_bar_preserves_the_boundary() {
    let mut s = stock();
    let (before_bar, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &2.0.into(), None).unwrap();
    let (after_bar, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &4.0.into(), None).unwrap();
    s.p.set_rollback(Some(3));
    assert!(s.rebuild().errors.is_empty());
    s.feature_delete(before_bar, false).unwrap();
    assert_eq!(s.p.rollback, Some(2), "deletion must not promote nodes from below the rollback bar");
    let index = s.p.timeline.iter().position(|n| n.id == after_bar).unwrap();
    assert!(index >= s.p.rollback.unwrap());
}

#[test]
fn native_thicken_join_depends_on_the_target_body() {
    use qymcad_core::feature::FeatureKind;
    let mut s = stock();
    let body = s.result_bodies()[0].id;
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &30.0.into(), &0.0.into(), &4.0.into(), &6.0.into(), false).unwrap();
    let (source, _) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: 2.0.into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    let thicken = s.p.add_thicken(source, 0, 1.0);
    if let FeatureKind::Thicken { join, .. } = &mut s.p.timeline.last_mut().unwrap().kind {
        *join = body;
    }
    let error = s.feature_delete(body, false).unwrap_err();
    assert!(error.to_string().contains("dependents"), "join dependency must refuse before invalid native thicken rebuild: {error}");
    assert!(s.p.timeline.iter().any(|n| n.id == thicken));
}

#[test]
fn native_part_instances_depend_on_the_source_tip() {
    use qymcad_core::feature::{FeatureKind, FeatureNode};
    let mut s = stock();
    let body = s.result_bodies()[0].id;
    let src_comp = s.p.active_component.unwrap();
    // A native pending instance references a component's current body instead of storing a body input.
    s.p.timeline.push(FeatureNode {
        id: 1000,
        name: "instance".into(),
        kind: FeatureKind::PartInstance { src_comp, body: 1000 },
        parent: Some(s.p.root),
        dirty: false,
        suppressed: false,
    });
    let error = s.feature_delete(body, false).unwrap_err();
    assert!(error.to_string().contains("dependents"), "instance dependency must refuse before source loss: {error}");
}
