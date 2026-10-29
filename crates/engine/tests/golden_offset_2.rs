//! Stage-two native sketch offsets, analytic measures and persisted app drivers.
mod common;
use common::*;
use qymcad_engine::*;
use std::f64::consts::PI;
fn v(x: f64) -> Num {
    Num::Value(x)
}
fn e(x: &str) -> Num {
    Num::Expr(x.into())
}
fn fixture() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("Gap", &v(2.0)).unwrap();
    s.param_set("R", &v(10.0)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("Offset")).unwrap();
    (s, sk)
}
fn spec(ids: Vec<Id>, toward: [f64; 2], name: Option<&str>) -> SketchOffsetSpec {
    SketchOffsetSpec {
        curves: SketchCurveSel::Ids(ids),
        distance: e("Gap"),
        toward: toward.map(v),
        dimension_name: name.map(str::to_string),
    }
}
fn extrude(s: &mut Session, sk: Id) -> f64 {
    let (_, r) = s
        .extrude(&Extrude {
            down: None,
            sketch: sk,
            profiles: None,
            height: v(3.0),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r.bodies[0].volume
}
fn ring(r: f64, d: f64) -> f64 {
    PI * ((r + d).powi(2) - r * r) * 3.0
}
#[test]
fn offset_circle_formula_is_native_curve_distance_and_survives_open_app_and_setdriver() {
    let (mut s, sk) = fixture();
    let id = s.sketch_circle(sk, &v(0.0), &v(0.0), &e("2*R"), false).unwrap();
    let a = s.sketch_offset(sk, &spec(vec![id], [15.0, 0.0], Some("OffsetSize"))).unwrap();
    assert_eq!(a.created_entities.len(), 1);
    assert_eq!(a.dof, (0, 0));
    let d = s.sketch_detail(sk).unwrap();
    assert_eq!(d.constraints[a.dimensions[0]].kind, "curve_distance");
    assert_eq!(d.constraints[a.dimensions[0]].expr.as_deref(), Some("Gap"));
    assert_close(extrude(&mut s, sk), ring(10.0, 2.0), 1e-6, "circle offset annulus");
    let path = scratch("offset-circle-2.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_close(r.bodies[0].volume, ring(10.0, 2.0), 1e-6, "opened offset");
    assert_eq!(opened.sketch_info(sk).unwrap().dof, (0, 0));
    assert_close(opened.param_set("Gap", &v(3.0)).unwrap().bodies[0].volume, ring(10.0, 3.0), 1e-6, "server offset parameter");
    assert_close(gui_edit_param(&path, "Gap", "3"), ring(10.0, 3.0), 1e-6, "app offset formula");
    assert_close(gui_edit_driver(&path, "OffsetSize", "4"), ring(10.0, 4.0), 1e-6, "app named offset SetDriver");
    assert_close(gui_edit_param(&path, "R", "12"), ring(12.0, 2.0), 1e-6, "app source radius edit");
}
#[test]
fn offset_rectangle_all_native_drivers_keep_formula_and_native_mitre_area() {
    let (mut s, sk) = fixture();
    let ids = s.sketch_rect(sk, &v(0.0), &v(0.0), &v(20.0), &v(10.0), false).unwrap();
    let a = s.sketch_offset(sk, &spec(ids.clone(), [20.0, 10.0], None)).unwrap();
    assert_eq!(a.dof, (0, 0));
    assert!(a.dimensions.len() >= 4);
    let detail = s.sketch_detail(sk).unwrap();
    for ci in a.dimensions {
        assert_eq!(detail.constraints[ci].expr.as_deref(), Some("Gap"));
    }
    let volume = |d: f64| ((20.0 + 2.0 * d) * (10.0 + 2.0 * d) - 200.0) * 3.0;
    assert_close(extrude(&mut s, sk), volume(2.0), 1e-6, "native mitred rectangle offset ring");
    let path = scratch("offset-rectangle-2.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_close(r.bodies[0].volume, volume(2.0), 1e-6, "opened mitred offset");
    assert_close(opened.param_set("Gap", &v(3.0)).unwrap().bodies[0].volume, volume(3.0), 1e-6, "all offset dimensions change");
    assert_close(gui_edit_param(&path, "Gap", "3"), volume(3.0), 1e-6, "app all offset dimensions");
    let before = s.sketch_detail(sk).unwrap();
    assert!(s.sketch_offset(sk, &spec(ids, [20.0, 10.0], Some("ManyDrivers"))).unwrap_err().to_string().contains("one"));
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
}
#[test]
fn offset_straight_spline_keeps_derived_record_and_formula_after_open_and_app() {
    let (mut s, sk) = fixture();
    let source = s
        .sketch_spline(
            sk,
            &SplineSpec {
                points: vec![Xy(v(0.0), v(0.0)), Xy(v(10.0), v(0.0)), Xy(v(20.0), v(0.0))],
                closed: false,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    let a = s.sketch_offset(sk, &spec(source.entities, [10.0, 5.0], Some("SplineGap"))).unwrap();
    assert_eq!(a.dof, (0, 0));
    let d = s.sketch_detail(sk).unwrap();
    for id in &a.created_points {
        let p = d.points.iter().find(|p| p.id == *id).unwrap();
        assert_close(p.y, 2.0, 1e-6, "spline offset y");
    }
    // Close the region with independent formula-bearing end lines. Shared offset endpoints need no extra pins.
    for x in [0.0, 20.0] {
        s.sketch_line(sk, &LineSpec { x1: v(x), y1: v(0.0), x2: v(x), y2: e("Gap"), construction: false, dimensioned: false }).unwrap();
    }
    assert_close(extrude(&mut s, sk), 20.0 * 2.0 * 3.0, 1e-6, "straight spline strip area");
    let path = scratch("offset-straight-spline-2.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_close(r.bodies[0].volume, 120.0, 1e-6, "opened spline offset");
    assert_close(opened.param_set("Gap", &v(3.0)).unwrap().bodies[0].volume, 180.0, 1e-6, "derived spline relaid");
    assert_close(gui_edit_param(&path, "Gap", "3"), 180.0, 1e-6, "app derived spline relaid");
    assert_close(gui_edit_driver(&path, "SplineGap", "4"), 240.0, 1e-6, "app spline SetDriver");
}
#[test]
fn offset_refuses_zero_tied_side_and_construction_chain_without_mutation() {
    let (mut s, sk) = fixture();
    let id = s.sketch_circle(sk, &v(0.0), &v(0.0), &v(20.0), false).unwrap();
    for (distance, toward) in [(v(0.0), [15.0, 0.0]), (e("Gap"), [10.0, 0.0])] {
        let before = s.sketch_detail(sk).unwrap();
        let mut q = spec(vec![id], toward, None);
        q.distance = distance;
        assert!(s.sketch_offset(sk, &q).is_err());
        assert_eq!(s.sketch_detail(sk).unwrap(), before);
    }
    let a =
        s.sketch_line(sk, &LineSpec { x1: v(0.0), y1: v(25.0), x2: v(10.0), y2: v(25.0), construction: true, dimensioned: true }).unwrap();
    let before = s.sketch_detail(sk).unwrap();
    assert!(s.sketch_offset(sk, &spec(a.entities, [5.0, 30.0], None)).is_err());
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
}

#[test]
fn offset_arc_curve_distance_drives_exact_annular_sector_in_app() {
    let (mut s, sk) = fixture();
    let source = s
        .sketch_arc(
            sk,
            &ArcSpec {
                cx: v(0.0),
                cy: v(0.0),
                r: Some(e("R")),
                start_angle: Some(v(0.0)),
                end_angle: Some(v(90.0)),
                start: None,
                end: None,
                ccw: true,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    let a = s.sketch_offset(sk, &spec(source.entities, [15.0, 0.0], Some("ArcGap"))).unwrap();
    assert_eq!(a.dof, (0, 0));
    let detail = s.sketch_detail(sk).unwrap();
    assert_eq!(detail.constraints[a.dimensions[0]].kind, "curve_distance");
    assert_eq!(detail.constraints[a.dimensions[0]].expr.as_deref(), Some("Gap"));
    for (x1, y1, x2, y2) in [(e("R"), v(0.0), e("R+Gap"), v(0.0)), (v(0.0), e("R"), v(0.0), e("R+Gap"))] {
        s.sketch_line(sk, &LineSpec { x1, y1, x2, y2, construction: false, dimensioned: false }).unwrap();
    }
    assert_close(extrude(&mut s, sk), ring(10.0, 2.0) / 4.0, 1e-6, "exact circular offset arc sector");
    let path = scratch("offset-arc-2.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_close(r.bodies[0].volume, ring(10.0, 2.0) / 4.0, 1e-6, "opened arc sector");
    assert_close(opened.param_set("Gap", &v(3.0)).unwrap().bodies[0].volume, ring(10.0, 3.0) / 4.0, 1e-6, "server arc offset edit");
    assert_close(gui_edit_param(&path, "Gap", "3"), ring(10.0, 3.0) / 4.0, 1e-6, "app arc offset edit");
    assert_close(gui_edit_driver(&path, "ArcGap", "4"), ring(10.0, 4.0) / 4.0, 1e-6, "app arc offset SetDriver");
}

#[test]
fn offset_ellipse_circle_control_retains_native_record_and_analytic_annulus() {
    let (mut s, sk) = fixture();
    let source = s
        .sketch_ellipse(
            sk,
            &EllipseSpec { cx: v(0.0), cy: v(0.0), rx: e("R"), ry: e("R"), turn: v(0.0), construction: false, dimensioned: true },
        )
        .unwrap();
    let a = s.sketch_offset(sk, &spec(source.entities, [15.0, 0.0], Some("EllipseGap"))).unwrap();
    assert_eq!(a.dof, (0, 0));
    assert_eq!(s.project().sketches[0].offsets.len(), 1);
    // Generic native offsets fit a spline within 0.01 mm. A circle control has a closed-form annulus;
    // perimeter*fit tolerance*height is an independent conservative area/volume allowance.
    let tolerance = 2.0 * PI * 14.0 * 0.01 * 3.0;
    assert_close(extrude(&mut s, sk), ring(10.0, 2.0), tolerance, "ellipse circular control");
    let path = scratch("offset-ellipse-2.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_eq!(opened.project().sketches[0].offsets.len(), 1);
    assert_close(r.bodies[0].volume, ring(10.0, 2.0), tolerance, "opened ellipse offset");
    assert_close(opened.param_set("Gap", &v(3.0)).unwrap().bodies[0].volume, ring(10.0, 3.0), tolerance, "server ellipse offset edit");
    assert_close(gui_edit_param(&path, "Gap", "3"), ring(10.0, 3.0), tolerance, "app ellipse offset edit");
    assert_close(gui_edit_driver(&path, "EllipseGap", "4"), ring(10.0, 4.0), tolerance, "app ellipse offset SetDriver");
}

#[test]
fn a_circle_offset_takes_the_side_nearer_the_hint_by_exact_radius() {
    // Review round 3 (Codex): the side was chosen again from the native preview's chords, which lie inside the
    // circle, so a hint just inside a large circle took the outer copy.
    let (mut s, sk) = fixture();
    let id = s.sketch_circle(sk, &v(0.0), &v(0.0), &v(200.0), false).unwrap();
    let spec = SketchOffsetSpec {
        curves: SketchCurveSel::Ids(vec![id]),
        distance: v(0.01),
        toward: [v(99.89982691707787), v(4.361720639596774)],
        dimension_name: None,
    };
    let out = s.sketch_offset(sk, &spec).unwrap();
    let d = s.sketch_detail(sk).unwrap();
    let copy = d.entities.iter().find(|e| out.created_entities.contains(&e.id)).unwrap();
    assert_close(copy.r.unwrap(), 99.99, 1e-9, "inner copy, nearer the hint");
}
