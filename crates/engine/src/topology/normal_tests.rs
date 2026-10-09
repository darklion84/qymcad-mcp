use super::*;

#[test]
fn narrow_native_cylinders_and_cones_keep_their_curved_normal_path() {
    use crate::{AxisRef, BaseName, Direction, Op, PlaneRef, PolylineSpec, Revolve, Xy};

    // A 0.1-degree cylindrical/conical sector passes the mesh-normal planarity heuristic.
    // Native analytic type must still take precedence, as it does in topology().
    for top_radius in [2.0, 1.5] {
        let mut s = Session::new_part();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XZ), None).unwrap();
        s.sketch_polyline(
            sk,
            &PolylineSpec {
                points: [(0.0, 0.0), (2.0, 0.0), (top_radius, 2.0), (0.0, 2.0)].into_iter().map(|(x, z)| Xy(x.into(), z.into())).collect(),
                closed: true,
                construction: false,
                dimensioned: false,
            },
        )
        .unwrap();
        let (body, report) = s
            .revolve(&Revolve {
                sketch: sk,
                profiles: None,
                axis: AxisRef::SketchY,
                angle: 0.1.into(),
                direction: Direction::Normal,
                op: Op::Add,
                target: None,
                name: None,
            })
            .unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        // Frustum volume times sector fraction; cylinder is the R=r case.
        let expected = std::f64::consts::PI * 2.0 * (4.0 + 2.0 * top_radius + top_radius * top_radius) / 3.0 * 0.1 / 360.0;
        assert!((s.result_bodies()[0].volume - expected).abs() < 1e-8);
        let shape = &s.shapes[&body];
        let mesh = &s.p.bodies[s.p.mesh_index(body).unwrap()].mesh;
        let _gate = qymcad_kernel::kernel_gate();
        let curved: Vec<_> = s.p.regen_faces[&body].iter().filter(|f| shape.face_axis(f.id).is_some()).collect();
        assert_eq!(curved.len(), 1);
        let face = curved[0];
        assert!(planar_normal(face, mesh).is_some(), "fixture must appear planar to the mesh heuristic");
        assert!(mesh_corner_planar_normal(shape, face, mesh).is_none(), "native cylinder/cone must not receive a plane normal");
    }
}

#[test]
fn cylinder_normal_corrects_a_facet_that_reverses_a_shallow_sign() {
    let _gate = qymcad_kernel::kernel_gate();
    let shape = Shape::cylinder_named(1.0, 2.0, [1, 2, 3]).unwrap();
    // At x=1 the exact outward normal is +X. A facet rotated 8 degrees (within F-021's
    // 0.3 rad allowance) reverses its dot with an inward vector 4 degrees from tangency.
    let error = 8.0_f64.to_radians();
    let facet = [error.cos(), error.sin(), 0.0];
    let angle = 4.0_f64.to_radians();
    let inward = [angle.sin(), -angle.cos(), 0.0];
    assert!(dot(inward, facet) < 0.0, "facet gives the wrong convex sign");
    let exact = local_normal(&shape, 3, [1.0, 0.0, 1.0], facet);
    assert!(dot(inward, exact.0) > 0.0, "analytic cylinder normal must recover the concave sign");
    assert_eq!(exact.0, [1.0, 0.0, 0.0]);
    let na = ([angle.cos(), angle.sin(), 0.0], 1e-6);
    assert_eq!(robust_corner_sign(na, exact, [0.0, 0.0, 1.0], 1e-6), Some(true), "analytic normal makes the shallow sign robust");
    let bore = local_normal(&shape, 3, [1.0, 0.0, 1.0], facet.map(|x| -x));
    assert_eq!(bore.0, [-1.0, 0.0, 0.0], "winding reverses the analytic radial on bores");
}

#[test]
fn nearly_perpendicular_cylinder_orientation_preserves_the_mesh_normal() {
    let _gate = qymcad_kernel::kernel_gate();
    let shape = Shape::cylinder_named(1.0, 2.0, [1, 2, 3]).unwrap();
    for x in [-1e-12, 0.0, 1e-12] {
        let facet = [x, 1.0, 0.0];
        assert_eq!(local_normal(&shape, 3, [1.0, 0.0, 1.0], facet).1, angular_error(MESH_ANGLE), "fallback retains mesh uncertainty");
        assert_eq!(local_normal(&shape, 3, [1.0, 0.0, 1.0], facet).0, facet, "unreliable winding alignment must preserve the mesh normal");
    }
}

