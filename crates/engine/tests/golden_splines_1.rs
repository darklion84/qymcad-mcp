//! Native interpolation splines, with independent polynomial quadrature and app-path rebuilds.
mod common;
use common::*;
use qymcad_engine::*;

fn e(s: &str) -> Num {
    Num::Expr(s.into())
}
fn v(x: f64) -> Num {
    Num::Value(x)
}

fn fixture(closed: bool) -> (Session, Id, Added) {
    let mut s = Session::new_part();
    s.param_set("L", &v(20.0)).unwrap();
    s.param_set("H", &v(10.0)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("Spline")).unwrap();
    let points = if closed {
        vec![Xy(v(2.0), v(3.0)), Xy(e("L+2"), v(3.0)), Xy(e("L+2"), e("H+3")), Xy(v(2.0), e("H+3"))]
    } else {
        vec![Xy(v(2.0), v(3.0)), Xy(e("L/2+2"), e("H+3")), Xy(e("L+2"), v(3.0))]
    };
    let a = s.sketch_spline(sk, &SplineSpec { points, closed, construction: false, dimensioned: true }).unwrap();
    if !closed {
        s.sketch_line(sk, &LineSpec { x1: v(2.0), y1: v(3.0), x2: e("L+2"), y2: v(3.0), construction: false, dimensioned: true }).unwrap();
    }
    (s, sk, a)
}

