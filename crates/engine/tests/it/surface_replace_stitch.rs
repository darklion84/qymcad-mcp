//! T2: Replace face (P21) and Stitch (P23). Mirrors the app's testkit scenarios with the same numbers (`face_copy.rs` round trip,
//! `an_offset_surface_moves_its_faces.rs` 18000 / 8400, matrix "a sheet that does not close what it replaces", `stitch.rs`).
use crate::common;
use qymcad_engine::*;

fn v(x: f64) -> Num {
    Num::Value(x)
}

fn extrude(sketch: Id, height: &str) -> Extrude {
    serde_json::from_value(serde_json::json!({"sketch": sketch, "height": height, "op": "add"})).unwrap()
}

/// A `w` x `d` block centred on the origin, 12 tall unless `h` says otherwise; `w` is the parameter `w` when `param` is set.
fn block(w: f64, d: f64, h: &str) -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("w", &v(w)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(0.), &v(0.), &Num::from("w"), &v(d), false).unwrap();
    let (body, r) = s.extrude(&extrude(sk, h)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    (s, body)
}

fn up() -> Sel {
    Sel::Facing { dir: [0., 0., 1.], tol_deg: 1.0 }
}

fn down() -> Sel {
    Sel::Facing { dir: [0., 0., -1.], tol_deg: 1.0 }
}

fn front() -> Sel {
    Sel::Facing { dir: [0., -1., 0.], tol_deg: 1.0 }
}

fn info(s: &Session, body: Id) -> BodyInfo {
    s.result_bodies().into_iter().find(|b| b.id == body).unwrap_or_else(|| panic!("body {body} is not current"))
}

fn consumed(s: &Session, body: Id) -> bool {
    s.project().consumed_bodies().contains(&body)
}

fn recipe(s: &Session) -> String {
    ron::ser::to_string(s.project()).unwrap()
}

fn error<T>(r: Result<T>) -> String {
    match r {
        Ok(_) => panic!("must be refused"),
        Err(e) => e.to_string(),
    }
}

/// The sheet of the top of `body` lifted by `dist`.
fn lifted(s: &mut Session, dist: f64) -> Id {
    let (sheet, _, r) = s.offset_surface(None, &up(), &v(dist), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    sheet
}

// ---- surface_replace (P21) ----

/// The circle closes: the top copied and put back is the same part; both inputs are consumed.
#[test]
fn a_face_put_back_gives_the_same_part_and_consumes_both() {
    let (mut s, plate) = block(60., 40., "12");
    let (sheet, _, _) = s.face_copy(None, &up(), None).unwrap();
    let (out, ids, r) = s.surface_replace(Some(plate), &up(), sheet, Some("swap")).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(ids.len(), 1);
    let body = info(&s, out);
    assert!(!body.sheet, "the result is a solid");
    common::assert_close(body.volume, 28800., 1.0, "the plate is whole again");
    assert!(consumed(&s, plate) && consumed(&s, sheet));
    assert_eq!(s.node_name(out), "swap");
    // Stored as picks of the resolved ids, the way the app's edit mode stores them.
    let node = s.project().timeline.iter().find(|n| n.id == out).unwrap();
    let qymcad_core::feature::FeatureKind::SurfaceReplace { src, faces, surface, .. } = &node.kind else { panic!("not a SurfaceReplace") };
    assert_eq!((*src, *surface), (plate, sheet));
    assert_eq!(*faces, qymcad_core::refs::Ref::picks(&ids));
}

/// What stands after the replacement builds on: a fillet of the top edges takes material off.
#[test]
fn the_result_takes_a_fillet() {
    let (mut s, plate) = block(60., 40., "12");
    let (sheet, _, _) = s.face_copy(None, &up(), None).unwrap();
    let (out, _, _) = s.surface_replace(Some(plate), &up(), sheet, None).unwrap();
    let edges: Vec<u32> = s
        .topology(Some(out), false)
        .unwrap()
        .edges
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Line && (e.a[2] - 12.).abs() < 1e-6 && (e.b[2] - 12.).abs() < 1e-6)
        .map(|e| e.id)
        .collect();
    assert_eq!(edges.len(), 4, "the top rim");
    let (filleted, r) = s.fillet(Some(out), &Sel::Ids(edges), &v(1.), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(info(&s, filleted).volume < 28800. - 1., "the fillet removed material: {}", info(&s, filleted).volume);
}

/// The top replaced by the sheet lifted 5: the block grows to 15, 18000; sunk 3: cut down to 7, 8400.
#[test]
fn a_lifted_sheet_makes_the_block_taller_and_a_sunk_one_lower() {
    for (dist, volume) in [(5., 18000.), (-3., 8400.)] {
        let (mut s, block) = block(40., 30., "10");
        let sheet = lifted(&mut s, dist);
        let (out, _, r) = s.surface_replace(Some(block), &up(), sheet, None).unwrap();
        assert!(r.errors.is_empty(), "{dist}: {:?}", r.errors);
        let b = info(&s, out);
        assert!(!b.sheet);
        common::assert_close(b.volume, volume, 1.0, &format!("dist {dist}"));
        assert!(consumed(&s, block) && consumed(&s, sheet));
    }
}

/// The sheet follows its base: the plate 60 -> 90 long, replaced by its own copy, is 90 x 40 x 12.
#[test]
fn the_replacement_follows_an_edit_of_the_base() {
    let (mut s, plate) = block(60., 40., "12");
    let (sheet, _, _) = s.face_copy(None, &up(), None).unwrap();
    let (out, _, _) = s.surface_replace(Some(plate), &up(), sheet, None).unwrap();
    let r = s.param_set("w", &v(90.)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&s, out).volume, 90. * 40. * 12., 1.0, "after the base grew");
}

