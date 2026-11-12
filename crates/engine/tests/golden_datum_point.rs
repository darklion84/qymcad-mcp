//! D4: datum points by coordinates and at a body vertex: formulas, following the vertex, ranges, refusals, files.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

fn block(s: &mut Session, w: &str) -> u64 {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &Num::Expr(w.into()), &6.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":4,"op":"new_body"})).unwrap()).unwrap().0
}

fn snap(s: &Session) -> String {
    ron::to_string(s.project()).unwrap()
}

fn at(s: &Session, id: u64) -> [f64; 3] {
    s.project().datum_points.iter().find(|p| p.id == id).unwrap().at
}

fn num3(v: [&str; 3]) -> [Num; 3] {
    v.map(|e| if let Ok(n) = e.parse::<f64>() { Num::Value(n) } else { Num::Expr(e.into()) })
}

#[test]
fn a_point_by_coordinates_keeps_its_formulas_and_follows_parameters_and_dimension_set() {
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let (info, _) = s.datum_point(&PointSpec::At(num3(["w/2", "5", "-w"])), Some("anchor")).unwrap();
    assert_eq!((info.def, info.at, info.name.as_str()), ("manual", [20.0, 5.0, -40.0], "anchor"));
    assert_eq!(s.project().datum_points[0].name, "anchor", "the app's tree reads the pool name (F-171)");
    s.param_set("w", &10.0.into()).unwrap();
    assert_eq!(at(&s, info.id), [5.0, 5.0, -10.0], "formulas on x and z follow w; the literal y stays");
    s.dimension_set(&DimensionTarget::Feature { feature: info.id, key: "y".into() }, Some(&"w+1".into()), None).unwrap();
    assert_eq!(at(&s, info.id), [5.0, 11.0, -10.0]);
    // Save and reopen: the same point, the formulas still live.
    let path = scratch("datum-point.qcad");
    s.save(Some(&path)).unwrap();
    let (mut loaded, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_eq!(at(&loaded, info.id), [5.0, 11.0, -10.0]);
    loaded.param_set("w", &20.0.into()).unwrap();
    assert_eq!(at(&loaded, info.id), [10.0, 21.0, -20.0]);
}

#[test]
fn coordinates_beyond_the_apps_field_are_refused_at_creation_and_on_edits() {
    let mut s = Session::new_part();
    s.param_set("w", &1.0.into()).unwrap();
    let before = snap(&s);
    for bad in [["10000001", "0", "0"], ["0", "-10000001", "0"], ["0", "0", "w*100000000"], ["0", "0", "1e400"]] {
        let e = s.datum_point(&PointSpec::At(num3(bad)), None).unwrap_err().to_string();
        assert!(e.contains("datum") || e.contains("not finite"), "{bad:?}: {e}");
    }
    assert!(snap(&s) == before, "a refusal changes nothing");
    s.datum_point(&PointSpec::At(num3(["10000000", "0", "w"])), None).unwrap();
    let before = snap(&s);
    assert!(s.param_set("w", &2.0e7.into()).is_err(), "a later edit that pushes z out of range is refused");
    assert!(snap(&s) == before, "a refusal changes nothing");
}

#[test]
fn a_vertex_point_follows_its_vertex_and_is_stored_as_body_edge_end() {
    let mut s = Session::new_part();
    s.param_set("w", &10.0.into()).unwrap();
    let body = block(&mut s, "w");
    let topo = s.topology(Some(body), false).unwrap();
    let e = topo.edges.iter().find(|e| e.kind == EdgeKind::Line && (e.a[2] - e.b[2]).abs() > 1.0).unwrap().clone();
    let (info, _) = s.datum_point(&PointSpec::Vertex { edge: e.id, end: VertexEnd::End, body: None }, Some("corner")).unwrap();
    assert_eq!(info.def, "vertex");
    assert_eq!(info.vertex, Some(VertexInfo { body, edge: e.id, end: "end" }));
    assert_eq!(info.at, e.b);
    s.param_set("w", &30.0.into()).unwrap();
    let new_body = *s.result_bodies().iter().map(|b| &b.id).next().unwrap();
    let moved = s.topology(Some(new_body), false).unwrap().edges.into_iter().find(|x| x.id == e.id).unwrap();
    assert_ne!(moved.b, e.b, "the block grew, the vertex moved");
    assert_eq!(at(&s, info.id), moved.b, "the point travels with the vertex");
    let path = scratch("datum-vertex.qcad");
    s.save(Some(&path)).unwrap();
    let (loaded, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    let p = loaded.project().datum_points.iter().find(|p| p.id == info.id).unwrap();
    assert_eq!(p.at, moved.b);
    assert_eq!(p.name, "corner");
    assert!(matches!(p.def, qymcad_core::model::PointDef::AtVertex { edge, end: true, .. } if edge == e.id));
}

#[test]
fn vertex_refusals_leave_the_document_untouched() {
    let mut s = Session::new_part();
    let body = block(&mut s, "10");
    let edge = s.topology(Some(body), false).unwrap().edges[0].id;
    let step = ArrayDir { dx: 20.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: 2.0.into() };
    s.linear_array(Some(body), &step, None, None).unwrap();
    let before = snap(&s);
    let e = s.datum_point(&PointSpec::Vertex { edge: 9999, end: VertexEnd::Start, body: None }, None).unwrap_err().to_string();
    assert!(e.contains("edge 9999"), "{e}");
    let e = s.datum_point(&PointSpec::Vertex { edge, end: VertexEnd::Start, body: Some(12345) }, None).unwrap_err().to_string();
    assert!(e.contains("12345"), "{e}");
    // A consumed body (the array replaced it) is not a current body.
    let e = s.datum_point(&PointSpec::Vertex { edge, end: VertexEnd::Start, body: Some(body) }, None).unwrap_err().to_string();
    assert!(e.contains("consumed") || e.contains("not built"), "{e}");
    assert!(snap(&s) == before, "a refusal changes nothing");
}

#[test]
fn deleting_or_suppressing_the_body_under_a_vertex_point_is_refused_and_a_cascade_takes_the_point() {
    let mut s = Session::new_part();
    let body = block(&mut s, "10");
    let e = s.topology(Some(body), false).unwrap().edges[0].clone();
    let (info, _) = s.datum_point(&PointSpec::Vertex { edge: e.id, end: VertexEnd::Start, body: None }, None).unwrap();
    let before = snap(&s);
    let refused = s.feature_delete(body, false).unwrap_err().to_string();
    assert!(refused.contains("has dependents"), "{refused}");
    assert!(snap(&s) == before, "a refusal changes nothing");
    let e = s.feature_suppress(body, true).unwrap_err().to_string();
    assert!(e.contains("F-169") && e.contains(&format!("({})", info.id)), "{e}");
    assert!(snap(&s) == before, "a refused suppression changes nothing");
    s.feature_delete(body, true).unwrap();
    assert!(s.project().datum_points.iter().all(|p| p.id != info.id), "the cascade removed the point with its body");
}
