//! S2: the app's construction toggle (`toggle_construction`), its one-state rule and the refusals around it.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

fn plate_with_hole() -> (Session, u64, u64, u64) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("plate")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &10.0.into(), false).unwrap();
    let hole = s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
    // The hole is entity id `hole`; the contour of the plate is the one without a parent.
    let plate = s.sketch_info(sk).unwrap().contours.iter().find(|c| c.parent.is_none()).unwrap().id;
    (s, sk, hole, plate)
}

fn toggle(s: &mut Session, sk: u64, ids: &[u64], to: bool) -> Result<ConstructionEdit> {
    s.sketch_edit(sk, |s| s.sketch_construction(sk, ids, to)).map(|(done, _)| done)
}

#[test]
fn a_construction_curve_leaves_the_profile_and_comes_back() {
    let (mut s, sk, hole, plate) = plate_with_hole();
    assert_eq!(s.sketch_info(sk).unwrap().contours.len(), 2);
    let done = toggle(&mut s, sk, &[hole], true).unwrap();
    assert_eq!(done.entities, vec![ConstructionState { id: hole, construction: true }]);
    let contours = s.sketch_info(sk).unwrap().contours;
    assert_eq!(contours.len(), 1, "the circle is no longer a contour");
    assert_eq!(contours[0].id, plate);
    let back = toggle(&mut s, sk, &[hole], false).unwrap();
    assert_eq!(back.entities, vec![ConstructionState { id: hole, construction: false }]);
    assert_eq!(s.sketch_info(sk).unwrap().contours.len(), 2);
    // Without a consumer both ways are free; a plate extruded with the hole as construction has no hole: 20 x 10 x 5.
    toggle(&mut s, sk, &[hole], true).unwrap();
    let (_, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
    assert_close(r.bodies[0].volume, 1000.0, 1e-6, "plate without the construction hole");
    // ... and now that a consumer holds the contour the hole cannot come back silently (F-168).
    assert!(toggle(&mut s, sk, &[hole], false).unwrap_err().to_string().contains("F-168"));
}

#[test]
fn the_app_gives_a_mixed_set_one_state_and_a_wrong_to_is_refused_unchanged() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let line = |s: &mut Session, y: f64, construction: bool| {
        s.sketch_line(sk, &LineSpec { x1: 0.0.into(), y1: y.into(), x2: 10.0.into(), y2: y.into(), construction, dimensioned: false })
            .unwrap()
            .entities[0]
    };
    let (a, b) = (line(&mut s, 0.0, false), line(&mut s, 5.0, true));
    let before = ron::to_string(s.project()).unwrap();
    let wrong = toggle(&mut s, sk, &[a, b], false).unwrap_err().to_string();
    assert!(wrong.contains("would become construction"), "{wrong}");
    assert_eq!(ron::to_string(s.project()).unwrap(), before, "a refusal changes nothing");
    let done = toggle(&mut s, sk, &[a, b], true).unwrap();
    assert!(done.entities.iter().all(|e| e.construction), "mixed set -> all construction");
    let all = toggle(&mut s, sk, &[a, b], true).unwrap_err().to_string();
    assert!(all.contains("already all construction"), "{all}");
    let done = toggle(&mut s, sk, &[a, b], false).unwrap();
    assert!(done.entities.iter().all(|e| !e.construction), "all construction -> all ordinary");
}

#[test]
fn text_points_foreign_ids_and_empty_sets_are_refused() {
    let (mut s, sk, hole, _) = plate_with_hole();
    let text = s
        .sketch_text(sk, &TextSpec { text: "A".into(), at: [0.0, 0.0], height: 4.0, angle: 0.0, font: None, construction: false })
        .unwrap();
    let point = s.project().sketches[0].points.iter().map(|p| p.id).find(|id| s.project().sketches[0].entity(*id).is_none()).unwrap();
    let other = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let foreign = s.sketch_circle(other, &0.0.into(), &0.0.into(), &3.0.into(), false).unwrap();
    let before = ron::to_string(s.project()).unwrap();
    for (ids, word) in [
        (vec![], "non-empty"),
        (vec![text], "sketch text"),
        (vec![point], "sketch point"),
        (vec![foreign], "not a curve of this sketch"),
        (vec![hole, text], "sketch text"),
    ] {
        let e = toggle(&mut s, sk, &ids, true).unwrap_err().to_string();
        assert!(e.contains(word), "{ids:?}: {e}");
    }
    assert_eq!(ron::to_string(s.project()).unwrap(), before);
}

#[test]
fn a_hole_turned_construction_under_a_consumer_is_refused_f168_and_the_vanishing_contour_rolls_back() {
    let (mut s, sk, hole, _) = plate_with_hole();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
    let volume = s.result_bodies()[0].volume;
    let before = ron::to_string(s.project()).unwrap();
    let e = toggle(&mut s, sk, &[hole], true).unwrap_err().to_string();
    assert!(e.contains("F-168") && e.contains("holes"), "{e}");
    assert_eq!(ron::to_string(s.project()).unwrap(), before);
    assert_close(s.result_bodies()[0].volume, volume, 1e-9, "the consumer is untouched");
    // The outer contour of a sketch whose only consumer holds it: the app errors, the edit rolls back.
    let mut t = Session::new_part();
    let sk = t.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let circle = t.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
    t.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
    let before = ron::to_string(t.project()).unwrap();
    let e = toggle(&mut t, sk, &[circle], true).unwrap_err().to_string();
    assert!(e.contains("profile was not found"), "{e}");
    assert_eq!(ron::to_string(t.project()).unwrap(), before);
}

#[test]
fn construction_survives_save_and_open() {
    let (mut s, sk, hole, _) = plate_with_hole();
    toggle(&mut s, sk, &[hole], true).unwrap();
    let path = scratch("construction.qcad");
    s.save(Some(&path)).unwrap();
    let (loaded, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert!(loaded.project().sketches[0].entity(hole).unwrap().construction);
    assert_eq!(loaded.sketch_info(sk).unwrap().contours.len(), 1);
}