/// The top and the front taken away, a copy of the top alone put in: the front stays open. Refused BEFORE a node is laid,
/// with the app's `SurfaceDoesNotClose` words, the document untouched.
#[test]
fn a_sheet_that_does_not_close_what_it_replaces_is_refused_before_a_node_is_laid() {
    let (mut s, plate) = block(60., 40., "12");
    let (sheet, _, _) = s.face_copy(None, &up(), None).unwrap();
    let before = recipe(&s);
    let e = error(s.surface_replace(Some(plate), &Sel::Union(vec![up(), front()]), sheet, None));
    assert!(e.contains("does not close what it replaces") && e.contains("free edges"), "{e}");
    assert_eq!(recipe(&s), before, "no node was laid");
}

/// A face the solid does not have, and no face at all, are refused (the app's `msg-faces-not-found`).
#[test]
fn a_lost_target_face_is_refused() {
    let (mut s, plate) = block(60., 40., "12");
    let (sheet, _, _) = s.face_copy(None, &up(), None).unwrap();
    let before = recipe(&s);
    assert!(error(s.surface_replace(Some(plate), &Sel::Ids(vec![9999]), sheet, None)).contains("9999"));
    assert!(error(s.surface_replace(Some(plate), &Sel::Facing { dir: [1., 1., 1.], tol_deg: 1.0 }, sheet, None)).contains("no face"));
    assert_eq!(recipe(&s), before);
}

/// The app takes a solid for the faces and a sheet for the surface: the wrong kind is refused with a clear message.
#[test]
fn the_wrong_kinds_are_refused() {
    let (mut s, plate) = block(60., 40., "12");
    let (sheet, _, _) = s.face_copy(None, &up(), None).unwrap();
    let before = recipe(&s);
    // A sheet where the solid belongs.
    let top = s.topology(Some(sheet), false).unwrap().faces[0].id;
    let e = error(s.surface_replace(Some(sheet), &Sel::Ids(vec![top]), sheet, None));
    assert!(e.contains("is a sheet") && e.contains("`surface`"), "{e}");
    // A solid where the sheet belongs.
    let e = error(s.surface_replace(Some(plate), &up(), plate, None));
    assert!(e.contains("is a solid") && e.contains("must be a sheet"), "{e}");
    // Nothing there at all.
    assert!(error(s.surface_replace(Some(plate), &up(), 99999, None)).contains("99999"));
    assert_eq!(recipe(&s), before);
    // A sheet used up by an earlier replacement cannot be used twice.
    s.surface_replace(Some(plate), &up(), sheet, None).unwrap();
    let again = error(s.surface_replace(None, &up(), sheet, None));
    assert!(again.contains("consumed"), "{again}");
}

