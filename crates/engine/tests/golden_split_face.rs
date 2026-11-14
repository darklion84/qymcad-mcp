//! P14: Split faces. Hand counts and volumes of the app's one action, the plane references, formulas, every refusal with the
//! document unchanged (F-187..F-191), the app's accepted behaviour (F-192, F-193) and the lifecycle of the node.
mod common;
use common::*;
use qymcad_engine::*;
use std::f64::consts::PI;

fn snap(s: &Session) -> String {
    ron::to_string(s.project()).unwrap()
}

fn spec_box(dx: impl Into<Num>, dy: impl Into<Num>, dz: impl Into<Num>) -> PrimitiveSpec {
    PrimitiveSpec::Box { dx: dx.into(), dy: dy.into(), dz: dz.into() }
}

fn expr(text: &str) -> Num {
    Num::Expr(text.into())
}

/// The block 40 x 30 x 10 of the app's acceptance test: x -20..20, y -15..15, z 0..10.
fn block() -> (Session, Id) {
    let mut s = Session::new_part();
    let b = s.primitive(&spec_box(40.0, 30.0, 10.0), &Placement::Origin, None).unwrap().0.body;
    (s, b)
}

fn xy(offset: impl Into<Num>) -> SplitFaceOptions {
    SplitFaceOptions { body: None, plane: PlaneRef::Base(BaseName::XY), offset: offset.into(), name: None }
}

fn faces(s: &Session, body: Id) -> usize {
    s.project().regen_faces[&body].len()
}

fn ids(s: &Session, body: Id) -> Vec<u32> {
    let mut v: Vec<u32> = s.project().regen_faces[&body].iter().map(|f| f.id).collect();
    v.sort_unstable();
    v
}

fn only_body(s: &Session) -> BodyInfo {
    let bodies = s.result_bodies();
    assert_eq!(bodies.len(), 1, "one body: {:?}", bodies.iter().map(|b| b.id).collect::<Vec<_>>());
    bodies[0].clone()
}

/// The z levels of the horizontal edges of a body: where the cuts stand.
fn levels(s: &Session, body: Id) -> Vec<f64> {
    let mut z: Vec<f64> =
        s.project().regen_edges[&body].iter().filter(|e| e.dir[2].abs() < 1e-6).map(|e| (e.mid[2] * 1e4).round() / 1e4).collect();
    z.sort_by(|a, b| a.partial_cmp(b).unwrap());
    z.dedup();
    z
}

/// A refused call: its text, and the document byte for byte as it was.
fn refused(s: &mut Session, options: &SplitFaceOptions) -> String {
    let before = snap(s);
    let error = s.split_face(options).unwrap_err().to_string();
    assert!(snap(s) == before, "a refused split_face changes nothing: {error}");
    error
}

fn face_with(s: &Session, body: Id, normal: [f64; 3], at_z: f64) -> u32 {
    s.project().regen_faces[&body]
        .iter()
        .find(|f| (0..3).all(|i| (f.normal[i] - normal[i]).abs() < 1e-6) && (f.centroid.z - at_z).abs() < 1e-6)
        .unwrap_or_else(|| panic!("no face with normal {normal:?} at z {at_z}"))
        .id
}

