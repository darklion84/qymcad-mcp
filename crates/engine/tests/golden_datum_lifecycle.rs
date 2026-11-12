//! Datum points and axes under history rollback, reorder, Part copy and Part delete.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

fn doc() -> (Session, u64, u64, u64, u64) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    let (b, _) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":4,"op":"new_body"})).unwrap()).unwrap();
    let (axis, _) = s.datum_axis(&AxisSpec::Manual { origin: [30.0, 0.0, 0.0], dir: [0.0, 0.0, 1.0] }, None).unwrap();
    let (arr, _) = s.circular_array(Some(b), &4.0.into(), &360.0.into(), Some(&AxisRef::Datum(axis.id)), None).unwrap();
    let (pt, _) = s.datum_point(&PointSpec::At([1.0.into(), 2.0.into(), 3.0.into()]), None).unwrap();
    (s, b, axis.id, arr, pt.id)
}

fn last_bbox(s: &Session) -> [f64; 6] {
    s.result_bodies().last().unwrap().bbox
}

#[test]
fn rollback_before_and_after_a_datum_keeps_the_nodes_and_rebuilds_the_array() {
    let (mut s, b, axis, arr, _) = doc();
    let full = last_bbox(&s);
    s.history_rollback(Some(axis), None).unwrap();
    assert_eq!(s.project().datum_axes.len(), 1, "the axis stays, the array stands past the bar");
    assert!(s.result_bodies().iter().all(|x| x.id != arr), "the array is not built past the bar");
    assert!(s.result_bodies().iter().any(|x| x.id == b));
    s.history_rollback(None, Some("end")).unwrap();
    assert_eq!(last_bbox(&s), full, "the array is back about the same axis");
}

#[test]
fn datum_nodes_are_not_reordered_or_suppressed() {
    let (mut s, _, axis, _, pt) = doc();
    for id in [axis, pt] {
        assert!(s.feature_reorder(id, "up").is_err());
        assert!(s.feature_suppress(id, true).is_err());
    }
    assert_eq!(s.project().datum_axes.len(), 1);
}

#[test]
fn part_delete_takes_the_datums_of_the_part_and_a_copy_gets_its_own() {
    let (mut s, ..) = doc();
    let source = PartRef::Id(s.project().active_component.unwrap());
    let copy = s.part_copy(&source, "copy", Some([0.0, 100.0, 0.0]), None).unwrap();
    let copy_id = copy["part"]["id"].as_u64().unwrap();
    assert_eq!(s.project().datum_axes.len(), 2, "the copy carries its own axis: {copy}");
    assert_eq!(s.project().datum_points.len(), 2);
    let own_axis = s.project().datum_axes.iter().map(|a| a.id).max().unwrap();
    let own_array = s
        .project()
        .timeline
        .iter()
        .find(|n| matches!(n.kind, qymcad_core::feature::FeatureKind::CircularArray { axis, .. } if axis == own_axis));
    assert!(own_array.is_some(), "the copied array turns about the copied axis, not the source's");
    s.part_delete(&PartRef::Id(copy_id)).unwrap();
    assert_eq!(s.project().datum_axes.len(), 1);
    assert_eq!(s.project().datum_points.len(), 1);
    assert!(s.info().datums.errors.is_empty());
}

fn def_name(def: &qymcad_core::model::AxisDef) -> &'static str {
    use qymcad_core::model::AxisDef;
    match def {
        AxisDef::Manual { .. } => "manual",
        AxisDef::TwoPoints { .. } => "two_points",
        AxisDef::FromEdge { .. } => "edge",
        AxisDef::FromFace { .. } => "face",
    }
}

fn snap(s: &Session) -> String {
    ron::to_string(s.project()).unwrap()
}

/// A part with a block, a boss (cylinder), and datums standing on them: edge axis, face axis, two-point axis (manual points).
fn part_with_geometry_datums() -> (Session, u64, u64, u64) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    let (block, _) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":4,"op":"new_body"})).unwrap()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &30.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
    let (boss, _) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":10,"op":"new_body"})).unwrap()).unwrap();
    (s, block, boss, 0)
}

fn axis_through(s: &Session, id: u64) -> ([f64; 3], [f64; 3]) {
    let a = s.project().datum_axes.iter().find(|a| a.id == id).unwrap();
    (a.origin(), a.dir())
}