/// Re-pick: the sheet and the faces of an existing node change, its id stays, a refused re-pick leaves the document alone.
#[test]
fn surface_replace_can_be_repicked() {
    let (mut s, block) = block(40., 30., "10");
    let high = lifted(&mut s, 5.);
    let low = lifted(&mut s, 2.);
    let (out, _, _) = s.surface_replace(Some(block), &up(), high, None).unwrap();
    common::assert_close(info(&s, out).volume, 18000., 1.0, "5 over");
    assert!(!consumed(&s, low));
    // The other sheet: 40 x 30 x 12.
    let (body, ids, r) = s.repick_surface_replace(out, None, Some(low)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!((body, ids.len()), (out, 0));
    common::assert_close(info(&s, out).volume, 14400., 1.0, "2 over");
    assert!(consumed(&s, low) && !consumed(&s, high), "the old sheet is free again");
    // The faces again, by ids: the same top.
    let top =
        s.topology(Some(block), false).unwrap().faces.into_iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.9)).map(|f| f.id).unwrap();
    let (_, ids, r) = s.repick_surface_replace(out, Some(&Sel::Ids(vec![top])), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(ids, vec![top]);
    // Refusals leave the document alone: a face that is not there, a sheet that does not close the new faces, a solid, a sheet
    // built after the node, another kind of node.
    let before = recipe(&s);
    assert!(error(s.repick_surface_replace(out, Some(&Sel::Ids(vec![9999])), None)).contains("9999"));
    assert!(error(s.repick_surface_replace(out, Some(&front()), None)).contains("did not rebuild"));
    assert!(error(s.repick_surface_replace(out, None, Some(block))).contains("is a solid"));
    let later = lifted(&mut s, 1.);
    let before_later = recipe(&s);
    assert!(error(s.repick_surface_replace(out, None, Some(later))).contains("built after"));
    assert_eq!(recipe(&s), before_later);
    assert_ne!(before, before_later);
    assert!(error(s.repick_surface_replace(later, None, Some(high))).contains("not a surface_replace"));
}

/// Save and open: the replacement comes back, and the app's own edit path rebuilds it.
#[test]
fn surface_replace_survives_save_open_and_the_app_edit_path() {
    let (mut s, plate) = block(60., 40., "12");
    let (sheet, _, _) = s.face_copy(None, &up(), None).unwrap();
    let (out, _, _) = s.surface_replace(Some(plate), &up(), sheet, None).unwrap();
    let path = common::scratch("t2-replace.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty() && r.warnings.iter().all(|w| !w.message.contains("sealed")), "{:?}", r);
    assert!(!info(&opened, out).sheet);
    common::assert_close(info(&opened, out).volume, 28800., 1.0, "after open");
    let r = opened.param_set("w", &v(80.)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&opened, out).volume, 80. * 40. * 12., 1.0, "edit after open");
    let (volume, _, _, project) = common::gui_edit_param_model(&path, "w", "80");
    common::assert_close(volume, 80. * 40. * 12., 1.0, "the app's edit of w");
    assert!(!project.bodies.iter().find(|b| b.id == out).unwrap().sheet);
}

// ---- stitch (P23) ----

/// The sheets of the faces `sels` of the plate, one copy each.
fn copies(s: &mut Session, plate: Id, sels: &[Sel]) -> Vec<Id> {
    sels.iter()
        .map(|sel| {
            let (sheet, _, r) = s.face_copy(Some(plate), sel, None).unwrap();
            assert!(r.errors.is_empty(), "{:?}", r.errors);
            sheet
        })
        .collect()
}

/// Top and front copies become one sheet of two faces, the area the sum; both parts are consumed.
#[test]
fn two_touching_sheets_become_one_sheet() {
    let (mut s, plate) = block(60., 40., "12");
    let parts = copies(&mut s, plate, &[up(), front()]);
    let (one, r) = s.stitch(&parts, &v(1e-4), Some("skin")).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let b = info(&s, one);
    assert!(b.sheet, "two open faces do not close a shell");
    common::assert_close(b.area, 2400. + 720., 1.0, "area");
    assert_eq!(s.topology(Some(one), false).unwrap().faces.len(), 2);
    assert!(parts.iter().all(|p| consumed(&s, *p)), "every part is consumed");
    assert!(!consumed(&s, plate), "the plate it was copied from is not");
    assert_eq!(s.node_name(one), "skin");
    // The first part is the lineage.
    let node = s.project().timeline.iter().find(|n| n.id == one).unwrap();
    let qymcad_core::feature::FeatureKind::Stitch { parts: stored, .. } = &node.kind else { panic!("not a Stitch") };
    assert_eq!(*stored, parts);
}

