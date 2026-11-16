//! T3: Trim (P24). Mirrors the app's testkit `trim.rs` with the same numbers: a 60x40x12 plate and a sheet copy of its top face, a box
//! tool across it. The plate is centred on the origin here (x -30..30), so the tool stands on x 0..60 and the cut line is x = 0: keep
//! left (x < 0) or right (x > 0) is 30 x 40 = 1200 either way.
use crate::common;
use qymcad_engine::*;

fn v(x: f64) -> Num {
    Num::Value(x)
}

fn extrude(sketch: Id, height: &str, op: Op) -> Extrude {
    serde_json::from_value(serde_json::json!({"sketch": sketch, "height": height, "op": op})).unwrap()
}

/// The plate: `w` x `d` (parameters of those names), 12 tall, centred on the origin.
fn plate() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("w", &v(60.)).unwrap();
    s.param_set("d", &v(40.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(0.), &v(0.), &Num::from("w"), &Num::from("d"), false).unwrap();
    let (body, r) = s.extrude(&extrude(sk, "12", Op::Add)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    (s, body)
}

/// A box tool, 30 tall from z = 0, `w` wide from `cx - w/2`, 200 long in y (it runs past the plate: a tool that stops short would
/// leave the two pieces joined round its end).
fn tool(s: &mut Session, cx: Num, w: f64, op: Op) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &cx, &v(0.), &v(w), &v(200.), false).unwrap();
    let (body, r) = s.extrude(&extrude(sk, "30", op)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    body
}

/// The plate with its top copied to a sheet, and the box tool on x 0..60. Returns (session, plate, sheet, tool).
fn setup() -> (Session, Id, Id, Id) {
    let (mut s, plate) = plate();
    let (sheet, _, r) = s.face_copy(Some(plate), &up(), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let t = tool(&mut s, v(30.), 60., Op::NewBody);
    (s, plate, sheet, t)
}

fn up() -> Sel {
    Sel::Facing { dir: [0., 0., 1.], tol_deg: 1.0 }
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

/// Mean x of the faces of a body, part-local.
fn centre_x(s: &mut Session, body: Id) -> f64 {
    let faces = s.topology(Some(body), false).unwrap().faces;
    faces.iter().map(|f| f.centroid[0] * f.area).sum::<f64>() / faces.iter().map(|f| f.area).sum::<f64>()
}

const LEFT: [f64; 3] = [-15., 0., 12.];
const RIGHT: [f64; 3] = [15., 0., 12.];

/// The piece under the point stays: left of the tool, 30 x 40; the sheet is consumed, the tool is not.
#[test]
fn the_piece_under_the_point_is_what_stays() {
    let (mut s, _, sheet, t) = setup();
    common::assert_close(info(&s, sheet).area, 2400., 1.0, "setup: the whole sheet");
    let (left, r) = s.trim(sheet, t, LEFT, Some("left")).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let b = info(&s, left);
    assert!(b.sheet, "what was trimmed is still a surface");
    common::assert_close(b.area, 1200., 5.0, "30 x 40");
    assert!(centre_x(&mut s, left) < 0.);
    assert!(consumed(&s, sheet), "the sheet is consumed");
    assert!(!consumed(&s, t), "the tool goes on cutting");
    assert_eq!(s.node_name(left), "left");
    let node = s.project().timeline.iter().find(|n| n.id == left).unwrap();
    let qymcad_core::feature::FeatureKind::Trim { src, tool, keep, .. } = &node.kind else { panic!("not a Trim") };
    assert_eq!((*src, *tool, *keep), (sheet, t, LEFT));
}

/// The other side: the other piece stays. Otherwise the point decides nothing.
#[test]
fn the_other_side_keeps_the_other_piece() {
    let (mut s, _, sheet, t) = setup();
    let (right, r) = s.trim(sheet, t, RIGHT, None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&s, right).area, 1200., 5.0, "30 x 40");
    assert!(centre_x(&mut s, right) > 0.);
}

/// A tool that does not reach the sheet is a named refusal; so is a tool the kept side of which is empty: nothing is laid.
#[test]
fn a_tool_that_misses_is_refused_and_the_document_is_untouched() {
    let (mut s, _, sheet, _) = setup();
    let far = tool(&mut s, v(500.), 60., Op::NewBody);
    let before = recipe(&s);
    let e = error(s.trim(sheet, far, LEFT, None));
    assert!(e.contains("rolled back") && e.contains("Trim"), "{e}");
    assert_eq!(recipe(&s), before);
    assert!(!consumed(&s, sheet));
}

/// A sheet is a tool too: the x = 0 side face of the box, copied.
#[test]
fn a_sheet_can_be_the_cutting_tool() {
    let (mut s, _, sheet, t) = setup();
    let (cutter, _, r) = s.face_copy(Some(t), &Sel::Facing { dir: [-1., 0., 0.], tol_deg: 1.0 }, None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(info(&s, cutter).sheet);
    let (left, r) = s.trim(sheet, cutter, LEFT, None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&s, left).area, 1200., 5.0, "30 x 40");
    assert!(!consumed(&s, cutter), "a sheet tool is not consumed either");
}

/// The trimmed sheet follows the base: the plate deeper by 20 and the same piece stays, 30 x 60.
#[test]
fn the_trimmed_sheet_follows_the_base() {
    let (mut s, _, sheet, t) = setup();
    let (left, _) = s.trim(sheet, t, LEFT, None).unwrap();
    let r = s.param_set("d", &v(60.)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&s, left).area, 1800., 5.0, "30 x 60");
    let r = s.param_set("w", &v(80.)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&s, left).area, 40. * 60., 5.0, "the left piece is now 40 wide (x -40..0)");
}

