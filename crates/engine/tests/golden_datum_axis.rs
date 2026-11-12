//! D3: datum axes (manual, edge, face, two points) used by circular_array and revolve; hand-computed boxes and volumes,
//! parameter edits, files, refusals and deletion order.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

fn snap(s: &Session) -> String {
    ron::to_string(s.project()).unwrap()
}

fn extrude_new(s: &mut Session, sk: u64, h: f64) -> u64 {
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":h,"op":"new_body"})).unwrap()).unwrap().0
}

/// A 10 x 6 x 4 block B centred on the origin (x -5..5, y -3..3, z 0..4).
fn block_b(s: &mut Session) -> u64 {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    extrude_new(s, sk, 4.0)
}

/// A block D of width `w` (a parameter) and depth 10, height 10, centred on (30, 0).
fn block_d(s: &mut Session) -> u64 {
    s.param_set("w", &10.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &30.0.into(), &0.0.into(), &"w".into(), &10.0.into(), false).unwrap();
    extrude_new(s, sk, 10.0)
}

fn bbox_of(s: &Session, body: u64) -> [f64; 6] {
    s.result_bodies().iter().find(|b| b.id == body).map(|b| b.bbox).unwrap_or_else(|| panic!("no body {body}"))
}

fn last_bbox(s: &Session) -> [f64; 6] {
    s.result_bodies().last().unwrap().bbox
}

fn assert_box(got: [f64; 6], want: [f64; 6], what: &str) {
    for (g, w) in got.into_iter().zip(want) {
        assert_close(g, w, 1e-3, what);
    }
}

fn array(s: &mut Session, b: u64, axis: u64) -> u64 {
    s.circular_array(Some(b), &4.0.into(), &360.0.into(), Some(&AxisRef::Datum(axis)), None).unwrap().0
}

fn axis_state(s: &Session, id: u64) -> ([f64; 3], [f64; 3]) {
    let a = s.project().datum_axes.iter().find(|a| a.id == id).unwrap();
    (a.origin(), a.dir())
}

#[test]
fn a_manual_axis_turns_an_array_about_it_by_hand_computed_box_and_volume() {
    let mut s = Session::new_part();
    let b = block_b(&mut s);
    let (info, _) = s.datum_axis(&AxisSpec::Manual { origin: [30.0, 0.0, 0.0], dir: [0.0, 0.0, 2.0] }, Some("pivot")).unwrap();
    assert_eq!((info.def, info.origin, info.name.as_str()), ("manual", [30.0, 0.0, 0.0], "pivot"));
    assert_eq!(s.project().datum_axes[0].name, "pivot", "the app's tree shows the pool name (F-171)");
    array(&mut s, b, info.id);
    // B is 35 mm from the axis; four quarter turns: relative x and y both -35..35 -> abs x -5..65, y -35..35.
    assert_box(last_bbox(&s), [-5.0, -35.0, 0.0, 65.0, 35.0, 4.0], "array about a manual axis");
    assert_close(s.result_bodies().last().unwrap().volume, 4.0 * 240.0, 1e-6, "four blocks");
}

#[test]
fn an_edge_axis_follows_the_edge_when_the_block_changes() {
    let mut s = Session::new_part();
    let b = block_b(&mut s);
    let d = block_d(&mut s);
    let topo = s.topology(Some(d), false).unwrap();
    let edge =
        topo.edges.iter().find(|e| e.kind == EdgeKind::Line && (e.a[2] - e.b[2]).abs() > 5.0 && e.a[0] > 30.0 && e.a[1] > 0.0).unwrap();
    assert_eq!((edge.a[0], edge.a[1]), (35.0, 5.0));
    let (info, _) = s.datum_axis(&AxisSpec::Edge { edge: edge.id, body: Some(d) }, None).unwrap();
    assert_eq!(info.def, "edge");
    assert_eq!(info.origin, [35.0, 5.0, 5.0], "the midpoint of the edge");
    assert_close(info.dir[2].abs(), 1.0, 1e-9, "vertical");
    array(&mut s, b, info.id);
    // About (35, 5): B is at relative (-35, -5); quarter turns give relative x and y within -40..40.
    assert_box(bbox_of(&s, s.result_bodies().last().unwrap().id), [-5.0, -35.0, 0.0, 75.0, 45.0, 4.0], "array about an edge");
    s.param_set("w", &20.0.into()).unwrap();
    assert_eq!(axis_state(&s, info.id).0, [40.0, 5.0, 5.0], "the axis moved with the edge");
    // D grew to x 20..40; B relative x -45..-35 -> bbox x -5..85 (the farthest quarter turn is 45 away).
    assert_box(last_bbox(&s), [-5.0, -40.0, 0.0, 85.0, 50.0, 4.0], "array follows the edge");
}

