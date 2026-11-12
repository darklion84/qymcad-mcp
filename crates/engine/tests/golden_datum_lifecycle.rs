//! Datum points and axes under history rollback, reorder, Part copy and Part delete.
mod common;
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