#[test]
fn shallow_wrong_facet_sign_is_omitted_by_the_uncertainty_margin() {
    // At a 3-degree spotface the true outward cone normal has +X component,
    // giving convex with inward=-X. A facet 8 degrees across tangency has -X
    // component, falsely giving concave. Both are within the F-021 allowance.
    let true_angle = 3.0_f64.to_radians();
    let facet_angle = (-5.0_f64).to_radians();
    let na = ([0.0, 0.0, 1.0], 1e-6);
    let tangent = [0.0, 1.0, 0.0];
    let true_nb = ([true_angle.sin(), 0.0, true_angle.cos()], 1e-6);
    let facet = ([facet_angle.sin(), 0.0, facet_angle.cos()], angular_error(MESH_ANGLE));
    assert_eq!(robust_corner_sign(na, true_nb, tangent, 1e-6), Some(false), "true corner is convex");
    assert!(dot(cross(na.0, tangent), facet.0) > 0.0, "unbounded facet sign is wrongly concave");
    assert_eq!(robust_corner_sign(na, facet, tangent, 1e-6), None, "wrong shallow facet sign must be omitted");
}

#[test]
fn single_triangle_native_sphere_must_not_receive_a_plane_normal() {
    let _gate = qymcad_kernel::kernel_gate();
    let shape = Shape::sphere_named(2.0, [1, 2, 3]).unwrap();
    assert!((shape.volume() - 4.0 * std::f64::consts::PI * 8.0 / 3.0).abs() < 1e-8);
    let (mesh, faces) = shape.tessellate_merged(0.05).unwrap();
    let mut face = faces[0].clone();
    face.triangles.truncate(1);
    assert!(planar_normal(&face, &mesh).is_some(), "one curved facet appears planar");
    assert!(
        mesh_corner_planar_normal(&shape, &face, &mesh).is_none(),
        "native sphere must not receive a plane normal even with one triangle"
    );
}

#[test]
fn native_cone_meridian_normals_match_formula_and_outward_winding() {
    let _gate = qymcad_kernel::kernel_gate();
    for slope_deg in [0.9_f64, 3.0, 5.0, 10.0, 20.0, 45.0] {
        let angle = slope_deg.to_radians();
        let height = 5.0 * angle.tan();
        let shape = Shape::cone_named(10.0, 5.0, height, [1, 2, 3]).unwrap();
        let expected_volume = std::f64::consts::PI * height * (100.0 + 50.0 + 25.0) / 3.0;
        assert!((shape.volume() - expected_volume).abs() < 1e-8);
        let (mesh, faces) = shape.tessellate_merged(0.05).unwrap();
        let face = faces.iter().find(|f| shape.face_axis(f.id).is_some()).unwrap();
        let (origin, axis) = shape.face_axis(face.id).unwrap();
        let slope = cone_slope(face, &mesh, origin, axis).expect("native cone vertices determine its generator");
        assert!(
            (slope + 5.0 / height).abs() < 1e-7 * (5.0 / height).max(1.0),
            "dr/dz follows formula: slope={slope}, expected={}, axis={axis:?}",
            -5.0 / height
        );
        for azimuth in [0.0_f64, 0.7, 2.0, 4.0] {
            let point = [7.5 * azimuth.cos(), 7.5 * azimuth.sin(), height / 2.0];
            let expected = [angle.sin() * azimuth.cos(), angle.sin() * azimuth.sin(), angle.cos()];
            // A facet eight degrees away still orients the reconstructed normal correctly.
            let facet_angle = angle + 8.0_f64.to_radians();
            let facet = [facet_angle.sin() * azimuth.cos(), facet_angle.sin() * azimuth.sin(), facet_angle.cos()];
            let normal = cone_normal(origin, axis, slope, point, facet).unwrap();
            assert!(norm(sub(normal.0, expected)) < 1e-7, "cone normal equals analytic formula at {slope_deg} degrees");
            assert_eq!(normal.1, angular_error(1.0_f64.to_radians()));
            let inward = cone_normal(origin, axis, slope, point, facet.map(|x| -x)).unwrap();
            assert!(norm(sub(inward.0, expected.map(|x| -x))) < 1e-7, "bore winding reverses analytic cone normal");
            let reversed_axis = cone_normal(origin, axis.map(|x| -x), -slope, point, facet).unwrap();
            assert!(norm(sub(reversed_axis.0, expected)) < 1e-7, "axis sense does not change the normal");
        }
    }
}