/// A trim that cuts nothing after an edit: the server cannot leave a red node, so a parameter edit that makes the tool miss is
/// refused and rolled back, the trimmed sheet stays as it was. (The app keeps the unchanged sheet on such a rebuild, core run.rs;
/// that branch is not reachable through the server.)
#[test]
fn an_edit_that_makes_the_tool_miss_is_rolled_back() {
    let (mut s, _) = plate();
    let (sheet, _, _) = s.face_copy(None, &up(), None).unwrap();
    s.param_set("tx", &v(30.)).unwrap();
    let t = tool(&mut s, Num::from("tx"), 60., Op::NewBody);
    let (left, _) = s.trim(sheet, t, LEFT, None).unwrap();
    let before = recipe(&s);
    let miss = s.param_set("tx", &v(500.));
    let e = error(miss);
    assert!(e.contains("rolled back") && e.contains("Trim"), "{e}");
    assert_eq!(recipe(&s), before);
    common::assert_close(info(&s, left).area, 1200., 5.0, "unchanged");
}

/// The keep point is in the Part's own frame, not the world's: on a Part placed with an offset and a quarter turn the same local
/// point keeps the same piece as on an unplaced one, and its WORLD image keeps the other one (it lies beyond the cut, part-locally).
#[test]
fn keep_is_part_local_on_a_placed_part() {
    let rot = PartRotation { axis: [0., 0., 1.], angle_deg: 90. };
    // The world image of LEFT: (x, y) -> (-y, x) + (500, 200) = (500, 185). Read part-locally it lies on the right of the cut.
    let world_of_left = [500., 185., 12.];
    for (placed, keep, right) in [(false, LEFT, false), (true, LEFT, false), (true, world_of_left, true)] {
        let (mut s, _, sheet, t) = setup();
        if placed {
            let part = s.project().active_component.unwrap();
            s.part_place(&PartRef::Id(part), [500., 200., 0.], Some(&rot)).unwrap();
        }
        let (piece, r) = s.trim(sheet, t, keep, None).unwrap();
        assert!(r.errors.is_empty(), "placed {placed}: {:?}", r.errors);
        common::assert_close(info(&s, piece).area, 1200., 5.0, "area");
        assert_eq!(centre_x(&mut s, piece) > 0., right, "placed {placed}, keep {keep:?}");
    }
}