#[test]
fn a_block_cut_at_mid_height_has_ten_faces_twenty_edges_and_the_same_volume() {
    let (mut s, b) = block();
    assert_eq!((faces(&s, b), s.project().regen_edges[&b].len()), (6, 12));
    let source_ids = ids(&s, b);
    let r = s.split_face(&SplitFaceOptions { name: Some("mid cut".into()), ..xy(5.0) }).unwrap();
    assert_eq!((r.feature, r.body, r.name.as_str(), r.offset), (r.feature, r.feature, "mid cut", 5.0));
    assert_eq!((r.faces.before, r.faces.after), (6, 10));
    assert!(r.rebuild.errors.is_empty(), "{:?}", r.rebuild.errors);
    let after = ids(&s, r.body);
    assert_eq!((faces(&s, r.body), s.project().regen_edges[&r.body].len()), (10, 20));
    // The four walls are cut: each keeps its id for one piece and the other piece is new; the top and bottom keep theirs.
    assert_eq!(r.added_faces.len(), 4, "{:?}", r.added_faces);
    assert!(r.added_faces.iter().all(|id| after.contains(id) && !source_ids.contains(id)));
    assert!(source_ids.iter().all(|id| after.contains(id)), "no source id is lost");
    let body = only_body(&s);
    assert_eq!(body.id, r.body);
    assert_close(body.volume, 12000.0, 1e-6, "material is not removed");
    for (got, want) in body.bbox_local.iter().zip([-20.0, -15.0, 0.0, 20.0, 15.0, 10.0]) {
        assert_close(*got, want, 1e-6, "bounds are unchanged");
    }
    assert_eq!(levels(&s, r.body), vec![0.0, 5.0, 10.0]);
    // The timeline holds one node of the app's kind.
    let node = s.project().timeline.iter().find(|n| n.id == r.feature).unwrap();
    assert!(matches!(node.kind, qymcad_core::feature::FeatureKind::SplitFace { .. }));
}

#[test]
fn a_cylinder_gets_one_more_curved_face_and_keeps_its_volume() {
    let mut s = Session::new_part();
    s.primitive(&PrimitiveSpec::Cylinder { r: 10.0.into(), h: 20.0.into() }, &Placement::Origin, None).unwrap();
    let r = s.split_face(&xy(8.0)).unwrap();
    assert_eq!((r.faces.before, r.faces.after, r.added_faces.len()), (3, 4, 1));
    assert_close(only_body(&s).volume, PI * 100.0 * 20.0, 1e-6, "volume of the cylinder");
}

#[test]
fn a_datum_plane_cuts_where_it_stands_and_the_cut_follows_it() {
    let (mut s, b) = block();
    s.param_set("d", &4.0.into()).unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &expr("d"), None).unwrap();
    let r = s.split_face(&SplitFaceOptions { plane: PlaneRef::Plane(plane), ..xy(0.0) }).unwrap();
    assert_eq!((r.faces.before, r.faces.after), (6, 10));
    assert_eq!(levels(&s, r.body), vec![0.0, 4.0, 10.0]);
    // Offset along the datum's own normal: the plane stands at d + 2.
    let more = s.split_face(&SplitFaceOptions { plane: PlaneRef::Plane(plane), ..xy(2.0) }).unwrap();
    assert_eq!(levels(&s, more.body), vec![0.0, 4.0, 6.0, 10.0]);
    assert_eq!(more.faces.after, 14);
    // Moving the datum moves the first cut; the second stays on the datum's offset: both follow it.
    s.param_set("d", &3.0.into()).unwrap();
    assert_eq!(levels(&s, more.body), vec![0.0, 3.0, 5.0, 10.0]);
    assert!(b != more.body);
}

#[test]
fn the_plane_of_a_face_follows_the_face_when_the_body_changes() {
    let mut s = Session::new_part();
    s.param_set("h", &10.0.into()).unwrap();
    let b = s.primitive(&spec_box(40.0, 30.0, expr("h")), &Placement::Origin, None).unwrap().0.body;
    let top = face_with(&s, b, [0.0, 0.0, 1.0], 10.0);
    let plane = PlaneRef::Face { body: b, face: top };
    let r = s.split_face(&SplitFaceOptions { plane: plane.clone(), ..xy(-5.0) }).unwrap();
    assert_eq!((r.faces.before, r.faces.after), (6, 10), "the app's acceptance case: the top face, offset -5");
    assert_eq!(levels(&s, r.body), vec![0.0, 5.0, 10.0]);
    s.param_set("h", &14.0.into()).unwrap();
    assert_eq!(levels(&s, r.body), vec![0.0, 9.0, 14.0], "the plane went up with the face");
    assert_close(only_body(&s).volume, 40.0 * 30.0 * 14.0, 1e-6, "volume");
}