#[test]
fn analytic_circle_tangent_matches_formula_at_chord_points() {
    let mut edge = MeshEdge { radius: 7.0, center: [2.0, 3.0, 4.0], axis: [0.0, 0.0, 1.0], ..Default::default() };
    for theta in [0.0_f64, 0.3, 1.9, 4.5] {
        // Midpoint of a symmetric circle chord lies radially inward by cos(half sweep).
        let radius = 7.0 * 0.15_f64.cos();
        let point = [2.0 + radius * theta.cos(), 3.0 + radius * theta.sin(), 4.0];
        let expected = [-theta.sin(), theta.cos(), 0.0];
        assert!(norm(sub(circle_tangent(&edge, point).unwrap(), expected)) < 1e-12);
        edge.axis = [0.0, 0.0, -1.0];
        assert!(norm(sub(circle_tangent(&edge, point).unwrap(), expected.map(|x| -x))) < 1e-12);
        edge.axis = [0.0, 0.0, 1.0];
    }
}

#[test]
fn sparse_mesh_only_planes_keep_uncertainty_that_prevents_shallow_wrong_signs() {
    let one = mesh_plane_allowance(1, false);
    let two = mesh_plane_allowance(2, false);
    let many = mesh_plane_allowance(100, false);
    assert!(one > two && two > many && many > 1e-6);
    let facet_angle = (-9.0_f64).to_radians();
    let facet = ([facet_angle.sin(), 0.0, facet_angle.cos()], one);
    assert_eq!(robust_corner_sign(([0.0, 0.0, 1.0], 1e-6), facet, [0.0, 1.0, 0.0], 1e-6), None);
    assert_eq!(mesh_plane_allowance(1, true), 1e-6, "native plane proof preserves ordinary planar corners");
}

#[test]
fn cone_meridian_fit_at_large_coordinates_accepts_f32_rounding_but_rejects_noncone_deviation() {
    use qymcad_core::geom::{Mesh, Point3};

    let origin = [1000.0; 3];
    let axis = [0.0, 0.0, 1.0];
    // A true cone generator r(z)=1+0.37z, translated to (1000,1000,1000), then rounded
    // exactly like native f32 mesh coordinates. f32 spacing here is 2^(9-23) mm.
    let mut mesh = Mesh {
        verts: [0.0, 0.25, 0.5, 0.75, 1.0]
            .into_iter()
            .map(|z| Point3::new((1001.0 + 0.37 * z) as f32 as f64, 1000.0, (1000.0 + z) as f32 as f64))
            .collect(),
        tris: vec![[0, 1, 2], [2, 3, 4]],
    };
    let face = MeshFace { triangles: vec![0, 1], normal: [0.0; 3], centroid: Point3::new(1000.0, 1000.0, 1000.0), area: 0.0, id: 1 };
    let slope = cone_slope(&face, &mesh, origin, axis).expect("true translated cone survives native coordinate rounding");
    // Endpoint rounding contributes at most one f32 spacing over this 1 mm span.
    assert!((slope - 0.37).abs() <= 2.0_f64.powi(9 - 23), "formula slope survives f32 endpoints: {slope}");
    // 0.01 mm is nearly ten times the 1e-6*1001.37 mm coordinate-scaled fit band.
    mesh.verts[2].x += 0.01;
    assert!(cone_slope(&face, &mesh, origin, axis).is_none(), "deviation above the fit band must not acquire analytic cone normals");
}
