//! Curved signed corners and a boss-base fillet checked against analytic geometry.

mod common;
use common::*;
use qymcad_engine::*;
use std::f64::consts::PI;

const W: f64 = 60.0;
const L: f64 = 50.0;

fn n(expr: &str) -> Num {
    Num::Expr(expr.into())
}

fn extrude(s: &mut Session, sketch: Id, height: Num, op: Op, direction: Direction, name: &str) {
    let (_, report) = s
        .extrude(&Extrude { sketch, profiles: None, height, op, direction, through: false, target: None, name: Some(name.into()) })
        .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
}

fn plate() -> Session {
    let mut s = Session::new_part();
    for (name, value) in [("t", 6.0), ("d", 20.0), ("h", 10.0), ("r", 2.0)] {
        s.param_set(name, &value.into()).unwrap();
    }
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("plate sketch")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &W.into(), &L.into(), false).unwrap();
    extrude(&mut s, sk, n("t"), Op::Add, Direction::Normal, "plate");
    s
}

fn top_circle(s: &mut Session, diameter: Num, depth: Num, op: Op, name: &str) {
    let (plane, report) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &n("t"), Some(name)).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let sk = s.sketch_create(&PlaneRef::Plane(plane), Some(name)).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &diameter, false).unwrap();
    extrude(s, sk, depth, op, if matches!(op, Op::Cut) { Direction::Reverse } else { Direction::Normal }, name);
}

fn boss_plate() -> Session {
    let mut s = plate();
    top_circle(&mut s, n("d"), n("h"), Op::Add, "boss");
    s
}

fn volume(s: &Session) -> f64 {
    let bodies = s.result_bodies();
    assert_eq!(bodies.len(), 1);
    bodies[0].volume
}

fn corners(s: &mut Session, selection: &Sel) -> Vec<u32> {
    s.select(None, Element::Edges, selection).unwrap().1
}

fn circle_at(topo: &Topology, radius: f64, z: f64) -> u32 {
    let edges: Vec<_> = topo
        .edges
        .iter()
        .filter(|e| {
            e.kind == EdgeKind::Circle
                && e.radius.is_some_and(|r| (r - radius).abs() < 1e-6)
                && e.center.is_some_and(|c| c[0].abs() < 1e-6 && c[1].abs() < 1e-6 && (c[2] - z).abs() < 1e-6)
        })
        .collect();
    assert_eq!(edges.len(), 1, "exactly one circle R={radius} at z={z}: {edges:?}");
    assert_close(edges[0].length, 2.0 * PI * radius, 1e-6, "analytic circumference");
    edges[0].id
}

#[test]
fn boss_base_is_concave_and_top_rim_is_convex() {
    let mut s = boss_plate();
    assert_close(volume(&s), W * L * 6.0 + PI * 10.0_f64.powi(2) * 10.0, 1e-3, "plate plus boss");
    let topo = s.topology(None, true).unwrap();
    let base = circle_at(&topo, 10.0, 6.0);
    let top = circle_at(&topo, 10.0, 6.0 + 10.0);
    assert_eq!(corners(&mut s, &Sel::Concave), vec![base], "boss/plate circle is the sole concave corner");
    let convex = corners(&mut s, &Sel::Convex);
    assert!(convex.contains(&top), "boss top rim is convex: {convex:?}");
    assert!(!convex.contains(&base), "concave base must not also be convex");
    assert_eq!(convex.len(), 12 + 1, "twelve plate edges and one boss top circle");
    assert!(topo.edges.iter().filter(|e| e.seam).all(|e| !convex.contains(&e.id)), "cylinder seam is excluded");
}

#[test]
fn both_through_hole_rims_are_convex() {
    let mut s = plate();
    top_circle(&mut s, 20.0.into(), n("t"), Op::Cut, "through hole");
    assert_close(volume(&s), W * L * 6.0 - PI * 10.0_f64.powi(2) * 6.0, 1e-3, "plate minus through hole");
    let topo = s.topology(None, true).unwrap();
    let rims = [circle_at(&topo, 10.0, 0.0), circle_at(&topo, 10.0, 6.0)];
    let convex = corners(&mut s, &Sel::Convex);
    assert!(rims.iter().all(|id| convex.contains(id)), "both hole rims are convex: {rims:?}, got {convex:?}");
    assert_eq!(convex.len(), 12 + 2, "twelve plate edges and both hole rims");
    assert!(corners(&mut s, &Sel::Concave).is_empty());
    assert!(topo.edges.iter().filter(|e| e.seam).all(|e| !convex.contains(&e.id)), "hole seam is excluded");
}