/// Six face copies of a closed plate: the shell closes, and the result is a SOLID of 28800.
#[test]
fn a_closed_set_of_sheets_becomes_a_solid() {
    let (mut s, plate) = block(60., 40., "12");
    let ids: Vec<u32> = s.topology(Some(plate), false).unwrap().faces.iter().map(|f| f.id).collect();
    assert_eq!(ids.len(), 6);
    let sels: Vec<Sel> = ids.into_iter().map(|f| Sel::Ids(vec![f])).collect();
    let parts = copies(&mut s, plate, &sels);
    let (solid, r) = s.stitch(&parts, &Num::from(0.01), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let b = info(&s, solid);
    assert!(!b.sheet, "a closed shell is a solid");
    common::assert_close(b.volume, 28800., 1.0, "volume");
}

/// Top and bottom share no edge: the app's named refusal.
#[test]
fn sheets_that_do_not_touch_are_refused() {
    let (mut s, plate) = block(60., 40., "12");
    let parts = copies(&mut s, plate, &[up(), down()]);
    let before = recipe(&s);
    let e = error(s.stitch(&parts, &v(1e-4), None));
    assert!(e.contains("Stitch") && e.contains("share no edges") && e.contains("nothing to stitch"), "{e}");
    assert_eq!(recipe(&s), before);
}

/// F-198: a tolerance wider than the gap glues two parallel sheets into a "solid" that encloses nothing; refused. A cylinder stitched from its three faces is a real solid and passes.
#[test]
fn a_tolerance_wider_than_the_gap_is_refused_f198() {
    let (mut s, plate) = block(60., 40., "12");
    let (a, _, _) = s.face_copy(Some(plate), &up(), None).unwrap();
    let (b, _, _) = s.offset_surface(Some(plate), &up(), &v(5.), None).unwrap();
    let before = recipe(&s);
    let e = error(s.stitch(&[a, b], &v(5.1), None));
    assert!(e.contains("not one") && e.contains("smaller `tolerance`"), "{e}");
    assert_eq!(recipe(&s), before);
    // A tolerance below the gap is the app's own refusal.
    assert!(error(s.stitch(&[a, b], &v(4.9), None)).contains("nothing to stitch"));
    // A real solid is never refused: a cylinder from its side and two caps.
    let mut cyl = Session::new_part();
    let sk = cyl.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    cyl.sketch_circle(sk, &v(0.), &v(0.), &v(20.), false).unwrap();
    let (body, _) = cyl.extrude(&extrude(sk, "30")).unwrap();
    let ids: Vec<u32> = cyl.topology(Some(body), false).unwrap().faces.iter().map(|f| f.id).collect();
    let parts: Vec<Id> = ids.iter().map(|f| cyl.face_copy(Some(body), &Sel::Ids(vec![*f]), None).unwrap().0).collect();
    let (solid, r) = cyl.stitch(&parts, &v(0.01), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(!info(&cyl, solid).sheet);
    common::assert_close(info(&cyl, solid).volume, std::f64::consts::PI * 100. * 30., 1.0, "cylinder");
}

/// The stitched sheet follows its base: 60 -> 90 long is 90 x 40 + 90 x 12 = 4680. No edge name appears twice.
#[test]
fn the_stitched_sheet_follows_the_base_and_its_edge_names_are_unique() {
    let (mut s, plate) = block(60., 40., "12");
    let parts = copies(&mut s, plate, &[up(), front()]);
    let (one, _) = s.stitch(&parts, &v(1e-4), None).unwrap();
    let edges: Vec<u32> = s.topology(Some(one), false).unwrap().edges.iter().map(|e| e.id).collect();
    let mut unique = edges.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), edges.len(), "two edges under one name: {edges:?}");
    let r = s.param_set("w", &v(90.)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&s, one).area, 4680., 5.0, "after the base grew");
}

/// What the app refuses when sheets are picked: fewer than two, a solid, the same sheet twice; and what does not exist.
#[test]
fn stitch_refuses_what_the_app_refuses() {
    let (mut s, plate) = block(60., 40., "12");
    let parts = copies(&mut s, plate, &[up(), front()]);
    let before = recipe(&s);
    assert!(error(s.stitch(&parts[..1], &v(0.01), None)).contains("at least two"));
    assert!(error(s.stitch(&[], &v(0.01), None)).contains("at least two"));
    let e = error(s.stitch(&[parts[0], plate], &v(0.01), None));
    assert!(e.contains("is a solid") && e.contains("must be a sheet"), "{e}");
    assert!(error(s.stitch(&[parts[0], parts[0]], &v(0.01), None)).contains("twice"));
    assert!(error(s.stitch(&[parts[0], 99999], &v(0.01), None)).contains("99999"));
    assert_eq!(recipe(&s), before);
    // Parts used up by a stitch are no longer available.
    s.stitch(&parts, &v(0.01), None).unwrap();
    let third = copies(&mut s, plate, &[down()]);
    let e = error(s.stitch(&[parts[0], third[0]], &v(0.01), None));
    assert!(e.contains("consumed"), "{e}");
}

