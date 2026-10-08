//! Regressions from the real phase-4 shelf; fixture recipes remain editable.
mod common;
use common::*;
use qymcad_engine::*;
use std::path::Path;

fn shelf() -> Session {
    let (s, r) = Session::open(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hanging_shelf.qcad"))).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    s
}

fn assert_floor(s: &mut Session) {
    let t = s.topology(None, true).unwrap();
    // The internal XY cut starts at z=10; native entry clearance removes another .001 mm (F-061).
    // Bottom R4 fillets trim the floor boundary without changing its support plane or outward +Z normal.
    let floor = t.faces.iter().find(|f| (f.centroid[2] - (10.0 - 0.001)).abs() < 1e-5 && f.area > 30_000.0).unwrap();
    assert_eq!(floor.kind, FaceKind::Plane, "the pocket floor must be a plane");
    assert_eq!(floor.radius, None);
    assert_eq!(floor.center, None);
    for (actual, expected) in floor.normal.unwrap().into_iter().zip([0.0, 0.0, 1.0]) {
        assert_close(actual, expected, 1e-6, "floor outward normal");
    }
    let facing = s.select(None, Element::Faces, &Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }).unwrap().1;
    assert!(facing.contains(&floor.id));
    let planes = s.select(None, Element::Faces, &Sel::Kind(SelectionKind::Plane)).unwrap().1;
    assert!(planes.contains(&floor.id), "kind and facing selections agree about the floor");
}

#[test]
fn shelf_floor_is_planar_after_open_and_rebuild() {
    let mut s = shelf();
    assert_floor(&mut s);
    // Change then restore a driving blend parameter, forcing the full downstream geometry to rebuild.
    s.param_set("rim_floor_fillet", &3.5.into()).unwrap();
    let r = s.param_set("rim_floor_fillet", &4.0.into()).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_floor(&mut s);
}

fn assert_shelf_bounds(bounds: [f64; 6]) {
    // Stock is centered 268×170×17. R15 corners, subtractive seats/pocket and rim blends
    // retain extrema on straight outer walls; bottom .6 bevel and top R1 leave wall midsections.
    // Requested .005 mm chord deflection plus f32 rounding at the largest coordinate (134 mm).
    let tolerance = 0.005 + f32::EPSILON as f64 * (268.0 / 2.0);
    for (actual, expected) in bounds.into_iter().zip([-268.0 / 2.0, -170.0 / 2.0, 0.0, 268.0 / 2.0, 170.0 / 2.0, 17.0]) {
        assert_close(actual, expected, tolerance, "tight shelf bbox");
    }
}

#[test]
fn shelf_bounds_are_tight_on_open_rebuild_and_save_round_trip() {
    let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hanging_shelf.qcad"));
    let (mut s, r) = Session::open(path).unwrap();
    assert!(r.errors.is_empty());
    assert_shelf_bounds(r.bodies[0].bbox);
    let b = r.bodies[0].id;
    let original_brep = s.shape(b).unwrap().to_brep_bytes().unwrap();
    // B-rep bytes omit triangles, so byte equality alone cannot detect live remeshing.
    // Independently load the unmeshed reference to detect reporting mutation even during open.
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let reference = qymcad_kernel::Shape::from_brep_bytes(&loaded.breps.into_iter().find(|(id, _)| *id == b).unwrap().1).unwrap();
    let native_bounds = reference.bbox().unwrap();
    assert_eq!(s.shape(b).unwrap().bbox().unwrap(), native_bounds, "opening preserves the native triangulation state");
    assert_shelf_bounds(s.info().bodies[0].bbox);
    assert_shelf_bounds(s.render(View::Top, 128, 128, None).unwrap().bbox);
    assert_eq!(s.shape(b).unwrap().to_brep_bytes().unwrap(), original_brep, "reporting does not edit geometry");
    assert_eq!(s.shape(b).unwrap().bbox().unwrap(), native_bounds, "info/render preserve the native triangulation state");
    let saved = scratch("h1-shelf.qcad");
    s.save(Some(&saved)).unwrap();
    let (mut reopened, r) = Session::open(&saved).unwrap();
    assert!(r.errors.is_empty());
    assert_shelf_bounds(r.bodies[0].bbox);
    reopened.param_set("rim_floor_fillet", &3.5.into()).unwrap();
    let r = reopened.param_set("rim_floor_fillet", &4.0.into()).unwrap();
    assert_shelf_bounds(r.bodies[0].bbox);
    assert_shelf_bounds(reopened.info().bodies[0].bbox);
    assert_shelf_bounds(reopened.render(View::Top, 128, 128, None).unwrap().bbox);
}

/// A minimal upstream fit repro: one stored planar face, no recipe rebuild or native Shape.
/// This test intentionally pins the upstream defect; remove it if QymCAD fixes the fit.
#[test]
fn upstream_sphere_fit_accepts_a_single_coplanar_floor_mesh() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hanging_shelf.qcad");
    let mut p = qymcad_io::load_project(path).unwrap();
    let body = 123;
    let bi = p.mesh_index(body).unwrap();
    let face = p.bodies[bi].faces.iter().find(|f| f.id == 1073741842).unwrap().clone();
    p.regen_faces.insert(body, vec![face.clone()]);
    let key = qymcad_core::feature::FaceKey {
        index: 0,
        id: face.id,
        centroid: [face.centroid.x, face.centroid.y, face.centroid.z],
        normal: face.normal,
    };
    let mesh = &p.bodies[bi].mesh;
    // Every vertex used by this face belongs to plane z=10−entry_clearance. A plane's
    // section by a sphere is a circle, so these noncircular rounded-pocket vertices cannot
    // all lie on a sphere; check the candidate's actual radial residual below.
    let points: Vec<_> = face.triangles.iter().flat_map(|&ti| mesh.tris[ti as usize]).map(|vi| &mesh.verts[vi as usize]).collect();
    for v in &points {
        assert_close(v.z, 10.0 - 0.001, f32::EPSILON as f64 * 10.0, "coplanar floor vertex");
    }
    let (center, radius) = p.face_sphere(body, &key).expect("pinned upstream accepts this planar mesh as a sphere");
    let scale = points.iter().flat_map(|v| [v.x.abs(), v.y.abs(), v.z.abs()]).chain(center.map(f64::abs)).fold(radius.max(1.0), f64::max);
    let residual = points
        .iter()
        .map(|v| {
            let distance = ((v.x - center[0]).powi(2) + (v.y - center[1]).powi(2) + (v.z - center[2]).powi(2)).sqrt();
            (distance - radius).abs()
        })
        .fold(0.0_f64, f64::max);
    assert!(residual > 16.0 * f32::EPSILON as f64 * scale, "fitted radius contradicts the face's own vertices");
}
