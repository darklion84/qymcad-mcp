//! Directed sketch references remain on the positive x side during large rearrangements.
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
const EDITS: [(&str, f64); 9] =
    [("r", 100.0), ("r", 5.0), ("cx", 200.0), ("cy", 300.0), ("rot", 120.0), ("rot", 210.0), ("rot", 300.0), ("rot", 30.0), ("cx", 5.0)];
#[test]
fn direction_reference_survives_large_server_rearrangements() {
    let (mut s, sk, added) = polygon(30.0);
    let (mut r, mut rot, mut cx, mut cy) = (10.0, 30.0, 20.0, 30.0);
    for (name, value) in EDITS {
        s.param_set(name, &v(value)).unwrap();
        match name {
            "r" => r = value,
            "rot" => rot = value,
            "cx" => cx = value,
            "cy" => cy = value,
            _ => unreachable!(),
        }
        direction_points(&s, sk, &added, r, rot, cx, cy);
    }
}
#[test]
fn direction_reference_survives_large_gui_rearrangements() {
    let (mut s, sk, added) = polygon(30.0);
    let path = scratch("direction_reference_gui.qcad");
    s.save(Some(&path)).unwrap();
    let mut p = qymcad_io::load_project(path.to_str().unwrap()).unwrap();
    let si = p.sketches.iter().position(|s| s.id == sk).unwrap();
    let reference = s.sketch_detail(sk).unwrap().points.iter().find(|p| p.role == Some("angle_reference")).unwrap().id;
    for (name, value) in EDITS {
        p.parameters.iter_mut().find(|q| q.name == name).unwrap().expr = value.to_string();
        // QymCAD GUI apply_param_edit: eval_parameters, then a single solve for each expression-bearing sketch.
        assert!(p.eval_parameters().is_empty());
        assert!(p.solve_sketch(si) <= 1e-6, "GUI solve failed at {name}={value}");
        let vars = p.param_map();
        let point = |id| p.sketches[si].points.iter().find(|q| q.id == id).unwrap();
        assert_close(point(reference).x, vars["cx"] + vars["r"], 1e-5, "GUI reference stays on +x side");
        let angle = vars["rot"].to_radians();
        assert_close(point(added.points[1]).x, vars["cx"] + vars["r"] * angle.cos(), 1e-5, "GUI directed vertex x");
        assert_close(point(added.points[1]).y, vars["cy"] + vars["r"] * angle.sin(), 1e-5, "GUI directed vertex y");
    }
}