#[test]
fn the_offset_is_a_formula_that_follows_parameters_and_dimension_set() {
    let (mut s, _) = block();
    s.param_set("cut", &3.0.into()).unwrap();
    let r = s.split_face(&xy(expr("cut + 1"))).unwrap();
    assert_eq!(r.offset, 4.0);
    assert_eq!(levels(&s, r.body), vec![0.0, 4.0, 10.0]);
    s.param_set("cut", &6.0.into()).unwrap();
    assert_eq!(levels(&s, r.body), vec![0.0, 7.0, 10.0]);
    assert_eq!(faces(&s, r.body), 10);
    // A plain number is edited by dimension_set on the key `offset`.
    s.dimension_set(&DimensionTarget::Feature { feature: r.feature, key: "offset".into() }, Some(&2.0.into()), None).unwrap();
    assert_eq!(levels(&s, r.body), vec![0.0, 2.0, 10.0]);
}

#[test]
fn a_curved_face_as_the_plane_is_refused_with_the_document_unchanged() {
    let mut s = Session::new_part();
    let b = s.primitive(&PrimitiveSpec::Cylinder { r: 10.0.into(), h: 20.0.into() }, &Placement::Origin, None).unwrap().0.body;
    let side = s.project().regen_faces[&b].iter().find(|f| f.normal[2].abs() < 1e-6).map(|f| f.id).unwrap_or_else(|| {
        // The mean normal of a full cylinder may be tiny but not axial: take the face that is neither a cap.
        s.project().regen_faces[&b].iter().find(|f| (f.centroid.z - 10.0).abs() < 1e-3).unwrap().id
    });
    let e = refused(&mut s, &SplitFaceOptions { plane: PlaneRef::Face { body: b, face: side }, ..xy(0.0) });
    assert!(e.contains("not planar") && e.contains("F-191"), "{e}");
}

#[test]
fn a_plane_that_misses_or_lies_on_a_face_splits_nothing_and_is_refused() {
    let (mut s, _) = block();
    for offset in [50.0, -3.0, 10.0, 0.0] {
        let e = refused(&mut s, &xy(offset));
        assert!(e.contains("splits nothing"), "offset {offset}: {e}");
    }
    let e = refused(&mut s, &xy(f64::NAN));
    assert!(e.contains("finite"), "{e}");
    let e = refused(&mut s, &xy(expr("1/0")));
    assert!(!e.is_empty());
}

#[test]
fn a_plane_a_hair_from_a_face_gains_no_face_and_is_refused_f187() {
    let (mut s, _) = block();
    // Observed: below the top face 1e-6 and 1e-5 mm fail silently; above the bottom face 1e-6 does (1e-5 splits).
    for gap in [1e-6, 1e-5] {
        let e = refused(&mut s, &xy(10.0 - gap));
        assert!(e.contains("divided no face") && e.contains("F-187"), "gap {gap}: {e}");
    }
    let e = refused(&mut s, &xy(1e-6));
    assert!(e.contains("divided no face"), "{e}");
    // 1e-4 mm is a real split (observed): the refusal is the missing face, not the distance.
    let r = s.split_face(&xy(10.0 - 1e-4)).unwrap();
    assert_eq!((r.faces.before, r.faces.after), (6, 10));
    // A torus cut through its axis: the app builds it green and splits nothing.
    let mut t = Session::new_part();
    t.primitive(&PrimitiveSpec::Torus { major: 12.0.into(), minor: 4.0.into() }, &Placement::Origin, None).unwrap();
    let e = refused(&mut t, &SplitFaceOptions { plane: PlaneRef::Base(BaseName::XZ), ..xy(0.0) });
    assert!(e.contains("divided no face") && e.contains("torus"), "{e}");
}

