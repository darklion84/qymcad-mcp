//! Reporting uses native geometry, with formula-derived world frames and finite cylinder placement.

mod common;
use common::*;
use qymcad_engine::*;
use serde_json::{json, Value};

fn extrude(sketch: Id, height: Num, op: Op) -> Extrude {
    Extrude { sketch, profiles: None, height, op, direction: Direction::Normal, through: false, target: None, name: None }
}

fn stock() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("h", &10.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &30.0.into(), &40.0.into(), &20.0.into(), &16.0.into(), false).unwrap();
    let body = s.extrude(&extrude(sk, Num::Expr("h".into()), Op::NewBody)).unwrap().0;
    (s, body)
}

fn frame(s: &Session, sketch: Id) -> Value {
    serde_json::to_value(s.sketch_detail(sketch).unwrap()).unwrap()["world_frame"].clone()
}

#[test]
fn face_sketch_reports_projected_origin_and_positive_axes() {
    let (mut s, body) = stock();
    let topo = s.topology(Some(body), false).unwrap();
    // Face origins are n * (centroid dot n), not face centroids (30,40,10).
    for (normal, origin, x_axis, y_axis) in [
        ([0.0, 0.0, 1.0], [0.0, 0.0, 10.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
        ([1.0, 0.0, 0.0], [40.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 1.0, 0.0], [0.0, 48.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    ] {
        let f = topo.faces.iter().find(|f| f.normal == Some(normal)).unwrap();
        let sk = s.sketch_create(&PlaneRef::Face { body, face: f.id }, None).unwrap();
        assert_eq!(
            frame(&s, sk),
            json!({ "origin": origin, "x_axis": x_axis, "y_axis": y_axis, "normal": normal }),
            "reported face world frame"
        );
    }
}

#[test]
fn face_frame_follows_height_on_server_and_gui_paths() {
    let (mut s, body) = stock();
    let top = s.topology(Some(body), false).unwrap().faces.into_iter().find(|f| f.normal == Some([0.0, 0.0, 1.0])).unwrap();
    let sk = s.sketch_create(&PlaneRef::Face { body, face: top.id }, None).unwrap();
    s.sketch_rect(sk, &30.0.into(), &40.0.into(), &4.0.into(), &6.0.into(), false).unwrap();
    s.extrude(&extrude(sk, 2.0.into(), Op::Add)).unwrap();
    let path = scratch("usability_face_frame.qcad");
    s.save(Some(&path)).unwrap();
    s.param_set("h", &14.0.into()).unwrap();
    assert_eq!(frame(&s, sk)["origin"], json!([0.0, 0.0, 14.0]), "face frame follows stock height");
    let expected = 20.0 * 16.0 * 14.0 + 4.0 * 6.0 * 2.0;
    assert_close(s.result_bodies()[0].volume, expected, 1e-6, "server boss volume");
    let (v, _, faces) = gui_edit_param_faces(&path, "h", "14");
    assert_close(v, expected, 1e-6, "GUI boss volume");
    let cap = faces.iter().find(|f| f.normal[2] > 0.99 && (f.area - 24.0).abs() < 1e-6).unwrap();
    assert_close(cap.centroid.z, 14.0 + 2.0, 1e-6, "GUI boss cap");
}

#[test]
fn timeline_reports_extrude_operation() {
    for (op, expected) in [(Op::Cut, "extrude (cut)"), (Op::Add, "extrude (add)"), (Op::Intersect, "extrude (intersect)")] {
        let (mut s, _) = stock();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_circle(sk, &30.0.into(), &40.0.into(), &4.0.into(), false).unwrap();
        let height = if op == Op::Add { 12.0 } else { 4.0 };
        let id = s.extrude(&extrude(sk, height.into(), op)).unwrap().0;
        assert_eq!(s.info().timeline.iter().find(|n| n.id == id).unwrap().kind, expected, "extrude operation label");
    }
}

#[test]
fn cylinder_axis_point_is_at_the_faces_axial_centroid() {
    let mut s = Session::new_part();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &25.0.into(), None).unwrap();
    let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    s.sketch_circle(sk, &30.0.into(), &40.0.into(), &8.0.into(), false).unwrap();
    s.extrude(&extrude(sk, 10.0.into(), Op::NewBody)).unwrap();
    let topo = s.topology(None, false).unwrap();
    let f = topo.faces.iter().find(|f| f.kind == FaceKind::Cylinder).unwrap();
    let [point, dir] = f.axis.unwrap();
    // A full cylinder axis has x=cx, y=cy, z=plane + height/2.
    for (actual, expected) in point.into_iter().zip([30.0, 40.0, 25.0 + 10.0 / 2.0]) {
        assert_close(actual, expected, 1e-6, "cylinder axis point at axial centroid");
    }
    assert_close(dir[2].abs(), 1.0, 1e-9, "vertical cylinder axis");
}

#[test]
fn sketch_frame_includes_component_world_placement() {
    let (mut s, _) = stock();
    let sketch = s.info().sketches[0].id;
    let path = scratch("usability_placed_frame.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { project: mut p, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let owner = p.sketch_owner(sketch).unwrap();
    p.components.iter_mut().find(|c| c.id == owner).unwrap().transform = [0.0, -1.0, 0.0, 100.0, 1.0, 0.0, 0.0, 20.0, 0.0, 0.0, 1.0, 30.0];
    qymcad_io::save_project_guarded_with_brep(&p, path.to_str().unwrap(), &breps).unwrap();
    let (s, _) = Session::open(&path).unwrap();
    assert_eq!(
        frame(&s, sketch),
        json!({
            "origin": [100.0, 20.0, 30.0], "x_axis": [0.0, 1.0, 0.0],
            "y_axis": [-1.0, 0.0, 0.0], "normal": [0.0, 0.0, 1.0]
        }),
        "frame carries component placement"
    );
}
