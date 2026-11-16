//! T1: Copy faces (P20) and Offset surface (P27). Mirrors the app's testkit scenarios with the same numbers
//! (`face_copy.rs`, `an_offset_surface_moves_its_faces.rs`): the copy of a 60x40 top is 2400 and follows the base,
//! the top lifted 5 is a sheet of 1200 at z 15, a cylinder side +2 is radius 12, -12 on radius 10 is refused.
use crate::common;
use qymcad_core::feature::FeatureKind;
use qymcad_engine::*;
use std::f64::consts::PI;

fn v(x: f64) -> Num {
    Num::Value(x)
}

fn extrude(sketch: Id, height: &str) -> Extrude {
    serde_json::from_value(serde_json::json!({"sketch": sketch, "height": height, "op": "add"})).unwrap()
}

/// A `w` x `d` block centred on the origin, `h` tall from the XY plane up (an expression).
fn block(w: f64, d: f64, h: &str) -> (Session, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(0.), &v(0.), &v(w), &v(d), false).unwrap();
    let (body, r) = s.extrude(&extrude(sk, h)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    (s, body)
}

/// A block 10 tall whose height is the parameter `h`.
fn block_h(w: f64, d: f64) -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("h", &v(10.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(0.), &v(0.), &v(w), &v(d), false).unwrap();
    let (body, r) = s.extrude(&extrude(sk, "h")).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    (s, body)
}

fn up() -> Sel {
    Sel::Facing { dir: [0., 0., 1.], tol_deg: 1.0 }
}

fn front() -> Sel {
    Sel::Facing { dir: [0., -1., 0.], tol_deg: 1.0 }
}

fn info(s: &Session, body: Id) -> BodyInfo {
    s.result_bodies().into_iter().find(|b| b.id == body).unwrap_or_else(|| panic!("body {body} is not current"))
}

fn error(r: Result<(Id, Vec<u32>, Rebuild)>) -> String {
    r.expect_err("must be refused").to_string()
}

/// Everything a refused call must leave alone.
fn recipe(s: &Session) -> String {
    ron::ser::to_string(s.project()).unwrap()
}

// ---- face_copy (P20) ----

/// The top of a 60x40 plate copied: a sheet of 2400 mm2 beside the plate; the plate is untouched, not a sheet, not consumed.
#[test]
fn face_copy_of_the_top_is_a_sheet_of_2400_and_the_source_stays() {
    let (mut s, plate) = block(60., 40., "12");
    let volume = info(&s, plate).volume;
    let (feature, ids, r) = s.face_copy(None, &up(), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(ids.len(), 1);
    assert_eq!(feature, r.bodies.iter().find(|b| b.sheet).unwrap().id, "the node id is the sheet's id");
    let sheet = info(&s, feature);
    assert!(sheet.sheet);
    common::assert_close(sheet.area, 2400., 1e-3, "sheet area");
    assert_eq!(sheet.volume, 0.);
    common::assert_close(sheet.bbox[2], 12., 1e-3, "sheet lies on the top");
    common::assert_close(sheet.bbox[5], 12., 1e-3, "sheet lies on the top");
    let src = info(&s, plate);
    assert!(!src.sheet, "the source stays a solid");
    common::assert_close(src.volume, volume, 1e-6, "source volume");
    assert!(!s.project().consumed_bodies().contains(&plate), "a copy does not consume its source");
    // Stored as picks of the resolved ids, the way the app's edit mode stores them (ADR 0052 addendum).
    let FeatureKind::FaceCopy { src, faces, .. } = &s.project().timeline.iter().find(|n| n.id == feature).unwrap().kind else {
        panic!("not a FaceCopy node")
    };
    assert_eq!(*src, plate);
    assert_eq!(*faces, qymcad_core::refs::Ref::picks(&ids));
}

/// The copy follows its base: the plate 60 -> 80 long makes the copy 3200.
#[test]
fn face_copy_follows_an_edit_of_its_base() {
    let mut s = Session::new_part();
    s.param_set("w", &v(60.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(0.), &v(0.), &Num::from("w"), &v(40.), false).unwrap();
    s.extrude(&extrude(sk, "12")).unwrap();
    let (sheet, _, r) = s.face_copy(None, &up(), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&s, sheet).area, 2400., 1e-3, "before");
    let r = s.param_set("w", &v(80.)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&s, sheet).area, 3200., 1e-3, "after the base grew");
}

/// Two faces make ONE sheet body (the top 2400 and the front 60x12 = 720).
#[test]
fn face_copy_of_two_faces_is_one_sheet() {
    let (mut s, _) = block(60., 40., "12");
    let (sheet, ids, r) = s.face_copy(None, &Sel::Union(vec![up(), front()]), Some("copy")).unwrap();
    assert_eq!(ids.len(), 2);
    assert_eq!(r.bodies.iter().filter(|b| b.sheet).count(), 1, "one sheet body: {:?}", r.bodies);
    common::assert_close(info(&s, sheet).area, 2400. + 720., 1e-3, "two faces");
    assert_eq!(s.node_name(sheet), "copy");
}

