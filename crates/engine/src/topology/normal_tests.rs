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
        assert!(corner_planar_normal(shape, face, mesh).is_none(), "native cylinder/cone must not receive a plane normal");
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
