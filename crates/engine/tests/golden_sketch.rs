//! Golden sketches: lines, polylines, arcs, regular polygons and slots, extruded and checked against
//! hand-computed volumes; parameter edits through the engine and through the QymCAD GUI path.

mod common;
use common::*;
use qymcad_engine::*;
use std::f64::consts::PI;

fn n(s: &str) -> Num {
    Num::Expr(s.into())
}

fn v(x: f64) -> Num {
    Num::Value(x)
}

fn xy(x: Num, y: Num) -> Xy {
    Xy(x, y)
}

fn part(params: &[(&str, f64)]) -> (Session, Id) {
    let mut s = Session::new_part();
    for (k, val) in params {
        s.param_set(k, &Num::Value(*val)).unwrap();
    }
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
    (s, sk)
}

fn extrude(s: &mut Session, sk: Id, h: Num) -> f64 {
    let (_, r) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: h,
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("body".into()),
        })
        .unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.bodies.len(), 1, "{:?}", r.bodies);
    r.bodies[0].volume
}

fn volume(r: &Rebuild) -> f64 {
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r.bodies[0].volume
}

fn dof(s: &Session, sk: Id) -> (i32, i32) {
    s.sketch_info(sk).unwrap().dof
}

// ---------- L-shaped closed polyline ----------

/// An L of outer size a × b, leg thickness t: area a·t + t·(b − t).
fn l_area(a: f64, b: f64, t: f64) -> f64 {
    a * t + t * (b - t)
}

fn l_shape() -> (Session, Id) {
    let (mut s, sk) = part(&[("a", 40.0), ("b", 30.0), ("t", 8.0), ("h", 5.0)]);
    let pts = vec![xy(v(0.0), v(0.0)), xy(n("a"), v(0.0)), xy(n("a"), n("t")), xy(n("t"), n("t")), xy(n("t"), n("b")), xy(v(0.0), n("b"))];
    let added = s.sketch_polyline(sk, &PolylineSpec { points: pts, closed: true, construction: false, dimensioned: true }).unwrap();
    assert_eq!(added.entities.len(), 6);
    assert_eq!(added.points.len(), 6);
    (s, sk)
}

#[test]
fn l_shape_polyline_is_fully_defined_and_extrudes() {
    let (mut s, sk) = l_shape();
    assert_eq!(dof(&s, sk), (0, 0));
    let info = s.sketch_info(sk).unwrap();
    assert_eq!(info.contours.len(), 1);
    assert_close(info.contours[0].area, l_area(40.0, 30.0, 8.0), 1e-6, "contour area");
    let vol = extrude(&mut s, sk, n("h"));
    assert_close(vol, l_area(40.0, 30.0, 8.0) * 5.0, 1e-3, "L volume");
}

#[test]
fn l_shape_follows_its_parameters() {
    let (mut s, sk) = l_shape();
    extrude(&mut s, sk, n("h"));
    let r = s.param_set("a", &v(50.0)).unwrap();
    assert_close(volume(&r), l_area(50.0, 30.0, 8.0) * 5.0, 1e-3, "a=50");
    let r = s.param_set("t", &v(10.0)).unwrap();
    assert_close(volume(&r), l_area(50.0, 30.0, 10.0) * 5.0, 1e-3, "t=10");
    assert_eq!(dof(&s, sk), (0, 0));
}

#[test]
fn l_shape_gui_param_edit_rebuilds() {
    let (mut s, sk) = l_shape();
    extrude(&mut s, sk, n("h"));
    let path = scratch("l_shape_gui.qcad");
    s.save(Some(&path)).unwrap();
    let vol = gui_edit_param(&path, "t", "10");
    assert_close(vol, l_area(40.0, 30.0, 10.0) * 5.0, 1e-3, "GUI t=10");
}

// ---------- line entities ----------

#[test]
fn three_lines_close_a_triangle() {
    let (mut s, sk) = part(&[("a", 30.0)]);
    let line = |x1: Num, y1: Num, x2: Num, y2: Num| LineSpec { x1, y1, x2, y2, construction: false, dimensioned: true };
    s.sketch_line(sk, &line(v(0.0), v(0.0), n("a"), v(0.0))).unwrap();
    s.sketch_line(sk, &line(n("a"), v(0.0), v(0.0), v(20.0))).unwrap();
    let last = s.sketch_line(sk, &line(v(0.0), v(20.0), v(0.0), v(0.0))).unwrap();
    assert_eq!(last.points.len(), 2);
    assert_eq!(dof(&s, sk), (0, 0), "shared endpoints are dimensioned once");
    let detail = s.sketch_detail(sk).unwrap();
    assert_eq!(detail.entities.iter().filter(|e| e.kind == "line").count(), 3);
    assert_eq!(detail.points.iter().filter(|p| p.role.is_none()).count(), 3, "endpoints are shared: {:?}", detail.points);
    assert_close(extrude(&mut s, sk, v(2.0)), 600.0, 1e-3, "triangle");
    let r = s.param_set("a", &v(45.0)).unwrap();
    assert_close(volume(&r), 900.0, 1e-3, "a=45");
}

