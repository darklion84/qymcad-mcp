//! T4: Patch (P22). Mirrors the app's testkit `patch.rs`, `a_patch_over_one_closed_edge.rs`, `thicken_sheet.rs` and the acceptance
//! `PATCH` with the same numbers: a 40x30x20 box shelled open on top (a patch over the OUTER rim is the lid, 1200), the rim of a hole
//! r5 (a disc, pi 25) and of a hole 20 across in a 40x30x10 block (two bodies, volume 8858.407).
use crate::common;
use qymcad_core::feature::FeatureKind;
use qymcad_engine::*;
use std::f64::consts::PI;

fn v(x: f64) -> Num {
    Num::Value(x)
}

fn extrude(sketch: Id, height: &str, op: Op) -> Extrude {
    serde_json::from_value(serde_json::json!({"sketch": sketch, "height": height, "op": op})).unwrap()
}

fn up() -> Sel {
    Sel::Facing { dir: [0., 0., 1.], tol_deg: 1.0 }
}

/// A `w` x 30 x 20 box (parameter `w`, 40), shelled 2 inward with the top face open: the box of the app's tests. Returns (session, box).
fn open_box() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("w", &v(40.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(0.), &v(0.), &Num::from("w"), &v(30.), false).unwrap();
    let (body, r) = s.extrude(&extrude(sk, "20", Op::Add)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let (shell, r) = s.shell(Some(body), &up(), &v(2.), Side::Inward, None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    (s, shell)
}

/// The edges of the top opening (z = 20) of `body`: `outer` picks the ones on the body's outline, otherwise the inner outline.
fn rim(s: &mut Session, body: Id, outer: bool) -> Vec<u32> {
    let edges = s.topology(Some(body), false).unwrap().edges;
    let top = edges.iter().flat_map(|e| [e.a[2], e.b[2]]).fold(f64::MIN, f64::max);
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for e in &edges {
        for q in [e.a, e.b] {
            (x0, x1, y0, y1) = (x0.min(q[0]), x1.max(q[0]), y0.min(q[1]), y1.max(q[1]));
        }
    }
    let on_border =
        |q: [f64; 3]| (q[0] - x0).abs() < 1e-6 || (q[0] - x1).abs() < 1e-6 || (q[1] - y0).abs() < 1e-6 || (q[1] - y1).abs() < 1e-6;
    edges
        .iter()
        .filter(|e| (e.a[2] - top).abs() < 1e-6 && (e.b[2] - top).abs() < 1e-6 && (on_border(e.a) && on_border(e.b)) == outer)
        .map(|e| e.id)
        .collect()
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

fn ids(v: &[u32]) -> Sel {
    Sel::Ids(v.to_vec())
}

fn node_of(s: &Session, id: Id) -> (Id, Vec<u32>, bool) {
    let node = s.project().timeline.iter().find(|n| n.id == id).expect("the node");
    let FeatureKind::Patch { src, edges, tangent, .. } = &node.kind else { panic!("not a Patch") };
    (*src, edges.query.picked_descs(), *tangent)
}

/// The lid: a patch over the outer rim of the open box is a flat sheet of 40 x 30 = 1200; the box is not consumed and the node
/// holds picks of the rim, not a query (F-024).
#[test]
fn a_patch_spans_the_opening() {
    let (mut s, shell) = open_box();
    let edges = rim(&mut s, shell, true);
    assert_eq!(edges.len(), 4, "the opening has four outer edges");
    let volume = info(&s, shell).volume;
    let (patch, got, r) = s.patch(Some(shell), &ids(&edges), false, Some("lid")).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(got, {
        let mut e = edges.clone();
        e.sort_unstable();
        e
    });
    let b = info(&s, patch);
    assert!(b.sheet, "a patch is a surface");
    common::assert_close(b.area, 1200., 5.0, "the lid over 40 x 30");
    assert_eq!(s.node_name(patch), "lid");
    assert!(!consumed(&s, shell), "the source is not consumed");
    common::assert_close(info(&s, shell).volume, volume, 1e-6, "the box is as it was");
    let (src, stored, tangent) = node_of(&s, patch);
    assert_eq!((src, tangent), (shell, false));
    assert_eq!(stored, got, "stored as picks of the resolved ids");
    assert!(!s
        .project()
        .timeline
        .iter()
        .find(|n| n.id == patch)
        .map(|n| matches!(&n.kind, FeatureKind::Patch { edges, .. } if !edges.query.is_pick_list()))
        .unwrap());
}

/// The patch follows the base: the opening 40 -> 60 and the lid is 1800.
#[test]
fn the_patch_follows_the_opening() {
    let (mut s, shell) = open_box();
    let edges = rim(&mut s, shell, true);
    let (patch, _, _) = s.patch(Some(shell), &ids(&edges), false, None).unwrap();
    let r = s.param_set("w", &v(60.)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&s, patch).area, 1800., 5.0, "60 x 30");
}

/// Tangent bends the surface to meet the walls: over the same boundary it is larger than the flat one, and the mode is stored.
#[test]
fn a_tangent_patch_is_larger_than_a_flat_one_and_is_stored() {
    let (mut s, shell) = open_box();
    let edges = rim(&mut s, shell, true);
    let (flat, _, _) = s.patch(Some(shell), &ids(&edges), false, None).unwrap();
    let a_flat = info(&s, flat).area;
    let (mut t, shell) = open_box();
    let edges = rim(&mut t, shell, true);
    let (smooth, _, r) = t.patch(Some(shell), &ids(&edges), true, None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let a_smooth = info(&t, smooth).area;
    assert!(a_smooth > a_flat + 5., "smooth {a_smooth} against flat {a_flat}");
    assert!(node_of(&t, smooth).2, "tangent is stored in the node");
}

/// Ids that are not edges of the body are refused (a lost boundary), the document untouched; so is an empty selection.
#[test]
fn a_lost_boundary_is_refused() {
    let (mut s, shell) = open_box();
    let before = recipe(&s);
    let e = error(s.patch(Some(shell), &ids(&[0xDEAD_BEEF, 0xDEAD_BEEE]), false, None));
    assert!(e.contains("not on body"), "{e}");
    let e = error(s.patch(Some(shell), &Sel::Ids(vec![]), false, None));
    assert!(e.contains("The edges do not close"), "{e}");
    assert_eq!(recipe(&s), before);
}

/// A block 40 x 30 x 10 with a hole of diameter `d` through it at the middle of the top.
fn holed(d: f64) -> (Session, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(20.), &v(15.), &v(40.), &v(30.), false).unwrap();
    let (block, r) = s.extrude(&extrude(sk, "10", Op::Add)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let hole: Hole = Hole {
        body: Some(block),
        face: up(),
        at: None,
        offset: None,
        diameter: v(d),
        depth: Some(v(30.)),
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: None,
    };
    let (body, r) = s.hole(&hole).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    (s, body)
}

/// The circular edges of the hole at height `z`.
fn ring_at(s: &mut Session, body: Id, z: f64) -> Vec<u32> {
    let edges = s.topology(Some(body), false).unwrap().edges;
    edges.iter().filter(|e| e.kind == EdgeKind::Circle && (e.a[2] - z).abs() < 1e-6).map(|e| e.id).collect()
}

/// One closed edge is a boundary: the rim of a hole of radius 5 takes a disc of pi 25.
#[test]
fn the_rim_of_a_hole_takes_a_disc() {
    let (mut s, body) = holed(10.);
    let rim = ring_at(&mut s, body, 10.);
    assert_eq!(rim.len(), 1);
    let (disc, _, r) = s.patch(Some(body), &ids(&rim), false, None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&s, disc).area, PI * 25., 0.5, "disc r5");
}

/// The acceptance case: a hole 20 across, a flat disc over its top rim; two bodies, the block keeps its volume 12000 - 100 pi x 10.
#[test]
fn a_disc_over_the_hole_leaves_the_block_alone() {
    let (mut s, body) = holed(20.);
    let rim = ring_at(&mut s, body, 10.);
    let (disc, _, _) = s.patch(Some(body), &ids(&rim), true, None).unwrap();
    let bodies = s.result_bodies();
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    let volume: f64 = bodies.iter().map(|b| b.volume).sum();
    common::assert_close(volume, 8858.407346410208, 1e-3, "the block's volume");
    assert!(info(&s, disc).sheet);
    common::assert_close(info(&s, disc).area, PI * 100., 1.0, "disc r10 (tangent over a flat disc is the same)");
}

/// The lid closes the loop: a thicken of the patch is an ordinary solid joined to the box (the app's `thicken_sheet.rs`): the box
/// volume plus 40 x 30 x 2, ONE live solid, the box and the sheet consumed.
#[test]
fn patch_and_thicken_make_a_lid() {
    let (mut s, shell) = open_box();
    let v_box = info(&s, shell).volume;
    let edges = rim(&mut s, shell, true);
    let (patch, _, _) = s.patch(Some(shell), &ids(&edges), false, None).unwrap();
    let face = s.topology(Some(patch), false).unwrap().faces[0].id;
    let (lid, r) = s.thicken(Some(patch), &ids(&[face]), &v(2.), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let b = info(&s, lid);
    assert!(!b.sheet);
    common::assert_close(b.volume, v_box + 2400., 30., "the closed box");
    assert!(consumed(&s, patch) && consumed(&s, shell));
    let solids: Vec<_> = s.result_bodies().into_iter().filter(|b| !b.sheet).collect();
    assert_eq!(solids.len(), 1, "{solids:?}");
}

/// The pre-check of the app (`patch_boundary`), refused with its words and the document untouched: one straight edge, an open
/// chain (three of the four outer edges), an edge picked past the loop (the four outer edges and one more), no edge at all.
#[test]
fn the_boundary_pre_check_refuses_what_the_app_refuses() {
    let (mut s, shell) = open_box();
    let outer = rim(&mut s, shell, true);
    let inner = rim(&mut s, shell, false);
    assert_eq!((outer.len(), inner.len()), (4, 4));
    let before = recipe(&s);
    let mut past = outer.clone();
    past.push(inner[0]);
    for (what, picked) in [
        ("one straight edge", vec![outer[0]]),
        ("an open chain", outer[..3].to_vec()),
        ("two opposite edges", vec![outer[0], outer[2]]),
        ("an edge past the loop", past),
    ] {
        let e = error(s.patch(Some(shell), &ids(&picked), false, None));
        assert!(e.contains("The edges do not close: pick edges that run all the way round the opening, or one ring"), "{what}: {e}");
        assert_eq!(recipe(&s), before, "{what}");
    }
}

/// The OUTER rim of a shelled opening is the lid; the INNER rim (2 mm in) is a smaller boundary and a patch over it is 36 x 26 = 936.
/// Both rims at once pass the app's pre-check (each end meets one other end) and QymCAD then builds the outer lid and drops the inner
/// edges with only a console warning (F-199): refused, naming the boundaries, the document untouched. Same for a chain with a ring.
#[test]
fn outer_and_inner_rim_are_different_boundaries_and_not_both_f199() {
    let (mut s, shell) = open_box();
    let (outer, inner) = (rim(&mut s, shell, true), rim(&mut s, shell, false));
    let (lid, _, _) = s.patch(Some(shell), &ids(&outer), false, None).unwrap();
    let (small, _, r) = s.patch(Some(shell), &ids(&inner), false, None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    common::assert_close(info(&s, lid).area, 1200., 5.0, "outer");
    common::assert_close(info(&s, small).area, 936., 5.0, "inner 36 x 26");
    let before = recipe(&s);
    let both: Vec<u32> = outer.iter().chain(&inner).copied().collect();
    let e = error(s.patch(Some(shell), &ids(&both), false, None));
    assert!(e.contains("2 separate boundaries") && e.contains("F-199"), "{e}");
    assert_eq!(recipe(&s), before);
}

/// The same in a block with a hole: the four edges of the top and the ring of the hole are two boundaries.
#[test]
fn a_chain_and_a_ring_are_two_boundaries_f199() {
    let (mut s, body) = holed(10.);
    let topo = s.topology(Some(body), false).unwrap();
    let chain: Vec<u32> = topo
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Line && (e.a[2] - 10.).abs() < 1e-6 && (e.b[2] - 10.).abs() < 1e-6)
        .map(|e| e.id)
        .collect();
    assert_eq!(chain.len(), 4);
    let ring = ring_at(&mut s, body, 10.);
    let before = recipe(&s);
    let all: Vec<u32> = ring.iter().chain(&chain).copied().collect();
    assert!(error(s.patch(Some(body), &ids(&all), false, None)).contains("2 separate boundaries"));
    // The top and the bottom ring: two boundaries (QymCAD itself fails on these: "empty body").
    let both: Vec<u32> = ring.iter().chain(&ring_at(&mut s, body, 0.)).copied().collect();
    assert!(error(s.patch(Some(body), &ids(&both), false, None)).contains("2 separate boundaries"));
    assert_eq!(recipe(&s), before);
    // Each alone is fine: the outline is the full top (1200), the ring a disc.
    let (top, _, _) = s.patch(Some(body), &ids(&chain), false, None).unwrap();
    common::assert_close(info(&s, top).area, 1200., 5.0, "the outline");
}

/// Re-pick: the edges (outer to inner: 1200 to 936) and the mode (flat to tangent: larger); the node id stays. Refused: nothing given
/// is the tool's business; a node of another kind; edges of another body; ids that are not edges.
#[test]
fn a_patch_can_be_repicked() {
    let (mut s, shell) = open_box();
    let (outer, inner) = (rim(&mut s, shell, true), rim(&mut s, shell, false));
    let (patch, _, _) = s.patch(Some(shell), &ids(&outer), false, None).unwrap();
    let (body, got, r) = s.repick_patch(patch, Some(&ids(&inner)), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(body, patch);
    assert_eq!(got.len(), 4);
    common::assert_close(info(&s, patch).area, 936., 5.0, "inner");
    assert_eq!(node_of(&s, patch).1, got);
    // The mode only: the edges stay.
    let (_, none, _) = s.repick_patch(patch, None, Some(true)).unwrap();
    assert!(none.is_empty());
    assert!(node_of(&s, patch).2);
    assert_eq!(node_of(&s, patch).1, got);
    assert!(info(&s, patch).area > 936. + 1.);
    s.repick_patch(patch, Some(&ids(&outer)), Some(false)).unwrap();
    common::assert_close(info(&s, patch).area, 1200., 5.0, "back to the outer rim, flat");
    // Refusals leave the document alone.
    let before = recipe(&s);
    assert!(error(s.repick_patch(patch, Some(&ids(&[0xDEAD_BEEF])), None)).contains("not on body"));
    assert!(error(s.repick_patch(shell, Some(&ids(&outer)), None)).contains("not a patch"));
    let both: Vec<u32> = outer.iter().chain(&inner).copied().collect();
    assert!(error(s.repick_patch(patch, Some(&ids(&both)), None)).contains("2 separate boundaries"), "F-199 holds on a re-pick too");
    assert!(error(s.repick_patch(99999, None, Some(true))).contains("99999"));
    assert_eq!(recipe(&s), before);
}

/// A re-pick has no pre-check in the app's edit mode (`cmd_ready` is true for any edit), so a straight edge or an open chain reaches the
/// rebuild; the kernel refuses them itself ("one edge does not bound a patch", "operation failed: Patch") and the re-pick is rolled back.
#[test]
fn a_repick_onto_an_open_chain_is_refused_by_the_rebuild() {
    let (mut s, shell) = open_box();
    let outer = rim(&mut s, shell, true);
    let (patch, _, _) = s.patch(Some(shell), &ids(&outer), false, None).unwrap();
    let before = recipe(&s);
    for (what, picked) in [("one straight edge", vec![outer[0]]), ("three of four", outer[..3].to_vec())] {
        let e = error(s.repick_patch(patch, Some(&ids(&picked)), None));
        assert!(e.contains("rolled back") && e.contains("Patch"), "{what}: {e}");
        assert_eq!(recipe(&s), before, "{what}: the document is as it was");
    }
    common::assert_close(info(&s, patch).area, 1200., 5.0, "unchanged");
}

/// A sheet is a source too: the four boundary edges of the copy of a block's top take a patch of the same 1200 (a second sheet).
#[test]
fn a_sheet_can_be_the_source() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(0.), &v(0.), &v(40.), &v(30.), false).unwrap();
    let (block, _) = s.extrude(&extrude(sk, "10", Op::Add)).unwrap();
    let (copy, _, _) = s.face_copy(Some(block), &up(), None).unwrap();
    let edges: Vec<u32> = s.topology(Some(copy), false).unwrap().edges.iter().map(|e| e.id).collect();
    assert_eq!(edges.len(), 4);
    let (patch, _, r) = s.patch(Some(copy), &ids(&edges), false, None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(info(&s, patch).sheet);
    common::assert_close(info(&s, patch).area, 1200., 5.0, "the same rectangle");
    assert!(!consumed(&s, copy), "the source sheet is not consumed");
}

/// The node lands in the Part of its source, not in the active one; save and open bring it back and the app's own edit path rebuilds it.
#[test]
fn the_patch_lands_in_its_part_and_survives_save_open() {
    let (mut s, shell) = open_box();
    let part = s.project().active_component.unwrap();
    let edges = rim(&mut s, shell, true);
    s.part_new("other").unwrap();
    assert_ne!(s.project().active_component, Some(part));
    let (patch, _, r) = s.patch(Some(shell), &ids(&edges), true, None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(info(&s, patch).owner_part, Some(part));
    let area = info(&s, patch).area;
    let path = common::scratch("t4-patch.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(info(&opened, patch).sheet);
    common::assert_close(info(&opened, patch).area, area, 1e-3, "after open");
    assert!(node_of(&opened, patch).2, "tangent comes back");
    let r = opened.param_set("w", &v(60.)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(info(&opened, patch).area > 1800., "edit after open: {}", info(&opened, patch).area);
    let (_, _, _, project) = common::gui_edit_param_model(&path, "w", "60");
    let a: f64 = project.regen_faces[&patch].iter().map(|f| f.area).sum();
    assert!(a > 1800., "the app's edit of w: {a}");
}
