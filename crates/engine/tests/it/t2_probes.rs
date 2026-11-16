//! T2 native probe: what QymCAD itself does with a Stitch whose tolerance is wider than the gap between two sheets (F-198).
//! Core API only (the app's adders and regeneration), none of the server's guards. It pins the observation: when the app
//! refuses or builds something right, it goes red and the server refusal of `stitch` (`stitch_refusals`) can be dropped.
use qymcad_core::geom::Point2;
use qymcad_core::model::Project;
use qymcad_core::refs::{Fingerprint, Ref};

/// A 60 x 40 x 12 plate, its top copied as a sheet and the top lifted by `gap` as another; both stitched with `tol`.
/// Returns the stitch's build errors, whether it is a sheet, its kernel volume and the volume of its mesh.
fn stitched(gap: f64, tol: f64) -> (Vec<String>, bool, f64, f64) {
    let mut p = Project::default();
    p.new_document();
    let sid = p.add_line_sketch(
        "Sketch 1",
        vec![Point2::new(0.0, 0.0), Point2::new(60.0, 0.0), Point2::new(60.0, 40.0), Point2::new(0.0, 40.0)],
        true,
    );
    let si = p.sketch_index(sid).unwrap();
    p.regen_sketch(si);
    if let Some(o) = p.sketch_owner(sid) {
        p.set_active_component(Some(o));
    }
    p.add_sketch_node(sid, "Sketch 1");
    let closed: Vec<u64> = p.sketches[si].contour_ids.iter().copied().filter(|c| p.contour_profile_xy(*c).is_some()).collect();
    let body = p.add_extrude_multi(sid, closed, 12.0, qymcad_core::feature::Reach::Forward, 0.0, vec![]);
    qymcad_testkit::regenerate(&mut p);
    let top = p.regen_faces[&body].iter().find(|f| f.normal[2] > 0.9).map(|f| f.id).unwrap();
    let a = p.add_face_copy(body, Ref::one(top, Fingerprint::default()));
    let b = p.add_offset_surface(body, Ref::picks(&[top]), gap);
    qymcad_testkit::regenerate(&mut p);
    let one = p.add_stitch(vec![a, b], tol);
    let (report, shapes) = qymcad_testkit::regenerate(&mut p);
    let kernel = shapes.get(&one).map_or(f64::NAN, |s| s.volume());
    let built = p.bodies.iter().find(|x| x.id == one).unwrap();
    (report.errors.iter().map(|(_, e)| e.to_string()).collect(), built.sheet, kernel, built.mesh.volume())
}

/// F-198: two parallel sheets 5 apart. At tol 4.9 the app says "nothing to stitch". At tol 5.1 the node is GREEN and a SOLID, the
/// kernel says its volume is 4000 mm3 while its mesh encloses nothing: a closed shell of two faces that is no body.
#[test]
fn the_app_sews_parallel_sheets_into_a_solid_that_encloses_nothing_f198() {
    let (errors, sheet, ..) = stitched(5.0, 4.9);
    assert!(errors.iter().any(|e| e.contains("nothing to stitch")), "{errors:?}");
    assert!(sheet);
    let (errors, sheet, kernel, mesh) = stitched(5.0, 5.1);
    assert!(errors.is_empty(), "the app builds it: {errors:?}");
    assert!(!sheet, "and calls it a solid");
    assert!((kernel - 4000.0).abs() < 1.0, "kernel volume {kernel}");
    assert!(mesh.abs() < 1e-6, "mesh volume {mesh}");
}