#[test]
fn counterbore_floor_to_wall_circle_is_concave() {
    let mut s = plate();
    top_circle(&mut s, 10.0.into(), n("t"), Op::Cut, "through bore");
    top_circle(&mut s, 20.0.into(), 3.0.into(), Op::Cut, "counterbore");
    let expected = W * L * 6.0 - PI * 5.0_f64.powi(2) * 6.0 - PI * (10.0_f64.powi(2) - 5.0_f64.powi(2)) * 3.0;
    assert_close(volume(&s), expected, 1e-3, "plate minus through bore and annular counterbore");
    let topo = s.topology(None, true).unwrap();
    let recessed = circle_at(&topo, 10.0, 6.0 - 3.0);
    let shoulder_rim = circle_at(&topo, 5.0, 6.0 - 3.0);
    assert_eq!(corners(&mut s, &Sel::Concave), vec![recessed], "recessed floor-to-counterbore-wall circle is concave");
    // At the small-radius shoulder the material occupies a 90-degree quadrant; the large-radius floor
    // junction has 270 degrees of material. This distinguishes the two circles on the same step face.
    let convex = corners(&mut s, &Sel::Convex);
    assert!(convex.contains(&shoulder_rim), "small-radius shoulder is convex");
    assert_eq!(convex.len(), 12 + 3, "plate edges, two opening rims and the small-radius shoulder");
}

#[test]
fn junction_of_two_curved_cylinder_walls_has_the_material_corner_sign() {
    let radius = 10.0_f64;
    let distance = 10.0_f64;
    let height = 10.0;
    // Two equal discs at separation c overlap by 2R²*acos(c/2R)-c*sqrt(4R²-c²)/2.
    let overlap =
        2.0 * radius.powi(2) * (distance / (2.0 * radius)).acos() - distance * (4.0 * radius.powi(2) - distance.powi(2)).sqrt() / 2.0;
    for (op, expected, concave) in
        [(Op::Add, (2.0 * PI * radius.powi(2) - overlap) * height, true), (Op::Intersect, overlap * height, false)]
    {
        let mut s = Session::new_part();
        for (x, operation) in [(0.0, Op::Add), (distance, op)] {
            let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
            s.sketch_circle(sk, &x.into(), &0.0.into(), &(2.0 * radius).into(), false).unwrap();
            extrude(&mut s, sk, height.into(), operation, Direction::Normal, "cylinder");
        }
        assert_close(volume(&s), expected, 1e-3, "union/intersection of two cylinders");
        let topo = s.topology(None, true).unwrap();
        let corners = corners(&mut s, if concave { &Sel::Concave } else { &Sel::Convex });
        let joins: Vec<_> = topo
            .edges
            .iter()
            .filter(|e| {
                e.kind == EdgeKind::Line
                    && !e.seam
                    && e.faces.is_some_and(|ids| {
                        ids[0] != ids[1]
                            && ids.iter().all(|id| topo.faces.iter().any(|face| face.id == *id && face.kind == FaceKind::Cylinder))
                    })
            })
            .collect();
        assert_eq!(joins.len(), 2, "two crossings of the circular profiles");
        for edge in joins {
            assert_close(edge.mid[0], distance / 2.0, 1e-6, "profile crossing x");
            assert_close(edge.mid[1].abs(), (radius.powi(2) - (distance / 2.0).powi(2)).sqrt(), 1e-6, "profile crossing |y|");
            assert_close(edge.length, height, 1e-6, "wall junction length");
            assert!(corners.contains(&edge.id), "two-cylinder {op:?} corner has concave={concave}: {corners:?}");
        }
    }
}

#[test]
fn elliptic_cylinder_to_oblique_plane_rim_is_convex() {
    let mut s = Session::new_part();
    let cylinder = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(cylinder, &0.0.into(), &0.0.into(), &20.0.into(), false).unwrap();
    extrude(&mut s, cylinder, 20.0.into(), Op::Add, Direction::Normal, "cylinder");
    let wedge = s.sketch_create(&PlaneRef::Base(BaseName::XZ), None).unwrap();
    s.sketch_polyline(
        wedge,
        &PolylineSpec {
            points: [(-15.0, -1.0), (15.0, -1.0), (15.0, 15.0), (-15.0, 5.0)].into_iter().map(|(x, z)| Xy(x.into(), z.into())).collect(),
            closed: true,
            construction: false,
            dimensioned: false,
        },
    )
    .unwrap();
    extrude(&mut s, wedge, 40.0.into(), Op::Intersect, Direction::Symmetric, "oblique top");
    // Top is z=10+x/3. Integrating height over the symmetric R=10 disc cancels the odd x term.
    assert_close(volume(&s), PI * 10.0_f64.powi(2) * 10.0, 1e-3, "cylinder below oblique plane");
    let topo = s.topology(None, true).unwrap();
    let elliptic: Vec<_> = topo.edges.iter().filter(|e| e.kind == EdgeKind::Other && !e.seam).collect();
    assert_eq!(elliptic.len(), 1, "one elliptic oblique top rim");
    let convex = corners(&mut s, &Sel::Convex);
    assert!(convex.contains(&elliptic[0].id), "elliptic cylinder/plane rim is convex: {convex:?}");
    assert_eq!(convex.len(), 2, "bottom circle and top ellipse, excluding cylinder seam");
    assert!(corners(&mut s, &Sel::Concave).is_empty());
}

