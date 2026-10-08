use super::*;
use qymcad_core::model::Body;

#[test]
fn straight_polyline_detection_uses_relative_collinearity() {
    assert!(polyline_is_straight(&[[1.0, 2.0, 3.0], [3.0, 4.0, 5.0]]), "two distinct native points define a line");
    assert!(polyline_is_straight(&[[1.0, 2.0, 3.0], [2.0, 3.0, 4.0], [3.0, 4.0, 5.0]]), "subdivided oblique line");
    for length in [1e-3_f32, 1.0, 1e6] {
        let near = [[0.0, 0.0, 0.0], [length / 2.0, length * 5e-10, 0.0], [length, 0.0, 0.0]];
        let curved = [[0.0, 0.0, 0.0], [length / 2.0, length * 2e-9, 0.0], [length, 0.0, 0.0]];
        assert!(polyline_is_straight(&near), "within relative collinearity at length {length}");
        assert!(!polyline_is_straight(&curved), "curved beyond relative collinearity at length {length}");
    }
    assert!(!polyline_is_straight(&[]));
    assert!(!polyline_is_straight(&[[1.0; 3]]));
    assert!(!polyline_is_straight(&[[1.0; 3], [1.0; 3]]));
    assert!(!polyline_is_straight(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0; 3]]), "closed curve is not a line");
}

fn bilinear_patch(f: impl Fn(f64, f64) -> [f64; 3]) -> [[[f64; 3]; 4]; 4] {
    std::array::from_fn(|i| std::array::from_fn(|j| f(i as f64 / 3.0, j as f64 / 3.0)))
}

fn curved_patch(f: impl Fn(f64, f64, f64) -> [f64; 3]) -> [[[f64; 3]; 4]; 4] {
    // Cubic Bezier controls for q(y)=0.1*(y²-1), y=-1+2*v.
    let height = [0.0, -2.0 / 15.0, -2.0 / 15.0, 0.0];
    std::array::from_fn(|i| std::array::from_fn(|j| f(i as f64 / 3.0, j as f64 / 3.0, height[j])))
}

#[test]
fn native_spline_crease_with_changing_sign_is_omitted() {
    // With q(y)=0.1*(y²-1), the top is z=q(y) on -1<=x<=0, and
    // z=q(y)+40*x*(y-1/4) on 0<=x<=1, over -1<=y<=1, closed at z=-60.
    // The crease x=0 changes sign at y=1/4, away from its midpoint. Its gentle
    // curvature forces the native mesher to resolve several adjacent triangles;
    // a straight seam can remain one chord despite varying surface derivatives.
    let patches = [
        curved_patch(|u, v, q| [-1.0 + u, -1.0 + 2.0 * v, q]),
        curved_patch(|u, v, q| [u, -1.0 + 2.0 * v, q + 40.0 * u * (-1.25 + 2.0 * v)]),
        bilinear_patch(|u, v| [-1.0 + 2.0 * v, -1.0 + 2.0 * u, -60.0]),
        curved_patch(|u, v, q| [-1.0, -1.0 + 2.0 * v, -60.0 + (60.0 + q) * u]),
        curved_patch(|u, v, q| [1.0, -1.0 + 2.0 * v, q - 50.0 + 80.0 * v - u * (10.0 + q + 80.0 * v)]),
        bilinear_patch(|u, v| [-1.0 + u, -1.0, -60.0 + 60.0 * v]),
        bilinear_patch(|u, v| [u, -1.0, -60.0 + v * (60.0 - 50.0 * u)]),
        bilinear_patch(|u, v| [-1.0 + v, 1.0, -60.0 + 60.0 * u]),
        bilinear_patch(|u, v| [v, 1.0, -60.0 + u * (60.0 + 30.0 * v)]),
    ];
    let (shape, mesh, faces, crease) = {
        let _gate = qymcad_kernel::kernel_gate();
        let (shape, free) = Shape::from_bezier_patches(&patches, 1e-6, true).unwrap();
        assert_eq!(free, 0, "all patches close into a shell");
        assert!(shape.is_valid(), "changing-sign crease belongs to a valid native B-rep");
        assert_eq!(shape.solid_count(), 1);
        // Integral of 60 over the 2*2 footprint is 240. The right patch adds
        // integral[0,1] integral[-1,1] 40*x*(y-1/4) dy dx = -10.
        // q(y) contributes 2*integral[-1,1] 0.1*(y²-1) dy = -4/15.
        assert!((shape.volume() - (230.0 - 4.0 / 15.0)).abs() < 1e-8, "analytic solid volume: {}", shape.volume());
        let (lines, ids) = shape.edges_with_ids();
        let matching: Vec<_> = lines
            .iter()
            .zip(ids)
            .filter(|(line, _)| {
                line.iter().all(|p| p[0].abs() < 1e-6 && (f64::from(p[2]) - 0.1 * (f64::from(p[1]).powi(2) - 1.0)).abs() < 1e-6)
                    && line.first().zip(line.last()).is_some_and(|(a, b)| (a[1] - b[1]).abs() > 1.9)
            })
            .map(|(_, id)| id)
            .collect();
        assert_eq!(matching.len(), 1, "one continuous changing-sign crease");
        let crease = matching[0];
        assert!(shape.edges_info().iter().any(|e| e.id == crease && !e.smooth), "native midpoint smoothness does not omit it");
        let (mesh, faces) = shape.tessellate_merged(0.001).unwrap();
        (shape, mesh, faces, crease)
    };
    let body = 42;
    let mut s = Session::new_part();
    s.p.bodies.push(Body { id: body, mesh, faces: faces.clone(), ..Default::default() });
    s.p.regen_faces.insert(body, faces);
    s.shapes.insert(body, shape);
    s.restore_edges().unwrap();
    {
        let _gate = qymcad_kernel::kernel_gate();
        let (_, a, b) = s.shapes[&body].edge_face_pairs().into_iter().find(|p| p.0 == crease).unwrap();
        let e = s.p.regen_edges[&body].iter().find(|e| e.id == crease).unwrap();
        let a = s.p.regen_faces[&body].iter().find(|f| f.id == a).unwrap();
        let b = s.p.regen_faces[&body].iter().find(|f| f.id == b).unwrap();
        let mesh = &s.p.bodies[0].mesh;
        let edge = s.shapes[&body].edges_info().into_iter().find(|e| e.id == crease).unwrap();
        let mut signs = Vec::new();
        for fraction in [0.1, 0.3, 0.5, 0.7, 0.9] {
            let target = polyline_point(&edge.poly, fraction).unwrap();
            let (point, tangent, na, nb) = edge_face_sample(e, a, b, mesh, target).unwrap();
            let na = local_normal(&s.shapes[&body], a.id, point, na);
            let nb = local_normal(&s.shapes[&body], b.id, point, nb);
            signs.push(
                robust_corner_sign(na, nb, tangent, angular_error(MESH_ANGLE)).expect("every fixture sample clears the uncertainty margin"),
            );
        }
        assert!(signs.contains(&true) && signs.contains(&false), "the native mesh resolves both corner signs: {signs:?}");
    }
    for concave in [false, true] {
        let corners = s.corner_edges(body, concave);
        assert!(!corners.contains(&crease), "changing-sign crease {crease} must be omitted for concave={concave}: {corners:?}");
    }
}
