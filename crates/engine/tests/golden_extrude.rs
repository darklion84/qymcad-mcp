//! Formula-derived coverage for extrusion intersection, through-all and symmetric reach.

mod common;
use common::*;
use qymcad_engine::*;

fn expr(name: &str) -> Num {
    Num::Expr(name.into())
}

fn rectangle(s: &mut Session, plane: PlaneRef, x: f64, width: f64, depth: f64) -> u64 {
    let sketch = s.sketch_create(&plane, None).unwrap();
    s.sketch_rect(sketch, &x.into(), &0.0.into(), &width.into(), &depth.into(), false).unwrap();
    sketch
}

fn extrude(s: &mut Session, sketch: u64, height: Num, op: Op, direction: Direction, through: bool) {
    let (_, report) = s.extrude(&Extrude { sketch, profiles: None, height, op, direction, through, target: None, name: None }).unwrap();
    assert!(report.errors.is_empty(), "extrusion rebuild errors: {:?}", report.errors);
    assert_eq!(report.bodies.len(), 1);
}

fn assert_body(volume: f64, bbox: [f64; 6], expected_volume: f64, expected_bbox: [f64; 6]) {
    assert_close(volume, expected_volume, 1e-3, "extrusion volume");
    for (i, (actual, expected)) in bbox.into_iter().zip(expected_bbox).enumerate() {
        assert_close(actual, expected, 0.05, &format!("extrusion bbox[{i}]"));
    }
}

fn assert_result(s: &Session, expected_volume: f64, expected_bbox: [f64; 6]) {
    assert!(s.info().errors.is_empty());
    let bodies = s.result_bodies();
    assert_eq!(bodies.len(), 1);
    assert_body(bodies[0].volume, bodies[0].bbox, expected_volume, expected_bbox);
}

#[test]
fn intersect_keeps_only_the_overlap_and_follows_height() {
    let mut s = Session::new_part();
    s.param_set("height", &8.0.into()).unwrap();
    let stock = rectangle(&mut s, PlaneRef::Base(BaseName::XY), 0.0, 40.0, 30.0);
    extrude(&mut s, stock, 12.0.into(), Op::Add, Direction::Normal, false);
    let tool = rectangle(&mut s, PlaneRef::Base(BaseName::XY), 15.0, 30.0, 20.0);
    extrude(&mut s, tool, expr("height"), Op::Intersect, Direction::Normal, false);
    // Stock x=[-20,20], tool x=[0,30]: overlap width 20. The tool's y=[-10,10]
    // lies within stock y=[-15,15]; h<12, so V=20*20*h and z=[0,h].
    assert_result(&s, 20.0 * 20.0 * 8.0, [0.0, -10.0, 0.0, 20.0, 10.0, 8.0]);
    let path = scratch("extrude_intersect_gui.qcad");
    s.save(Some(&path)).unwrap();
    s.param_set("height", &6.0.into()).unwrap();
    let expected_bbox = [0.0, -10.0, 0.0, 20.0, 10.0, 6.0];
    assert_result(&s, 20.0 * 20.0 * 6.0, expected_bbox);
    let (volume, bbox) = gui_edit_param_body(&path, "height", "6");
    assert_body(volume, bbox, 20.0 * 20.0 * 6.0, expected_bbox);
}

#[test]
fn through_cut_spans_both_sides_of_an_interior_plane_and_follows_stock_height() {
    let mut s = Session::new_part();
    s.param_set("height", &24.0.into()).unwrap();
    let stock = rectangle(&mut s, PlaneRef::Base(BaseName::XY), 0.0, 40.0, 30.0);
    extrude(&mut s, stock, expr("height"), Op::Add, Direction::Normal, false);
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &12.0.into(), None).unwrap();
    let tool = rectangle(&mut s, PlaneRef::Plane(plane), 0.0, 8.0, 10.0);
    // The plane lies inside the stock, and the nominal cut height is just 0.25 mm.
    // Through-all removes an 8*10 prism over all z=[0,h], on both sides of that plane.
    // Thus V=(40*30-8*10)*h; the outer bounding box stays 40*30*h.
    extrude(&mut s, tool, 0.25.into(), Op::Cut, Direction::Normal, true);
    assert_result(&s, (40.0 * 30.0 - 8.0 * 10.0) * 24.0, [-20.0, -15.0, 0.0, 20.0, 15.0, 24.0]);
    let path = scratch("extrude_through_gui.qcad");
    s.save(Some(&path)).unwrap();
    s.param_set("height", &36.0.into()).unwrap();
    let expected_bbox = [-20.0, -15.0, 0.0, 20.0, 15.0, 36.0];
    assert_result(&s, (40.0 * 30.0 - 8.0 * 10.0) * 36.0, expected_bbox);
    let (volume, bbox) = gui_edit_param_body(&path, "height", "36");
    assert_body(volume, bbox, (40.0 * 30.0 - 8.0 * 10.0) * 36.0, expected_bbox);
}

#[test]
fn symmetric_extrude_uses_half_the_height_on_each_side() {
    let mut s = Session::new_part();
    s.param_set("height", &12.0.into()).unwrap();
    let sketch = rectangle(&mut s, PlaneRef::Base(BaseName::XY), 0.0, 10.0, 6.0);
    extrude(&mut s, sketch, expr("height"), Op::Add, Direction::Symmetric, false);
    // The 10*6 profile sweeps from z=-h/2 to z=h/2, so V=10*6*h.
    assert_result(&s, 10.0 * 6.0 * 12.0, [-5.0, -3.0, -6.0, 5.0, 3.0, 6.0]);
    let path = scratch("extrude_symmetric_gui.qcad");
    s.save(Some(&path)).unwrap();
    s.param_set("height", &18.0.into()).unwrap();
    let expected_bbox = [-5.0, -3.0, -9.0, 5.0, 3.0, 9.0];
    assert_result(&s, 10.0 * 6.0 * 18.0, expected_bbox);
    let (volume, bbox) = gui_edit_param_body(&path, "height", "18");
    assert_body(volume, bbox, 10.0 * 6.0 * 18.0, expected_bbox);
}
