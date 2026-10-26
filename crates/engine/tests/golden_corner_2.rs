//! Analytic corner cuts, native formula persistence and exact app editing paths.
mod common;
use common::*;
use qymcad_engine::*;

fn n(v: f64) -> Num {
    Num::Value(v)
}
fn e(v: &str) -> Num {
    Num::Expr(v.into())
}
fn fixture() -> (Session, Id, [Id; 2], Id) {
    let mut s = Session::new_part();
    s.param_set("K", &n(4.0)).unwrap();
    s.param_set("Other", &n(6.0)).unwrap();
    s.param_set("Angle", &n(30.0)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let lines = s
        .sketch_polyline(
            sk,
            &PolylineSpec {
                points: vec![Xy(n(0.0), n(0.0)), Xy(n(30.0), n(0.0)), Xy(n(30.0), n(20.0)), Xy(n(0.0), n(20.0))],
                closed: true,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    (s, sk, [lines.entities[0], lines.entities[3]], lines.points[0])
}
fn modes() -> Vec<CornerSize> {
    vec![
        CornerSize::Radius { value: e("K") },
        CornerSize::Chord { value: e("K") },
        CornerSize::ArcLength { value: e("K") },
        CornerSize::Symmetric { value: e("K") },
        CornerSize::TwoDist { first: e("K"), second: e("Other") },
        CornerSize::DistAngle { first: e("K"), second: e("Angle") },
    ]
}
fn volume(mode: usize, k: f64, other: f64, angle: f64) -> f64 {
    let pi = std::f64::consts::PI;
    let removed = match mode {
        0 => k * k * (1.0 - pi / 4.0),
        1 => k * k / 2.0 * (1.0 - pi / 4.0),
        2 => (2.0 * k / pi).powi(2) * (1.0 - pi / 4.0),
        3 => k * k / 4.0,
        4 => k * other / 2.0,
        _ => k * k * angle.to_radians().tan() / 2.0,
    };
    (600.0 - removed) * 2.0
}
fn check_mode(mode: usize) {
    let size = modes().remove(mode);
    let (mut s, sk, curves, point) = fixture();
    let result = s.sketch_corner(sk, &CornerSpec { curves: curves.map(EntityRef::Id), point: PointSel::Id(point), size }).unwrap();
    assert_eq!(result.dof, (0, 0), "mode {mode}");
    assert!(result.free_points.is_empty(), "mode {mode}");
    assert_eq!(result.created_entities.len(), 1);
    assert_eq!(result.dimensions.len(), if mode >= 4 { 2 } else { 1 });
    s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: result.dimensions[0] }, None, Some("Size")).unwrap();
    let (body, report) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: n(2.0),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(report.errors.is_empty());
    assert_close(s.shape(body).unwrap().volume(), volume(mode, 4.0, 6.0, 30.0), 1e-5, "corner analytic volume");
    let path = scratch(&format!("corner_mode_{mode}.qcad"));
    s.save(Some(&path)).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    assert_eq!(opened.sketch_info(sk).unwrap().dof, (0, 0));
    assert_close(opened.shape(body).unwrap().volume(), volume(mode, 4.0, 6.0, 30.0), 1e-5, "corner save/open");
    s.param_set("K", &n(5.0)).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    assert_close(s.shape(body).unwrap().volume(), volume(mode, 5.0, 6.0, 30.0), 1e-5, "corner server formula edit");
    assert_close(gui_edit_param(&path, "K", "5"), volume(mode, 5.0, 6.0, 30.0), 1e-5, "corner app parameter edit");
    assert_close(gui_edit_driver(&path, "Size", "5"), volume(mode, 5.0, 6.0, 30.0), 1e-5, "corner app SetDriver");
    if mode >= 4 {
        let (key, value, other, angle) = if mode == 4 { ("Other", 7.0, 7.0, 30.0) } else { ("Angle", 40.0, 6.0, 40.0) };
        assert_close(
            gui_edit_param(&path, key, &value.to_string()),
            volume(mode, 4.0, other, angle),
            1e-5,
            "corner second formula app edit",
        );
    }
}
#[test]
fn corner_radius_formulas_and_app_drivers() {
    check_mode(0);
}
#[test]
fn corner_chord_formulas_and_app_drivers() {
    check_mode(1);
}
#[test]
fn corner_arc_length_formulas_and_app_drivers() {
    check_mode(2);
}
#[test]
fn corner_symmetric_chamfer_formulas_and_app_drivers() {
    check_mode(3);
}
#[test]
fn corner_two_distance_formulas_and_app_drivers() {
    check_mode(4);
}
#[test]
fn corner_distance_angle_formulas_and_app_drivers() {
    check_mode(5);
}
#[test]
fn corner_invalid_or_ambiguous_selection_is_atomic() {
    let (mut s, sk, curves, point) = fixture();
    let before = s.sketch_detail(sk).unwrap();
    for (curves, value) in [
        ([EntityRef::Ids(curves.to_vec()), EntityRef::Id(curves[1])], n(2.0)),
        (curves.map(EntityRef::Id), n(-1.0)),
        (curves.map(EntityRef::Id), n(100.0)),
    ] {
        assert!(s.sketch_corner(sk, &CornerSpec { curves, point: PointSel::Id(point), size: CornerSize::Radius { value } }).is_err());
        assert_eq!(s.sketch_detail(sk).unwrap(), before);
    }
}

#[test]
fn native_rectangle_corner_keeps_definition_and_size_formulas() {
    for (mode, size) in modes().into_iter().enumerate() {
        let mut s = Session::new_part();
        for (name, value) in [("K", 4.0), ("Other", 6.0), ("Angle", 30.0)] {
            s.param_set(name, &n(value)).unwrap();
        }
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let sides = s.sketch_rect(sk, &n(15.0), &n(10.0), &n(30.0), &n(20.0), false).unwrap();
        let detail = s.sketch_detail(sk).unwrap();
        let point = detail.entities.iter().find(|e| e.id == sides[0]).unwrap().points[0];
        let result = s
            .sketch_corner(sk, &CornerSpec { curves: [EntityRef::Id(sides[0]), EntityRef::Id(sides[3])], point: PointSel::Id(point), size })
            .unwrap();
        assert_eq!(result.dof, (0, 0), "native rect mode {mode}");
        assert_close(
            s.sketch_info(sk).unwrap().contours.iter().map(|c| c.area).sum(),
            volume(mode, 4.0, 6.0, 30.0) / 2.0,
            0.03,
            "native rectangle cut area",
        );
        s.param_set("K", &n(5.0)).unwrap();
        assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0), "native rect edited mode {mode}");
    }
}

