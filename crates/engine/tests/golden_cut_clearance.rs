//! The native cut-entry seam is part of the saved recipe's server and GUI geometry.
mod common;
use common::*;
use qymcad_engine::*;

fn build(top: bool) -> Session {
    let mut s = Session::new_part();
    s.param_set("depth", &4.0.into()).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &20.0.into(), &30.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch,
        profiles: None,
        height: 20.0.into(),
        op: Op::Add,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap();
    let z = if top { 20.0 } else { 3.0 };
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &z.into(), None).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &4.0.into(), &6.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch,
        profiles: None,
        height: Num::Expr("depth".into()),
        op: Op::Cut,
        direction: if top { Direction::Reverse } else { Direction::Normal },
        through: false,
        target: None,
        name: None,
    })
    .unwrap();
    s
}

fn expected(top: bool, depth: f64) -> (f64, f64) {
    // The tool spans [3-.001, 3+depth] inside stock, or [20-depth, 20+.001] at its top.
    // Only the first case intersects an extra .001 mm of material. Stock V=W*L*T.
    (20.0 * 30.0 * 20.0 - 4.0 * 6.0 * (depth + if top { 0.0 } else { 0.001 }), if top { 20.0 - depth } else { 3.0 - 0.001 })
}

fn check(s: &mut Session, top: bool, depth: f64) {
    let (volume, floor) = expected(top, depth);
    assert_close(s.result_bodies()[0].volume, volume, 1e-6, "native cut entry volume");
    let topology = s.topology(None, false).unwrap();
    let floor_face =
        topology.faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.999999) && (f.area - 4.0 * 6.0).abs() < 1e-6).expect("cut floor");
    assert_close(floor_face.centroid[2], floor, 1e-6, "native cut floor z");
}

#[test]
fn internal_cut_entry_seam_and_top_face_cut_agree_on_server_and_gui_paths() {
    for top in [false, true] {
        let mut s = build(top);
        check(&mut s, top, 4.0);
        let path = scratch(if top { "cut_clearance_top.qcad" } else { "cut_clearance_internal.qcad" });
        s.save(Some(&path)).unwrap();
        s.param_set("depth", &5.0.into()).unwrap();
        check(&mut s, top, 5.0);
        let (volume, _, faces) = gui_edit_param_faces(&path, "depth", "5");
        let (expected_volume, floor) = expected(top, 5.0);
        assert_close(volume, expected_volume, 1e-6, "GUI native cut entry volume");
        let floor_face = faces.iter().find(|f| f.normal[2] > 0.999999 && (f.area - 24.0).abs() < 1e-6).expect("GUI cut floor");
        assert_close(floor_face.centroid.z, floor, 1e-6, "GUI native cut floor z");
    }
}