#[test]
fn an_edit_that_moves_the_plane_into_the_gap_is_refused_and_rolled_back() {
    let (mut s, _) = block();
    s.param_set("cut", &5.0.into()).unwrap();
    let r = s.split_face(&xy(expr("cut"))).unwrap();
    let before = snap(&s);
    let e = s.param_set("cut", &(10.0 - 1e-6).into()).unwrap_err().to_string();
    assert!(e.contains("divided no face"), "{e}");
    assert!(snap(&s) == before, "the parameter stays 5");
    let e = s
        .dimension_set(&DimensionTarget::Feature { feature: r.feature, key: "offset".into() }, Some(&(10.0 - 1e-6).into()), None)
        .unwrap_err()
        .to_string();
    assert!(e.contains("divided no face"), "{e}");
    assert!(snap(&s) == before);
    assert_eq!(levels(&s, r.body), vec![0.0, 5.0, 10.0]);
}

#[test]
fn a_second_split_below_the_first_has_duplicate_face_ids_and_is_refused_f189() {
    let (mut s, _) = block();
    s.split_face(&xy(6.0)).unwrap();
    let e = refused(&mut s, &xy(3.0));
    assert!(e.contains("distinct ids") && e.contains("F-189"), "{e}");
    // A plane that crosses the first is refused as well (observed for every pair, k0 probe 14c).
    let e = refused(&mut s, &SplitFaceOptions { plane: PlaneRef::Base(BaseName::XZ), ..xy(0.0) });
    assert!(e.contains("distinct ids"), "{e}");
    // The other order is clean.
    let (mut s, _) = block();
    s.split_face(&xy(3.0)).unwrap();
    let r = s.split_face(&xy(6.0)).unwrap();
    assert_eq!((r.faces.before, r.faces.after), (10, 14));
    let mut all = ids(&s, r.body);
    all.dedup();
    assert_eq!(all.len(), 14, "ids are unique");
}

#[test]
fn an_edit_that_turns_the_order_of_two_splits_is_refused_f189() {
    let (mut s, _) = block();
    s.param_set("a", &3.0.into()).unwrap();
    s.param_set("b", &6.0.into()).unwrap();
    s.split_face(&xy(expr("a"))).unwrap();
    let second = s.split_face(&xy(expr("b"))).unwrap();
    let before = snap(&s);
    let e = s.param_set("a", &8.0.into()).unwrap_err().to_string();
    assert!(e.contains("distinct ids") || e.contains("changes the faces"), "{e}");
    assert!(snap(&s) == before);
    assert_eq!(faces(&s, second.body), 14);
}

#[test]
fn reordering_two_splits_into_the_bad_order_is_refused_and_leaves_the_document() {
    let (mut s, _) = block();
    s.split_face(&xy(3.0)).unwrap();
    let second = s.split_face(&xy(6.0)).unwrap();
    let before = snap(&s);
    let e = s.feature_reorder(second.feature, "up").unwrap_err().to_string();
    assert!(e.contains("distinct ids") || e.contains("changes the faces"), "{e}");
    assert!(snap(&s) == before);
}

/// Two boxes one over the other with a gap; the plane is the top of the lower one. Closing the gap fuses the faces.
fn two_boxes_with_a_gap() -> (Session, Id, u32) {
    let mut s = Session::new_part();
    s.param_set("ah", &5.0.into()).unwrap();
    s.primitive(&spec_box(40.0, 30.0, expr("ah")), &Placement::Origin, None).unwrap();
    let (point, _) = s.datum_point(&PointSpec::At([0.0.into(), 0.0.into(), 10.0.into()]), None).unwrap();
    let (_, _) = s.primitive(&spec_box(40.0, 30.0, 10.0), &Placement::Point(point.id), None).unwrap();
    let body = only_body(&s).id;
    let top = face_with(&s, body, [0.0, 0.0, 1.0], 5.0);
    (s, body, top)
}