#[test]
fn a_face_axis_is_the_cylinders_axis() {
    let mut s = Session::new_part();
    let b = block_b(&mut s);
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &30.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
    let c = extrude_new(&mut s, sk, 10.0);
    let face = s.topology(Some(c), false).unwrap().faces.iter().find(|f| f.kind == FaceKind::Cylinder).unwrap().id;
    let (info, _) = s.datum_axis(&AxisSpec::Face { face, body: None }, None).unwrap();
    assert_eq!(info.def, "face");
    assert_close(info.origin[0], 30.0, 1e-6, "axis through the cylinder centre x");
    assert_close(info.origin[1], 0.0, 1e-6, "axis through the cylinder centre y");
    array(&mut s, b, info.id);
    assert_box(last_bbox(&s), [-5.0, -35.0, 0.0, 65.0, 35.0, 4.0], "array about a cylinder axis");
}

#[test]
fn a_two_point_axis_follows_its_points_and_a_collapse_is_refused() {
    let mut s = Session::new_part();
    s.param_set("px", &30.0.into()).unwrap();
    let b = block_b(&mut s);
    let num = |v: [&str; 3]| v.map(|e| e.parse::<f64>().map(Num::Value).unwrap_or_else(|_| Num::Expr(e.into())));
    let (p, _) = s.datum_point(&PointSpec::At(num(["px", "0", "0"])), None).unwrap();
    let (q, _) = s.datum_point(&PointSpec::At(num(["px", "0", "10"])), None).unwrap();
    let (info, _) = s.datum_axis(&AxisSpec::Points([p.id, q.id]), Some("through")).unwrap();
    assert_eq!((info.def, info.origin, info.dir), ("two_points", [30.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    array(&mut s, b, info.id);
    assert_box(last_bbox(&s), [-5.0, -35.0, 0.0, 65.0, 35.0, 4.0], "array about a two-point axis");
    s.param_set("px", &20.0.into()).unwrap();
    // Axis at x = 20: relative B x -25..-15, so abs x -5..45, y -25..25.
    assert_box(last_bbox(&s), [-5.0, -25.0, 0.0, 45.0, 25.0, 4.0], "points moved, the axis and the array follow");
    // A parameter that would put the points on one spot is refused with nothing changed (F-172).
    let (r, _) = s.datum_point(&PointSpec::At(num(["20", "0", "10"])), None).unwrap();
    let other = s.datum_axis(&AxisSpec::Points([p.id, r.id]), None).unwrap().0;
    let before = snap(&s);
    let e = s.dimension_set(&DimensionTarget::Feature { feature: r.id, key: "z".into() }, Some(&0.0.into()), None).unwrap_err().to_string();
    assert!(e.contains("F-172"), "{e}");
    assert!(snap(&s) == before);
    let _ = other;
}

#[test]
fn every_definition_survives_save_and_open_and_follows_the_next_edit() {
    let mut s = Session::new_part();
    let b = block_b(&mut s);
    let d = block_d(&mut s);
    let e = s
        .topology(Some(d), false)
        .unwrap()
        .edges
        .into_iter()
        .find(|e| e.kind == EdgeKind::Line && (e.a[2] - e.b[2]).abs() > 5.0 && e.a[0] > 30.0 && e.a[1] > 0.0)
        .unwrap();
    let (axis, _) = s.datum_axis(&AxisSpec::Edge { edge: e.id, body: Some(d) }, None).unwrap();
    let (vp, _) = s.datum_point(&PointSpec::Vertex { edge: e.id, end: VertexEnd::End, body: Some(d) }, None).unwrap();
    let (mp, _) = s.datum_point(&PointSpec::At([30.0.into(), 0.0.into(), 0.0.into()]), None).unwrap();
    let (tp, _) = s.datum_axis(&AxisSpec::Points([vp.id, mp.id]), None).unwrap();
    let (man, _) = s.datum_axis(&AxisSpec::Manual { origin: [1.0, 2.0, 3.0], dir: [0.0, 1.0, 0.0] }, None).unwrap();
    array(&mut s, b, axis.id);
    let want = last_bbox(&s);
    let path = scratch("datum-axes.qcad");
    s.save(Some(&path)).unwrap();
    let (mut loaded, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_box(last_bbox(&loaded), want, "after open");
    assert_eq!(loaded.project().datum_axes.len(), 3);
    assert_eq!(axis_state(&loaded, axis.id), axis_state(&s, axis.id));
    assert_eq!(axis_state(&loaded, tp.id), axis_state(&s, tp.id));
    assert_eq!(axis_state(&loaded, man.id), ([1.0, 2.0, 3.0], [0.0, 1.0, 0.0]));
    let info = loaded.info();
    assert_eq!(info.datums.axes.len(), 3);
    assert_eq!(info.datums.points.len(), 2);
    assert!(info.datums.errors.is_empty());
    loaded.param_set("w", &20.0.into()).unwrap();
    assert_eq!(axis_state(&loaded, axis.id).0, [40.0, 5.0, 5.0], "an opened document's edge axis follows the edge");
    assert_eq!(loaded.project().datum_points.iter().find(|p| p.id == vp.id).unwrap().at, [40.0, 5.0, 10.0]);
}

#[test]
fn revolve_about_a_datum_axis_has_the_hand_volume() {
    let mut s = Session::new_part();
    let (axis, _) = s.datum_axis(&AxisSpec::Manual { origin: [0.0, 0.0, 0.0], dir: [0.0, 1.0, 0.0] }, None).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &5.0.into(), &0.0.into(), &2.0.into(), &10.0.into(), false).unwrap();
    let rev = Revolve {
        sketch: sk,
        profiles: None,
        axis: AxisRef::Datum(axis.id),
        angle: 360.0.into(),
        direction: Direction::Normal,
        op: Op::NewBody,
        target: None,
        name: None,
    };
    let (_, r) = s.revolve(&rev).unwrap();
    // A ring x 4..6, height 10 about the Y axis: pi (6^2 - 4^2) 10.
    assert_close(r.bodies[0].volume, std::f64::consts::PI * 20.0 * 10.0, 0.5, "revolved ring");
    for (got, want) in r.bodies[0].bbox.into_iter().zip([-6.0, -5.0, -6.0, 6.0, 5.0, 6.0]) {
        assert_close(got, want, 0.05, "revolved ring box (tessellated)");
    }
}

#[test]
fn refusals_leave_the_document_untouched() {
    let mut s = Session::new_part();
    let _b = block_b(&mut s);
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &30.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
    let c = extrude_new(&mut s, sk, 10.0);
    let topo = s.topology(Some(c), false).unwrap();
    let seam = topo.edges.iter().find(|e| e.seam).map(|e| e.id).expect("a cylinder has a seam");
    let circle = topo.edges.iter().find(|e| e.kind == EdgeKind::Circle).unwrap().id;
    let plane_face = topo.faces.iter().find(|f| f.kind == FaceKind::Plane).unwrap().id;
    let (p, _) = s.datum_point(&PointSpec::At([0.0.into(), 0.0.into(), 0.0.into()]), None).unwrap();
    let (q, _) = s.datum_point(&PointSpec::At([0.0.into(), 0.0.into(), 0.0.into()]), None).unwrap();
    let before = snap(&s);
    let cases: Vec<(AxisSpec, &str)> = vec![
        (AxisSpec::Edge { edge: seam, body: Some(c) }, "seam"),
        (AxisSpec::Edge { edge: circle, body: Some(c) }, "not a straight line"),
        (AxisSpec::Edge { edge: 99999, body: Some(c) }, "edge 99999"),
        (AxisSpec::Face { face: plane_face, body: Some(c) }, "not a cylinder or cone"),
        (AxisSpec::Face { face: 99999, body: Some(c) }, "face 99999"),
        (AxisSpec::Points([p.id, p.id]), "two different"),
        (AxisSpec::Points([p.id, q.id]), "coincide"),
        (AxisSpec::Points([p.id, c]), "datum point"),
        (AxisSpec::Manual { origin: [0.0; 3], dir: [0.0; 3] }, "no length"),
        (AxisSpec::Manual { origin: [1.0e8, 0.0, 0.0], dir: [0.0, 0.0, 1.0] }, "within"),
        (AxisSpec::Manual { origin: [0.0; 3], dir: [f64::NAN, 0.0, 1.0] }, "finite"),
    ];
    for (spec, word) in cases {
        let e = s.datum_axis(&spec, None).unwrap_err().to_string();
        assert!(e.contains(word), "{spec:?}: {e}");
        assert!(snap(&s) == before, "{spec:?} changed the document");
    }
}

#[test]
fn deleting_the_axis_or_the_points_under_a_consumer_is_refused_and_a_cascade_goes_in_order() {
    let mut s = Session::new_part();
    let b = block_b(&mut s);
    let (p, _) = s.datum_point(&PointSpec::At([30.0.into(), 0.0.into(), 0.0.into()]), None).unwrap();
    let (q, _) = s.datum_point(&PointSpec::At([30.0.into(), 0.0.into(), 10.0.into()]), None).unwrap();
    let (axis, _) = s.datum_axis(&AxisSpec::Points([p.id, q.id]), None).unwrap();
    let arr = array(&mut s, b, axis.id);
    let before = snap(&s);
    for victim in [axis.id, p.id, q.id] {
        let e = s.feature_delete(victim, false).unwrap_err().to_string();
        assert!(e.contains("has dependents"), "{victim}: {e}");
        assert!(snap(&s) == before);
    }
    // Suppression is for bodies only.
    assert!(s.feature_suppress(axis.id, true).is_err());
    s.feature_delete(p.id, true).unwrap();
    assert!(s.project().datum_axes.is_empty() && s.project().datum_points.len() == 1, "the cascade took the axis and its array");
    assert!(s.project().timeline.iter().all(|n| n.id != arr));
    // The axis alone (no consumer) deletes without cascade.
    let (axis2, _) = s.datum_axis(&AxisSpec::Manual { origin: [0.0; 3], dir: [1.0, 0.0, 0.0] }, None).unwrap();
    s.feature_delete(axis2.id, false).unwrap();
    assert!(s.project().datum_axes.is_empty());
}

#[test]
fn an_axis_made_on_purpose_is_never_reused_by_a_feature_that_gives_a_line() {
    let mut s = Session::new_part();
    let b = block_b(&mut s);
    let (axis, _) = s.datum_axis(&AxisSpec::Manual { origin: [30.0, 0.0, 0.0], dir: [0.0, 0.0, 1.0] }, None).unwrap();
    s.circular_array(Some(b), &4.0.into(), &360.0.into(), Some(&AxisRef::Through { origin: [30.0, 0.0, 0.0], dir: [0.0, 0.0, 1.0] }), None)
        .unwrap();
    assert_eq!(s.project().datum_axes.len(), 2, "the array made its own axis");
    // Deleting the explicit axis does not touch the array.
    s.feature_delete(axis.id, false).unwrap();
    assert_box(last_bbox(&s), [-5.0, -35.0, 0.0, 65.0, 35.0, 4.0], "the array is unchanged");
}

#[test]
fn a_formula_on_an_axis_coordinate_is_refused_because_the_app_ignores_it_f170() {
    let mut s = Session::new_part();
    s.param_set("px", &30.0.into()).unwrap();
    let (axis, _) = s.datum_axis(&AxisSpec::Manual { origin: [30.0, 0.0, 0.0], dir: [0.0, 0.0, 1.0] }, None).unwrap();
    let before = snap(&s);
    let r = s.dimension_set(&DimensionTarget::Feature { feature: axis.id, key: "ox".into() }, Some(&"px".into()), None);
    assert!(r.unwrap_err().to_string().contains("F-170"));
    assert!(snap(&s) == before);
}