// ---------- slot ----------

/// Slot of width w with centres l apart: π(w/2)² + w·l.
fn slot_area(l: f64, w: f64) -> f64 {
    PI * (w / 2.0).powi(2) + w * l
}

fn slot() -> (Session, Id) {
    let (mut s, sk) = part(&[("l", 30.0), ("w", 10.0), ("h", 4.0)]);
    let added = s
        .sketch_slot(
            sk,
            &SlotSpec { x1: n("-l/2"), y1: v(0.0), x2: n("l/2"), y2: v(0.0), width: n("w"), construction: false, dimensioned: true },
        )
        .unwrap();
    assert_eq!(added.entities.len(), 4, "two sides and two arcs");
    assert_eq!(added.points.len(), 2);
    (s, sk)
}

#[test]
fn slot_is_fully_defined_and_extrudes() {
    let (mut s, sk) = slot();
    assert_eq!(dof(&s, sk), (0, 0));
    let vol = extrude(&mut s, sk, n("h"));
    assert_close(vol, slot_area(30.0, 10.0) * 4.0, 1e-3, "slot volume");
}

#[test]
fn slot_follows_its_parameters() {
    let (mut s, sk) = slot();
    extrude(&mut s, sk, n("h"));
    let r = s.param_set("w", &v(12.0)).unwrap();
    assert_close(volume(&r), slot_area(30.0, 12.0) * 4.0, 1e-3, "w=12");
    let r = s.param_set("l", &v(40.0)).unwrap();
    assert_close(volume(&r), slot_area(40.0, 12.0) * 4.0, 1e-3, "l=40");
    let bb = r.bodies[0].bbox;
    assert_close(bb[3] - bb[0], 40.0 + 12.0, 0.05, "slot length");
    assert_close(bb[4] - bb[1], 12.0, 0.05, "slot width");
}

#[test]
fn slot_gui_param_edit_rebuilds() {
    let (mut s, sk) = slot();
    extrude(&mut s, sk, n("h"));
    let path = scratch("slot_gui.qcad");
    s.save(Some(&path)).unwrap();
    let vol = gui_edit_param(&path, "w", "14");
    assert_close(vol, slot_area(30.0, 14.0) * 4.0, 1e-3, "GUI w=14");
}

// ---------- regular hexagon ----------

fn hex_area(r: f64) -> f64 {
    3.0 * 3f64.sqrt() / 2.0 * r * r
}

