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
    build_impl("direct", true)
}

fn build_offset(mode: &str) -> Session {
    build_impl(mode, false)
}

fn build_impl(mode: &str, pocket_dimensions: bool) -> Session {
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
    let (base, dist) = match mode {
        "direct" => (PlaneRef::Base(BaseName::XY), n("T")),
        "parent" => {
            let (base, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &n("t-2"), Some("parent")).unwrap();
            (PlaneRef::Plane(base), 2.0.into())
        }
        "child" => {
            let (base, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &2.0.into(), Some("parent")).unwrap();
            (PlaneRef::Plane(base), n("t-2"))
        }
        "both" => {
            let (base, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &n("t/2"), Some("parent")).unwrap();
            (PlaneRef::Plane(base), n("t/2"))
        }
        _ => unreachable!(),
    };
    let (top, _) = s.plane_offset(&base, &dist, Some("top")).unwrap();
    let pocket = s.sketch_create(&PlaneRef::Plane(top), Some("pocket sketch")).unwrap();
    // Literal pocket dimensions reproduce the MCP recipe: unrelated sketch expressions can mask the bug
    // by making the sketch dirty before the datum resolves.
    let (pw, pl) = if pocket_dimensions { (n("PW"), n("pl")) } else { (PW.into(), PL.into()) };
    s.sketch_rect(pocket, &0.0.into(), &0.0.into(), &pw, &pl, false).unwrap();
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
    check_pocket(&mut s, 10.0);
    assert_close(r.bodies[0].volume, expected(10.0), 1e-3, "volume after t=10");
    let r = s.param_set("hx", &Num::Value(25.0)).unwrap();
    check_pocket(&mut s, 10.0);
    assert_close(r.bodies[0].volume, expected(10.0), 1e-3, "moving holes keeps volume");
    let holes: Vec<_> = s.topology(None, false).unwrap().faces.into_iter().filter(|f| f.kind == FaceKind::Cylinder).collect();
    assert_eq!(holes.len(), 4);
    for hole in holes {
        let at = hole.axis.unwrap()[0];
        assert_close(at[0].abs(), 25.0, 1e-6, "hole x follows hx");
        assert_close(at[1].abs(), 12.0, 1e-6, "hole y");
    }
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
    let mut s = build_offset("direct");
    let path = scratch("plate_gui.qcad");
    s.save(Some(&path)).unwrap();
    check_gui_pocket(&path);
}

fn check_pocket(s: &mut Session, t: f64) {
    assert_top_pocket(s, W, L, t, PW, PL, PD, 4.0 * PI * (D / 2.0).powi(2), 15);
    assert!(s.info().errors.is_empty());
}

fn check_gui_pocket(path: &std::path::Path) {
    let (v, _, faces) = gui_edit_param_faces(path, "t", "10");
    let geometry: Vec<_> = faces.iter().map(|f| (f.centroid.z, f.normal, f.area)).collect();
    assert_pocket_faces(&geometry, W, L, 10.0, PW, PL, PD, 4.0 * PI * (D / 2.0).powi(2), 15);
    assert_close(v, expected(10.0), 1e-3, "volume after GUI edit t=10");
}

#[test]
fn reopened_param_edit_moves_the_pocket() {
    let mut s = build_offset("direct");
    let path = scratch("plate_reopened_edit.qcad");
    s.save(Some(&path)).unwrap();
    let (mut s, _) = Session::open(&path).unwrap();
    let r = s.param_set("t", &10.0.into()).unwrap();
    check_pocket(&mut s, 10.0);
    assert_close(r.bodies[0].volume, expected(10.0), 1e-3, "reopened edit volume");
}

#[test]
fn literal_pocket_param_edit_moves_the_pocket() {
    let mut s = build_offset("direct");
    let r = s.param_set("t", &10.0.into()).unwrap();
    check_pocket(&mut s, 10.0);
    assert_close(r.bodies[0].volume, expected(10.0), 1e-3, "literal pocket edit volume");
}

#[test]
fn chained_datum_param_edit_moves_the_pocket() {
    for mode in ["parent", "child", "both"] {
        let mut s = build_offset(mode);
        check_pocket(&mut s, 6.0);
        let r = s.param_set("t", &10.0.into()).unwrap();
        check_pocket(&mut s, 10.0);
        assert_close(r.bodies[0].volume, expected(10.0), 1e-3, "chained edit volume");
    }
}

#[test]
fn chained_datum_gui_edit_moves_the_pocket() {
    for mode in ["parent", "child", "both"] {
        let mut s = build_offset(mode);
        let path = scratch(&format!("plate_chained_gui_{mode}.qcad"));
        s.save(Some(&path)).unwrap();
        check_gui_pocket(&path);
    }
}

#[test]
fn chained_datum_reopened_edit_moves_the_pocket() {
    for mode in ["parent", "child", "both"] {
        let mut s = build_offset(mode);
        let path = scratch(&format!("plate_chained_open_{mode}.qcad"));
        s.save(Some(&path)).unwrap();
        let (mut s, _) = Session::open(&path).unwrap();
        let r = s.param_set("t", &10.0.into()).unwrap();
        check_pocket(&mut s, 10.0);
        assert_close(r.bodies[0].volume, expected(10.0), 1e-3, "chained reopened volume");
    }
}

#[test]
fn opening_legacy_datum_sketches_repairs_server_and_saved_gui_edits() {
    for mode in ["direct", "parent", "child", "both"] {
        let mut s = build_offset(mode);
        let path = scratch(&format!("plate_legacy_{mode}.qcad"));
        s.save(Some(&path)).unwrap();
        let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
        for sketch in &project.sketches {
            project.feat_dims.remove(&sketch.id);
        }
        qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
        let (mut s, r) = Session::open(&path).unwrap();
        assert!(r.errors.is_empty(), "{r:?}");
        s.save(Some(&path)).unwrap();
        check_gui_pocket(&path);
        s.param_set("t", &10.0.into()).unwrap();
        check_pocket(&mut s, 10.0);
        assert_close(s.result_bodies()[0].volume, expected(10.0), 1e-3, "legacy edit volume");
    }
}