#[test]
fn native_spline_line_corner_retains_full_definition() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let spline = s
        .sketch_spline(
            sk,
            &SplineSpec {
                points: vec![Xy(n(-30.0), n(-10.0)), Xy(n(-15.0), n(0.0)), Xy(n(0.0), n(0.0))],
                closed: false,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    let line =
        s.sketch_line(sk, &LineSpec { x1: n(0.0), y1: n(0.0), x2: n(0.0), y2: n(30.0), construction: false, dimensioned: true }).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    let result = s
        .sketch_corner(
            sk,
            &CornerSpec {
                curves: [EntityRef::Id(spline.entities[0]), EntityRef::Id(line.entities[0])],
                point: PointSel::Id(spline.points[2]),
                size: CornerSize::Radius { value: n(2.0) },
            },
        )
        .unwrap();
    assert_eq!(result.dof, (0, 0));
    assert!(result.free_points.is_empty());
    let detail = s.sketch_detail(sk).unwrap();
    let arc = detail.entities.iter().find(|e| e.id == result.created_entities[0]).unwrap();
    assert_eq!(arc.kind, "arc");
    assert_close(arc.r.unwrap(), 2.0, 1e-6, "curved corner radius");
}

/// A chamfer on a spline–line corner: QymCAD does not hold the sharp on the spline leg (blends.rs), so the cut
/// point on the spline keeps a freedom. The edit is applied and reports it, as every sketch edit does (ADR 0022).
#[test]
fn a_chamfer_on_a_spline_corner_is_applied_and_reports_its_freedom() {
    for size in [
        CornerSize::Symmetric { value: n(2.0) },
        CornerSize::TwoDist { first: n(2.0), second: n(3.0) },
        CornerSize::DistAngle { first: n(2.0), second: n(40.0) },
    ] {
        let mut s = Session::new_part();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let spline = s
            .sketch_spline(
                sk,
                &SplineSpec {
                    points: vec![Xy(n(-30.0), n(-10.0)), Xy(n(-15.0), n(0.0)), Xy(n(0.0), n(0.0))],
                    closed: false,
                    construction: false,
                    dimensioned: true,
                },
            )
            .unwrap();
        let line = s
            .sketch_line(sk, &LineSpec { x1: n(0.0), y1: n(0.0), x2: n(0.0), y2: n(30.0), construction: false, dimensioned: true })
            .unwrap();
        assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
        let result = s
            .sketch_corner(
                sk,
                &CornerSpec {
                    curves: [EntityRef::Id(spline.entities[0]), EntityRef::Id(line.entities[0])],
                    point: PointSel::Id(spline.points[2]),
                    size: size.clone(),
                },
            )
            .unwrap_or_else(|e| panic!("{size:?}: {e}"));
        assert_eq!(result.dof.1, 0, "{size:?}: no redundancy");
        assert_eq!(result.dof.0 > 0, !result.free_points.is_empty(), "{size:?}: freedom and free points agree");
        assert_eq!(s.sketch_detail(sk).unwrap().entities.iter().find(|e| e.id == result.created_entities[0]).unwrap().kind, "line");
    }
}

#[test]
fn a_corner_of_an_offset_outline_is_cut_without_redundancy() {
    // Live test D2: rounding a corner of a formula offset outline was refused as redundant.
    let mut s = Session::new_part();
    s.param_set("Gap", &n(2.0)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let ids = s.sketch_rect(sk, &n(0.0), &n(0.0), &n(20.0), &n(10.0), false).unwrap();
    let offset = s
        .sketch_offset(
            sk,
            &SketchOffsetSpec { curves: SketchCurveSel::Ids(ids), distance: e("Gap"), toward: [n(20.0), n(10.0)], dimension_name: None },
        )
        .unwrap();
    assert_eq!(offset.dof, (0, 0));
    let detail = s.sketch_detail(sk).unwrap();
    let corner = detail.points.iter().find(|p| (p.x - 12.0).abs() < 1e-9 && (p.y - 7.0).abs() < 1e-9).unwrap().id;
    let sides: Vec<Id> =
        detail.entities.iter().filter(|e| offset.created_entities.contains(&e.id) && e.points.contains(&corner)).map(|e| e.id).collect();
    let result = s
        .sketch_corner(
            sk,
            &CornerSpec {
                curves: [EntityRef::Id(sides[0]), EntityRef::Id(sides[1])],
                point: PointSel::Id(corner),
                size: CornerSize::Radius { value: e("Gap") },
            },
        )
        .unwrap();
    assert_eq!(result.dof, (0, 0));
    // The ring between the 20x10 source and the outline offset by Gap, its outer corner rounded by Gap.
    let ring = |g: f64| ((20.0 + 2.0 * g) * (10.0 + 2.0 * g) - g * g * (1.0 - std::f64::consts::PI / 4.0) - 200.0) * 2.0;
    let (body, report) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: n(2.0),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(report.errors.is_empty());
    assert_close(s.shape(body).unwrap().volume(), ring(2.0), 1e-6, "rounded offset ring");
    let path = scratch("corner_of_offset.qcad");
    s.save(Some(&path)).unwrap();
    s.param_set("Gap", &n(3.0)).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    assert_close(s.shape(body).unwrap().volume(), ring(3.0), 1e-6, "rounded offset ring, server edit");
    assert_close(gui_edit_param(&path, "Gap", "3"), ring(3.0), 1e-6, "rounded offset ring, app edit");
}

#[test]
fn a_fillet_centre_on_an_existing_point_leaves_that_points_rows_alone() {
    // Review round 2 (Codex): the first fix gave the arc its own centre by moving every changed row off the source
    // corner, including a parallel the cut had only rewritten, so a later radius edit failed to solve.
    let mut s = Session::new_part();
    s.param_set("Gap", &n(2.0)).unwrap();
    s.param_set("R", &n(2.0)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let ids = s.sketch_rect(sk, &n(0.0), &n(0.0), &n(20.0), &n(10.0), false).unwrap();
    let offset = s
        .sketch_offset(
            sk,
            &SketchOffsetSpec {
                curves: SketchCurveSel::Ids(ids.clone()),
                distance: e("Gap"),
                toward: [n(20.0), n(10.0)],
                dimension_name: None,
            },
        )
        .unwrap();
    let d = s.sketch_detail(sk).unwrap();
    let at = |x: f64, y: f64| d.points.iter().find(|p| (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9).unwrap().id;
    let (corner, below, src_a, src_b) = (at(12.0, 7.0), at(12.0, -7.0), at(10.0, -5.0), at(10.0, 5.0));
    let entity = |ps: [Id; 2]| d.entities.iter().find(|e| e.points.contains(&ps[0]) && e.points.contains(&ps[1])).unwrap().id;
    let (right, source_right) = (entity([corner, below]), entity([src_a, src_b]));
    let top =
        d.entities.iter().find(|e| offset.created_entities.contains(&e.id) && e.id != right && e.points.contains(&corner)).unwrap().id;
    let distance = d.constraints.iter().find(|c| c.kind == "distance_point_line" && c.points == vec![corner, src_a, src_b]).unwrap().index;
    s.sketch_remove_constraint(sk, distance).unwrap();
    let parallel = ConstrainSpec {
        kind: ConstraintKind::Parallel,
        refs: vec![SketchRef::Id(right), SketchRef::Id(source_right)],
        value: None,
        axis: DistAxis::Aligned,
        reference: false,
    };
    s.sketch_constrain(sk, &parallel).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    let result = s
        .sketch_corner(
            sk,
            &CornerSpec {
                curves: [EntityRef::Id(right), EntityRef::Id(top)],
                point: PointSel::Id(corner),
                size: CornerSize::Radius { value: e("R") },
            },
        )
        .unwrap();
    assert_eq!(result.dof, (0, 0));
    s.param_set("R", &n(3.0)).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    let d = s.sketch_detail(sk).unwrap();
    assert!(d.points.iter().any(|p| p.id == src_b && (p.x - 10.0).abs() < 1e-9 && (p.y - 5.0).abs() < 1e-9), "source corner stays");
    let arc = d.entities.iter().find(|e| e.id == result.created_entities[0]).unwrap();
    assert_close(arc.r.unwrap(), 3.0, 1e-9, "fillet radius follows R");
}