#[test]
fn a_plane_taken_from_a_face_that_disappears_is_refused_f190() {
    let (mut s, body, top) = two_boxes_with_a_gap();
    let r = s.split_face(&SplitFaceOptions { body: Some(body), plane: PlaneRef::Face { body, face: top }, ..xy(-2.0) }).unwrap();
    assert_eq!(levels(&s, r.body).first().copied(), Some(0.0));
    assert!(levels(&s, r.body).contains(&3.0));
    let before = snap(&s);
    // The lower box grows to touch the upper one: its top face is fused into the interior and its id is gone.
    let e = s.param_set("ah", &10.0.into()).unwrap_err().to_string();
    assert!(e.contains("F-190") && e.contains("no longer exists"), "{e}");
    assert!(snap(&s) == before);
    // While the face lives the plane follows it.
    s.param_set("ah", &6.0.into()).unwrap();
    assert!(levels(&s, r.body).contains(&4.0), "{:?}", levels(&s, r.body));
}

/// A U-shaped stock (the profile in XY, 10 tall): the base 10 wide at y 0..4 and two arms up to y = 20.
fn u_stock(s: &mut Session) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let vertices = [(-10.0, 0.0), (10.0, 0.0), (10.0, 20.0), (6.0, 20.0), (6.0, 4.0), (-6.0, 4.0), (-6.0, 20.0), (-10.0, 20.0)];
    s.sketch_polyline(
        sk,
        &PolylineSpec {
            points: vertices.into_iter().map(|(x, y)| Xy(x.into(), y.into())).collect(),
            closed: true,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    s.extrude(&Extrude {
        down: None,
        sketch: sk,
        profiles: None,
        profiles_at: None,
        height: 10.0.into(),
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: Some("U stock".into()),
    })
    .unwrap()
    .0
}

#[test]
fn an_edit_that_changes_which_faces_are_cut_is_refused_f188() {
    let mut s = Session::new_part();
    u_stock(&mut s);
    s.param_set("y", &10.0.into()).unwrap();
    let r = s.split_face(&SplitFaceOptions { plane: PlaneRef::Base(BaseName::XZ), ..xy(expr("y")) }).unwrap();
    // The two U faces split in three, the two outer and the two inner walls in two.
    assert_eq!((r.faces.before, r.faces.after), (10, 18));
    // Moving within the arms keeps the same pieces.
    s.param_set("y", &15.0.into()).unwrap();
    assert_eq!(faces(&s, r.body), 18);
    s.param_set("y", &10.0.into()).unwrap();
    let before = snap(&s);
    // Below the notch the plane cuts another set of faces: the ids would name other pieces.
    let e = s.param_set("y", &2.0.into()).unwrap_err().to_string();
    assert!(e.contains("F-188") && e.contains("recreate the split"), "{e}");
    assert!(snap(&s) == before, "the parameter stays where it was");
    let e = s
        .dimension_set(&DimensionTarget::Feature { feature: r.feature, key: "offset".into() }, Some(&2.0.into()), None)
        .unwrap_err()
        .to_string();
    assert!(e.contains("F-188"), "{e}");
}

#[test]
fn an_edit_that_puts_the_plane_off_the_body_shows_the_apps_own_error_not_ours() {
    let (mut s, _) = block();
    s.param_set("cut", &5.0.into()).unwrap();
    let r = s.split_face(&xy(expr("cut"))).unwrap();
    let before = snap(&s);
    let e = s.param_set("cut", &50.0.into()).unwrap_err().to_string();
    assert!(e.contains("Split faces") && !e.contains("divided no face"), "{e}");
    assert!(snap(&s) == before);
    assert_eq!(levels(&s, r.body), vec![0.0, 5.0, 10.0]);
    let mut t = Session::new_part();
    t.primitive(&PrimitiveSpec::Torus { major: 12.0.into(), minor: 4.0.into() }, &Placement::Origin, None).unwrap();
    let e = t.split_face(&SplitFaceOptions { plane: PlaneRef::Base(BaseName::XZ), ..xy(0.0) }).unwrap_err().to_string();
    assert!(e.contains("divided no face"), "{e}");
}

fn wall_piece(s: &Session, body: Id, normal: [f64; 3], above: f64) -> u32 {
    s.project().regen_faces[&body]
        .iter()
        .find(|f| (0..3).all(|i| (f.normal[i] - normal[i]).abs() < 1e-6) && f.centroid.z > above)
        .expect("the piece")
        .id
}

#[test]
fn suppressing_the_split_gives_a_consumer_the_whole_face_f192_and_unsuppressing_returns_the_pieces() {
    let (mut s, b) = block();
    let r = s.split_face(&xy(5.0)).unwrap();
    let upper = wall_piece(&s, r.body, [1.0, 0.0, 0.0], 5.0);
    assert!(r.added_faces.contains(&upper), "the upper piece of the x = 20 wall is a new face");
    s.push_face(Some(r.body), &Sel::Ids(vec![upper]), &3.0.into(), None).unwrap();
    assert_close(only_body(&s).volume, 12000.0 + 3.0 * 30.0 * 5.0, 1e-6, "the piece alone moves");
    // F-192: the consumer's id is gone from the unsplit body; the app finds the face by place and pushes all of it.
    s.feature_suppress(r.feature, true).unwrap();
    assert_close(only_body(&s).volume, 12000.0 + 3.0 * 30.0 * 10.0, 1e-6, "the whole wall moves");
    s.feature_suppress(r.feature, false).unwrap();
    assert_close(only_body(&s).volume, 12000.0 + 3.0 * 30.0 * 5.0, 1e-6, "the piece alone again");
    assert!(b != r.body);
}

#[test]
fn a_fillet_on_the_edge_of_one_piece_rounds_the_whole_edge_f193() {
    let (mut s, _) = block();
    let r = s.split_face(&xy(5.0)).unwrap();
    // The corner edge x = 20, y = 15: two pieces, z 0..5 and 5..10, after the cut.
    let piece = s.project().regen_edges[&r.body]
        .iter()
        .find(|e| e.dir[2].abs() > 0.99 && (e.mid[0] - 20.0).abs() < 1e-6 && (e.mid[1] - 15.0).abs() < 1e-6 && e.mid[2] > 5.0)
        .expect("the upper piece of the corner edge")
        .id;
    s.fillet(Some(r.body), &Sel::Ids(vec![piece]), &2.0.into(), None).unwrap();
    let removed = (1.0 - PI / 4.0) * 4.0;
    assert_close(only_body(&s).volume, 12000.0 - removed * 10.0, 1e-3, "the whole 10 mm corner is rounded, not the 5 mm piece");
}

#[test]
fn delete_rollback_and_save_open_keep_a_clean_split() {
    let (mut s, b) = block();
    let r = s.split_face(&xy(5.0)).unwrap();
    // Rollback to before the node and back to the end.
    s.history_rollback(Some(b), None).unwrap();
    assert_eq!(faces(&s, b), 6);
    s.history_rollback(None, Some("end")).unwrap();
    assert_eq!(faces(&s, r.body), 10);
    // Save and open: the same ids and faces, no warnings, and the offset formula survives.
    let path = scratch("split_face.qcad");
    s.save(Some(&path)).unwrap();
    let (mut loaded, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty() && report.warnings.is_empty(), "{:?} {:?}", report.errors, report.warnings);
    assert_eq!(ids(&loaded, r.body), ids(&s, r.body));
    assert_close(only_body(&loaded).volume, 12000.0, 1e-6, "volume");
    // An edit of the opened file is judged like one of the live document.
    let e = loaded
        .dimension_set(&DimensionTarget::Feature { feature: r.feature, key: "offset".into() }, Some(&(10.0 - 1e-6).into()), None)
        .unwrap_err()
        .to_string();
    assert!(e.contains("divided no face"), "{e}");
    loaded.dimension_set(&DimensionTarget::Feature { feature: r.feature, key: "offset".into() }, Some(&2.0.into()), None).unwrap();
    assert_eq!(levels(&loaded, r.body), vec![0.0, 2.0, 10.0]);
    // Delete: the source comes back.
    s.feature_delete(r.feature, false).unwrap();
    assert_eq!(faces(&s, b), 6);
    assert_close(only_body(&s).volume, 12000.0, 1e-6, "volume");
}

#[test]
fn a_part_copy_carries_the_split_and_follows_global_parameters() {
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    s.param_set("cut", &5.0.into()).unwrap();
    s.primitive(&spec_box(expr("w"), 30.0, 10.0), &Placement::Origin, None).unwrap();
    s.split_face(&xy(expr("cut"))).unwrap();
    let part = s.project().components.iter().find(|c| c.kind == qymcad_core::feature::ComponentKind::Part).unwrap().id;
    s.part_copy(&PartRef::Id(part), "copy", None, None).unwrap();
    let bodies = s.result_bodies();
    assert_eq!(bodies.len(), 2);
    for b in &bodies {
        assert_eq!(faces(&s, b.id), 10, "the copy has the split too");
        assert_eq!(levels(&s, b.id), vec![0.0, 5.0, 10.0]);
    }
    s.param_set("cut", &7.0.into()).unwrap();
    s.param_set("w", &60.0.into()).unwrap();
    for b in s.result_bodies() {
        assert_eq!(levels(&s, b.id), vec![0.0, 7.0, 10.0], "both follow the parameter");
        assert_close(b.volume, 60.0 * 30.0 * 10.0, 1e-6, "volume");
    }
    // Ids of the copy are its own: a refusal on the shared parameter names the first node that fails.
    let before = snap(&s);
    assert!(s.param_set("cut", &(10.0 - 1e-6).into()).unwrap_err().to_string().contains("divided no face"));
    assert!(snap(&s) == before);
}

#[test]
fn the_general_preconditions_refuse_with_the_document_unchanged() {
    // No body.
    let mut s = Session::new_part();
    let before = snap(&s);
    assert!(s.split_face(&xy(1.0)).is_err());
    assert!(snap(&s) == before);
    // A consumed body: the block before a later feature.
    let (mut s, b) = block();
    s.split_face(&xy(5.0)).unwrap();
    let e = refused(&mut s, &SplitFaceOptions { body: Some(b), ..xy(2.0) });
    assert!(e.contains("consumed"), "{e}");
    // A suppressed base feature: nothing is built to cut.
    let (mut s, b) = block();
    let box_node = s.project().timeline[0].id;
    s.feature_suppress(box_node, true).unwrap();
    let e = refused(&mut s, &SplitFaceOptions { body: Some(b), ..xy(2.0) });
    assert!(e.contains("suppressed"), "{e}");
    let e = refused(&mut s, &xy(2.0));
    assert!(e.contains("no body"), "{e}");
    // A clone made by part_instance may not be modelled in.
    let (mut s, _) = block();
    let part = s.project().components.iter().find(|c| c.kind == qymcad_core::feature::ComponentKind::Part).unwrap().id;
    s.part_instance(&PartRef::Id(part), "clone", None, None).unwrap();
    let clone = s.result_bodies().into_iter().find(|b| b.owner_part != Some(part)).unwrap().id;
    let e = refused(&mut s, &SplitFaceOptions { body: Some(clone), ..xy(2.0) });
    assert!(e.contains("clone") || e.contains("instance"), "{e}");
    // The assembly root as the active context, with no body named: no Part to take it from. A named body still works.
    let (mut s, b) = block();
    s.part_select(&PartRef::Root { root: true }).unwrap();
    let e = refused(&mut s, &xy(2.0));
    assert!(e.contains("not a Part"), "{e}");
    assert_eq!(s.split_face(&SplitFaceOptions { body: Some(b), ..xy(2.0) }).unwrap().faces.after, 10);
}