/// F-175: the app's copy remaps edge, face and two-point axes onto the copy's own bodies, but not a vertex point.
#[test]
fn part_copy_remaps_edge_face_and_two_point_axes_onto_its_own_bodies() {
    let (mut s, block, boss, _) = part_with_geometry_datums();
    let edge = s
        .topology(Some(block), false)
        .unwrap()
        .edges
        .into_iter()
        .find(|e| e.kind == EdgeKind::Line && (e.a[2] - e.b[2]).abs() > 1.0)
        .unwrap();
    let face = s.topology(Some(boss), false).unwrap().faces.iter().find(|f| f.kind == FaceKind::Cylinder).unwrap().id;
    let (ea, _) = s.datum_axis(&AxisSpec::Edge { edge: edge.id, body: Some(block) }, None).unwrap();
    let (fa, _) = s.datum_axis(&AxisSpec::Face { face, body: Some(boss) }, None).unwrap();
    let (p, _) = s.datum_point(&PointSpec::At([1.0.into(), 1.0.into(), 0.0.into()]), None).unwrap();
    let (q, _) = s.datum_point(&PointSpec::At([1.0.into(), 1.0.into(), 7.0.into()]), None).unwrap();
    let (ta, _) = s.datum_axis(&AxisSpec::Points([p.id, q.id]), None).unwrap();
    let source = PartRef::Id(s.project().active_component.unwrap());
    let copy = s.part_copy(&source, "copy", Some([0.0, 100.0, 0.0]), None).unwrap();
    let copy_id = copy["part"]["id"].as_u64().unwrap();
    assert_eq!(s.project().datum_axes.len(), 6);
    let owned: Vec<qymcad_core::model::DatumAxis> = s
        .project()
        .timeline
        .iter()
        .filter(|n| n.parent == Some(copy_id))
        .filter_map(|n| match n.kind {
            qymcad_core::feature::FeatureKind::DatumAxis { axis } => s.project().datum_axes.iter().find(|a| a.id == axis).cloned(),
            _ => None,
        })
        .collect();
    assert_eq!(owned.len(), 3, "the copy owns three axes");
    let copy_bodies: Vec<u64> = s.result_bodies().iter().filter(|b| b.owner_part == Some(copy_id)).map(|b| b.id).collect();
    let on_copy = |body: u64| copy_bodies.contains(&body);
    for a in &owned {
        match a.def {
            qymcad_core::model::AxisDef::FromEdge { body, .. } | qymcad_core::model::AxisDef::FromFace { body, .. } => {
                assert!(on_copy(body), "axis {} follows body {body}, which is not the copy's {copy_bodies:?}", a.id);
            }
            qymcad_core::model::AxisDef::TwoPoints { a: pa, b: pb } => {
                assert!(![p.id, q.id].contains(&pa) && ![p.id, q.id].contains(&pb), "the copy's two-point axis stands on its own points");
            }
            _ => unreachable!(),
        }
    }
    // The copy's axes lie where the source's lie (the copy is a Part-frame copy, the frame is the same).
    for (src, def) in [(ea.id, "edge"), (fa.id, "face"), (ta.id, "two_points")] {
        let twin = owned.iter().find(|a| def_name(&a.def) == def);
        let twin = twin.unwrap_or_else(|| panic!("no copied {def} axis"));
        assert_eq!(axis_through(&s, twin.id), axis_through(&s, src), "{def}");
    }
    // Deleting the source body leaves the copy's datums alone: they follow the copy's bodies.
    s.feature_delete(block, true).unwrap();
    assert!(s.info().datums.errors.is_empty());
    let left = s
        .project()
        .timeline
        .iter()
        .filter(|n| n.parent == Some(copy_id) && matches!(n.kind, qymcad_core::feature::FeatureKind::DatumAxis { .. }))
        .count();
    assert_eq!(left, 3, "the copy keeps its three axes");
}

