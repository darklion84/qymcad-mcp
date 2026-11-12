//! Package I phase 0: probes of what the app does around datum points, datum axes and the construction toggle.
//! Each probe prints what it observed (run with --nocapture); assertions pin the observation, so a changed app shows here.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

/// The app's side of F-168, through the core API only (no Session, no server guard): the document is saved by the server,
/// loaded as the app does, the app's `toggle_construction` runs, the dirty consumers regenerate. Answers the consumer's
/// body volume and the regeneration errors.
fn native_toggle(path: &std::path::Path, si: usize, entities: &[u64]) -> (f64, Vec<String>) {
    let mut m = native_assembly_load(path);
    native_assembly_rebuild(&mut m, true);
    let sketch = m.project.sketches[si].id;
    m.project.toggle_construction(si, entities);
    m.project.mark_sketch_dirty(sketch);
    m.project.eval_parameters();
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut m.project, std::mem::take(&mut m.shapes));
    m.shapes = shapes;
    let consumed = m.project.consumed_bodies();
    let last = m.project.timeline.iter().flat_map(|n| n.kind.bodies()).rfind(|b| !consumed.contains(b)).unwrap();
    let volume = {
        let _g = qymcad_kernel::kernel_gate();
        m.shapes[&last].volume()
    };
    (volume, report.errors.iter().map(|e| format!("{e:?}")).collect())
}

fn plate_doc(name: &str, island: bool) -> (std::path::PathBuf, u64) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let cx = if island { 20.0 } else { 0.0 };
    s.sketch_rect(
        sk,
        &(if island { -20.0 } else { 0.0 }).into(),
        &0.0.into(),
        &(if island { 10.0 } else { 20.0 }).into(),
        &10.0.into(),
        false,
    )
    .unwrap();
    let circle = s.sketch_circle(sk, &cx.into(), &0.0.into(), &(if island { 10.0 } else { 4.0 }).into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
    let path = scratch(name);
    s.save(Some(&path)).unwrap();
    (path, circle)
}

/// P11 (native): a hole circle turned construction under a plate extrude. The app follows the held outer contour: the hole
/// goes, the volume grows, no error. A legal edit, so the server lets it through (F-168 as rewritten).
#[test]
fn p11_native_a_hole_turned_construction_is_followed_by_the_consumer() {
    let (path, circle) = plate_doc("p11-hole.qcad", false);
    let (volume, errors) = native_toggle(&path, 0, &[circle]);
    eprintln!("P11 native hole: volume {volume} errors {errors:?}");
    assert!(errors.is_empty());
    assert_close(volume, 1000.0, 1e-6, "plate without the hole");
}

/// P11 (native): one island of a two-profile extrude turned construction drops out of the feature, with no error.
#[test]
fn p11_native_a_vanished_island_drops_out_silently() {
    let (path, circle) = plate_doc("p11-island.qcad", true);
    let (volume, errors) = native_toggle(&path, 0, &[circle]);
    eprintln!("P11 native island: volume {volume} errors {errors:?}");
    // F-168: 500 + 392.7 before, 500 after, `errors: []`. If the app starts reporting this, the server guard can go.
    assert!(errors.is_empty(), "the app reports the dropped island now: {errors:?}");
    assert_close(volume, 500.0, 1e-6, "the island is gone from the body");
}

/// P11b (native): the only profile of the feature turned construction: the app itself fails.
#[test]
fn p11b_native_the_only_profile_turned_construction_is_an_error() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let circle = s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
    let path = scratch("p11b.qcad");
    s.save(Some(&path)).unwrap();
    let (_, errors) = native_toggle(&path, 0, &[circle]);
    eprintln!("P11b native: {errors:?}");
    assert!(errors.iter().any(|e| e.contains("rofile")), "the app's own error: {errors:?}");
}

/// P12: the app's toggle on a text id, and the entity of a text.
#[test]
fn p12_text_is_not_toggled() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let text = s
        .sketch_text(sk, &TextSpec { text: "A".into(), at: [0.0, 0.0], height: 8.0, angle: 0.0, font: None, construction: false })
        .unwrap();
    let path = scratch("p12.qcad");
    s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    let si = 0;
    let before: Vec<bool> = m.project.sketches[si].texts.iter().map(|t| t.construction).collect();
    let now = m.project.toggle_construction(si, &[text]);
    let after: Vec<bool> = m.project.sketches[si].texts.iter().map(|t| t.construction).collect();
    eprintln!("P12 text {text}: before {before:?} toggle returned {now} after {after:?}");
    assert_eq!((before, now, after), (vec![false], false, vec![false]), "the app's toggle does not touch a text");
    let r = s.sketch_construction(sk, &[text], true);
    eprintln!("P12 server: {:?}", r.map(|_| ()).map_err(|e| e.to_string()));
}

