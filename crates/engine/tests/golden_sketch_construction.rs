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
    // ... and with a consumer the hole comes back as the app's own rebuild: the held outer contour is the same one.
    toggle(&mut s, sk, &[hole], false).unwrap();
    assert_close(s.result_bodies()[0].volume, 1000.0 - std::f64::consts::PI * 4.0 * 5.0, 1e-3, "the plate has its hole again");
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

fn extrude_all(s: &mut Session, sk: u64, height: u32) {
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":height,"op":"new_body"})).unwrap()).unwrap();
}

/// F-168 (rewritten): the app follows a held contour through a change of its shape, so a hole turned construction is a
/// legal edit (probe `i0_probes::p11_*` and `native_*` pin the app's side).
#[test]
fn a_hole_turned_construction_under_a_consumer_is_followed_by_it() {
    let (mut s, sk, hole, _) = plate_with_hole();
    extrude_all(&mut s, sk, 5);
    assert_close(s.result_bodies()[0].volume, 1000.0 - std::f64::consts::PI * 4.0 * 5.0, 1e-3, "plate with the hole");
    let done = toggle(&mut s, sk, &[hole], true).unwrap();
    assert!(done.entities[0].construction);
    assert_close(s.result_bodies()[0].volume, 1000.0, 1e-6, "the plate lost its hole: the volume grew");
    assert!(s.current_rebuild().errors.is_empty());
}

/// Two islands in one extrude: the one turned construction would silently drop out of the feature.
#[test]
fn a_vanished_island_of_a_several_profile_feature_is_refused_f168() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &(-20.0).into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    let island = s.sketch_circle(sk, &20.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
    extrude_all(&mut s, sk, 5);
    let volume = s.result_bodies()[0].volume;
    assert_close(volume, 500.0 + std::f64::consts::PI * 25.0 * 5.0, 1e-3, "square and disc");
    let before = ron::to_string(s.project()).unwrap();
    let e = toggle(&mut s, sk, &[island], true).unwrap_err().to_string();
    assert!(e.contains("silently drops out") && e.contains("F-168"), "{e}");
    assert_eq!(ron::to_string(s.project()).unwrap(), before);
    assert_close(s.result_bodies()[0].volume, volume, 1e-9, "the consumer is untouched");
}

/// The guard sits in the shared sketch-edit path: removing the island is refused as well, removing a hole passes (F-177).
#[test]
fn removing_an_island_under_a_several_profile_feature_is_refused_and_a_hole_passes() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &(-20.0).into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    let island = s.sketch_circle(sk, &20.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
    extrude_all(&mut s, sk, 5);
    let volume = s.result_bodies()[0].volume;
    let before = ron::to_string(s.project()).unwrap();
    let e = s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, island)).unwrap_err().to_string();
    assert!(e.contains("F-168") && e.contains("silently drops out") && e.contains("this edit would remove or open contour"), "{e}");
    assert_eq!(ron::to_string(s.project()).unwrap(), before);
    assert_close(s.result_bodies()[0].volume, volume, 1e-9, "the consumer is untouched");
    // A hole circle removed from a plate extrude: the consumer follows.
    let (mut p, psk, hole, _) = plate_with_hole();
    extrude_all(&mut p, psk, 5);
    p.sketch_edit(psk, |s| s.sketch_remove_entity(psk, hole)).unwrap();
    assert_close(p.result_bodies()[0].volume, 1000.0, 1e-6, "the plate lost its hole: the volume grew");
}

/// A ring no feature holds may be turned construction: the consumer's own island is untouched.
#[test]
fn an_unused_ring_turned_construction_passes() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &(-20.0).into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    let outer = s.sketch_circle(sk, &20.0.into(), &0.0.into(), &12.0.into(), false).unwrap();
    s.sketch_circle(sk, &20.0.into(), &0.0.into(), &6.0.into(), false).unwrap();
    // The extrude picks the square by a point; the ring is not in its profiles.
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body","profiles_at":[[-20.0, 0.0]]})).unwrap()).unwrap();
    assert_close(s.result_bodies()[0].volume, 500.0, 1e-6, "the square only");
    toggle(&mut s, sk, &[outer], true).unwrap();
    assert_close(s.result_bodies()[0].volume, 500.0, 1e-6, "the consumer is as it was");
    assert!(s.current_rebuild().errors.is_empty());
}

/// A feature with one profile: the app itself reports "profile was not found" and the edit rolls back.
#[test]
fn the_only_profile_turned_construction_rolls_back_through_the_apps_error() {
    let mut t = Session::new_part();
    let sk = t.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let circle = t.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
    extrude_all(&mut t, sk, 5);
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

fn line(s: &mut Session, sk: u64, a: [f64; 2], b: [f64; 2]) -> Added {
    s.sketch_line(
        sk,
        &LineSpec { x1: a[0].into(), y1: a[1].into(), x2: b[0].into(), y2: b[1].into(), construction: false, dimensioned: false },
    )
    .unwrap()
}

/// A 20 x 10 plate split in two regions by a divider; the extrude holds both regions.
fn divided_plate() -> (Session, u64, u64) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &10.0.into(), false).unwrap();
    let divider = line(&mut s, sk, [0.0, -5.0], [0.0, 5.0]).entities[0];
    assert_eq!(s.sketch_info(sk).unwrap().contours.iter().filter(|c| c.closed).count(), 2, "two regions");
    extrude_all(&mut s, sk, 5);
    assert_close(s.result_bodies()[0].volume, 1000.0, 1e-6, "both regions extruded");
    (s, sk, divider)
}

