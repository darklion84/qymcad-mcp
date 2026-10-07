//! Negative parametric angles use explicit operand grouping and preserve directed vertices.
mod common;
use common::*;
use qymcad_engine::*;

fn n(expr: &str) -> Num {
    Num::Expr(expr.into())
}
fn v(value: f64) -> Num {
    Num::Value(value)
}
fn polygon(angle: f64) -> (Session, Id, Added) {
    let mut s = Session::new_part();
    for (name, value) in [("r", 10.0), ("rot", angle), ("cx", 20.0), ("cy", 30.0)] {
        s.param_set(name, &v(value)).unwrap();
    }
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let added = s
        .sketch_polygon(
            sk,
            &PolygonSpec {
                cx: n("cx"),
                cy: n("cy"),
                sides: 3,
                r: Some(n("r")),
                angle: Some(n("rot")),
                vertex: None,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    (s, sk, added)
}
fn direction_points(s: &Session, sk: Id, added: &Added, r: f64, rot: f64, cx: f64, cy: f64) {
    let d = s.sketch_detail(sk).unwrap();
    assert_eq!(d.summary.dof, (0, 0));
    let reference = d.points.iter().find(|p| p.role == Some("angle_reference")).unwrap();
    assert_close(reference.x, cx + r, 1e-5, "reference stays on +x side");
    assert_close(reference.y, cy, 1e-5, "reference y");
    let vertex = d.points.iter().find(|p| p.id == added.points[1]).unwrap();
    // The directed vertex is C + R(cos θ, sin θ), including negative θ.
    assert_close(vertex.x, cx + r * rot.to_radians().cos(), 1e-5, "directed vertex x");
    assert_close(vertex.y, cy + r * rot.to_radians().sin(), 1e-5, "directed vertex y");
}
#[test]
fn negative_parametric_angle_is_parenthesized_and_follows_edits() {
    let (mut s, sk, added) = polygon(-30.0);
    let d = s.sketch_detail(sk).unwrap();
    assert!(
        d.constraints.iter().filter_map(|c| c.expr.as_deref()).any(|e| e.contains("(rot)-(-360)")),
        "negative turn must be a parenthesized operand: {:?}",
        d.constraints
    );
    direction_points(&s, sk, &added, 10.0, -30.0, 20.0, 30.0);
    s.param_set("rot", &v(-120.0)).unwrap();
    direction_points(&s, sk, &added, 10.0, -120.0, 20.0, 30.0);
}
#[test]
fn negative_parametric_angle_follows_gui_edits() {
    let (mut s, sk, added) = polygon(-30.0);
    let path = scratch("negative_direction_gui.qcad");
    s.save(Some(&path)).unwrap();
    let mut p = qymcad_io::load_project(path.to_str().unwrap()).unwrap();
    p.parameters.iter_mut().find(|q| q.name == "rot").unwrap().expr = "-120".into();
    assert!(p.eval_parameters().is_empty());
    let si = p.sketches.iter().position(|s| s.id == sk).unwrap();
    assert!(p.solve_sketch(si) <= 1e-6);
    let vertex = p.sketches[si].points.iter().find(|p| p.id == added.points[1]).unwrap();
    assert_close(vertex.x, 20.0 + 10.0 * (-120f64).to_radians().cos(), 1e-5, "GUI negative vertex x");
    assert_close(vertex.y, 30.0 + 10.0 * (-120f64).to_radians().sin(), 1e-5, "GUI negative vertex y");
}
