//! Regressions for entity ownership, helper cleanup and scale-independent parallelism.
mod common;
use common::*;
use qymcad_engine::*;

fn v(x: f64) -> Num {
    Num::Value(x)
}
fn constraint(kind: ConstraintKind, refs: Vec<SketchRef>, value: Option<Num>) -> ConstrainSpec {
    ConstrainSpec { kind, refs, value, axis: DistAxis::Aligned, reference: false }
}

#[test]
fn removing_a_line_drops_its_endpoint_dimension_to_surviving_geometry() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let circle = s.sketch_circle(sk, &v(0.0), &v(0.0), &v(4.0), false).unwrap();
    let line =
        s.sketch_line(sk, &LineSpec { x1: v(10.0), y1: v(0.0), x2: v(20.0), y2: v(0.0), construction: true, dimensioned: false }).unwrap();
    let before = s.sketch_detail(sk).unwrap();
    let center = before.entities.iter().find(|e| e.id == circle).unwrap().points[0];
    // The endpoint lies 10 mm from the circle centre: sqrt((10-0)^2 + (0-0)^2).
    s.sketch_constrain(
        sk,
        &constraint(ConstraintKind::Distance, vec![SketchRef::Id(line.points[0]), SketchRef::Id(center)], Some(v(10.0))),
    )
    .unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (3, 0)); // Four free line coordinates minus one dimension.
    s.sketch_remove_entity(sk, line.entities[0]).unwrap();
    let after = s.sketch_detail(sk).unwrap();
    for endpoint in line.points {
        assert!(!after.points.iter().any(|p| p.id == endpoint), "removed line endpoint {endpoint} survived");
        assert!(!after.constraints.iter().any(|c| c.points.contains(&endpoint)), "removed endpoint's dimension survived");
    }
    assert_eq!(after.summary.dof, (0, 0));
    assert_eq!(after.entities, before.entities.into_iter().filter(|e| e.id == circle).collect::<Vec<_>>());
    assert_eq!(after.points.iter().find(|p| p.id == center), before.points.iter().find(|p| p.id == center));
}

#[test]
fn removing_an_entity_keeps_a_preexisting_self_midpoint() {
    use qymcad_core::{
        feature::Purpose,
        model::{Constraint, Project},
    };
    let mut p = Project::default();
    p.new_document();
    let sk = p.add_sketch("S", vec![], None);
    p.add_sketch_node(sk, "S");
    let si = p.sketch_index(sk).unwrap();
    let circle = p.add_circle_entity(si, 0.0, 0.0, 2.0, Purpose::Real);
    let center = match p.sketches[si].entities.iter().find(|e| e.id == circle).unwrap().kind {
        qymcad_core::model::EntityKind::Circle { center, .. } => center,
        _ => unreachable!(),
    };
    let original = Constraint::Midpoint { p: center, a: center, b: center };
    p.sketches[si].constraints.push(original.clone());
    let line = p.add_line_entity(si, 10.0, 0.0, 20.0, 0.0, Purpose::Construction);
    let path = scratch("original_self_midpoint.qcad");
    qymcad_io::save_project_guarded_with_brep(&p, path.to_str().unwrap(), &[]).unwrap();
    let (mut s, _) = Session::open(&path).unwrap();
    s.sketch_remove_entity(sk, line).unwrap();
    assert!(
        s.project().sketches[si]
            .constraints
            .iter()
            .any(|c| matches!(*c, Constraint::Midpoint {p,a,b} if p == center && a == center && b == center)),
        "a preexisting self-midpoint was stripped"
    );
}

#[test]
fn removing_the_last_direction_dimension_prunes_its_angle_reference() {
    for first_support in [None, Some("horizontal"), Some("point_on_circle")] {
        let mut s = Session::new_part();
        s.param_set("rot", &v(30.0)).unwrap();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_polygon(
            sk,
            &PolygonSpec {
                cx: v(0.0),
                cy: v(0.0),
                r: Some(v(10.0)),
                vertex: None,
                sides: 6,
                angle: Some(Num::Expr("rot".into())),
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
        let d = s.sketch_detail(sk).unwrap();
        let reference = d.points.iter().find(|p| p.role == Some("angle_reference")).unwrap().id;
        if let Some(kind) = first_support {
            let index = d.constraints.iter().find(|c| c.kind == kind && c.points.contains(&reference)).unwrap().index;
            s.sketch_remove_constraint(sk, index).unwrap();
            assert!(s.sketch_detail(sk).unwrap().points.iter().any(|p| p.id == reference), "the direction still needs its reference");
        }
        let index = s
            .sketch_detail(sk)
            .unwrap()
            .constraints
            .iter()
            .find(|c| c.kind == "arc_length" && c.points.contains(&reference))
            .unwrap()
            .index;
        s.sketch_remove_constraint(sk, index).unwrap();
        let d = s.sketch_detail(sk).unwrap();
        assert!(!d.points.iter().any(|p| p.id == reference), "unused angle reference survived after removing {first_support:?}");
        assert!(!d.constraints.iter().any(|c| c.points.contains(&reference)), "angle-reference support constraints survived");
        // A regular hexagon with centre/radius pinned gains precisely its one rotational freedom.
        assert_eq!(d.summary.dof, (1, 0));
    }
}

#[test]
fn removing_a_direction_keeps_an_angle_reference_used_by_another_dimension() {
    let mut s = Session::new_part();
    s.param_set("rot", &v(30.0)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_polygon(
        sk,
        &PolygonSpec {
            cx: v(0.0),
            cy: v(0.0),
            r: Some(v(10.0)),
            vertex: None,
            sides: 6,
            angle: Some(Num::Expr("rot".into())),
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    let before = s.sketch_detail(sk).unwrap();
    let reference = before.points.iter().find(|p| p.role == Some("angle_reference")).unwrap();
    let helper = reference.id;
    let mut dimension = constraint(ConstraintKind::Distance, vec![SketchRef::Id(helper), SketchRef::Frame(FrameRef::Origin)], None);
    dimension.reference = true;
    s.sketch_constrain(sk, &dimension).unwrap();
    let d = s.sketch_detail(sk).unwrap();
    let index = d.constraints.iter().find(|c| c.kind == "arc_length" && c.points.contains(&helper)).unwrap().index;
    s.sketch_remove_constraint(sk, index).unwrap();
    let after = s.sketch_detail(sk).unwrap();
    let point = after.points.iter().find(|p| p.id == helper).expect("another dimension still references this angle helper");
    assert_close(point.x, reference.x, 1e-6, "the referenced helper x stays fixed");
    assert_close(point.y, reference.y, 1e-6, "the referenced helper y stays fixed");
    // Radius 10 around the origin gives sqrt(10²+0²) = 10 mm, even when the rotation dimension is gone.
    let retained = after
        .constraints
        .iter()
        .find(|c| c.kind == "distance" && c.reference && c.points.contains(&helper))
        .expect("the other reference dimension survives");
    assert_close(retained.value.unwrap(), 10.0, 1e-6, "retained reference distance");
    assert_eq!(after.summary.dof, (1, 0)); // The polygon has one free rotation; the helper is still held by its supports.
    let index = retained.index;
    s.sketch_remove_constraint(sk, index).unwrap();
    let after = s.sketch_detail(sk).unwrap();
    assert!(!after.points.iter().any(|p| p.id == helper), "angle helper survived after its final reference dimension was removed");
    assert!(!after.constraints.iter().any(|c| c.points.contains(&helper)), "unused helper support survived");
    assert_eq!(after.summary.dof, (1, 0));
}
