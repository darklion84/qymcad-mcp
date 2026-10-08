use super::*;

#[test]
fn sparse_two_latitude_sphere_mesh_is_rejected_without_ring_boundary_metadata() {
    use qymcad_core::geom::{Mesh, Point3};

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cone_negative_axis_reversed.qcad");
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let body = report.bodies[0].id;
    let _gate = qymcad_kernel::kernel_gate();
    let sphere = Shape::sphere_named(2.0, [9001, 9002, 9003]).unwrap();
    let (_, faces) = sphere.tessellate_merged(0.1).unwrap();
    let mut face = faces[0].clone();
    let axis = unit([1.0, 0.0, 1.0]).unwrap();
    let u = [0.0, 1.0, 0.0];
    let v = cross(axis, u);
    let mut mesh = Mesh::default();
    // For R=2, latitude z has circle radius sqrt(4-z²). Two complete
    // rings at axial heights 0 and 1 therefore fit the same sphere exactly.
    for height in [0.0_f64, 1.0] {
        let radius = (4.0 - height * height).sqrt();
        for i in 0..16 {
            let angle = i as f64 * std::f64::consts::TAU / 16.0;
            let p: [f64; 3] = std::array::from_fn(|j| height * axis[j] + radius * (angle.cos() * u[j] + angle.sin() * v[j]));
            mesh.verts.push(Point3::new(p[0], p[1], p[2]));
        }
    }
    for i in 0..16_u32 {
        let next = (i + 1) % 16;
        mesh.tris.extend([[i, next, 16 + i], [next, 16 + next, 16 + i]]);
    }
    face.triangles = (0..mesh.tris.len() as u32).collect();
    let mi = s.p.mesh_index(body).unwrap();
    s.p.bodies[mi].mesh = mesh;
    s.p.regen_faces.insert(body, vec![face.clone()]);
    s.shapes.insert(body, sphere);
    assert!(s.p.face_sphere(body, &face_key(&face)).is_some(), "two-latitude mesh must admit the upstream sphere fit");
    assert!(s.validated_face_sphere(body, &face).is_none(), "two-latitude support must be rejected even on a native sphere");
}

#[test]
fn two_ring_cone_bevel_is_rejected_even_when_another_native_face_is_spherical() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cone_negative_chamfer.qcad");
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let body = report.bodies[0].id;
    let _gate = qymcad_kernel::kernel_gate();
    let face = s.p.regen_faces[&body]
        .iter()
        .find(|f| {
            f.centroid.z > 5.0
                && s.p.regen_faces[&body].iter().filter(|other| other.id == f.id).count() > 1
                && s.p.face_sphere(body, &face_key(f)).is_some()
        })
        .expect("upper conical bevel must expose the upstream false sphere fit")
        .clone();
    assert!(s.shapes[&body].face_axis(face.id).is_none(), "native axis lookup must miss the split bevel");
    let (center, radius) = s.p.face_sphere(body, &face_key(&face)).unwrap();
    let mesh = &s.p.bodies[s.p.mesh_index(body).unwrap()].mesh;
    let points: Vec<_> = face
        .triangles
        .iter()
        .flat_map(|&ti| mesh.tris[ti as usize])
        .map(|vi| {
            let v = mesh.verts[vi as usize];
            [v.x, v.y, v.z]
        })
        .collect();
    // Mesh vertices have f32 coordinate precision. Both boundary rings lie on
    // their common fitted sphere: radial agreement alone cannot disprove it.
    let scale = points.iter().flatten().chain(center.iter()).map(|v| v.abs()).fold(radius.max(1.0), f64::max);
    let tolerance = 16.0 * f32::EPSILON as f64 * scale;
    assert!(!coplanar_points(&points, tolerance));
    assert!(two_axial_levels(&points, [0.0, 0.0, 1.0], tolerance));
    assert!(points.iter().all(|&p| (norm(sub(p, center)) - radius).abs() <= tolerance), "false fit passes radial validation");

    // The stock spans x=±7.5 mm. A radius-1 sphere centered at x=100 is disjoint;
    // its presence defeats the aggregate zero-spheres shortcut without changing
    // the original bevel's mesh or boundary geometry.
    let sphere = Shape::sphere_named(1.0, [2_000_000_000, 2_000_000_001, 2_000_000_002])
        .unwrap()
        .transformed(&[1.0, 0.0, 0.0, 100.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0])
        .unwrap();
    let aggregate = Shape::fuse_many(&[&s.shapes[&body], &sphere]).unwrap();
    assert!(aggregate.face_kinds().unwrap()[3] > 0, "the aggregate must contain a genuine native sphere");
    s.shapes.insert(body, aggregate);
    assert!(s.p.face_sphere(body, &face_key(&face)).is_some(), "the retained bevel mesh still admits the false sphere");
    assert!(s.validated_face_sphere(body, &face).is_none(), "two coaxial rings cannot establish a spherical surface");
}

#[test]
fn coplanarity_uses_a_rotated_plane_and_rejects_normal_displacement() {
    let origin = [10.0, -20.0, 30.0];
    // u=(2,-1,0), v=(3,6,-5) both satisfy x+2y+3z=0 and are
    // mutually perpendicular. Their combinations define a tilted plane.
    let u = [2.0, -1.0, 0.0];
    let v = [3.0, 6.0, -5.0];
    let mut points: Vec<_> = [(0.0, 0.0), (0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (0.0, 1.0), (-1.0, 2.0)]
        .into_iter()
        .map(|(a, b)| std::array::from_fn(|i| origin[i] + a * u[i] + b * v[i]))
        .collect();
    assert!(coplanar_points(&points, 1e-6), "duplicate/collinear leading points must not hide the plane");
    let normal = unit([1.0, 2.0, 3.0]).unwrap();
    let candidate = point_plane_normal(&points).unwrap();
    assert!((dot(candidate, normal).abs() - 1.0).abs() < 1e-12, "candidate must be a unit normal of the tilted plane");
    assert!(point_plane_normal(&[]).is_none());
    assert!(point_plane_normal(&[origin, origin]).is_none());
    assert!(point_plane_normal(&points[..4]).is_none(), "collinear points cannot define an axis");
    // Move the interior origin point 0.0001 mm along the exact unit normal,
    // well beyond the 0.000001 mm plane-distance tolerance.
    points[1] = std::array::from_fn(|i| origin[i] + 1e-4 * normal[i]);
    assert!(!coplanar_points(&points, 1e-6), "normal displacement must disprove coplanarity");
}

#[test]
fn axial_levels_follow_rotated_axis_and_reject_a_third_circle() {
    let axis = unit([1.0, 0.0, 1.0]).unwrap();
    let u = [0.0, 1.0, 0.0];
    let v = cross(axis, u);
    let origin = [10.0, 20.0, 30.0];
    // Each ring is origin + z*axis + r*(cos(theta)*u+sin(theta)*v).
    // Orthogonality makes its axial projection origin·axis+z, independent of radius.
    let ring = |z: f64, radius: f64| {
        (0..8).map(move |i| {
            let angle = i as f64 * std::f64::consts::TAU / 8.0;
            std::array::from_fn(|j| origin[j] + z * axis[j] + radius * (angle.cos() * u[j] + angle.sin() * v[j]))
        })
    };
    let mut points: Vec<_> = ring(0.0, 2.0).chain(ring(1.0, 3.0)).collect();
    assert!(two_axial_levels(&points, axis, 1e-6));
    points.extend(ring(0.5, 2.5));
    assert!(!two_axial_levels(&points, axis, 1e-6), "third distinct axial circle establishes more than two levels");
}
