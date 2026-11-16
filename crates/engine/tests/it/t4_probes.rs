//! T4 native probe: what QymCAD itself does with a Patch over edges that form more than one boundary (F-199). Core API only (the app's
//! adders and regeneration), none of the server's guards. It pins the observation: when the app refuses or builds the surface over
//! all the edges, it goes red and the server refusal of `patch` (`patch_loops`) can be dropped.
use qymcad_core::model::Project;
use qymcad_core::refs::Ref;

/// A 40 x 30 x 10 block with a hole 20 across through it. Returns (project, block with the hole, top outline ids, top ring id).
fn holed_block() -> (Project, u64, Vec<u32>, u32) {
    let mut p = Project::default();
    p.new_document();
    let block = p.add_box(40.0, 30.0, 10.0);
    let bore = p.add_cylinder(10.0, 30.0);
    let holed = p.add_body_boolean(block, bore, 0);
    let _ = qymcad_testkit::regenerate(&mut p);
    let edges = &p.regen_edges[&holed];
    let ring = edges.iter().find(|e| (e.radius - 10.0).abs() < 0.05 && e.a[2] > 9.99).map(|e| e.id).expect("the top ring of the hole");
    let outline: Vec<u32> = edges.iter().filter(|e| e.radius == 0.0 && e.a[2] > 9.99 && e.b[2] > 9.99).map(|e| e.id).collect();
    assert_eq!(outline.len(), 4);
    (p, holed, outline, ring)
}

/// F-199: the outline of the top and the ring of the hole picked together. The pre-check of the app accepts them (a closed chain and a
/// ring); the node is GREEN and the surface is the full 40 x 30 = 1200 outline face with four edges: the ring was dropped, the
/// kernel's console says "WireFromList: can't find the next edge ... some edges are lost".
#[test]
fn the_app_spans_one_boundary_and_drops_the_other_edges_f199() {
    let (mut p, holed, outline, ring) = holed_block();
    let all: Vec<u32> = outline.iter().copied().chain([ring]).collect();
    let patch = p.add_patch(holed, Ref::picks(&all), false);
    let (report, _) = qymcad_testkit::regenerate(&mut p);
    assert!(report.errors.is_empty(), "the app builds it: {:?}", report.errors);
    let faces = &p.regen_faces[&patch];
    assert_eq!(faces.len(), 1);
    assert!((faces[0].area - 1200.0).abs() < 1.0, "the outline face, the hole ignored: {}", faces[0].area);
    assert_eq!(p.regen_edges[&patch].len(), 4, "four edges: the ring is not on the surface");
}