/// `tol` is a feature dimension of the app's range 1e-6..10: held at creation, in parameter and dimension edits; a formula
/// follows its parameter; named, it is listed.
#[test]
fn the_tolerance_is_a_dimension_with_the_apps_range() {
    let (mut s, plate) = block(60., 40., "12");
    let parts = copies(&mut s, plate, &[up(), front()]);
    let before = recipe(&s);
    for bad in [0., 1e-7, 10.5, -1.] {
        let e = error(s.stitch(&parts, &v(bad), None));
        assert!(e.contains("within 1e-6..10 mm"), "{bad}: {e}");
    }
    // A non-finite number never reaches the node: the expression layer refuses it.
    error(s.stitch(&parts, &v(f64::NAN), None));
    assert_eq!(recipe(&s), before);
    s.param_set("t", &v(0.5)).unwrap();
    let (one, r) = s.stitch(&parts, &Num::from("t"), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(s.project().feat_dims[&one]["tol"], "t");
    let before = recipe(&s);
    let e = error(s.param_set("t", &v(11.)));
    assert!(e.contains("stitch tolerance"), "{e}");
    assert_eq!(recipe(&s), before);
    let target = DimensionTarget::Feature { feature: one, key: "tol".into() };
    assert!(error(s.dimension_set(&target, Some(&v(0.)), None)).contains("stitch tolerance"));
    assert_eq!(recipe(&s), before);
    // The limits themselves are the app's: allowed.
    s.dimension_set(&target, Some(&v(1e-6)), None).unwrap();
    s.dimension_set(&target, Some(&v(10.)), Some("seam")).unwrap();
    let listed = s.info().named_dimensions;
    assert_eq!(listed.iter().find(|d| d.name == "seam").expect("listed").value, Some(10.));
    assert_eq!(STITCH_TOLERANCE_DEFAULT, 0.01);
}

/// Re-pick: another set of sheets (the lineage and the consumed parts follow), a refused one leaves the document alone.
#[test]
fn stitch_can_be_repicked() {
    let (mut s, plate) = block(60., 40., "12");
    let parts = copies(&mut s, plate, &[up(), front(), Sel::Facing { dir: [1., 0., 0.], tol_deg: 1.0 }]);
    let (one, _) = s.stitch(&parts[..2], &v(1e-4), None).unwrap();
    common::assert_close(info(&s, one).area, 3120., 1.0, "top and front");
    assert!(!consumed(&s, parts[2]));
    // The top and the right side instead: 2400 + 480; the front is free again.
    let (body, r) = s.repick_stitch(one, &[parts[0], parts[2]]).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(body, one);
    common::assert_close(info(&s, one).area, 2400. + 480., 1.0, "top and right");
    assert!(consumed(&s, parts[2]) && !consumed(&s, parts[1]));
    // Refusals.
    let before = recipe(&s);
    assert!(error(s.repick_stitch(one, &[parts[0]])).contains("at least two"));
    assert!(error(s.repick_stitch(one, &[parts[0], plate])).contains("is a solid"));
    assert!(error(s.repick_stitch(one, &[parts[0], one])).contains("is a solid") || recipe(&s) == before);
    let bottom = copies(&mut s, plate, &[down()]);
    let before = recipe(&s);
    assert!(error(s.repick_stitch(one, &[parts[0], bottom[0]])).contains("built after"));
    assert_eq!(recipe(&s), before);
    assert!(error(s.repick_stitch(parts[0], &[parts[0], parts[1]])).contains("not a stitch"));
}

/// Save and open: the stitched sheet comes back, and the app's own edit path rebuilds it.
#[test]
fn stitch_survives_save_open_and_the_app_edit_path() {
    let (mut s, plate) = block(60., 40., "12");
    let parts = copies(&mut s, plate, &[up(), front()]);
    let (one, _) = s.stitch(&parts, &v(1e-4), None).unwrap();
    let path = common::scratch("t2-stitch.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(info(&opened, one).sheet);
    common::assert_close(info(&opened, one).area, 3120., 1.0, "after open");
    let r = opened.param_set("w", &v(90.)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&opened, one).area, 4680., 5.0, "edit after open");
    let (_, _, _, project) = common::gui_edit_param_model(&path, "w", "90");
    let area: f64 = project.regen_faces[&one].iter().map(|f| f.area).sum();
    common::assert_close(area, 4680., 5.0, "the app's edit of w");
}