// ---------------------------------------------------------------------------------------------------------------
// Datum probes (P1, P2, P3): native app data path, no Session repairs. A body C (a cylinder or a block at x = 30) carries
// the datum, a block B is circularly arrayed about it, the count is the parameter `cnt`.

use qymcad_core::model::{FeatNumber, PlainNumber};

struct Fixture {
    path: std::path::PathBuf,
    c: u64,
    edge: (u32, [f64; 3], [f64; 3]),
    b: u64,
}

fn fixture(name: &str) -> Fixture {
    let mut s = Session::new_part();
    s.param_set("cnt", &3.0.into()).unwrap();
    s.param_set("px", &30.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &30.0.into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    let (c, _) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":10,"op":"new_body"})).unwrap()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &6.0.into(), &6.0.into(), false).unwrap();
    let (b, _) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":4,"op":"new_body"})).unwrap()).unwrap();
    let topo = s.topology(Some(c), false).unwrap();
    let e = topo.edges.iter().find(|e| e.kind == EdgeKind::Line && (e.a[2] - e.b[2]).abs() > 5.0 && e.a[0] > 30.0 && e.a[1] > 0.0).unwrap();
    let edge = (e.id, e.a, e.b);
    let path = scratch(name);
    s.save(Some(&path)).unwrap();
    Fixture { path, c, edge, b }
}

fn final_bbox(m: &NativeAssemblyModel) -> [f64; 6] {
    let consumed = m.project.consumed_bodies();
    let last = m.project.timeline.iter().flat_map(|n| n.kind.bodies()).rfind(|b| !consumed.contains(b)).unwrap();
    let _g = qymcad_kernel::kernel_gate();
    m.shapes[&last].bbox().unwrap()
}

fn r3(v: [f64; 6]) -> Vec<f64> {
    v.iter().map(|x| (x * 1000.0).round() / 1000.0).collect()
}

/// kind: "manual_formula", "edge", "two_points", "vertex_points"
fn scenario(kind: &str) {
    let f = fixture(&format!("p-{kind}.qcad"));
    let mut m = native_assembly_load(&f.path);
    native_assembly_rebuild(&mut m, true);
    let p = &mut m.project;
    let axis = match kind {
        "manual_formula" => {
            let a = p.add_axis_manual([30.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
            p.write_feat_formula(&FeatNumber { node: a, key: "ox" }, "px", PlainNumber::Stands).unwrap();
            a
        }
        "edge" => p.add_axis_from_edge(f.c, f.edge.0),
        "two_points" => {
            let a = p.add_point_at([f.edge.1[0], f.edge.1[1], 0.0]);
            let b = p.add_point_at([f.edge.1[0], f.edge.1[1], 10.0]);
            p.add_axis_two_points(a, b)
        }
        "vertex_points" => {
            let a = p.add_point_at_vertex(f.edge.1, f.c, f.edge.0, false);
            let b = p.add_point_at_vertex(f.edge.2, f.c, f.edge.0, true);
            p.add_axis_two_points(a, b)
        }
        _ => unreachable!(),
    };
    let arr = p.add_circular_array_axis(f.b, 3, 360.0, axis);
    p.write_feat_formula(&FeatNumber { node: arr, key: "count" }, "cnt", PlainNumber::Stands).unwrap();
    native_assembly_rebuild(&mut m, false);
    let axis_of = |m: &NativeAssemblyModel| {
        let a = m.project.datum_axes.iter().find(|a| a.id == axis).unwrap();
        (a.origin(), a.dir())
    };
    eprintln!("P[{kind}] built: axis {:?} bbox {:?}", axis_of(&m), r3(final_bbox(&m)));
    native_assembly_parameter(&mut m, "cnt", "4");
    let saved = scratch(&format!("p-{kind}-saved.qcad"));
    native_assembly_save(&m, &saved);
    let before_axis = axis_of(&m);
    if kind == "manual_formula" {
        // P1: the formula's parameter moves; does the axis (and the array about it) follow?
        native_assembly_parameter(&mut m, "px", "20");
        eprintln!("P1[{kind}] px 30 -> 20: axis {:?} (was {before_axis:?}) bbox {:?}", axis_of(&m), r3(final_bbox(&m)));
        // F-170: the axis stays where its definition puts it, whatever the formula says.
        assert_eq!(axis_of(&m), before_axis);
        assert_eq!(r3(final_bbox(&m))[0], -3.0, "the array still turns about x = 30 (bbox -3..63 is symmetric about 30)");
        assert_eq!(r3(final_bbox(&m))[3], 63.0);
    }
    let mut ctl = native_assembly_load(&saved);
    native_assembly_rebuild(&mut ctl, false);
    native_assembly_parameter(&mut ctl, "cnt", "5");
    let ctl_bbox = r3(final_bbox(&ctl));
    // P2: the way the app opens a file, then the same edit.
    let mut app = native_app_open(&saved);
    let node_dirty: Vec<bool> = app
        .project
        .timeline
        .iter()
        .filter(|n| matches!(n.kind, qymcad_core::feature::FeatureKind::DatumAxis { .. }))
        .map(|n| n.dirty)
        .collect();
    let a = app.project.datum_axes.iter().find(|a| a.id == axis).unwrap();
    eprintln!("P2[{kind}] after app open: axis origin {:?} dir {:?} node dirty {node_dirty:?}", a.origin(), a.dir());
    // Observation: the app's open path resolves axis caches too (nothing is left at zero); the edit then agrees with an in-memory one.
    assert_ne!(a.dir(), [0.0; 3]);
    native_assembly_parameter(&mut app, "cnt", "5");
    assert_eq!(r3(final_bbox(&app)), ctl_bbox, "{kind}: a reopened document rebuilds the consumer about the same axis");
    let a = app.project.datum_axes.iter().find(|a| a.id == axis).unwrap();
    eprintln!(
        "P2[{kind}] after cnt 5: axis {:?} {:?}; bbox app-open {:?} vs in-memory {ctl_bbox:?}",
        a.origin(),
        a.dir(),
        r3(final_bbox(&app))
    );
    if kind == "vertex_points" {
        // P3: delete the body under the vertex points, then rebuild what is dirty.
        let c = f.c;
        let gone = ctl.project.delete_feature_op(c);
        eprintln!("P3 deleted {gone:?}");
        let (report, _) = qymcad_testkit::regenerate_dirty_with_shapes(&mut ctl.project, std::mem::take(&mut ctl.shapes));
        eprintln!("P3 errors after deleting the body under AtVertex points: {:?}", report.errors);
        eprintln!("P3 points {:?}", ctl.project.datum_points.iter().map(|p| (p.id, p.at)).collect::<Vec<_>>());
        // F-169: no error, the points keep their last place.
        assert!(report.errors.is_empty());
        assert_eq!(ctl.project.datum_points.iter().map(|p| p.at).collect::<Vec<_>>(), vec![[35.0, 5.0, 0.0], [35.0, 5.0, 10.0]]);
    }
}

#[test]
fn p1_a_formula_on_a_manual_axis() {
    scenario("manual_formula");
}
#[test]
fn p2_edge_axis_after_open() {
    scenario("edge");
}
#[test]
fn p2_two_points_axis_after_open() {
    scenario("two_points");
}
#[test]
fn p2_p3_vertex_points_axis_after_open_and_delete() {
    scenario("vertex_points");
}

/// P6: the datum axis under a circular array, deleted without cascade.
#[test]
fn p6_deleting_the_axis_under_a_circular_array() {
    let f = fixture("p6.qcad");
    let (mut s, _) = Session::open(&f.path).unwrap();
    let (arr, _) = s
        .circular_array(
            Some(f.b),
            &"cnt".into(),
            &360.0.into(),
            Some(&AxisRef::Through { origin: [30.0, 0.0, 0.0], dir: [0.0, 0.0, 1.0] }),
            None,
        )
        .unwrap();
    let axis = s.project().datum_axes[0].id;
    let before = s.result_bodies().iter().map(|b| b.bbox).collect::<Vec<_>>();
    let r = s.feature_delete(axis, false);
    eprintln!("P6 delete axis {axis} under array {arr}: {:?}", r.as_ref().map(|_| ()).map_err(|e| e.to_string()));
    eprintln!("P6 bbox before {before:?} after {:?}", s.result_bodies().iter().map(|b| b.bbox).collect::<Vec<_>>());
    eprintln!("P6 axis left: {}", s.project().datum_axes.len());
    assert!(r.unwrap_err().to_string().contains("has dependents"), "the server's closure already refuses");
}

/// P7: where a datum's name lives: the node (`rename` tool, `plane_offset name`) or the pool the tree shows.
#[test]
fn p7_the_name_of_a_datum_plane_node_and_pool() {
    let mut s = Session::new_part();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &5.0.into(), Some("top_z5")).unwrap();
    let node = |s: &Session| s.project().timeline.iter().find(|n| n.id == plane).unwrap().name.clone();
    let pool = |s: &Session| s.project().planes.iter().find(|p| p.id == plane).unwrap().name.clone();
    eprintln!("P7 created with name: node {:?} pool {:?}", node(&s), pool(&s));
    s.rename(RenameTarget::Feature(plane), "renamed").unwrap();
    eprintln!("P7 after rename: node {:?} pool {:?}", node(&s), pool(&s));
    // F-171, fixed on the server side: both stores carry the name.
    assert_eq!((node(&s), pool(&s)), ("renamed".to_string(), "renamed".to_string()));
}

/// P7 (native): the app's own rename writes the pool record and leaves the timeline node's name (`rename_plane`, the tree's
/// rename of a datum point or axis). The tree row reads the pool. This pins why the server writes both (F-171): if the app
/// ever writes the node too, the server's second write is redundant.
#[test]
fn p7_native_the_apps_rename_writes_the_pool_only() {
    let mut s = Session::new_part();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &5.0.into(), None).unwrap();
    let path = scratch("p7-native.qcad");
    s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    let node = |m: &NativeAssemblyModel| m.project.timeline.iter().find(|n| n.id == plane).unwrap().name.clone();
    let before = node(&m);
    assert!(m.project.rename_plane(plane, "app_name".to_string()));
    let pool = m.project.planes.iter().find(|p| p.id == plane).unwrap().name.clone();
    eprintln!("P7 native: node {before:?} -> {:?}, pool {pool:?}", node(&m));
    assert_eq!((node(&m), pool), (before, "app_name".to_string()));
}

// ---------------------------------------------------------------------------------------------------------------
// Review round (package I, round 1): probes of the three defects the server now refuses, through the core API only.

/// A hexagonal prism `n` sides (parameter), with a vertex point on each of the given vertical edges (by position in the list).
fn prism_doc(name: &str) -> (std::path::PathBuf, Vec<u32>) {
    let mut s = Session::new_part();
    s.param_set("n", &6.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let spec = PolygonSpec {
        cx: 0.0.into(),
        cy: 0.0.into(),
        sides: 6,
        r: Some(10.0.into()),
        angle: None,
        vertex: None,
        construction: false,
        dimensioned: true,
    };
    s.sketch_polygon_sides(sk, &spec, &"n".into()).unwrap();
    let body = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap().0;
    let edges: Vec<u32> = s
        .topology(Some(body), false)
        .unwrap()
        .edges
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Line && (e.a[2] - e.b[2]).abs() > 1.0)
        .map(|e| e.id)
        .collect();
    let doomed = *edges.last().unwrap();
    s.datum_point(&PointSpec::Vertex { edge: doomed, end: VertexEnd::End, body: None }, None).unwrap();
    s.datum_axis(&AxisSpec::Edge { edge: doomed, body: None }, None).unwrap();
    let path = scratch(name);
    s.save(Some(&path)).unwrap();
    (path, edges)
}

/// P13 (native, F-176): the edge under a vertex point and an edge axis disappears from a body that stays: no error, and the
/// point and the axis stay where they were.
#[test]
fn p13_native_an_edge_that_vanishes_under_datums_leaves_them_in_place_without_an_error() {
    let (path, _) = prism_doc("p13.qcad");
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    let (point, axis) = (m.project.datum_points[0].clone(), m.project.datum_axes[0].clone());
    native_assembly_parameter(&mut m, "n", "4"); // native_assembly_rebuild panics on any regeneration error
    assert!(m.project.regen_errors.is_empty());
    eprintln!("P13 point {:?} -> {:?}; axis {:?}", point.at, m.project.datum_points[0].at, m.project.datum_axes[0].origin());
    assert_eq!(m.project.datum_points[0].at, point.at, "the point keeps its last place");
    assert_eq!(m.project.datum_axes[0].origin(), axis.origin(), "the axis keeps its last line");
}

/// P14 (native, F-174): a coordinate typed on a vertex point is stored and ignored by the app's rebuild.
#[test]
fn p14_native_a_coordinate_typed_on_a_vertex_point_is_ignored() {
    let (path, _) = prism_doc("p14.qcad");
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    let id = m.project.datum_points[0].id;
    let at = m.project.datum_points[0].at;
    m.project.write_feat_formula(&FeatNumber { node: id, key: "x" }, "123", PlainNumber::Stands).unwrap();
    native_assembly_rebuild(&mut m, true);
    eprintln!("P14 x typed 123: at {at:?} -> {:?}", m.project.datum_points[0].at);
    assert_eq!(m.project.datum_points[0].at, at, "the point stays on its vertex");
}

/// P15 (native, F-175): the app's Part copy remaps a point's id but not its vertex: the copy's point stands on the SOURCE body.
#[test]
fn p15_native_a_part_copy_leaves_a_vertex_point_on_the_source_body() {
    let (path, _) = prism_doc("p15.qcad");
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    let source = m.project.active_component.unwrap();
    let source_body = m.project.datum_points.iter().find_map(|p| match p.def {
        qymcad_core::model::PointDef::AtVertex { body, .. } => Some(body),
        _ => None,
    });
    let target = m.project.root;
    let copy = m.project.clone_component(source, target).expect("copy");
    let points: Vec<_> = m.project.datum_points.iter().map(|p| (p.id, p.def.clone())).collect();
    eprintln!("P15 copy {copy}: points {points:?}, source body {source_body:?}");
    let stands_on: Vec<u64> = points
        .iter()
        .filter_map(|(_, d)| match d {
            qymcad_core::model::PointDef::AtVertex { body, .. } => Some(*body),
            _ => None,
        })
        .collect();
    assert_eq!(stands_on.len(), 2);
    assert_eq!(stands_on[0], stands_on[1], "both points name the same body: the copy's point was not remapped");
    assert_eq!(Some(stands_on[1]), source_body);
}

/// P16 (server path, side note of the review): `sketch_remove` of a curve under an extrude. The reviewer saw "passes, volume
/// unchanged, errors []"; what is observed here: a hole follows (volume grows), the only profile fails and rolls back, and an
/// island of a several-profile feature drops out with `errors: []` (the same silent drop as F-168, through another door: F-177).
#[test]
fn p16_sketch_remove_of_a_curve_under_an_extrude() {
    let vol = |s: &Session| s.result_bodies().iter().map(|b| b.volume).collect::<Vec<_>>();
    // A: a hole circle in a plate: the consumer follows its held outer contour.
    let (mut s, sk, hole, _) = rect_with_hole_doc();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
    assert_close(vol(&s)[0], 937.1681469, 1e-5, "plate with the hole");
    s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, hole)).unwrap();
    assert_close(vol(&s)[0], 1000.0, 1e-6, "A: the hole is gone from the plate");
    assert!(s.current_rebuild().errors.is_empty());
    // B: square + disc island, two profiles: the island drops out silently.
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &(-20.0).into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    let island = s.sketch_circle(sk, &20.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
    assert_close(vol(&s)[0], 892.6990817, 1e-5, "square and disc");
    s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, island)).unwrap();
    assert_close(vol(&s)[0], 500.0, 1e-6, "B: the island dropped out of the feature");
    assert!(s.current_rebuild().errors.is_empty(), "B: and nothing says so (F-177)");
    // C: the only profile: the app errors and the edit rolls back.
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let c = s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
    let e = s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, c)).unwrap_err().to_string();
    assert!(e.contains("profile was not found"), "C: {e}");
    assert_close(vol(&s)[0], 62.8318531, 1e-5, "C: rolled back");
}

fn rect_with_hole_doc() -> (Session, u64, u64, u64) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &10.0.into(), false).unwrap();
    let circle = s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
    (s, sk, circle, 0)
}
