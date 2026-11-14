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
    // Above the bottom face 1e-5 mm splits normally too, with the volume intact.
    let (mut b, _) = block();
    let r = b.split_face(&xy(1e-5)).unwrap();
    assert_eq!((r.faces.before, r.faces.after), (6, 10));
    assert_close(only_body(&b).volume, 12000.0, 1e-6, "volume");
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
    assert!(e.contains("F-190") && e.contains("is gone"), "{e}");
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

// ---- review round 1 ----

fn boxed_u() -> (Session, SplitFaceResult) {
    let mut s = Session::new_part();
    u_stock(&mut s);
    s.param_set("y", &10.0.into()).unwrap();
    let r = s.split_face(&SplitFaceOptions { plane: PlaneRef::Base(BaseName::XZ), ..xy(expr("y")) }).unwrap();
    (s, r)
}

fn edge_id(s: &Session, body: Id, at: [f64; 3], tol: f64) -> u32 {
    s.project().regen_edges[&body].iter().find(|e| (0..3).all(|i| (e.mid[i] - at[i]).abs() < tol)).expect("the edge").id
}

#[test]
fn a_change_of_a_face_the_split_did_not_cut_is_not_refused() {
    // A fillet below the split changes the body's faces on edges the plane does not cross: suppressing it is not the split's business.
    let (mut s, b) = block();
    let top_edge = edge_id(&s, b, [0.0, 15.0, 10.0], 1e-6);
    let (fillet, _) = s.fillet(Some(b), &Sel::Ids(vec![top_edge]), &1.0.into(), None).unwrap();
    let r = s.split_face(&xy(5.0)).unwrap();
    assert_eq!(r.faces.after, r.faces.before + 4);
    s.feature_suppress(fillet, true).unwrap();
    s.feature_suppress(fillet, false).unwrap();
    s.dimension_set(&DimensionTarget::Feature { feature: fillet, key: "radius".into() }, Some(&2.0.into()), None).unwrap();
}