/// Round 2, M1: removing the divider merges two held regions; the app builds the whole plate and nothing is lost.
#[test]
fn removing_or_turning_a_divider_into_construction_merges_regions_and_passes() {
    let (mut s, sk, divider) = divided_plate();
    s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, divider)).unwrap();
    assert_close(s.result_bodies()[0].volume, 1000.0, 1e-6, "removed divider: the same plate");
    assert!(s.current_rebuild().errors.is_empty());
    let (mut t, sk, divider) = divided_plate();
    toggle(&mut t, sk, &[divider], true).unwrap();
    assert_close(t.result_bodies()[0].volume, 1000.0, 1e-6, "construction divider: the same plate");
    assert!(t.current_rebuild().errors.is_empty());
}

/// M2: an open chain held by the extrude is not a profile; a stray line comes and goes freely, an open polyline may be closed.
#[test]
fn open_chains_are_not_profiles_to_the_guard() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    let stray = line(&mut s, sk, [30.0, 0.0], [40.0, 0.0]).entities[0];
    extrude_all(&mut s, sk, 5);
    assert_close(s.result_bodies()[0].volume, 500.0, 1e-6, "the square");
    s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, stray)).unwrap();
    assert_close(s.result_bodies()[0].volume, 500.0, 1e-6, "the stray line was no profile");
    // An open U next to the square, closed afterwards.
    let u = PolylineSpec {
        points: vec![Xy(30.0.into(), 0.0.into()), Xy(30.0.into(), 10.0.into()), Xy(40.0.into(), 10.0.into()), Xy(40.0.into(), 0.0.into())],
        closed: false,
        construction: false,
        dimensioned: false,
    };
    s.sketch_edit(sk, |s| s.sketch_polyline(sk, &u).map(|_| ())).unwrap();
    s.sketch_edit(sk, |s| {
        s.sketch_line(
            sk,
            &LineSpec { x1: 40.0.into(), y1: 0.0.into(), x2: 30.0.into(), y2: 0.0.into(), construction: false, dimensioned: false },
        )
        .map(|_| ())
    })
    .unwrap();
    assert!(s.current_rebuild().errors.is_empty());
}

/// Accepted app behaviour: a held circle cut in two by a new line stays the same profile for its consumer (the app matches it to a
/// half), and a new hole inside a held contour is a changed shape; neither is refused (F-177).
#[test]
fn dividing_a_held_contour_or_adding_a_hole_is_accepted_app_behaviour() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &(-20.0).into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    s.sketch_circle(sk, &20.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
    extrude_all(&mut s, sk, 5);
    assert_close(s.result_bodies()[0].volume, 500.0 + std::f64::consts::PI * 125.0, 1e-3, "square and disc");
    s.sketch_edit(sk, |s| {
        s.sketch_line(
            sk,
            &LineSpec { x1: 10.0.into(), y1: 0.0.into(), x2: 30.0.into(), y2: 0.0.into(), construction: false, dimensioned: false },
        )
        .map(|_| ())
    })
    .unwrap();
    // The app keeps the held id on one half; the other half is a new region the feature does not hold (accepted, F-177).
    assert_close(s.result_bodies()[0].volume, 500.0 + std::f64::consts::PI * 125.0 / 2.0, 1e-3, "square and the held half disc");
    s.sketch_edit(sk, |s| s.sketch_circle(sk, &(-20.0).into(), &0.0.into(), &4.0.into(), false).map(|_| ())).unwrap();
    assert!(s.current_rebuild().errors.is_empty());
}

/// Features that are suppressed or past the rollback bar do not build, so an island they hold is nobody's loss.
#[test]
fn suppressed_and_rolled_back_features_are_not_judged() {
    let island_doc = || {
        let mut s = Session::new_part();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sk, &(-20.0).into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
        let island = s.sketch_circle(sk, &20.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
        let (body, _) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
        (s, sk, island, body)
    };
    let (mut s, sk, island, body) = island_doc();
    s.feature_suppress(body, true).unwrap();
    s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, island)).unwrap();
    let (mut s, sk, island, _) = island_doc();
    s.history_rollback(Some(sk), None).unwrap();
    s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, island)).unwrap();
}

/// Trimming an island open is refused like removing it.
#[test]
fn trimming_a_held_island_open_is_refused() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &(-20.0).into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    s.sketch_circle(sk, &20.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
    // A vertical cutter through the disc cuts it into two regions held by the extrude; trimming the right half's arc opens it.
    line(&mut s, sk, [20.0, -8.0], [20.0, 8.0]);
    extrude_all(&mut s, sk, 5);
    let circle = s.sketch_detail(sk).unwrap().entities.iter().find(|e| e.kind == "circle" || e.kind == "arc").map(|e| e.id).unwrap();
    let before = ron::to_string(s.project()).unwrap();
    let e = s
        .sketch_edit(sk, |s| s.sketch_trim(sk, &CurvePickSpec { curve: EntityRef::Id(circle), at: [25.0.into(), 0.0.into()] }).map(|_| ()));
    let e = e.unwrap_err().to_string();
    assert!(e.contains("remove or open contour") && e.contains("F-177"), "{e}");
    assert_eq!(ron::to_string(s.project()).unwrap(), before);
}