/// A face that is not there, an empty selection and a body that is not a B-rep are refused with the document unchanged.
#[test]
fn face_copy_refusals_leave_the_document_unchanged() {
    let (mut s, plate) = block(60., 40., "12");
    let before = recipe(&s);
    assert!(error(s.face_copy(None, &Sel::Ids(vec![9999]), None)).contains("9999"));
    assert!(error(s.face_copy(None, &Sel::Facing { dir: [1., 1., 1.], tol_deg: 1.0 }, None)).contains("no face"));
    assert!(error(s.face_copy(Some(plate + 100), &up(), None)).contains("not built"));
    assert_eq!(recipe(&s), before);
}

/// A copy of a face the base later loses fails honestly: the base edit is refused and rolled back.
#[test]
fn face_copy_of_a_lost_face_is_refused() {
    let (mut s, plate) = block(60., 40., "12");
    // A hole's wall: a face that exists only while the hole does.
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &v(0.), &v(0.), &v(10.), false).unwrap();
    let cut: Extrude = serde_json::from_value(serde_json::json!({"sketch": sk, "height": 12, "op": "cut", "through": true})).unwrap();
    let (holed, r) = s.extrude(&cut).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_ne!(holed, plate);
    // The circle's number is its diameter: r 5, a wall of 2 pi 5 12.
    let wall: Vec<u32> =
        s.topology(Some(holed), false).unwrap().faces.into_iter().filter(|f| f.kind == FaceKind::Cylinder).map(|f| f.id).collect();
    let (sheet, _, r) = s.face_copy(Some(holed), &Sel::Ids(wall), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&s, sheet).area, PI * 10. * 12., 0.5, "the wall of the hole");
    let before = recipe(&s);
    // Suppressing the cut takes the wall away: the copy cannot find it.
    let e = s.feature_suppress(holed, true).expect_err("a copy of a lost face must not rebuild").to_string();
    assert!(e.contains("Face copy") && e.contains("no longer in the source body"), "{e}");
    assert_eq!(recipe(&s), before, "refused and rolled back");
}

