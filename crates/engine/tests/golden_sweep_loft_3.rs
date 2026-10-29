//! Stage 3: sweep, loft, profiles picked by points and formula-driven hole shifts, with volumes from closed forms
//! and the app's parameter-edit path.
mod common;
use common::*;
use qymcad_engine::*;
use std::f64::consts::PI;

fn v(x: f64) -> Num {
    Num::Value(x)
}
fn e(x: &str) -> Num {
    Num::Expr(x.into())
}
fn sketch(s: &mut Session, plane: PlaneRef) -> Id {
    s.sketch_create(&plane, None).unwrap()
}
fn line(s: &mut Session, sk: Id, a: [Num; 2], b: [Num; 2]) -> Added {
    s.sketch_line(
        sk,
        &LineSpec { x1: a[0].clone(), y1: a[1].clone(), x2: b[0].clone(), y2: b[1].clone(), construction: false, dimensioned: true },
    )
    .unwrap()
}
fn sweep(path: SweepPath, sketch: Id, op: Op) -> Sweep {
    Sweep { sketch, profiles: None, path, op, target: None, name: None }
}
fn volume(r: &Rebuild) -> f64 {
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r.bodies.last().unwrap().volume
}

/// A W×W square at x = 7 on XY, swept along a line from the origin up Z to L: the path starts on the profile's
/// plane, so the profile stays where it is drawn. V = W²·L, x from 7 − W/2 to 7 + W/2.
#[test]
fn a_straight_sweep_is_a_prism_that_follows_its_profile_and_path_formulas() {
    let mut s = Session::new_part();
    s.param_set("W", &v(4.)).unwrap();
    s.param_set("L", &v(40.)).unwrap();
    let profile = sketch(&mut s, PlaneRef::Base(BaseName::XY));
    s.sketch_rect(profile, &v(7.), &v(0.), &e("W"), &e("W"), false).unwrap();
    let path = sketch(&mut s, PlaneRef::Base(BaseName::XZ));
    line(&mut s, path, [v(0.), v(0.)], [v(0.), e("L")]);
    assert_eq!(s.sketch_info(path).unwrap().dof, (0, 0));
    let (_, r) = s.sweep(&sweep(SweepPath::Sketch { sketch: path, profile: None }, profile, Op::Add)).unwrap();
    assert_close(volume(&r), 640., 1e-6, "W²L");
    let bb = r.bodies[0].bbox;
    assert_close(bb[0], 5., 1e-3, "the profile stays at x = 7 − W/2");
    assert_close(bb[3], 9., 1e-3, "x = 7 + W/2");
    let path_file = scratch("stage3-sweep-straight.qcad");
    s.save(Some(&path_file)).unwrap();
    let (mut opened, r) = Session::open(&path_file).unwrap();
    assert_close(volume(&r), 640., 1e-6, "reopened");
    assert_close(volume(&opened.param_set("L", &v(50.)).unwrap()), 800., 1e-6, "server: path length");
    assert_close(gui_edit_param(&path_file, "L", "50"), 800., 1e-6, "app: path length");
    assert_close(gui_edit_param(&path_file, "W", "5"), 1000., 1e-6, "app: profile size");
}

