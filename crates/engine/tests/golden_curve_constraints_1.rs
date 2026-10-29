//! Native generic curve relations and strict shared sketch selectors.
use qymcad_engine::*;

#[test]
fn generic_curve_constraint_kinds_and_selectors_are_parseable() {
    for text in ["\"point_on_curve\"", "\"curve_distance\""] {
        serde_json::from_str::<ConstraintKind>(text).expect("generic curve constraint kind");
    }
    serde_json::from_str::<SketchRef>(r#"{"curve":{"kind":"spline"},"role":"node","node_index":1}"#)
        .expect("a native spline node selector");
}

mod common;
use common::*;
use qymcad_core::{
    feature::{Ends, Purpose},
    geom::Point2,
    model::{Constraint as C, Param, Project},
};
use std::f64::consts::PI;

fn dim(a: Id, b: Id, axis: u8, d: f64, expr: &str) -> C {
    C::Distance { a, b, axis, d, off: 0.0, expr: expr.into(), driven: false, at: None }
}

fn pin(p: &mut Project, point: Id, x: f64, y: f64) {
    let o = p.ensure_origin(0);
    for c in [dim(point, o, 1, x, &x.to_string()), dim(point, o, 2, y, &y.to_string())] {
        assert!(p.add_constraint_if_independent(0, c));
    }
}

/// A deliberately one-freedom native sketch; the engine relation must finish it.
fn relation_fixture(spline: bool, line: bool) -> (Session, Id, Id, Id, Id) {
    let mut p = Project::default();
    p.new_document();
    let sk = p.add_sketch("curve relation", vec![], None);
    p.add_sketch_node(sk, "curve relation");
    for (name, value) in [("B", 4.0), ("H", 5.0), ("Gap", 6.0)] {
        p.parameters.push(Param { name: name.into(), expr: value.to_string(), value });
    }
    let curve = if spline {
        let id = p
            .add_spline(0, vec![Point2::new(2.0, 3.0), Point2::new(7.0, 8.0), Point2::new(12.0, 3.0)], Ends::Open, Purpose::Construction)
            .unwrap();
        let nodes = p.sketches[0].entity(id).unwrap().kind.as_spline().unwrap().nodes.to_vec();
        for (i, node) in nodes.into_iter().enumerate() {
            let q = *p.sketches[0].point(node).unwrap();
            pin(&mut p, node, q.x, q.y);
            if i == 1 {
                let c = p.sketches[0].constraints.last_mut().unwrap();
                if let C::Distance { expr, .. } = c {
                    *expr = "H+3".into();
                }
            }
        }
        id
    } else {
        let centre = p.add_ellipse_entity(0, Point2::new(20.0, 30.0), 8.0, 4.0, 0.0, Purpose::Construction);
        let e = p.sketches[0].entities[0].clone();
        let oval = e.kind.as_ellipse().unwrap();
        pin(&mut p, centre, 20.0, 30.0);
        p.sketches[0].constraints.extend([
            dim(centre, oval.ma, 0, 8.0, "8"),
            dim(centre, oval.mi, 0, 4.0, "B"),
            C::Horizontal { a: centre, b: oval.ma },
        ]);
        e.id
    };
    let o = p.ensure_origin(0);
    let (other, point, cy) = if line {
        let id = p.add_line_entity(0, 5.0, 40.0, 35.0, 40.0, Purpose::Construction);
        let edge = p.sketches[0].entity(id).unwrap().kind.as_line().unwrap();
        p.sketches[0].constraints.extend([
            dim(edge.a, o, 1, 5.0, "5"),
            dim(edge.b, o, 1, 35.0, "35"),
            C::Horizontal { a: edge.a, b: edge.b },
        ]);
        (id, edge.a, 40.0)
    } else {
        let (x, y) = if spline { (5.0, 6.96) } else { (24.0, 30.0 + 2.0 * 3f64.sqrt()) };
        let id = p.sketch_point_at(0, x, y, 1e-6);
        p.sketches[0].constraints.push(dim(id, o, 1, x, &x.to_string()));
        (id, id, y)
    };
    let x = p.sketches[0].point(point).unwrap().x;
    let circle = p.add_circle_entity(0, x, cy, 1.0, Purpose::Real);
    let centre = p.sketches[0].entity(circle).unwrap().kind.as_circle().unwrap().center;
    assert_eq!(centre, point, "circle shares the relation's moving point");
    p.sketches[0].constraints.push(C::Diameter { c: centre, d: 2.0, off: 0.0, expr: "2".into(), driven: false, diam: true, at: None });
    p.eval_parameters();
    p.solve_sketch(0);
    assert_eq!(p.sketch_dof(0), (1, 0));
    let path = scratch(&format!("curve-relation-{}-{}-{}.qcad", spline, line, std::thread::current().name().unwrap_or("test")));
    qymcad_io::save_project(&p, path.to_str().unwrap()).unwrap();
    let (s, _) = Session::open(&path).unwrap();
    (s, sk, curve, other, point)
}

fn constrain(s: &mut Session, sk: Id, kind: ConstraintKind, a: Id, b: Id, value: Option<Num>) -> Constrained {
    s.sketch_constrain(
        sk,
        &ConstrainSpec { kind, refs: vec![SketchRef::Id(a), SketchRef::Id(b)], value, axis: DistAxis::Aligned, reference: false },
    )
    .unwrap()
}

fn extrude_circle(s: &mut Session, sk: Id) {
    let (_, r) = s
        .extrude(&Extrude {
            down: None,
            sketch: sk,
            profiles: None,
            height: Num::Value(2.0),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_close(r.bodies[0].volume, 2.0 * PI, 1e-8, "circle profile exact volume");
}

fn assert_point(s: &Session, sk: Id, point: Id, y: f64) {
    let d = s.sketch_detail(sk).unwrap();
    assert_eq!(d.summary.dof, (0, 0));
    assert_close(d.points.iter().find(|p| p.id == point).unwrap().y, y, 1e-7, "independent curve relation ordinate");
}

fn check_persistence_and_edit(mut s: Session, sk: Id, point: Id, name: &str, value: f64, expected_y: f64, label: &str) {
    extrude_circle(&mut s, sk);
    let path = scratch(&format!("{label}.qcad"));
    s.save(Some(&path)).unwrap();
    let (mut reopened, rb) = Session::open(&path).unwrap();
    assert_close(rb.bodies[0].volume, 2.0 * PI, 1e-8, "same after open");
    assert_eq!(reopened.sketch_detail(sk).unwrap(), s.sketch_detail(sk).unwrap());
    let rb = reopened.param_set(name, &Num::Value(value)).unwrap();
    assert_point(&reopened, sk, point, expected_y);
    assert_close(rb.bodies[0].bbox[1], expected_y - 1.0, 0.006, "engine body follows relation");
    let (v, bbox) = gui_edit_param_body(&path, name, &value.to_string());
    assert_close(v, 2.0 * PI, 1e-8, "app edit volume");
    assert_close(bbox[1], expected_y - 1.0, 0.006, "app body follows relation (native tessellated bbox)");
}

#[test]
fn point_on_ellipse_survives_app_edit_and_save_open() {
    let (mut s, sk, curve, point, _) = relation_fixture(false, false);
    let r = constrain(&mut s, sk, ConstraintKind::PointOnCurve, point, curve, None);
    assert_eq!(r.dof, (0, 0));
    assert_eq!(s.sketch_detail(sk).unwrap().constraints[r.index.unwrap()].kind, "point_on_ellipse");
    assert_point(&s, sk, point, 30.0 + 2.0 * 3f64.sqrt());
    check_persistence_and_edit(s, sk, point, "B", 6.0, 30.0 + 3.0 * 3f64.sqrt(), "point-on-ellipse-stage1");
}

#[test]
fn point_on_spline_survives_app_edit_and_save_open() {
    let (mut s, sk, curve, point, _) = relation_fixture(true, false);
    constrain(&mut s, sk, ConstraintKind::PointOnCurve, point, curve, None);
    // Natural cubic arch at x=5 has u=3/5 on its first half: H*(3u-u^3)/2.
    assert_point(&s, sk, point, 3.0 + 5.0 * (3.0 * 0.6 - 0.6f64.powi(3)) / 2.0);
    check_persistence_and_edit(s, sk, point, "H", 8.0, 3.0 + 8.0 * (3.0 * 0.6 - 0.6f64.powi(3)) / 2.0, "point-on-spline-stage1");
}

#[test]
fn tangent_curves_survives_app_edit_and_save_open() {
    let (mut s, sk, curve, line, point) = relation_fixture(false, true);
    let r = constrain(&mut s, sk, ConstraintKind::Tangent, curve, line, None);
    assert_eq!(s.sketch_detail(sk).unwrap().constraints[r.index.unwrap()].kind, "tangent_curves");
    assert_point(&s, sk, point, 34.0);
    check_persistence_and_edit(s, sk, point, "B", 7.0, 37.0, "tangent-curves-stage1");
}

#[test]
fn curve_distance_formula_survives_app_edit_and_save_open() {
    let (mut s, sk, curve, line, point) = relation_fixture(false, true);
    let r = constrain(&mut s, sk, ConstraintKind::CurveDistance, curve, line, Some(Num::Expr("Gap".into())));
    assert_eq!(s.sketch_detail(sk).unwrap().constraints[r.index.unwrap()].expr.as_deref(), Some("Gap"));
    assert_point(&s, sk, point, 40.0);
    check_persistence_and_edit(s, sk, point, "Gap", 9.0, 43.0, "curve-distance-stage1");
}

#[test]
fn spline_tangent_and_distance_follow_node_formulas_in_app() {
    for (kind, expected, label) in
        [(ConstraintKind::Tangent, 8.0, "spline-tangent-stage1"), (ConstraintKind::CurveDistance, 14.0, "spline-distance-stage1")]
    {
        let (mut s, sk, curve, line, point) = relation_fixture(true, true);
        let value = (kind == ConstraintKind::CurveDistance).then(|| Num::Expr("Gap".into()));
        constrain(&mut s, sk, kind, curve, line, value);
        assert_point(&s, sk, point, expected);
        check_persistence_and_edit(s, sk, point, "H", 8.0, expected + 3.0, label);
    }
}

#[test]
fn generic_relations_refuse_redundancy_conflict_and_ambiguous_selectors_atomically() {
    let (mut s, sk, curve, line, _) = relation_fixture(false, true);
    constrain(&mut s, sk, ConstraintKind::CurveDistance, curve, line, Some(Num::Expr("Gap".into())));
    let before = s.sketch_detail(sk).unwrap();
    for value in [6.0, 8.0] {
        let err = s
            .sketch_constrain(
                sk,
                &ConstrainSpec {
                    kind: ConstraintKind::CurveDistance,
                    refs: vec![SketchRef::Id(curve), SketchRef::Id(line)],
                    value: Some(Num::Value(value)),
                    axis: DistAxis::Aligned,
                    reference: false,
                },
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("redundant or conflicting"), "{err}");
        assert_eq!(s.sketch_detail(sk).unwrap(), before);
    }
    let err = s
        .sketch_constrain(
            sk,
            &ConstrainSpec {
                kind: ConstraintKind::Tangent,
                refs: vec![SketchRef::Frame(FrameRef::XAxis), SketchRef::Curve(SketchCurveSel::Ids(vec![curve, line]))],
                value: None,
                axis: DistAxis::Aligned,
                reference: false,
            },
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("ambiguous"), "{err}");
    assert_eq!(s.sketch_detail(sk).unwrap(), before, "invalid selectors must not materialize axes");
    let points: Vec<_> = before.points.iter().filter(|p| p.role.is_none()).take(2).map(|p| SketchRef::Id(p.id)).collect();
    assert_eq!(points.len(), 2);
    assert!(s
        .sketch_constrain(
            sk,
            &ConstrainSpec { kind: ConstraintKind::PointOnCurve, refs: points, value: None, axis: DistAxis::Aligned, reference: false }
        )
        .unwrap_err()
        .to_string()
        .contains("point_on_curve takes [point, curve]"));
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
}

#[test]
fn shared_selectors_deny_unknown_fields_and_invalid_role_combinations() {
    for text in [
        r#"{"kind":"spline","typo":true}"#,
        r#"{"curve":1,"role":"node","node_index":0,"kind":"line"}"#,
        r#"{"kind":"line","curve":1,"role":"start"}"#,
    ] {
        assert!(serde_json::from_str::<SketchRef>(text).is_err(), "{text}");
    }
    let (s, sk, curve, _, _) = relation_fixture(true, false);
    for (role, node_index) in [(PointRole::Start, Some(0)), (PointRole::Node, None), (PointRole::Node, Some(99)), (PointRole::Major, None)]
    {
        assert!(s.sketch_point_id(sk, &PointSel::Role(PointRoleSel { curve: EntityRef::Id(curve), role, node_index })).is_err());
    }
    let ids = s.sketch_curve_ids(sk, &SketchCurveSel::Ids(vec![curve, curve])).unwrap();
    assert_eq!(ids, vec![curve]);
    let d = s.sketch_detail(sk).unwrap();
    let first = d.entities.iter().find(|e| e.id == curve).unwrap().nodes.as_ref().unwrap()[0];
    assert_eq!(
        s.sketch_point_id(sk, &PointSel::Role(PointRoleSel { curve: EntityRef::Id(curve), role: PointRole::Node, node_index: Some(0) }))
            .unwrap(),
        first
    );
}

#[test]
fn reference_curve_distance_and_nearest_selectors_keep_drawn_arc_spans() {
    for ellipse in [false, true] {
        for ccw in [false, true] {
            let mut s = Session::new_part();
            let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
            let v = Num::Value;
            let curve = if ellipse {
                s.sketch_ellipse_arc(
                    sk,
                    &EllipseArcSpec {
                        cx: v(20.0),
                        cy: v(30.0),
                        rx: v(8.0),
                        ry: v(4.0),
                        turn: v(0.0),
                        start_angle: v(30.0),
                        end_angle: v(120.0),
                        ccw,
                        construction: true,
                        dimensioned: true,
                    },
                )
                .unwrap()
                .entities[0]
            } else {
                s.sketch_arc(
                    sk,
                    &ArcSpec {
                        cx: v(20.0),
                        cy: v(30.0),
                        r: Some(v(5.0)),
                        start_angle: Some(v(30.0)),
                        end_angle: Some(v(120.0)),
                        start: None,
                        end: None,
                        ccw,
                        construction: true,
                        dimensioned: true,
                    },
                )
                .unwrap()
                .entities[0]
            };
            let x = if ellipse { 8.0 } else { 10.0 };
            let circle = s.sketch_circle(sk, &v(x), &v(30.0), &v(1.0), true).unwrap();
            let point = s.sketch_detail(sk).unwrap().entities.iter().find(|e| e.id == circle).unwrap().points[0];
            let r = s
                .sketch_constrain(
                    sk,
                    &ConstrainSpec {
                        kind: ConstraintKind::CurveDistance,
                        refs: vec![SketchRef::Id(point), SketchRef::Id(curve)],
                        value: None,
                        axis: DistAxis::Aligned,
                        reference: true,
                    },
                )
                .unwrap();
            let expected = match (ellipse, ccw) {
                (true, true) => 76.0f64.sqrt(),
                (true, false) => 4.0,
                (false, true) => 75.0f64.sqrt(),
                (false, false) => 5.0,
            };
            let detail = s.sketch_detail(sk).unwrap();
            assert_close(detail.constraints[r.index.unwrap()].value.unwrap(), expected, 1e-6, "distance to the drawn arc");
            let query = SketchCurveSel::Query(CurveQuery {
                kind: Some(if ellipse { SketchCurveKind::EllipseArc } else { SketchCurveKind::Arc }),
                construction: Some(true),
                nearest: Some([v(x), v(30.0)]),
                within: Some(v(6.0)),
            });
            if ccw {
                assert!(s.sketch_curve_ids(sk, &query).is_err(), "short arc is outside the search radius");
            } else {
                assert_eq!(s.sketch_curve_ids(sk, &query).unwrap(), vec![curve]);
            }
        }
    }
}