fn hexagon(angle: Option<Num>) -> (Session, Id, Added) {
    let (mut s, sk) = part(&[("r", 10.0), ("px", 5.0), ("rot", 30.0)]);
    let added = s
        .sketch_polygon(
            sk,
            &PolygonSpec {
                cx: n("px"),
                cy: v(-3.0),
                sides: 6,
                r: Some(n("r")),
                angle,
                vertex: None,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    assert_eq!(added.points.len(), 7, "centre + 6 vertices");
    (s, sk, added)
}

#[test]
fn hexagon_is_fully_defined_and_extrudes() {
    let (mut s, sk, _) = hexagon(None);
    assert_eq!(dof(&s, sk), (0, 0));
    let vol = extrude(&mut s, sk, v(3.0));
    assert_close(vol, hex_area(10.0) * 3.0, 1e-3, "hexagon volume");
    let r = s.param_set("r", &v(12.0)).unwrap();
    assert_close(volume(&r), hex_area(12.0) * 3.0, 1e-3, "r=12");
    let bb = r.bodies[0].bbox;
    assert_close(bb[3] - bb[0], 24.0, 0.05, "across corners (vertex at angle 0)");
    assert_close(bb[4] - bb[1], 12.0 * 3f64.sqrt(), 0.05, "across flats");
    assert_close((bb[0] + bb[3]) / 2.0, 5.0, 0.05, "centre x = px");
}

#[test]
fn hexagon_rotation_follows_an_expression() {
    let (mut s, sk, added) = hexagon(Some(n("rot")));
    assert_eq!(dof(&s, sk), (0, 0));
    let vertex = |s: &Session| {
        let d = s.sketch_detail(sk).unwrap();
        let p = d.points.iter().find(|p| p.id == added.points[1]).unwrap();
        (p.x, p.y)
    };
    let (x, y) = vertex(&s);
    assert_close(x, 5.0 + 10.0 * 30f64.to_radians().cos(), 1e-6, "vertex x at 30°");
    assert_close(y, -3.0 + 10.0 * 30f64.to_radians().sin(), 1e-6, "vertex y at 30°");
    s.param_set("rot", &v(45.0)).unwrap();
    let (x, y) = vertex(&s);
    assert_close(x, 5.0 + 10.0 * 45f64.to_radians().cos(), 1e-6, "vertex x at 45°");
    assert_close(y, -3.0 + 10.0 * 45f64.to_radians().sin(), 1e-6, "vertex y at 45°");
}

#[test]
fn hexagon_from_a_vertex() {
    let (mut s, sk) = part(&[("ve", 8.0)]);
    s.sketch_polygon(
        sk,
        &PolygonSpec {
            cx: v(0.0),
            cy: v(0.0),
            sides: 6,
            r: None,
            angle: None,
            vertex: Some(xy(n("ve"), v(0.0))),
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    assert_eq!(dof(&s, sk), (0, 0));
    assert_close(extrude(&mut s, sk, v(1.0)), hex_area(8.0), 1e-3, "vertex form");
    let r = s.param_set("ve", &v(9.0)).unwrap();
    assert_close(volume(&r), hex_area(9.0), 1e-3, "ve=9");
}

// ---------- profiles with arcs ----------

/// A tombstone: a w × hh rectangle under a half-disc of diameter w.
fn tomb_area(w: f64, hh: f64) -> f64 {
    w * hh + PI * (w / 2.0).powi(2) / 2.0
}

fn tombstone() -> (Session, Id) {
    let (mut s, sk) = part(&[("w", 20.0), ("hh", 15.0)]);
    let pts = vec![xy(n("-w/2"), n("hh")), xy(n("-w/2"), v(0.0)), xy(n("w/2"), v(0.0)), xy(n("w/2"), n("hh"))];
    s.sketch_polyline(sk, &PolylineSpec { points: pts, closed: false, construction: false, dimensioned: true }).unwrap();
    let arc = s
        .sketch_arc(
            sk,
            &ArcSpec {
                cx: v(0.0),
                cy: n("hh"),
                r: Some(n("w/2")),
                start_angle: Some(v(0.0)),
                end_angle: Some(v(180.0)),
                start: None,
                end: None,
                ccw: true,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    assert_eq!(arc.entities.len(), 1);
    (s, sk)
}

#[test]
fn arc_joins_a_polyline_into_a_tombstone() {
    let (mut s, sk) = tombstone();
    assert_eq!(dof(&s, sk), (0, 0), "{:?}", s.sketch_detail(sk).unwrap().constraints);
    assert_eq!(s.sketch_info(sk).unwrap().contours.len(), 1, "the arc closes the polyline");
    assert_close(extrude(&mut s, sk, v(2.0)), tomb_area(20.0, 15.0) * 2.0, 1e-3, "tombstone");
    let r = s.param_set("w", &v(24.0)).unwrap();
    assert_close(volume(&r), tomb_area(24.0, 15.0) * 2.0, 1e-3, "w=24");
    let r = s.param_set("hh", &v(10.0)).unwrap();
    assert_close(volume(&r), tomb_area(24.0, 10.0) * 2.0, 1e-3, "hh=10");
}

/// A circular sector: radius r between angles a0 and a1, area r²·(a1 − a0)/2.
fn sector() -> (Session, Id) {
    let (mut s, sk) = part(&[("r", 20.0), ("a0", 30.0), ("a1", 120.0)]);
    s.sketch_arc(
        sk,
        &ArcSpec {
            cx: v(0.0),
            cy: v(0.0),
            r: Some(n("r")),
            start_angle: Some(n("a0")),
            end_angle: Some(n("a1")),
            start: None,
            end: None,
            ccw: true,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    let pts = vec![xy(n("r*cos(a1)"), n("r*sin(a1)")), xy(v(0.0), v(0.0)), xy(n("r*cos(a0)"), n("r*sin(a0)"))];
    s.sketch_polyline(sk, &PolylineSpec { points: pts, closed: false, construction: false, dimensioned: true }).unwrap();
    (s, sk)
}

fn sector_area(r: f64, deg: f64) -> f64 {
    r * r * deg.to_radians() / 2.0
}

#[test]
fn sector_from_an_arc_and_two_radii() {
    let (mut s, sk) = sector();
    assert_eq!(dof(&s, sk), (0, 0));
    assert_close(extrude(&mut s, sk, v(3.0)), sector_area(20.0, 90.0) * 3.0, 1e-3, "sector");
    let r = s.param_set("r", &v(25.0)).unwrap();
    assert_close(volume(&r), sector_area(25.0, 90.0) * 3.0, 1e-3, "r=25");
    let r = s.param_set("a1", &v(150.0)).unwrap();
    assert_close(volume(&r), sector_area(25.0, 120.0) * 3.0, 1e-3, "a1=150");
}

#[test]
fn sector_gui_param_edit_rebuilds() {
    let (mut s, sk) = sector();
    extrude(&mut s, sk, v(3.0));
    let path = scratch("sector_gui.qcad");
    s.save(Some(&path)).unwrap();
    assert_close(gui_edit_param(&path, "a0", "60"), sector_area(20.0, 60.0) * 3.0, 1e-3, "GUI a0=60");
}

#[test]
fn arc_from_points_is_fully_defined() {
    // A quarter disc: arc from (r, 0) to (0, r) around the origin, closed by two lines.
    let (mut s, sk) = part(&[("r", 10.0)]);
    s.sketch_arc(
        sk,
        &ArcSpec {
            cx: v(0.0),
            cy: v(0.0),
            r: None,
            start_angle: None,
            end_angle: None,
            start: Some(xy(n("r"), v(0.0))),
            end: Some(xy(v(0.0), n("r"))),
            ccw: true,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    assert_eq!(dof(&s, sk), (0, 0));
    let pts = vec![xy(v(0.0), n("r")), xy(v(0.0), v(0.0)), xy(n("r"), v(0.0))];
    s.sketch_polyline(sk, &PolylineSpec { points: pts, closed: false, construction: false, dimensioned: true }).unwrap();
    assert_eq!(dof(&s, sk), (0, 0));
    assert_close(extrude(&mut s, sk, v(1.0)), PI * 100.0 / 4.0, 1e-3, "quarter disc");
    let r = s.param_set("r", &v(12.0)).unwrap();
    assert_close(volume(&r), PI * 144.0 / 4.0, 1e-3, "r=12");
}

#[test]
fn new_entities_survive_save_and_open() {
    let (mut s, sk) = slot();
    extrude(&mut s, sk, n("h"));
    let path = scratch("slot_roundtrip.qcad");
    s.save(Some(&path)).unwrap();
    let (mut o, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_close(o.result_bodies()[0].volume, slot_area(30.0, 10.0) * 4.0, 1e-3, "after reopen");
    assert_eq!(dof(&o, sk), (0, 0));
    let r = o.param_set("w", &v(8.0)).unwrap();
    assert_close(volume(&r), slot_area(30.0, 8.0) * 4.0, 1e-3, "edit after reopen");
}

#[test]
fn sector_gui_radius_edit_rebuilds() {
    // A radius change moves both arc ends; QymCAD's single GUI solve must settle it (FINDINGS F-3A-2).
    let (mut s, sk) = sector();
    extrude(&mut s, sk, v(3.0));
    let path = scratch("sector_gui_r.qcad");
    s.save(Some(&path)).unwrap();
    assert_close(gui_edit_param(&path, "r", "40"), sector_area(40.0, 90.0) * 3.0, 1e-3, "GUI r=40");
}

#[test]
fn hexagon_gui_radius_edit_with_rotation() {
    let (mut s, sk, _) = hexagon(Some(n("rot")));
    extrude(&mut s, sk, v(3.0));
    let path = scratch("hexagon_gui_r.qcad");
    s.save(Some(&path)).unwrap();
    assert_close(gui_edit_param(&path, "r", "20"), hex_area(20.0) * 3.0, 1e-3, "GUI r=20");
}

// ---------- parameters that change the sign of a coordinate (review round 1, #1, #4) ----------

/// rot 30° → 120°: cos changes sign. The first vertex must sit at 120° and the hexagon's x extent becomes the
/// across-corners size 2r (vertices at 0°/180°).
#[test]
fn hexagon_rotation_crosses_90_degrees() {
    let (mut s, sk, added) = hexagon(Some(n("rot")));
    extrude(&mut s, sk, v(3.0));
    let r = s.param_set("rot", &v(120.0)).unwrap();
    let d = s.sketch_detail(sk).unwrap();
    let p = d.points.iter().find(|p| p.id == added.points[1]).unwrap();
    assert_close(p.x, 5.0 + 10.0 * 120f64.to_radians().cos(), 1e-6, "vertex x at 120°");
    assert_close(p.y, -3.0 + 10.0 * 120f64.to_radians().sin(), 1e-6, "vertex y at 120°");
    let bb = r.bodies[0].bbox;
    assert_close(bb[3] - bb[0], 20.0, 0.05, "x extent = 2r");
    assert_close(volume(&r), hex_area(10.0) * 3.0, 1e-3, "volume");
}

#[test]
fn hexagon_rotation_crosses_90_degrees_gui() {
    let (mut s, sk, _) = hexagon(Some(n("rot")));
    extrude(&mut s, sk, v(3.0));
    let path = scratch("hexagon_rot_cross_gui.qcad");
    s.save(Some(&path)).unwrap();
    let (vol, bb) = gui_edit_param_body(&path, "rot", "120");
    assert_close(vol, hex_area(10.0) * 3.0, 1e-3, "GUI volume");
    assert_close(bb[3] - bb[0], 20.0, 0.05, "GUI x extent = 2r");
    assert_close((bb[0] + bb[3]) / 2.0, 5.0, 0.05, "GUI centre x");
}

/// a1 120° → 240°: the sector grows from 90° to 210° (cos 120° = cos 240°, so an x dimension alone cannot tell).
#[test]
fn sector_end_angle_crosses_180_degrees() {
    let (mut s, sk) = sector();
    extrude(&mut s, sk, v(3.0));
    let r = s.param_set("a1", &v(240.0)).unwrap();
    assert_close(volume(&r), sector_area(20.0, 210.0) * 3.0, 1e-3, "a1=240");
}

#[test]
fn sector_end_angle_crosses_180_degrees_gui() {
    let (mut s, sk) = sector();
    extrude(&mut s, sk, v(3.0));
    let path = scratch("sector_cross_gui.qcad");
    s.save(Some(&path)).unwrap();
    assert_close(gui_edit_param(&path, "a1", "240"), sector_area(20.0, 210.0) * 3.0, 1e-3, "GUI a1=240");
}

/// A 10 × 10 square centred at x = t − 20, t 10 → 30: the centre moves from −10 to +10.
fn shifted_square() -> (Session, Id) {
    let (mut s, sk) = part(&[("t", 10.0)]);
    s.sketch_rect(sk, &n("t-20"), &v(0.0), &v(10.0), &v(10.0), false).unwrap();
    extrude(&mut s, sk, v(1.0));
    (s, sk)
}

#[test]
#[ignore = "QymCAD dimensions keep their side (F-3A-7): a coordinate expression cannot cross zero; design decision pending"]
fn a_coordinate_crosses_zero() {
    let (mut s, _) = shifted_square();
    let r = s.param_set("t", &v(30.0)).unwrap();
    let bb = r.bodies[0].bbox;
    assert_close((bb[0] + bb[3]) / 2.0, 10.0, 0.05, "centre x = t − 20");
}

/// Until it is supported, crossing zero must fail loudly and leave the document as it was (review #1/#2).
#[test]
fn a_coordinate_that_would_cross_zero_is_refused() {
    let (mut s, _) = shifted_square();
    let e = s.param_set("t", &v(30.0)).unwrap_err();
    assert!(e.to_string().contains("does not solve"), "{e}");
    assert_eq!(s.params().iter().find(|p| p.name == "t").unwrap().value, 10.0, "parameter restored");
    let bb = s.result_bodies()[0].bbox;
    assert_close((bb[0] + bb[3]) / 2.0, -10.0, 0.05, "geometry restored");
}

/// Review #7: a parametric radius with a literal, non-cardinal rotation. The GUI path solves once, so the rotation
/// must not be an angle dimension (F-3A-2). After r 10 → 20 at 30° the x extent is across flats: √3·r.
#[test]
fn hexagon_parametric_radius_literal_angle_gui() {
    let (mut s, sk, _) = hexagon(Some(v(30.0)));
    assert_eq!(dof(&s, sk), (0, 0));
    extrude(&mut s, sk, v(3.0));
    let path = scratch("hexagon_lit_angle_gui.qcad");
    s.save(Some(&path)).unwrap();
    let (vol, bb) = gui_edit_param_body(&path, "r", "20");
    assert_close(vol, hex_area(20.0) * 3.0, 1e-3, "GUI r=20");
    assert_close(bb[3] - bb[0], 20.0 * 3f64.sqrt(), 0.05, "GUI x extent across flats");
}