/// What is refused, with the document untouched: a solid as the sheet, a consumed body, a mesh, an unknown id, the tool as its own
/// sheet, a tool from another Part, a non-finite point.
#[test]
fn refusals_leave_the_document_untouched() {
    let (mut s, plate, sheet, t) = setup();
    let before = recipe(&s);
    let e = error(s.trim(plate, t, LEFT, None));
    assert!(e.contains("is a solid") && e.contains("must be a sheet"), "{e}");
    assert!(error(s.trim(99999, t, LEFT, None)).contains("99999"));
    assert!(error(s.trim(sheet, 99999, LEFT, None)).contains("99999"));
    let e = error(s.trim(sheet, sheet, LEFT, None));
    assert!(e.contains("cannot trim itself"), "{e}");
    for bad in [[f64::NAN, 0., 12.], [0., f64::INFINITY, 12.]] {
        let e = error(s.trim(sheet, t, bad, None));
        assert!(e.contains("finite"), "{e}");
    }
    // The plate the sheet was copied from does not cut it (the sheet lies on the plate's own face): refused by the kernel.
    assert!(error(s.trim(sheet, plate, LEFT, None)).contains("rolled back"));
    assert_eq!(recipe(&s), before);
    // A mesh body, as sheet and as tool.
    let dir = crate::common::scratch("qymcad-trim-mesh");
    std::fs::create_dir_all(&dir).unwrap();
    let stl = dir.join("tetra.stl");
    std::fs::write(
        &stl,
        "solid t\n facet normal 0 0 0\n  outer loop\n   vertex 0 0 0\n   vertex 10 0 0\n   vertex 0 10 0\n  endloop\n endfacet\n \
         facet normal 0 0 0\n  outer loop\n   vertex 0 0 0\n   vertex 0 10 0\n   vertex 0 0 10\n  endloop\n endfacet\n \
         facet normal 0 0 0\n  outer loop\n   vertex 0 0 0\n   vertex 0 0 10\n   vertex 10 0 0\n  endloop\n endfacet\n \
         facet normal 0 0 0\n  outer loop\n   vertex 10 0 0\n   vertex 0 0 10\n   vertex 0 10 0\n  endloop\n endfacet\nendsolid t\n",
    )
    .unwrap();
    let mesh = s.import_mesh(&stl, None, None, 1.0, None).unwrap().pieces[0].body;
    let before = recipe(&s);
    assert!(error(s.trim(mesh, t, LEFT, None)).contains("mesh"));
    assert!(error(s.trim(sheet, mesh, LEFT, None)).contains("mesh"));
    assert_eq!(recipe(&s), before);
    std::fs::remove_dir_all(dir).unwrap();
    // A tool of another Part.
    let (mut s, _, sheet, _) = setup();
    s.part_new("other").unwrap();
    let foreign = tool(&mut s, v(30.), 60., Op::Add);
    let before = recipe(&s);
    let e = error(s.trim(sheet, foreign, LEFT, None));
    assert!(e.contains("same part"), "{e}");
    assert_eq!(recipe(&s), before);
    // A sheet used up by a trim cannot be trimmed twice.
    let (mut s, _, sheet, t) = setup();
    s.trim(sheet, t, LEFT, None).unwrap();
    let e = error(s.trim(sheet, t, RIGHT, None));
    assert!(e.contains("consumed"), "{e}");
}