/// A block, a boss on its top face and a split whose plane is that top face moved up by 3.
fn block_and_boss() -> (Session, SplitFaceResult) {
    let mut s = Session::new_part();
    s.param_set("h", &10.0.into()).unwrap();
    let base = s.primitive(&spec_box(40.0, 30.0, expr("h")), &Placement::Origin, None).unwrap().0.body;
    let top = face_with(&s, base, [0.0, 0.0, 1.0], 10.0);
    let sk = s.sketch_create(&PlaneRef::Face { body: base, face: top }, None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let (boss, _) = s
        .extrude(&Extrude {
            down: None,
            sketch: sk,
            profiles: None,
            profiles_at: None,
            height: 10.0.into(),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("boss".into()),
        })
        .unwrap();
    let ring = s.project().regen_faces[&boss].iter().find(|f| f.normal[2] > 0.99 && (f.centroid.z - 10.0).abs() < 1e-6).unwrap().id;
    let r = s.split_face(&SplitFaceOptions { body: Some(boss), plane: PlaneRef::Face { body: boss, face: ring }, ..xy(3.0) }).unwrap();
    (s, r)
}

#[test]
fn a_cut_that_moves_with_its_plane_is_the_same_cut() {
    let (mut s, r) = block_and_boss();
    assert_eq!((r.faces.before, r.faces.after), (11, 15), "the four boss walls are cut");
    // The plane follows the top of the block: 13 -> 17. The lower boss pieces (3 mm) move 4 mm, past their own extent.
    s.param_set("h", &14.0.into()).unwrap();
    assert_eq!(faces(&s, r.body), 15);
    assert!(levels(&s, r.body).contains(&17.0), "{:?}", levels(&s, r.body));
}

#[test]
fn a_suppressed_split_does_not_forget_what_it_cut() {
    let (mut s, r) = boxed_u();
    s.feature_suppress(r.feature, true).unwrap();
    s.param_set("y", &2.0.into()).unwrap();
    let before = snap(&s);
    let e = s.feature_suppress(r.feature, false).unwrap_err().to_string();
    assert!(e.contains("changes the faces the split cuts"), "{e}");
    assert!(snap(&s) == before, "still suppressed, the parameter at 2");
    // Back inside the same cut, the node is built again.
    s.param_set("y", &12.0.into()).unwrap();
    s.feature_suppress(r.feature, false).unwrap();
    assert_eq!(faces(&s, r.body), 18);
}

#[test]
fn a_split_past_the_rollback_bar_does_not_forget_what_it_cut() {
    let (mut s, r) = boxed_u();
    let before_split = s.project().timeline.iter().rev().find(|n| n.id != r.feature).unwrap().id;
    s.history_rollback(Some(before_split), None).unwrap();
    s.param_set("y", &2.0.into()).unwrap();
    let before = snap(&s);
    let e = s.history_rollback(None, Some("end")).unwrap_err().to_string();
    assert!(e.contains("changes the faces the split cuts"), "{e}");
    assert!(snap(&s) == before);
    s.param_set("y", &12.0.into()).unwrap();
    s.history_rollback(None, Some("end")).unwrap();
    assert_eq!(faces(&s, r.body), 18);
}

/// The file of `s` with the faces left out of its bodies, as the app can save one.
fn file_without_faces(s: &mut Session, name: &str) -> std::path::PathBuf {
    let path = scratch(name);
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    for b in &mut project.bodies {
        b.faces.clear();
    }
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    path
}

#[test]
fn the_first_edit_after_opening_is_judged_against_the_pieces_of_the_file() {
    for with_faces in [true, false] {
        let (mut s, r) = boxed_u();
        let path = if with_faces {
            let path = scratch("split_face_open.qcad");
            s.save(Some(&path)).unwrap();
            path
        } else {
            file_without_faces(&mut s, "split_face_open_no_faces.qcad")
        };
        let (mut opened, report) = Session::open(&path).unwrap();
        assert!(report.errors.is_empty() && report.warnings.is_empty(), "{with_faces}: {:?} {:?}", report.errors, report.warnings);
        // No topology call in between: the edit is the first thing done to the document.
        let before = snap(&opened);
        let e = opened.param_set("y", &2.0.into()).unwrap_err().to_string();
        assert!(e.contains("changes the faces the split cuts"), "{with_faces}: {e}");
        assert!(snap(&opened) == before);
        opened.param_set("y", &12.0.into()).unwrap();
        assert_eq!(faces(&opened, r.body), 18);
    }
}

/// A document with two splits that cross, built through the core as the app would, saved as it is.
fn app_file_with_repeated_ids(name: &str, faces_stored: bool) -> (std::path::PathBuf, Id) {
    let mut p = qymcad_core::model::Project::default();
    p.new_document();
    let a = p.add_box(40.0, 30.0, 10.0);
    let first = p.add_split_face(a, 0, 0, 3.0);
    p.set_feat_dim(first, "offset", "3".into());
    let second = p.add_split_face(first, 1, 0, 0.0);
    p.set_feat_dim(second, "offset", "0".into());
    p.eval_parameters();
    let (_, shapes) = qymcad_testkit::regenerate(&mut p);
    if faces_stored {
        let all: Vec<(Id, Vec<qymcad_core::geom::MeshFace>)> = p.regen_faces.iter().map(|(b, f)| (*b, f.clone())).collect();
        for (b, f) in all {
            p.set_body_faces(b, f);
        }
    }
    let breps: Vec<(Id, Vec<u8>)> = {
        let _g = qymcad_kernel::kernel_gate();
        shapes.iter().filter_map(|(id, sh)| sh.to_brep_bytes().map(|b| (*id, b))).collect()
    };
    let path = scratch(name);
    qymcad_io::save_project_guarded_with_brep(&p, path.to_str().unwrap(), &breps).unwrap();
    (path, second)
}

#[test]
fn a_file_that_is_already_wrong_opens_with_a_warning_that_names_the_way_out() {
    for (name, stored) in [("split_face_wrong_faces.qcad", true), ("split_face_wrong_no_faces.qcad", false)] {
        let (path, node) = app_file_with_repeated_ids(name, stored);
        let (mut s, report) = Session::open(&path).unwrap();
        let warning = report
            .warnings
            .iter()
            .find(|w| w.message.contains("feature_suppress or feature_delete"))
            .unwrap_or_else(|| panic!("{stored}: {:?} {:?}", report.warnings, report.errors));
        assert!(warning.message.contains("feature_suppress or feature_delete"), "{stored}: {}", warning.message);
        let info = s.info();
        assert!(
            info.warnings.iter().any(|w| w.contains("feature_suppress or feature_delete")),
            "{stored}: doc_info shows it too: {:?}",
            info.warnings
        );
        // Tools that need the topology of the file say how to go on, and the way out works.
        assert!(
            !warning.message.contains("rolled back") && !warning.message.contains(".."),
            "{stored}: one clean sentence: {}",
            warning.message
        );
        if !stored {
            let e = s.topology(None, false).unwrap_err().to_string();
            assert!(e.contains("feature_suppress or feature_delete"), "{e}");
        }
        s.feature_delete(node, false).unwrap();
        let t = s.topology(None, false);
        assert!(t.is_ok(), "{stored}: {:?}", t.err().map(|e| e.to_string()));
    }
}

#[test]
fn a_third_parallel_cut_is_refused_and_the_text_says_what_to_do() {
    let (mut s, _) = block();
    s.split_face(&xy(3.0)).unwrap();
    s.split_face(&xy(6.0)).unwrap();
    let e = refused(&mut s, &xy(8.0));
    assert!(e.contains("distinct ids") && e.contains("`split`"), "{e}");
    assert!(!e.contains("order that keeps") && e.contains("between 1 and 4"), "no order is promised, the limit is the measured range: {e}");
    assert!(!e.contains("upstream #"), "{e}");
}

#[test]
fn an_edit_that_puts_the_plane_off_the_body_says_what_happened_and_what_to_do() {
    let (mut s, r) = block();
    s.param_set("cut", &5.0.into()).unwrap();
    s.split_face(&xy(expr("cut"))).unwrap();
    s.param_set("h", &9.0.into()).ok();
    let before = snap(&s);
    let e = s.param_set("cut", &50.0.into()).unwrap_err().to_string();
    assert!(e.contains("would no longer meet the body") && e.contains("not applied") && !e.contains("Undo this edit"), "{e}");
    assert!(snap(&s) == before);
    let _ = r;
}

#[test]
fn a_face_plane_offset_runs_along_the_outward_normal_and_a_refusal_says_so() {
    let (mut s, b) = block();
    let bottom = face_with(&s, b, [0.0, 0.0, -1.0], 0.0);
    let e = refused(&mut s, &SplitFaceOptions { plane: PlaneRef::Face { body: b, face: bottom }, ..xy(3.0) });
    assert!(e.contains("OUTWARD normal"), "{e}");
    let r = s.split_face(&SplitFaceOptions { plane: PlaneRef::Face { body: b, face: bottom }, ..xy(-3.0) }).unwrap();
    assert_eq!(levels(&s, r.body), vec![0.0, 3.0, 10.0]);
}

#[test]
fn a_clone_of_a_placed_part_is_refused_as_a_clone_before_the_plane_is_looked_at() {
    let (mut s, _) = block();
    let part = s.project().components.iter().find(|c| c.kind == qymcad_core::feature::ComponentKind::Part).unwrap().id;
    s.part_place(&PartRef::Id(part), [0.0, 0.0, 100.0], None).unwrap();
    s.part_instance(&PartRef::Id(part), "clone", Some([100.0, 0.0, 0.0]), None).unwrap();
    let clone = s.result_bodies().into_iter().find(|b| b.owner_part != Some(part)).unwrap().id;
    let e = refused(&mut s, &SplitFaceOptions { body: Some(clone), ..xy(5.0) });
    assert!(e.contains("is an instance of"), "{e}");
}

#[test]
fn the_text_of_a_refused_creation_does_not_name_the_node_that_was_rolled_back() {
    let (mut s, _) = block();
    s.split_face(&xy(6.0)).unwrap();
    let e = refused(&mut s, &xy(3.0));
    assert!(e.contains("Split faces: the split leaves"), "{e}");
    assert!(e.contains("(F-189)") && !e.contains("upstream #"), "numbers of findings stand in brackets at the end: {e}");
}

#[test]
fn suppressing_a_split_warns_that_its_consumers_will_take_the_whole_face() {
    let (mut s, _) = block();
    let r = s.split_face(&xy(5.0)).unwrap();
    let r = s.feature_suppress(r.feature, true).unwrap();
    assert!(r.rebuild.notes.iter().any(|n| n.contains("WHOLE face")), "{:?}", r.rebuild.notes);
}

fn write_cube_stl(name: &str) -> std::path::PathBuf {
    let path = scratch(name);
    let v = [[0., 0., 0.], [10., 0., 0.], [10., 10., 0.], [0., 10., 0.], [0., 0., 10.], [10., 0., 10.], [10., 10., 10.], [0., 10., 10.]];
    let tris = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    let mut bytes = vec![0u8; 84 + tris.len() * 50];
    bytes[80..84].copy_from_slice(&(tris.len() as u32).to_le_bytes());
    for (k, t) in tris.into_iter().enumerate() {
        for (j, p) in t.into_iter().map(|i| v[i]).enumerate() {
            for (axis, value) in p.into_iter().enumerate() {
                let o = 84 + k * 50 + 12 + j * 12 + axis * 4;
                bytes[o..o + 4].copy_from_slice(&(value as f32).to_le_bytes());
            }
        }
    }
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn a_mesh_body_is_refused_with_the_document_unchanged() {
    let mut s = Session::new_part();
    let imported = s.import_mesh(&write_cube_stl("split_face_cube.stl"), None, Some("mm"), 1., Some("cube")).unwrap();
    let body = imported.pieces[0].body;
    let owner = s.project().body_owner(body).unwrap();
    s.part_select(&PartRef::Id(owner)).unwrap();
    let e = refused(&mut s, &SplitFaceOptions { body: Some(body), ..xy(5.0) });
    assert!(e.contains("imported mesh"), "{e}");
}

#[test]
fn a_change_upstream_that_moves_the_cut_faces_is_refused() {
    // A block 12 tall and a 20 x 20 post from z = 10 to 20, united; the plane z = 14 cuts the post's four walls only. Growing the block to
    // 15 makes the plane cut the block's walls instead: other faces carry the old ids.
    let mut s = Session::new_part();
    s.param_set("ah", &12.0.into()).unwrap();
    s.primitive(&spec_box(40.0, 30.0, expr("ah")), &Placement::Origin, None).unwrap();
    let (point, _) = s.datum_point(&PointSpec::At([0.0.into(), 0.0.into(), 10.0.into()]), None).unwrap();
    s.primitive(&spec_box(20.0, 20.0, 10.0), &Placement::Point(point.id), None).unwrap();
    let r = s.split_face(&xy(14.0)).unwrap();
    let before = snap(&s);
    let e = s.param_set("ah", &15.0.into()).unwrap_err().to_string();
    assert!(e.contains("changes the faces the split cuts"), "{e}");
    assert!(snap(&s) == before);
    s.param_set("ah", &13.0.into()).unwrap();
    assert_eq!(faces(&s, r.body), r.faces.after);
}

// ---- review round 2 ----

#[test]
fn a_block_with_rounded_corners_can_be_resized_without_moving_a_cut_id() {
    let mut s = Session::new_part();
    s.param_set("d", &30.0.into()).unwrap();
    s.param_set("w", &44.0.into()).unwrap();
    let b = s.primitive(&spec_box(expr("w"), expr("d"), 10.0), &Placement::Origin, None).unwrap().0.body;
    let corners: Vec<u32> = s.project().regen_edges[&b].iter().filter(|e| e.dir[2].abs() > 0.99).map(|e| e.id).collect();
    assert_eq!(corners.len(), 4);
    s.fillet(Some(b), &Sel::Ids(corners), &3.0.into(), None).unwrap();
    let r = s.split_face(&xy(5.0)).unwrap();
    let before = ids(&s, r.body);
    for (name, value) in [("d", 24.0), ("d", 30.0), ("d", 60.0), ("d", 20.0), ("w", 50.0), ("w", 44.0)] {
        s.param_set(name, &value.into()).unwrap_or_else(|e| panic!("{name} = {value}: {e}"));
        assert_eq!(ids(&s, r.body), before, "the same ids after {name} = {value}");
    }
}

#[test]
fn a_body_moved_by_a_parameter_further_than_its_size_keeps_its_cut_ids() {
    let mut s = Session::new_part();
    s.param_set("cx", &0.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &expr("cx"), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
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
        name: None,
    })
    .unwrap();
    let r = s.split_face(&xy(5.0)).unwrap();
    let before = ids(&s, r.body);
    s.param_set("cx", &100.0.into()).unwrap();
    assert_eq!(ids(&s, r.body), before);
}

/// A U split, saved while suppressed or past the rollback bar, and opened.
fn opened_with_the_split_not_built(bar: bool) -> (Session, SplitFaceResult) {
    let (mut s, r) = boxed_u();
    if bar {
        let at = s.project().timeline.iter().position(|n| n.id == r.feature).unwrap();
        let before = s.project().timeline[at - 1].id;
        s.history_rollback(Some(before), None).unwrap();
    } else {
        s.feature_suppress(r.feature, true).unwrap();
    }
    let path = scratch(if bar { "split_face_bar.qcad" } else { "split_face_suppressed.qcad" });
    s.save(Some(&path)).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    (opened, r)
}

fn bring_back(s: &mut Session, r: &SplitFaceResult, bar: bool) -> std::result::Result<(), String> {
    if bar {
        s.history_rollback(None, Some("end")).map(|_| ()).map_err(|e| e.to_string())
    } else {
        s.feature_suppress(r.feature, false).map(|_| ()).map_err(|e| e.to_string())
    }
}

#[test]
fn a_split_saved_not_built_comes_back_without_an_edit_and_is_judged_by_what_it_cut_after_one() {
    for bar in [false, true] {
        let (mut s, r) = opened_with_the_split_not_built(bar);
        bring_back(&mut s, &r, bar).unwrap_or_else(|e| panic!("bar {bar}: no edit, nothing to refuse: {e}"));
        assert_eq!(faces(&s, r.body), 18, "bar {bar}");
        let (mut s, r) = opened_with_the_split_not_built(bar);
        s.param_set("y", &2.0.into()).unwrap();
        let before = snap(&s);
        let e = bring_back(&mut s, &r, bar).unwrap_err();
        assert!(e.contains("changes the faces the split cuts"), "bar {bar}: {e}");
        assert!(!e.contains("feature_suppress"), "no advice to suppress what is being unsuppressed: {e}");
        assert!(snap(&s) == before, "bar {bar}");
        s.param_set("y", &12.0.into()).unwrap();
        bring_back(&mut s, &r, bar).unwrap();
    }
}

#[test]
fn the_copy_of_a_suppressed_split_is_measured_not_stuck() {
    let (mut s, r) = boxed_u();
    s.feature_suppress(r.feature, true).unwrap();
    let part = s.project().components.iter().find(|c| c.kind == qymcad_core::feature::ComponentKind::Part).unwrap().id;
    s.part_copy(&PartRef::Id(part), "copy", None, None).unwrap();
    let copy = s
        .project()
        .timeline
        .iter()
        .filter(|n| matches!(n.kind, qymcad_core::feature::FeatureKind::SplitFace { .. }) && n.id != r.feature)
        .map(|n| n.id)
        .next()
        .expect("the copied split");
    s.feature_suppress(copy, false).unwrap();
    assert_eq!(faces(&s, copy), 18);
}
