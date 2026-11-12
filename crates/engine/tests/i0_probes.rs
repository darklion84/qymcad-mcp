//! Package I phase 0: probes of what the app does around datum points, datum axes and the construction toggle.
//! Each probe prints what it observed (run with --nocapture); assertions pin the observation, so a changed app shows here.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

fn rect_with_hole() -> (Session, u64, u64, u64) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &10.0.into(), false).unwrap();
    let circle = s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
    (s, sk, circle, 0)
}

/// P11: a circle inside a rectangle turned construction. The consumer extrudes the rectangle; the hole silently disappears?
#[test]
fn p11_a_hole_turned_construction_changes_the_consumer_without_an_error() {
    let (mut s, sk, circle, _) = rect_with_hole();
    let info = s.sketch_info(sk).unwrap();
    eprintln!("P11 contours before: {:?}", info.contours);
    let (_body, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
    eprintln!("P11 volume with hole: {} errors {:?}", r.bodies[0].volume, r.errors);
    let done = s.sketch_edit(sk, |s| s.sketch_construction(sk, &[circle], true));
    eprintln!("P11 toggle result: {:?}", done.as_ref().map(|d| &d.0.entities).map_err(|e| e.to_string()));
    // Natively (see git history of this probe before the guard) the volume went 937.17 -> 1000 with errors []: refused now.
    assert!(done.unwrap_err().to_string().contains("F-168"));
    eprintln!("P11 after: contours {:?}", s.sketch_info(sk).unwrap().contours);
    eprintln!("P11 volume after: {:?}", s.result_bodies().iter().map(|b| b.volume).collect::<Vec<_>>());
    eprintln!("P11 errors {:?}", s.current_rebuild().errors);
}

/// P11b: the only profile a consumer uses turned construction: the contour is gone.
#[test]
fn p11b_the_consumers_only_contour_turned_construction() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let circle = s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
    let done = s.sketch_edit(sk, |s| s.sketch_construction(sk, &[circle], true));
    eprintln!("P11b toggle: {:?}", done.as_ref().map(|d| &d.0.entities).map_err(|e| e.to_string()));
    assert!(done.unwrap_err().to_string().contains("profile was not found"));
    eprintln!("P11b bodies after: {:?}", s.result_bodies().iter().map(|b| b.volume).collect::<Vec<_>>());
    eprintln!("P11b errors {:?}", s.current_rebuild().errors);
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