/// A copy of faces of a SHEET is the app's FaceCopy of a sheet face (same area, a new sheet).
#[test]
fn face_copy_of_a_sheet_is_a_sheet() {
    let (mut s, _) = block(60., 40., "12");
    let (first, _, _) = s.face_copy(None, &up(), None).unwrap();
    let face = s.topology(Some(first), false).unwrap().faces[0].id;
    let (second, ids, r) = s.face_copy(Some(first), &Sel::Ids(vec![face]), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_ne!(first, second);
    assert_eq!(ids.len(), 1);
    assert!(info(&s, second).sheet && info(&s, first).sheet);
    common::assert_close(info(&s, second).area, 2400., 1e-3, "copy of a sheet face");
}

/// Re-pick: the faces of the node change, its id stays, and the sheet rebuilds.
#[test]
fn face_copy_can_be_repicked() {
    let (mut s, _) = block(60., 40., "12");
    let (feature, _, _) = s.face_copy(None, &up(), None).unwrap();
    let before = recipe(&s);
    assert!(error(s.repick_face_copy(feature, &Sel::Ids(vec![9999]))).contains("9999"));
    assert_eq!(recipe(&s), before, "a refused re-pick leaves the node alone");
    let (body, ids, r) = s.repick_face_copy(feature, &front()).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(body, feature);
    assert_eq!(ids.len(), 1);
    common::assert_close(info(&s, feature).area, 720., 1e-3, "the front now");
    assert!(error(s.repick_face_copy(feature + 1000, &up())).contains("feature"));
    let (off, _, _) = s.offset_surface(None, &up(), &v(1.), None).unwrap();
    assert!(error(s.repick_face_copy(off, &up())).contains("not a face_copy"), "another kind of node is refused");
}

/// The copy survives save and open, and a person editing a parameter in the app gets the new area (the app's own path).
#[test]
fn face_copy_survives_save_open_and_the_app_edit_path() {
    let mut s = Session::new_part();
    s.param_set("w", &v(60.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(0.), &v(0.), &Num::from("w"), &v(40.), false).unwrap();
    s.extrude(&extrude(sk, "12")).unwrap();
    let (sheet, _, _) = s.face_copy(None, &up(), None).unwrap();
    let path = common::scratch("t1-face-copy.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let b = info(&opened, sheet);
    assert!(b.sheet);
    common::assert_close(b.area, 2400., 1e-3, "after open");
    let (_, _, _, project) = common::gui_edit_param_model(&path, "w", "80");
    let area: f64 = project.regen_faces[&sheet].iter().map(|f| f.area).sum();
    common::assert_close(area, 3200., 1e-3, "the app's edit of w");
    assert!(project.bodies.iter().find(|b| b.id == sheet).unwrap().sheet);
}

// ---- offset_surface (P27) ----

fn z_range(s: &Session, body: Id) -> (f64, f64) {
    let b = info(s, body);
    (b.bbox[2], b.bbox[5])
}

/// The top of a 40x30x10 block lifted 5: a sheet of 1200 mm2 at z 15; the block stays.
#[test]
fn the_top_lifted_5_is_a_sheet_of_1200_at_z_15() {
    let (mut s, block) = block(40., 30., "10");
    let (sheet, _, r) = s.offset_surface(None, &up(), &v(5.), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let b = info(&s, sheet);
    assert!(b.sheet);
    common::assert_close(b.area, 1200., 0.5, "area");
    let (lo, hi) = z_range(&s, sheet);
    common::assert_close(lo, 15., 1e-3, "z");
    common::assert_close(hi, 15., 1e-3, "z");
    assert!(!info(&s, block).sheet && !s.project().consumed_bodies().contains(&block));
}

/// A cylinder side +2 is the side of radius 12 (area 2 pi 12 20); the same side -12 on radius 10 is the app's own refusal.
#[test]
fn a_cylinder_side_grows_and_one_turned_inside_out_is_refused() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &v(0.), &v(0.), &v(20.), false).unwrap();
    let (body, _) = s.extrude(&extrude(sk, "20")).unwrap();
    let side = s.topology(Some(body), false).unwrap().faces.into_iter().find(|f| f.kind == FaceKind::Cylinder).unwrap().id;
    let (sheet, _, r) = s.offset_surface(None, &Sel::Ids(vec![side]), &v(2.), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let want = 2. * PI * 12. * 20.;
    assert!((info(&s, sheet).area - want).abs() / want < 0.01, "area {} against {want}", info(&s, sheet).area);
    let before = recipe(&s);
    let e = error(s.offset_surface(None, &Sel::Ids(vec![side]), &v(-12.), None));
    assert!(e.contains("OffsetPastRadius") || e.to_lowercase().contains("radius"), "{e}");
    assert_eq!(recipe(&s), before, "refused and rolled back");
}

/// Zero is a copy in place.
#[test]
fn distance_zero_is_a_copy_in_place() {
    let (mut s, _) = block(40., 30., "10");
    let (sheet, _, r) = s.offset_surface(None, &up(), &v(0.), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let (lo, _) = z_range(&s, sheet);
    common::assert_close(lo, 10., 1e-3, "in place");
    common::assert_close(info(&s, sheet).area, 1200., 0.5, "area");
}

/// The sheet follows the block: dz 10 -> 20 puts a sheet 5 over the top at z 25; the formula distance follows its parameter.
#[test]
fn the_offset_follows_the_block_and_its_formula() {
    let (mut s, _) = block_h(40., 30.);
    s.param_set("lift", &v(5.)).unwrap();
    let (sheet, _, r) = s.offset_surface(None, &up(), &Num::from("lift"), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(s.project().feat_dims[&sheet]["dist"], "lift");
    common::assert_close(z_range(&s, sheet).0, 15., 1e-3, "z");
    s.param_set("h", &v(20.)).unwrap();
    common::assert_close(z_range(&s, sheet).0, 25., 1e-3, "the block grew");
    s.param_set("lift", &v(-3.)).unwrap();
    common::assert_close(z_range(&s, sheet).0, 17., 1e-3, "pulled in 3");
}

/// The top and the front move apart (sharp edge): two pieces, one body, 1200 + 400 = 1600.
#[test]
fn two_faces_are_one_body_of_1600() {
    let (mut s, _) = block(40., 30., "10");
    let (sheet, ids, r) = s.offset_surface(None, &Sel::Union(vec![up(), front()]), &v(5.), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(ids.len(), 2);
    assert_eq!(r.bodies.iter().filter(|b| b.sheet).count(), 1);
    common::assert_close(info(&s, sheet).area, 1600., 0.5, "area");
}

/// A sheet's own faces offset: the sheet of the top (normal +z) moved 3 goes up 3, one 1200 mm2 sheet beside it.
#[test]
fn faces_of_a_sheet_can_be_offset() {
    let (mut s, _) = block(40., 30., "10");
    let (copy, _, _) = s.face_copy(None, &up(), None).unwrap();
    let face = s.topology(Some(copy), false).unwrap().faces[0].id;
    let (moved, _, r) = s.offset_surface(Some(copy), &Sel::Ids(vec![face]), &v(3.), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(info(&s, moved).sheet && info(&s, copy).sheet);
    common::assert_close(info(&s, moved).area, 1200., 0.5, "area");
    common::assert_close(z_range(&s, moved).0, 13., 1e-3, "along the sheet's normal");
    // Against its normal.
    let (down, _, _) = s.offset_surface(Some(copy), &Sel::Ids(vec![face]), &v(-3.), None).unwrap();
    common::assert_close(z_range(&s, down).0, 7., 1e-3, "against the sheet's normal");
}

/// The app's range is -10000..10000 and a non-finite value is refused: at creation, in param and dimension edits.
#[test]
fn the_distance_range_holds_everywhere() {
    let (mut s, _) = block(40., 30., "10");
    let before = recipe(&s);
    for bad in [10001., -10001.] {
        let e = error(s.offset_surface(None, &up(), &v(bad), None));
        assert!(e.contains("-10000..10000"), "{bad}: {e}");
    }
    // A non-finite number never reaches the node: the expression layer refuses it.
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        error(s.offset_surface(None, &up(), &v(bad), None));
    }
    assert_eq!(recipe(&s), before);
    s.param_set("d", &v(5.)).unwrap();
    let (sheet, _, _) = s.offset_surface(None, &up(), &Num::from("d"), None).unwrap();
    let before = recipe(&s);
    let e = s.param_set("d", &v(20000.)).expect_err("past the range").to_string();
    assert!(e.contains("-10000..10000"), "{e}");
    assert_eq!(recipe(&s), before);
    let target = DimensionTarget::Feature { feature: sheet, key: "dist".into() };
    let e = s.dimension_set(&target, Some(&v(-10001.)), None).expect_err("past the range").to_string();
    assert!(e.contains("-10000..10000"), "{e}");
    assert_eq!(recipe(&s), before);
    // The limits themselves are the app's: allowed.
    s.dimension_set(&target, Some(&v(10000.)), None).unwrap();
}

/// `dist` is editable with `dimension_set` and listed once named.
#[test]
fn dist_is_a_feature_dimension() {
    let (mut s, _) = block(40., 30., "10");
    let (sheet, _, _) = s.offset_surface(None, &up(), &v(5.), None).unwrap();
    let target = DimensionTarget::Feature { feature: sheet, key: "dist".into() };
    let edit = s.dimension_set(&target, Some(&v(8.)), Some("lift")).unwrap();
    assert!(edit.rebuild.errors.is_empty(), "{:?}", edit.rebuild.errors);
    common::assert_close(z_range(&s, sheet).0, 18., 1e-3, "edited to 8");
    let listed = s.info().named_dimensions;
    let row = listed.iter().find(|d| d.name == "lift").expect("named dimension listed");
    assert_eq!(row.value, Some(8.));
}

/// Re-pick keeps the distance (and its formula).
#[test]
fn offset_surface_can_be_repicked() {
    let (mut s, _) = block(40., 30., "10");
    s.param_set("d", &v(5.)).unwrap();
    let (sheet, _, _) = s.offset_surface(None, &up(), &Num::from("d"), None).unwrap();
    let before = recipe(&s);
    assert!(error(s.repick_offset_surface(sheet, &Sel::Ids(vec![9999]))).contains("9999"));
    assert_eq!(recipe(&s), before);
    let (body, ids, r) = s.repick_offset_surface(sheet, &front()).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!((body, ids.len()), (sheet, 1));
    assert_eq!(s.project().feat_dims[&sheet]["dist"], "d");
    common::assert_close(info(&s, sheet).area, 400., 0.5, "the front of 40 x 10");
    let (copy, _, _) = s.face_copy(None, &up(), None).unwrap();
    assert!(error(s.repick_offset_surface(copy, &up())).contains("not a offset_surface"));
}

/// Save and open: the sheet and its distance come back, and the app's own edit path moves it.
#[test]
fn offset_surface_survives_save_open_and_the_app_edit_path() {
    let (mut s, _) = block_h(40., 30.);
    let (sheet, _, _) = s.offset_surface(None, &up(), &v(5.), None).unwrap();
    let path = common::scratch("t1-offset.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(info(&opened, sheet).sheet);
    common::assert_close(z_range(&opened, sheet).0, 15., 1e-3, "after open");
    let r = opened.param_set("h", &v(20.)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(z_range(&opened, sheet).0, 25., 1e-3, "edit after open");
    let (_, _, _, project) = common::gui_edit_param_model(&path, "h", "20");
    let z: f64 = project.regen_faces[&sheet].iter().map(|f| f.centroid.z).sum::<f64>() / project.regen_faces[&sheet].len() as f64;
    common::assert_close(z, 25., 1e-3, "the app's edit of h");
}
