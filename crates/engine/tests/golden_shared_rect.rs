//! Rectangles share previously pinned corners without redundant dimensions.
mod common;
use common::*;
use qymcad_engine::*;

fn n(expr: &str) -> Num {
    Num::Expr(expr.into())
}
fn v(value: f64) -> Num {
    Num::Value(value)
}

fn rectangle_on_pinned_corners() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("w", &v(10.0)).unwrap();
    s.param_set("h", &v(8.0)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let diagonal = s
        .sketch_line(sk, &LineSpec { x1: v(20.0), y1: v(30.0), x2: n("20+w"), y2: n("30+h"), construction: true, dimensioned: true })
        .unwrap();
    let rect = s.sketch_rect(sk, &n("20+w/2"), &n("30+h/2"), &n("w"), &n("h"), false).unwrap();
    let d = s.sketch_detail(sk).unwrap();
    for point in diagonal.points {
        assert!(
            d.entities.iter().filter(|e| rect.contains(&e.id)).any(|e| e.points.contains(&point)),
            "the rectangle must share the previously pinned corner {point}"
        );
    }
    assert_eq!(d.summary.dof, (0, 0), "shared pins must not over-constrain the rectangle");
    let (_, rebuild) = s
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
    assert!(rebuild.errors.is_empty());
    // A rectangle of width w and height h extruded t mm has volume w h t.
    assert_close(rebuild.bodies[0].volume, 10.0 * 8.0 * 3.0, 1e-5, "initial shared rectangle volume");
    (s, sk)
}
fn rectangle_body(volume: f64, bbox: [f64; 6], width: f64) {
    assert_close(volume, width * 8.0 * 3.0, 1e-5, "edited shared rectangle volume");
    // Previously pinned bottom-left is (20,30); the opposite corner follows (20+w,30+h).
    for (actual, expected) in bbox.into_iter().zip([20.0, 30.0, 0.0, 20.0 + width, 38.0, 3.0]) {
        assert_close(actual, expected, 1e-5, "shared rectangle bounding box");
    }
}
#[test]
fn rectangle_on_previously_pinned_corners_follows_its_parameters() {
    let (mut s, sk) = rectangle_on_pinned_corners();
    let r = s.param_set("w", &v(16.0)).unwrap();
    assert!(r.errors.is_empty());
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    rectangle_body(r.bodies[0].volume, r.bodies[0].bbox, 16.0);
}
#[test]
fn rectangle_on_previously_pinned_corners_follows_gui_parameters() {
    let (mut s, sk) = rectangle_on_pinned_corners();
    let path = scratch("shared_rectangle_gui.qcad");
    s.save(Some(&path)).unwrap();
    let mut p = qymcad_io::load_project(path.to_str().unwrap()).unwrap();
    p.parameters.iter_mut().find(|q| q.name == "w").unwrap().expr = "16".into();
    assert!(p.eval_parameters().is_empty());
    let si = p.sketches.iter().position(|s| s.id == sk).unwrap();
    assert!(p.solve_sketch(si) <= 1e-6);
    assert_eq!(p.sketch_dof(si), (0, 0), "GUI edit preserves independent rectangle dimensions");
    let (volume, bbox) = gui_edit_param_body(&path, "w", "16");
    rectangle_body(volume, bbox, 16.0);
}