/// Re-pick: keep, tool and sheet of an existing node change, its id stays; refused re-picks leave the document alone.
#[test]
fn trim_can_be_repicked() {
    let (mut s, plate, sheet, t) = setup();
    // Bodies a re-pick may take must exist before the node: another tool, standing at x 10..70, and the top lifted by 5.
    let t2 = tool(&mut s, v(40.), 60., Op::NewBody);
    let (high, _, _) = s.offset_surface(Some(plate), &up(), &v(5.), None).unwrap();
    let (node, _) = s.trim(sheet, t, LEFT, None).unwrap();
    // The other side only.
    let (body, r) = s.repick_trim(node, None, None, Some(RIGHT)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(body, node);
    common::assert_close(info(&s, node).area, 1200., 5.0, "right");
    assert!(centre_x(&mut s, node) > 0.);
    // Another tool: the cut moves to x = 10.
    s.repick_trim(node, None, Some(t2), Some(LEFT)).unwrap();
    common::assert_close(info(&s, node).area, 40. * 40., 5.0, "x -30..10");
    assert!(!consumed(&s, t2));
    // Another sheet needs its own point.
    let before = recipe(&s);
    let e = error(s.repick_trim(node, Some(high), None, None));
    assert!(e.contains("own `keep`"), "{e}");
    assert_eq!(recipe(&s), before);
    s.repick_trim(node, Some(high), Some(t), Some([-15., 0., 17.])).unwrap();
    common::assert_close(info(&s, node).area, 1200., 5.0, "the lifted sheet");
    assert!(consumed(&s, high) && !consumed(&s, sheet), "the old sheet is free again");
    // Refusals.
    let before = recipe(&s);
    assert!(error(s.repick_trim(node, Some(plate), None, Some(LEFT))).contains("is a solid"));
    assert!(error(s.repick_trim(node, None, Some(high), None)).contains("cannot trim itself"));
    assert!(error(s.repick_trim(node, None, None, Some([f64::NAN, 0., 0.]))).contains("finite"));
    let far = tool(&mut s, v(500.), 60., Op::NewBody);
    // Built after the node: refused for that reason (the far tool is later than the node).
    let before_far = recipe(&s);
    assert!(error(s.repick_trim(node, None, Some(far), None)).contains("built after"));
    assert_eq!(recipe(&s), before_far);
    assert_ne!(before, before_far);
    assert!(error(s.repick_trim(plate, None, None, Some(LEFT))).contains("not a trim"));
    assert!(error(s.repick_trim(sheet, None, None, Some(LEFT))).contains("not a trim"));
}

/// A re-pick onto a tool that misses is refused at the rebuild and rolled back.
#[test]
fn a_repick_onto_a_tool_that_misses_is_rolled_back() {
    let (mut s, _, sheet, t) = setup();
    let far = tool(&mut s, v(500.), 60., Op::NewBody);
    let (node, _) = s.trim(sheet, t, LEFT, None).unwrap();
    let before = recipe(&s);
    assert!(error(s.repick_trim(node, None, Some(far), None)).contains("rolled back"));
    assert_eq!(recipe(&s), before);
    common::assert_close(info(&s, node).area, 1200., 5.0, "unchanged");
}

/// Save and open: the trim comes back, and the app's own edit path rebuilds it (depth 40 -> 60).
#[test]
fn trim_survives_save_open_and_the_app_edit_path() {
    let (mut s, _, sheet, t) = setup();
    let (left, _) = s.trim(sheet, t, LEFT, None).unwrap();
    let path = common::scratch("t3-trim.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(info(&opened, left).sheet);
    common::assert_close(info(&opened, left).area, 1200., 5.0, "after open");
    let r = opened.param_set("d", &v(60.)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&opened, left).area, 1800., 5.0, "edit after open");
    let (_, _, _, project) = common::gui_edit_param_model(&path, "d", "60");
    let area: f64 = project.regen_faces[&left].iter().map(|f| f.area).sum();
    common::assert_close(area, 1800., 5.0, "the app's edit of d");
}

/// The node lands in the Part of its sheet, not in the Part that happens to be active.
#[test]
fn the_trim_lands_in_the_part_of_its_sheet() {
    let (mut s, _, sheet, t) = setup();
    let part = s.project().active_component.unwrap();
    s.part_new("other").unwrap();
    assert_ne!(s.project().active_component, Some(part));
    let (left, r) = s.trim(sheet, t, LEFT, None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(info(&s, left).owner_part, Some(part));
}
