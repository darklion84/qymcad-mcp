//! Golden part: a parametric plate w×l×t with four through holes Ø d at (±hx, ±hy) and a top pocket pw×pl,
//! depth pd. Expected volumes are computed by hand.

mod common;
use common::*;
use qymcad_engine::*;
use std::f64::consts::PI;

const W: f64 = 60.0;
const L: f64 = 40.0;
const D: f64 = 4.5;
const PW: f64 = 30.0;
const PL: f64 = 16.0;
const PD: f64 = 3.0;

fn expected(t: f64) -> f64 {
    W * L * t - 4.0 * PI * (D / 2.0).powi(2) * t - PW * PL * PD
}

fn n(s: &str) -> Num {
    Num::Expr(s.into())
}

/// Build the plate through the engine API only. Parameter names deliberately use uppercase input: the engine
/// must store them lowercase (FINDINGS F-001).
fn build() -> Session {
    let mut s = Session::new_part();
    for (k, v) in [("W", W), ("L", L), ("T", 6.0), ("D", D), ("HX", 22.0), ("HY", 12.0), ("PW", PW), ("PL", PL), ("PD", PD)] {
        s.param_set(k, &Num::Value(v)).unwrap();
    }
    let plate = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("plate sketch")).unwrap();
    s.sketch_rect(plate, &0.0.into(), &0.0.into(), &n("w"), &n("l"), false).unwrap();
    for (x, y) in [("hx", "hy"), ("-hx", "hy"), ("-hx", "-hy"), ("hx", "-hy")] {
        s.sketch_circle(plate, &n(x), &n(y), &n("d"), false).unwrap();
    }
    let (body, r) = s
        .extrude(&Extrude {
            sketch: plate,
            profiles: None,
            height: n("t"),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("plate".into()),
        })
        .unwrap();
    assert_eq!(r.bodies.len(), 1);
    assert_eq!(r.bodies[0].id, body);
    let (top, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &n("T"), Some("top")).unwrap();
    let pocket = s.sketch_create(&PlaneRef::Plane(top), Some("pocket sketch")).unwrap();
    s.sketch_rect(pocket, &0.0.into(), &0.0.into(), &n("PW"), &n("pl"), false).unwrap();
    s.extrude(&Extrude {
        sketch: pocket,
        profiles: None,
        height: n("pd"),
        op: Op::Cut,
        direction: Direction::Reverse,
        through: false,
        target: None,
        name: Some("pocket".into()),
    })
    .unwrap();
    s
}

#[test]
fn plate_volume_and_bbox() {
    let s = build();
    let b = s.result_bodies();
    assert_eq!(b.len(), 1, "one result body: {b:?}");
    assert_close(b[0].volume, expected(6.0), 1e-3, "volume");
    let bb = b[0].bbox;
    assert_close(bb[3] - bb[0], W, 0.05, "size x");
    assert_close(bb[4] - bb[1], L, 0.05, "size y");
    assert_close(bb[5] - bb[2], 6.0, 0.05, "size z");
    assert!(s.info().errors.is_empty());
}

#[test]
fn params_are_stored_lowercase() {
    let s = build();
    let names: Vec<String> = s.params().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["w", "l", "t", "d", "hx", "hy", "pw", "pl", "pd"]);
    assert!(s.uppercase_params().is_empty());
}

#[test]
fn sketches_are_fully_defined() {
    let s = build();
    for sk in s.sketches() {
        assert_eq!(sk.dof, (0, 0), "sketch {} `{}` dof", sk.id, sk.name);
    }
}

#[test]
fn param_change_rebuilds() {
    let mut s = build();
    let r = s.param_set("t", &Num::Value(10.0)).unwrap();
    assert_close(r.bodies[0].volume, expected(10.0), 1.0, "volume after t=10");
    let r = s.param_set("hx", &Num::Value(25.0)).unwrap();
    assert_close(r.bodies[0].volume, expected(10.0), 1.0, "moving holes keeps volume");
}

#[test]
fn save_open_roundtrip() {
    let mut s = build();
    let path = scratch("plate_roundtrip.qcad");
    s.save(Some(&path)).unwrap();
    let (o, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_close(o.result_bodies()[0].volume, expected(6.0), 1e-3, "volume after reopen");
    assert_eq!(o.params(), s.params());
}

/// The regression this whole design hinges on: a person edits a parameter in the QymCAD GUI and the body
/// rebuilds (FINDINGS F-001).
#[test]
fn gui_param_edit_rebuilds_the_body() {
    let mut s = build();
    let path = scratch("plate_gui.qcad");
    s.save(Some(&path)).unwrap();
    let v = gui_edit_param(&path, "t", "10");
    assert_close(v, expected(10.0), 1.0, "volume after GUI edit t=10");
}
