//! Planar corner signs at non-right angles, checked against formula geometry.

mod common;
use common::*;
use qymcad_engine::*;

fn extrude(s: &mut Session, sketch: Id, height: f64, op: Op, direction: Direction) {
    let (_, report) = s
        .extrude(&Extrude { sketch, profiles: None, height: height.into(), op, direction, through: false, target: None, name: None })
        .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
}

fn block() -> Session {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &16.0.into(), false).unwrap();
    extrude(&mut s, sk, 10.0, Op::Add, Direction::Normal);
    s
}

fn triangle(s: &mut Session, points: [(f64, f64); 3]) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XZ), None).unwrap();
    s.sketch_polyline(
        sk,
        &PolylineSpec {
            points: points.into_iter().map(|(x, z)| Xy(x.into(), z.into())).collect(),
            closed: true,
            construction: false,
            dimensioned: false,
        },
    )
    .unwrap();
    sk
}

fn check_geometry(s: &Session, volume: f64, bbox: [f64; 6]) {
    let bodies = s.result_bodies();
    assert_eq!(bodies.len(), 1);
    assert_close(bodies[0].volume, volume, 1e-3, "formula volume");
    for (actual, expected) in bodies[0].bbox.into_iter().zip(bbox) {
        assert_close(actual, expected, 0.05, "formula bbox (F-016)");
    }
}

fn edge_at(topo: &Topology, midpoint: [f64; 3], length: f64) -> u32 {
    let edges: Vec<_> = topo.edges.iter().filter(|e| e.mid.into_iter().zip(midpoint).all(|(a, b)| (a - b).abs() < 1e-6)).collect();
    assert_eq!(edges.len(), 1, "unique edge at {midpoint:?}: {edges:?}");
    let e = edges[0];
    assert_eq!(e.kind, EdgeKind::Line);
    assert_close(e.length, length, 1e-6, "formula edge length");
    for id in e.faces.unwrap() {
        assert_eq!(topo.faces.iter().find(|f| f.id == id).unwrap().kind, FaceKind::Plane);
    }
    e.id
}

#[test]
fn forty_five_degree_chamfer_boundaries_are_convex() {
    let mut s = block();
    let topo = s.topology(None, true).unwrap();
    let top_edge = edge_at(&topo, [0.0, 8.0, 10.0], 20.0);
    let (_, report) = s.chamfer(None, &Sel::Ids(vec![top_edge]), &2.0.into(), None, None).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    // Remove a right triangle with equal 2 mm legs along the 20 mm top edge.
    check_geometry(&s, 20.0 * 16.0 * 10.0 - 2.0 * 2.0 / 2.0 * 20.0, [-10.0, -8.0, 0.0, 10.0, 8.0, 10.0]);
    let topo = s.topology(None, true).unwrap();
    let boundaries = [edge_at(&topo, [0.0, 6.0, 10.0], 20.0), edge_at(&topo, [0.0, 8.0, 8.0], 20.0)];
    let convex = s.select(None, Element::Edges, &Sel::Convex).unwrap().1;
    assert!(
        boundaries.iter().all(|id| convex.contains(id)),
        "both 135-degree chamfer boundaries must be convex: {boundaries:?}, got {convex:?}"
    );
    let concave = s.select(None, Element::Edges, &Sel::Concave).unwrap().1;
    assert!(boundaries.iter().all(|id| !concave.contains(id)));
}

#[test]
fn ninety_degree_v_groove_bottom_is_concave() {
    let mut s = block();
    let sk = triangle(&mut s, [(-2.0, 10.0), (0.0, 8.0), (2.0, 10.0)]);
    extrude(&mut s, sk, 16.0, Op::Cut, Direction::Symmetric);
    // Groove cross-section: base 4, depth 2, length 16; its walls meet at 90 degrees.
    check_geometry(&s, 20.0 * 16.0 * 10.0 - 4.0 * 2.0 / 2.0 * 16.0, [-10.0, -8.0, 0.0, 10.0, 8.0, 10.0]);
    let topo = s.topology(None, true).unwrap();
    let bottom = edge_at(&topo, [0.0, 0.0, 8.0], 16.0);
    let concave = s.select(None, Element::Edges, &Sel::Concave).unwrap().1;
    assert!(concave.contains(&bottom), "V-groove bottom must be concave: {bottom}, got {concave:?}");
    assert!(!s.select(None, Element::Edges, &Sel::Convex).unwrap().1.contains(&bottom));
}

#[test]
fn thirty_degree_wedge_corner_is_convex() {
    let mut s = Session::new_part();
    let width = 10.0 * 3.0_f64.sqrt();
    let sk = triangle(&mut s, [(0.0, 0.0), (width, 0.0), (0.0, 10.0)]);
    extrude(&mut s, sk, 12.0, Op::Add, Direction::Symmetric);
    // atan(10/(10*sqrt(3))) = 30 degrees; triangular prism V = base*height*length/2.
    check_geometry(&s, width * 10.0 / 2.0 * 12.0, [0.0, -6.0, 0.0, width, 6.0, 10.0]);
    let topo = s.topology(None, true).unwrap();
    let apex = edge_at(&topo, [width, 0.0, 0.0], 12.0);
    let convex = s.select(None, Element::Edges, &Sel::Convex).unwrap().1;
    assert!(convex.contains(&apex), "30-degree wedge corner must be convex: {apex}, got {convex:?}");
    assert!(!s.select(None, Element::Edges, &Sel::Concave).unwrap().1.contains(&apex));
}

#[test]
fn twenty_degree_plane_to_plane_wedge_corner_stays_convex() {
    let mut s = Session::new_part();
    let width = 10.0 / 20.0_f64.to_radians().tan();
    let sk = triangle(&mut s, [(0.0, 0.0), (width, 0.0), (0.0, 10.0)]);
    extrude(&mut s, sk, 12.0, Op::Add, Direction::Symmetric);
    check_geometry(&s, width * 10.0 * 12.0 / 2.0, [0.0, -6.0, 0.0, width, 6.0, 10.0]);
    let topo = s.topology(None, true).unwrap();
    let edge = edge_at(&topo, [width, 0.0, 0.0], 12.0);
    assert!(s.select(None, Element::Edges, &Sel::Convex).unwrap().1.contains(&edge));
    assert!(!s.select(None, Element::Edges, &Sel::Concave).unwrap().1.contains(&edge));
}
