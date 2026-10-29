//! Native circles touching two curves, with independently derived centre and radius.
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
fn supports() -> (Session, Id, [Id; 2]) {
    let mut s = Session::new_part();
    s.param_set("R", &v(2.0)).unwrap();
    s.param_set("X", &v(10.0)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let a =
        s.sketch_line(sk, &LineSpec { x1: e("X"), y1: v(0.0), x2: e("X"), y2: v(20.0), construction: true, dimensioned: true }).unwrap();
    let b =
        s.sketch_line(sk, &LineSpec { x1: v(0.0), y1: v(10.0), x2: v(30.0), y2: v(10.0), construction: true, dimensioned: true }).unwrap();
    (s, sk, [a.entities[0], b.entities[0]])
}
fn spec(ids: [Id; 2], near: [f64; 2]) -> TangentCircleSpec {
    TangentCircleSpec { curves: ids.map(EntityRef::Id), radius: e("R"), near: near.map(v), construction: false }
}
fn centre(s: &Session, sk: Id, a: &Added, x: f64, y: f64, r: f64) {
    let d = s.sketch_detail(sk).unwrap();
    let p = d.points.iter().find(|p| p.id == a.points[0]).unwrap();
    assert_close(p.x, x, 1e-6, "circle centre x from support+r");
    assert_close(p.y, y, 1e-6, "circle centre y from support+r");
    assert_close(d.entities.iter().find(|e| e.id == a.entities[0]).unwrap().r.unwrap(), r, 1e-6, "driving radius");
    assert_eq!(d.summary.dof, (0, 0));
}
#[test]
fn tangent_circle_radius_and_support_formulas_rebuild_after_open_and_in_app() {
    let (mut s, sk, ids) = supports();
    let a = s.sketch_circle_tangent(sk, &spec(ids, [12.0, 12.0])).unwrap();
    centre(&s, sk, &a, 12.0, 12.0, 2.0);
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
    assert!(r.errors.is_empty());
    assert_close(r.bodies[0].volume, PI * 4.0 * 3.0, 1e-7, "tangent circle volume");
    let path = scratch("stage1-tangent-circle.qcad");
    s.save(Some(&path)).unwrap();
    let (mut reopened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    centre(&reopened, sk, &a, 12.0, 12.0, 2.0);
    let r = reopened.param_set("R", &v(3.0)).unwrap();
    assert!(r.errors.is_empty());
    centre(&reopened, sk, &a, 13.0, 13.0, 3.0);
    assert_close(r.bodies[0].volume, PI * 9.0 * 3.0, 1e-6, "radius parameter volume");
    assert_close(gui_edit_param(&path, "R", "3"), PI * 9.0 * 3.0, 1e-6, "app radius parameter volume");
    let r = s.param_set("X", &v(15.0)).unwrap();
    assert!(r.errors.is_empty());
    centre(&s, sk, &a, 17.0, 12.0, 2.0);
    let (volume, bbox) = gui_edit_param_body(&path, "X", "15");
    assert_close(volume, PI * 4.0 * 3.0, 1e-6, "app moved tangent circle");
    // Native padded bbox after its own tessellation has a small tolerance; centre is independent of padding.
    assert_close((bbox[0] + bbox[3]) / 2.0, 17.0, 0.01, "app tangent centre");
    assert_close((bbox[1] + bbox[4]) / 2.0, 12.0, 0.01, "app tangent centre y");
}
#[test]
fn tangent_circle_refuses_ties_missing_solutions_and_ambiguous_selectors_before_mutation() {
    let (mut s, sk, ids) = supports();
    let before = s.sketch_detail(sk).unwrap();
    let err = s.sketch_circle_tangent(sk, &spec(ids, [10.0, 10.0])).unwrap_err().to_string();
    assert!(err.contains("ambiguous"), "{err}");
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
    let mut x = spec(ids, [12.0, 12.0]);
    x.curves[0] = EntityRef::Query(CurveQuery { kind: Some(SketchCurveKind::Line), construction: None, nearest: None, within: None });
    assert!(s.sketch_circle_tangent(sk, &x).unwrap_err().to_string().contains("ambiguous"));
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
    x = spec(ids, [12.0, 12.0]);
    x.curves[1] = x.curves[0].clone();
    assert!(s.sketch_circle_tangent(sk, &x).is_err());
    // Below the native 0.01 mm minimum the constructor would draw a bigger circle than asked: refused up front.
    x = spec(ids, [12.0, 12.0]);
    x.radius = Num::Value(0.004);
    let err = s.sketch_circle_tangent(sk, &x).unwrap_err().to_string();
    assert!(err.contains("0.01"), "{err}");
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
    x = spec(ids, [12.0, 12.0]);
    x.radius = v(-1.0);
    assert!(s.sketch_circle_tangent(sk, &x).is_err());
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
    let extra =
        s.sketch_line(sk, &LineSpec { x1: v(0.0), y1: v(30.0), x2: v(30.0), y2: v(30.0), construction: true, dimensioned: true }).unwrap();
    let before = s.sketch_detail(sk).unwrap();
    x = spec([ids[1], extra.entities[0]], [12.0, 20.0]);
    assert!(s.sketch_circle_tangent(sk, &x).unwrap_err().to_string().contains("no tangent circle"));
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
}