#[test]
fn both_frustum_cone_to_plane_rims_are_convex() {
    let mut s = Session::new_part();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XZ), None).unwrap();
    s.sketch_polyline(
        sketch,
        &PolylineSpec {
            points: [(0.0, 0.0), (10.0, 0.0), (5.0, 10.0), (0.0, 10.0)].into_iter().map(|(x, z)| Xy(x.into(), z.into())).collect(),
            closed: true,
            construction: false,
            dimensioned: false,
        },
    )
    .unwrap();
    let (_, report) = s
        .revolve(&Revolve {
            sketch,
            profiles: None,
            axis: AxisRef::SketchY,
            angle: 360.0.into(),
            direction: Direction::Normal,
            op: Op::Add,
            target: None,
            name: Some("frustum".into()),
        })
        .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    // Frustum V=π*h*(R²+R*r+r²)/3, from integrating π*(R+(r-R)*z/h)² dz.
    let expected = PI * 10.0 * (10.0_f64.powi(2) + 10.0 * 5.0 + 5.0_f64.powi(2)) / 3.0;
    assert_close(volume(&s), expected, 1e-3, "analytic frustum volume");
    let topo = s.topology(None, true).unwrap();
    assert_eq!(topo.faces.iter().filter(|face| face.kind == FaceKind::Cone).count(), 1);
    let rims = [circle_at(&topo, 10.0, 0.0), circle_at(&topo, 5.0, 10.0)];
    let convex = corners(&mut s, &Sel::Convex);
    assert!(rims.iter().all(|id| convex.contains(id)), "both cone/plane rims are convex: {rims:?}, got {convex:?}");
    assert_eq!(convex.len(), 2, "cone seam is excluded");
    assert!(corners(&mut s, &Sel::Concave).is_empty());
}

/// Pappus: the added quarter-circle spandrel has area r²(1-π/4). Its radial first moment is
/// r³(5/6-π/4), hence centroid R+r*(5/6-π/4)/(1-π/4), revolved once around the boss axis.
fn rounded_boss_volume(t: f64, d: f64, h: f64, r: f64) -> f64 {
    let radius = d / 2.0;
    let area = r * r * (1.0 - PI / 4.0);
    let centroid = radius + r * (5.0 / 6.0 - PI / 4.0) / (1.0 - PI / 4.0);
    W * L * t + PI * radius * radius * h + 2.0 * PI * centroid * area
}

#[test]
fn concave_boss_fillet_rounds_one_circle_and_follows_parameters_on_server_and_gui() {
    let mut s = boss_plate();
    let (_, report) = s.fillet(None, &Sel::Concave, &n("r"), Some("boss base round")).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let expected = rounded_boss_volume(6.0, 20.0, 10.0, 2.0);
    // Match the engine's native volume-roundoff threshold (F-052), not a tessellation allowance.
    assert_close(volume(&s), expected, (1e-9 * expected).max(1e-6), "Pappus added boss-base fillet");
    let topo = s.topology(None, true).unwrap();
    let unchanged_top = circle_at(&topo, 10.0, 16.0);
    assert!(corners(&mut s, &Sel::Convex).contains(&unchanged_top), "boss top circle stays sharp");
    let concave = corners(&mut s, &Sel::Concave);
    assert!(concave.is_empty(), "both fillet boundaries are G1 tangent, not concave: {concave:?}");
    let convex = corners(&mut s, &Sel::Convex);
    assert_eq!(convex.len(), 12 + 1, "fillet boundaries and all seams are excluded");

    let path = scratch("curved_boss_fillet_gui.qcad");
    s.save(Some(&path)).unwrap();
    for (name, value, expected) in [
        ("r", 3.0, rounded_boss_volume(6.0, 20.0, 10.0, 3.0)),
        ("d", 24.0, rounded_boss_volume(6.0, 24.0, 10.0, 2.0)),
        ("h", 12.0, rounded_boss_volume(6.0, 20.0, 12.0, 2.0)),
        ("t", 8.0, rounded_boss_volume(8.0, 20.0, 10.0, 2.0)),
    ] {
        let got = gui_edit_param(&path, name, &value.to_string());
        let tolerance = (1e-9 * expected).max(1e-6);
        assert_close(got, expected, tolerance, &format!("GUI boss-base fillet follows {name}"));
        let (mut opened, report) = Session::open(&path).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let report = opened.param_set(name, &value.into()).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_close(volume(&opened), expected, tolerance, &format!("server boss-base fillet follows {name}"));
    }
}