#[test]
fn part_copy_with_a_vertex_point_is_refused_f175_and_part_instance_does_not_copy_datums() {
    let (mut s, block, ..) = part_with_geometry_datums();
    let edge = s.topology(Some(block), false).unwrap().edges[0].id;
    let (vp, _) = s.datum_point(&PointSpec::Vertex { edge, end: VertexEnd::Start, body: Some(block) }, None).unwrap();
    let source = PartRef::Id(s.project().active_component.unwrap());
    s.rebuild();
    let before = snap(&s);
    let e = s.part_copy(&source, "copy", None, None).unwrap_err().to_string();
    assert!(e.contains("F-175") && e.contains(&format!("{}", vp.id)) && e.contains("SOURCE"), "{e}");
    assert!(snap(&s) == before, "the refusal changed nothing");
    // A clone repeats the body only: it has no datum nodes of its own, and the source's datums are untouched. The part has two
    // bodies (the block and the boss), which part_instance refuses for another reason (F-146): use a single-body part.
    let mut t = Session::new_part();
    let sk = t.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    t.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    let (body, _) = t.extrude(&serde_json::from_value(json!({"sketch":sk,"height":4,"op":"new_body"})).unwrap()).unwrap();
    let edge = t.topology(Some(body), false).unwrap().edges[0].id;
    t.datum_point(&PointSpec::Vertex { edge, end: VertexEnd::Start, body: None }, None).unwrap();
    t.datum_axis(&AxisSpec::Edge { edge, body: None }, None).unwrap();
    let source = PartRef::Id(t.project().active_component.unwrap());
    let clone = t.part_instance(&source, "twin", Some([0.0, 100.0, 0.0]), None).unwrap();
    let clone_id = clone["part"]["id"].as_u64().unwrap();
    assert_eq!((t.project().datum_points.len(), t.project().datum_axes.len()), (1, 1), "no datum was copied");
    assert!(t
        .project()
        .timeline
        .iter()
        .filter(|n| n.parent == Some(clone_id))
        .all(|n| matches!(n.kind, qymcad_core::feature::FeatureKind::PartInstance { .. })));
    assert!(t.info().datums.errors.is_empty());
    // Both datums still follow the source body after the clone exists.
    assert!(t.project().datum_points.iter().all(|p| matches!(p.def, qymcad_core::model::PointDef::AtVertex { body: b, .. } if b == body)));
}

/// F-170-style owner report: two Parts, each datum names its owner in doc_info.
#[test]
fn doc_info_names_the_owner_part_of_each_datum() {
    let mut s = Session::new_part();
    let first = s.project().active_component.unwrap();
    let (a, _) = s.datum_point(&PointSpec::At([0.0.into(), 0.0.into(), 0.0.into()]), None).unwrap();
    assert_eq!(a.owner_part, None, "a single unplaced Part says nothing");
    s.part_new("second").unwrap();
    let second = s.project().active_component.unwrap();
    let (b, _) = s.datum_axis(&AxisSpec::Manual { origin: [0.0; 3], dir: [0.0, 0.0, 1.0] }, None).unwrap();
    assert_eq!(b.owner_part, Some(second));
    let info = s.info();
    assert_eq!(info.datums.points[0].owner_part, Some(first));
    assert_eq!(info.datums.axes[0].owner_part, Some(second));
}

/// Opening reads the app's file: a two-point axis that lost its point (the app's own delete leaves it so) opens with a
/// warning, and unrelated edits stay possible; the refusal belongs to edits of the datum.
#[test]
fn opening_a_file_with_a_broken_datum_warns_and_does_not_lock_the_document() {
    let mut s = Session::new_part();
    s.param_set("w", &10.0.into()).unwrap();
    let (p, _) = s.datum_point(&PointSpec::At([0.0.into(), 0.0.into(), 0.0.into()]), None).unwrap();
    let (q, _) = s.datum_point(&PointSpec::At([0.0.into(), 0.0.into(), "w".into()]), None).unwrap();
    let (axis, _) = s.datum_axis(&AxisSpec::Points([p.id, q.id]), None).unwrap();
    let path = scratch("broken-axis.qcad");
    s.save(Some(&path)).unwrap();
    // The app deletes a point under an axis without a word (`delete_datum_point`); do the same to the saved file.
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    assert!(m.project.delete_datum_point(q.id));
    let broken = scratch("broken-axis-2.qcad");
    native_assembly_save(&m, &broken);
    let (mut opened, report) = Session::open(&broken).expect("opening a file is not refused for a datum fault");
    let text = format!("{:?}", report.warnings);
    assert!(text.contains("no longer exists") && text.contains(&format!("{}", axis.id)), "{text}");
    assert!(opened.info().warnings.iter().any(|w| w.contains("no longer exists")), "doc_info carries it too");
    // An unrelated edit still goes through.
    opened.param_set("w", &20.0.into()).unwrap();
    opened.param_set("other", &1.0.into()).unwrap();
    // A fresh fault on another axis is still refused.
    let (r, _) = opened.datum_point(&PointSpec::At([1.0.into(), 0.0.into(), 0.0.into()]), None).unwrap();
    let (r2, _) = opened.datum_point(&PointSpec::At([1.0.into(), 0.0.into(), "w".into()]), None).unwrap();
    opened.datum_axis(&AxisSpec::Points([r.id, r2.id]), None).unwrap();
    let e = opened.param_set("w", &0.0.into()).unwrap_err().to_string();
    assert!(e.contains("two coincident points"), "{e}");
}