/// A circle of radius r centred at (R, 0) on XZ, swept a quarter turn along an arc of radius R about Z: by
/// Pappus V = πr²·R·θ.
#[test]
fn a_sweep_along_an_arc_obeys_pappus() {
    let mut s = Session::new_part();
    s.param_set("R", &v(20.)).unwrap();
    let profile = sketch(&mut s, PlaneRef::Base(BaseName::XZ));
    s.sketch_circle(profile, &e("R"), &v(5.), &v(4.), false).unwrap();
    let path = sketch(&mut s, PlaneRef::Base(BaseName::XY));
    s.sketch_arc(
        path,
        &ArcSpec {
            cx: v(0.),
            cy: v(0.),
            r: Some(e("R")),
            start_angle: Some(v(0.)),
            end_angle: Some(v(90.)),
            start: None,
            end: None,
            ccw: true,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    let pappus = |r_big: f64| PI * 4. * r_big * PI / 2.;
    let (_, r) = s.sweep(&sweep(SweepPath::Sketch { sketch: path, profile: None }, profile, Op::NewBody)).unwrap();
    assert_close(volume(&r), pappus(20.), 1e-6, "πr²Rθ");
    let file = scratch("stage3-sweep-arc.qcad");
    s.save(Some(&file)).unwrap();
    assert_close(gui_edit_param(&file, "R", "25"), pappus(25.), 1e-6, "app: the arc and the profile move together");
}

/// A 4×4 rail drawn on YZ at z 10..14, swept along the front top edge of a 40×30×T block. The edge is kept by its
/// name, so it follows T: V = 40·30·T + 40·4·(14 − max(T, 10)).
#[test]
fn a_sweep_along_a_body_edge_follows_the_edge_when_the_body_changes() {
    let mut s = Session::new_part();
    s.param_set("T", &v(10.)).unwrap();
    let stock = sketch(&mut s, PlaneRef::Base(BaseName::XY));
    s.sketch_rect(stock, &v(20.), &v(15.), &v(40.), &v(30.), false).unwrap();
    let (block, r) = s
        .extrude(&Extrude {
            down: None,
            sketch: stock,
            profiles: None,
            height: e("T"),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(r.errors.is_empty());
    let rail = sketch(&mut s, PlaneRef::Base(BaseName::YZ));
    s.sketch_rect(rail, &v(2.), &v(12.), &v(4.), &v(4.), false).unwrap();
    let edge = s
        .topology(Some(block), false)
        .unwrap()
        .edges
        .iter()
        .find(|e| (e.a[1]).abs() < 1e-6 && (e.b[1]).abs() < 1e-6 && (e.a[2] - 10.).abs() < 1e-6 && (e.b[2] - 10.).abs() < 1e-6)
        .expect("the front top edge")
        .id;
    let along = SweepPath::Edges { body: Some(block), edges: Sel::Ids(vec![edge]) };
    let (_, r) = s.sweep(&sweep(along, rail, Op::Add)).unwrap();
    let expected = |t: f64| 40. * 30. * t + 40. * 4. * (14. - t.max(10.));
    assert_close(volume(&r), expected(10.), 1e-6, "block + rail");
    let file = scratch("stage3-sweep-edge.qcad");
    s.save(Some(&file)).unwrap();
    assert_close(gui_edit_param(&file, "T", "12"), expected(12.), 1e-6, "app: the rail follows the edge by its name");
    // Two edges that do not touch are not a path.
    let far =
        s.topology(None, false).unwrap().edges.iter().find(|e| (e.a[1] - 30.).abs() < 1e-6 && (e.b[1] - 30.).abs() < 1e-6).unwrap().id;
    let before = s.result_bodies();
    let apart = SweepPath::Edges { body: None, edges: Sel::Ids(vec![s.topology(None, false).unwrap().edges[0].id, far]) };
    assert!(s.sweep(&sweep(apart, rail, Op::Add)).is_err());
    assert_eq!(s.result_bodies(), before, "a refused sweep changes nothing");
}

/// Squares W and W/2 on planes H apart, ruled: a frustum H/3·(a² + ab + b²).
#[test]
fn a_ruled_loft_between_squares_is_a_frustum_that_follows_sizes_and_plane_distance() {
    let mut s = Session::new_part();
    s.param_set("W", &v(20.)).unwrap();
    s.param_set("H", &v(10.)).unwrap();
    let low = sketch(&mut s, PlaneRef::Base(BaseName::XY));
    s.sketch_rect(low, &v(0.), &v(0.), &e("W"), &e("W"), false).unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &e("H"), None).unwrap();
    let high = sketch(&mut s, PlaneRef::Plane(plane));
    s.sketch_rect(high, &v(0.), &v(0.), &e("W/2"), &e("W/2"), false).unwrap();
    let frustum = |w: f64, h: f64| h / 3. * (w * w + w * w / 4. + w * w / 2.);
    let sections = vec![LoftSection { sketch: low, profile: None }, LoftSection { sketch: high, profile: None }];
    let (_, r) = s.loft(&Loft { sections, ruled: true, op: Op::Add, target: None, surface: false, name: Some("frustum".into()) }).unwrap();
    assert_close(volume(&r), frustum(20., 10.), 1e-6, "ruled square frustum");
    let file = scratch("stage3-loft.qcad");
    s.save(Some(&file)).unwrap();
    assert_close(gui_edit_param(&file, "W", "30"), frustum(30., 10.), 1e-6, "app: section sizes");
    assert_close(gui_edit_param(&file, "H", "15"), frustum(20., 15.), 1e-6, "app: plane distance");
}

/// Circles R1 and R2 on planes H apart, smooth: two sections make a cone frustum πH/3·(R1² + R1R2 + R2²).
#[test]
fn a_smooth_loft_between_circles_is_a_cone_frustum() {
    let mut s = Session::new_part();
    let low = sketch(&mut s, PlaneRef::Base(BaseName::XY));
    s.sketch_circle(low, &v(0.), &v(0.), &v(20.), false).unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &v(12.), None).unwrap();
    let high = sketch(&mut s, PlaneRef::Plane(plane));
    s.sketch_circle(high, &v(0.), &v(0.), &v(10.), false).unwrap();
    let sections = vec![LoftSection { sketch: low, profile: None }, LoftSection { sketch: high, profile: None }];
    let (_, r) = s.loft(&Loft { sections, ruled: false, op: Op::NewBody, target: None, surface: false, name: None }).unwrap();
    assert_close(volume(&r), PI * 12. / 3. * (100. + 50. + 25.), 1e-3, "cone frustum");
}

#[test]
fn loft_and_sweep_refuse_bad_input_and_change_nothing() {
    let mut s = Session::new_part();
    let low = sketch(&mut s, PlaneRef::Base(BaseName::XY));
    s.sketch_circle(low, &v(0.), &v(0.), &v(20.), false).unwrap();
    s.sketch_circle(low, &v(40.), &v(0.), &v(20.), false).unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &v(12.), None).unwrap();
    let high = sketch(&mut s, PlaneRef::Plane(plane));
    s.sketch_circle(high, &v(0.), &v(0.), &v(10.), false).unwrap();
    let before = s.project().timeline.len();
    let section = |sketch| LoftSection { sketch, profile: None };
    let loft = |sections: Vec<LoftSection>, op, surface| Loft { sections, ruled: false, op, target: None, surface, name: None };
    let one = s.loft(&loft(vec![section(high)], Op::NewBody, false)).unwrap_err().to_string();
    assert!(one.contains("at least two"), "{one}");
    let many = s.loft(&loft(vec![section(low), section(high)], Op::NewBody, false)).unwrap_err().to_string();
    assert!(many.contains("several closed contours"), "{many}");
    let sheet = s.loft(&loft(vec![section(high), section(high)], Op::Add, true)).unwrap_err().to_string();
    assert!(sheet.contains("new_body"), "{sheet}");
    let same = s.sweep(&sweep(SweepPath::Sketch { sketch: high, profile: None }, high, Op::NewBody)).unwrap_err().to_string();
    assert!(same.contains("another sketch"), "{same}");
    assert_eq!(s.project().timeline.len(), before, "refusals add no node");
}

/// Nested squares 20, 12 and 4: a point in the outer ring picks the outer contour (its region minus the middle
/// square), a point at the centre picks the inner square. A = (20² − 12²) + 4².
#[test]
fn points_pick_the_innermost_contour_around_them() {
    let mut s = Session::new_part();
    s.param_set("h", &v(5.)).unwrap();
    let sk = sketch(&mut s, PlaneRef::Base(BaseName::XY));
    for w in [20., 12., 4.] {
        s.sketch_rect(sk, &v(0.), &v(0.), &v(w), &v(w), false).unwrap();
    }
    let picked = s.contours_at(sk, &[[v(8.), v(0.)], [v(0.), v(0.)], [v(0.5), v(0.5)]]).unwrap();
    assert_eq!(picked.len(), 2, "the two centre points pick the same contour once");
    let (_, r) = s
        .extrude(&Extrude {
            down: None,
            sketch: sk,
            profiles: Some(picked),
            height: e("h"),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert_close(volume(&r), 5. * (400. - 144. + 16.), 1e-6, "outer ring plus inner square");
    let file = scratch("stage3-profiles-at.qcad");
    s.save(Some(&file)).unwrap();
    assert_close(gui_edit_param(&file, "h", "6"), 6. * 272., 1e-6, "app: the stored contours keep their regions");
    assert!(s.contours_at(sk, &[[v(30.), v(0.)]]).is_err(), "outside every contour");
    assert!(s.contours_at(sk, &[[v(10.), v(0.)]]).is_err(), "on an edge");
}

/// A hole shifted (u, v) from the centre of the top face of a 40×30×10 block: its axis stands at (20 + u, 15 + v)
/// and follows u and v.
#[test]
fn a_hole_offset_from_the_face_centre_follows_its_formulas() {
    let mut s = Session::new_part();
    s.param_set("u", &v(5.)).unwrap();
    s.param_set("w", &v(3.)).unwrap();
    let sk = sketch(&mut s, PlaneRef::Base(BaseName::XY));
    s.sketch_rect(sk, &v(20.), &v(15.), &v(40.), &v(30.), false).unwrap();
    s.extrude(&Extrude {
        down: None,
        sketch: sk,
        profiles: None,
        height: v(10.),
        op: Op::Add,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap();
    let hole = Hole {
        body: None,
        face: Sel::Extreme { axis: Axis::Z, max: true },
        at: None,
        offset: Some([e("u"), e("w")]),
        diameter: v(4.),
        depth: Some(v(6.)),
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: None,
    };
    s.hole(&hole).unwrap();
    let axis_at = |s: &mut Session| {
        let t = s.topology(None, false).unwrap();
        let wall = t.faces.iter().find(|f| f.kind == FaceKind::Cylinder).expect("the hole wall");
        let [o, _] = wall.axis.unwrap();
        [o[0], o[1]]
    };
    let [x, y] = axis_at(&mut s);
    assert_close(x, 25., 1e-6, "x = 20 + u");
    assert_close(y, 18., 1e-6, "y = 15 + v");
    s.param_set("u", &v(-5.)).unwrap();
    let [x, _] = axis_at(&mut s);
    assert_close(x, 15., 1e-6, "the hole follows u");
    let file = scratch("stage3-hole-offset.qcad");
    s.save(Some(&file)).unwrap();
    let (_, _, faces) = gui_edit_param_faces(&file, "w", "-4");
    let wall = faces.iter().find(|f| (f.area - PI * 4. * 6.).abs() < 0.5).expect("the hole wall in the app");
    assert_close(wall.centroid.y, 11., 1e-2, "app: y = 15 + v");
    assert!(s.hole(&Hole { at: Some([1., 1., 10.]), ..hole.clone() }).is_err(), "at and offset exclude each other");
    // Plain numbers place the hole once: u along +X, v along +Y on a top face. The centre they count from is the
    // area centroid of the face as it stands (F-087): the d4 hole at (15, 18) moves it to
    // ((1200·20 − 4π·15) / (1200 − 4π), (1200·15 − 4π·18) / (1200 − 4π)).
    s.hole(&Hole { offset: Some([v(-10.), v(-6.)]), diameter: v(2.), ..hole }).unwrap();
    let t = s.topology(None, false).unwrap();
    let small = t.faces.iter().find(|f| f.kind == FaceKind::Cylinder && (f.radius.unwrap() - 1.).abs() < 1e-6).expect("the d2 wall");
    let [o, _] = small.axis.unwrap();
    let area = 1200. - 4. * PI;
    assert_close(o[0], (1200. * 20. - 4. * PI * 15.) / area - 10., 2e-3, "x = centroid x − 10");
    assert_close(o[1], (1200. * 15. - 4. * PI * 18.) / area - 6., 2e-3, "y = centroid y − 6");
}

/// A fillet on the straight edges of a swept prism is accepted (F-074) and removes (1 − π/4)·r² per unit length.
#[test]
fn a_fillet_on_a_swept_body_is_checked_and_accepted() {
    let mut s = Session::new_part();
    let profile = sketch(&mut s, PlaneRef::Base(BaseName::XY));
    s.sketch_rect(profile, &v(0.), &v(0.), &v(4.), &v(4.), false).unwrap();
    let path = sketch(&mut s, PlaneRef::Base(BaseName::XZ));
    line(&mut s, path, [v(0.), v(0.)], [v(0.), v(40.)]);
    s.sweep(&sweep(SweepPath::Sketch { sketch: path, profile: None }, profile, Op::Add)).unwrap();
    let (_, r) = s.fillet(None, &Sel::Along { dir: [0., 0., 1.], tol_deg: 1. }, &v(0.5), None).unwrap();
    assert_close(volume(&r), 640. - 4. * (1. - PI / 4.) * 0.25 * 40., 1e-6, "four rounded edges");
}

/// A section with concentric circles has two closed contours; the loft must ask which (Codex review r1).
#[test]
fn a_loft_section_with_nested_contours_must_name_one() {
    let mut s = Session::new_part();
    let low = sketch(&mut s, PlaneRef::Base(BaseName::XY));
    s.sketch_circle(low, &v(0.), &v(0.), &v(30.), false).unwrap();
    s.sketch_circle(low, &v(0.), &v(0.), &v(10.), false).unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &v(12.), None).unwrap();
    let high = sketch(&mut s, PlaneRef::Plane(plane));
    s.sketch_circle(high, &v(0.), &v(0.), &v(10.), false).unwrap();
    let sections = vec![LoftSection { sketch: low, profile: None }, LoftSection { sketch: high, profile: None }];
    let e = s.loft(&Loft { sections, ruled: true, op: Op::NewBody, target: None, surface: false, name: None }).unwrap_err().to_string();
    assert!(e.contains("several closed contours"), "{e}");
    let inner = s.contours_at(low, &[[v(0.), v(0.)]]).unwrap()[0];
    let sections = vec![LoftSection { sketch: low, profile: Some(inner) }, LoftSection { sketch: high, profile: None }];
    let (_, r) = s.loft(&Loft { sections, ruled: true, op: Op::NewBody, target: None, surface: false, name: None }).unwrap();
    assert_close(volume(&r), PI * 25. * 12., 1e-6, "the inner circle lofted to the same circle: a cylinder");
}