fn solid(s: &mut Session, sk: Id) -> f64 {
    let (_, r) = s
        .extrude(&Extrude {
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

// Independent composite Simpson integration of the natural interpolant. On each half of the
// symmetric arch x is linear, and y/H = (3u-u³)/2. Simpson integrates this cubic exactly;
// 1000 panels bounds accumulated floating arithmetic far below 1e-9 mm² for this fixture.
fn arch_area(l: f64, h: f64) -> f64 {
    let n = 1000;
    let f = |u: f64| h * (3.0 * u - u * u * u) / 2.0;
    let mut sum = f(0.0) + f(1.0);
    for i in 1..n {
        sum += if i % 2 == 0 { 2.0 } else { 4.0 } * f(i as f64 / n as f64);
    }
    l * sum / (3.0 * n as f64)
}

#[test]
fn spline_arch_formulas_survive_server_open_and_app_edits() {
    let (mut s, sk, a) = fixture(false);
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    let detail = s.sketch_detail(sk).unwrap();
    let curve = detail.entities.iter().find(|c| c.id == a.entities[0]).expect("spline must be visible");
    assert_eq!(curve.kind, "spline");
    assert_eq!(curve.points, a.points);
    assert_eq!(curve.closed, Some(false));
    assert_close(arch_area(20.0, 10.0), 125.0, 1e-9, "independent cubic quadrature");
    // Native default BRepGProp volume quadrature has a measured 0.00416 mm³ error here,
    // increasing to 0.00665 at H=16. A fixture-only .02 mm³ allowance covers both (stage-0 probe).
    assert_close(solid(&mut s, sk), arch_area(20.0, 10.0) * 3.0, 0.02, "spline arch volume");
    let path = scratch("stage1-spline-arch.qcad");
    s.save(Some(&path)).unwrap();
    let (mut reopened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_close(r.bodies[0].volume, arch_area(20.0, 10.0) * 3.0, 0.02, "reopened spline arch");
    assert_eq!(reopened.sketch_detail(sk).unwrap().entities[0].points, a.points);
    let r = reopened.param_set("H", &v(16.0)).unwrap();
    assert_close(r.bodies[0].volume, arch_area(20.0, 16.0) * 3.0, 0.02, "server node expression edit");
    assert_eq!(reopened.sketch_info(sk).unwrap().dof, (0, 0));
    assert_close(gui_edit_param(&path, "H", "16"), arch_area(20.0, 16.0) * 3.0, 0.02, "app node expression edit");
    let r = s.param_set("L", &v(24.0)).unwrap();
    assert_close(r.bodies[0].volume, arch_area(24.0, 10.0) * 3.0, 0.02, "server horizontal node expression");
    assert_close(gui_edit_param(&path, "L", "24"), arch_area(24.0, 10.0) * 3.0, 0.02, "app horizontal node expression");
}

// Independent Hermite line integral for the four-node periodic interpolant. Symmetry in
// its C2 equations gives derivatives (a, +/-b), where a=3H/(L+3H), b=3L/(3L+H).
// Integrate (x*y' - y*x')/2 on each polynomial span, without querying native curves.
fn closed_area(l: f64, h: f64) -> f64 {
    let a = 3.0 * h / (l + 3.0 * h);
    let b = 3.0 * l / (3.0 * l + h);
    let pts = [[0.0, 0.0], [l, 0.0], [l, h], [0.0, h]];
    let derivatives = [[a, -b], [a, b], [-a, b], [-a, -b]];
    let integrate = |n: usize| {
        let mut area = 0.0;
        for k in 0..4 {
            let j = (k + 1) % 4;
            let chord = if k % 2 == 0 { l } else { h };
            let f = |t: f64| {
                let mut pos = [0.0; 2];
                let mut vel = [0.0; 2];
                for d in 0..2 {
                    let p = pts[k][d];
                    let q = pts[j][d];
                    let u = derivatives[k][d] * chord;
                    let v = derivatives[j][d] * chord;
                    pos[d] = (2.0 * t * t * t - 3.0 * t * t + 1.0) * p
                        + (t * t * t - 2.0 * t * t + t) * u
                        + (-2.0 * t * t * t + 3.0 * t * t) * q
                        + (t * t * t - t * t) * v;
                    vel[d] = (6.0 * t * t - 6.0 * t) * p
                        + (3.0 * t * t - 4.0 * t + 1.0) * u
                        + (-6.0 * t * t + 6.0 * t) * q
                        + (3.0 * t * t - 2.0 * t) * v;
                }
                (pos[0] * vel[1] - pos[1] * vel[0]) / 2.0
            };
            let mut sum = f(0.0) + f(1.0);
            for i in 1..n {
                sum += if i % 2 == 0 { 2.0 } else { 4.0 } * f(i as f64 / n as f64);
            }
            area += sum / (3.0 * n as f64);
        }
        area
    };
    let fine = integrate(2000);
    assert_close(fine, integrate(1000), 1e-8, "closed spline independent quadrature convergence");
    fine
}

#[test]
fn closed_spline_is_native_and_keeps_node_formulas() {
    let (mut s, sk, a) = fixture(true);
    let d = s.sketch_detail(sk).unwrap();
    assert_eq!(d.summary.dof, (0, 0));
    assert_eq!(d.entities[0].closed, Some(true));
    assert_eq!(d.entities[0].points, a.points);
    assert_eq!(d.summary.contours.len(), 1);
    assert_close(closed_area(20.0, 10.0), 344.0, 1e-8, "periodic cubic area");
    let original = solid(&mut s, sk);
    // Closed-fixture native BRep volume integration differs by +0.02904 mm3 at L20/H10
    // and -0.19759 mm3 at L20/H12. The fixture-only 0.25 mm3 allowance covers both
    // (less than 0.022%); independent polynomial quadrature converges within 1e-8 mm2.
    assert_close(original, closed_area(20.0, 10.0) * 3.0, 0.25, "closed spline independent integral");
    let path = scratch("stage1-spline-closed.qcad");
    s.save(Some(&path)).unwrap();
    let (mut reopened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_close(r.bodies[0].volume, original, 1e-8, "closed spline persisted volume");
    let server = reopened.param_set("H", &v(12.0)).unwrap();
    let app = gui_edit_param(&path, "H", "12");
    assert!(server.errors.is_empty());
    assert_eq!(reopened.sketch_info(sk).unwrap().dof, (0, 0));
    assert!((app - original).abs() > 1.0, "closed spline formula changes geometry");
    assert_close(server.bodies[0].volume, app, 1e-7, "closed spline native app recipe");
    assert_close(app, closed_area(20.0, 12.0) * 3.0, 0.25, "closed spline edited independent integral");
}

#[test]
fn spline_validation_and_free_construction_are_atomic() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    for (points, closed) in [
        (vec![], false),
        (vec![Xy(v(1.0), v(2.0))], false),
        (vec![Xy(v(1.0), v(2.0)), Xy(v(3.0), v(4.0))], true),
        (vec![Xy(v(1.0), v(2.0)), Xy(v(1.0), v(2.0))], false),
    ] {
        let before = s.sketch_detail(sk).unwrap();
        assert!(s.sketch_spline(sk, &SplineSpec { points, closed, construction: false, dimensioned: true }).is_err());
        assert_eq!(s.sketch_detail(sk).unwrap(), before);
    }
    let a = s
        .sketch_spline(
            sk,
            &SplineSpec { points: vec![Xy(v(2.0), v(3.0)), Xy(v(5.0), v(6.0))], closed: false, construction: true, dimensioned: false },
        )
        .unwrap();
    let d = s.sketch_detail(sk).unwrap();
    assert_eq!(d.summary.dof, (4, 0));
    assert!(d.entities[0].construction);
    assert_eq!(a.points.len(), 2);
}
