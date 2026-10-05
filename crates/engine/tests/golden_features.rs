//! Golden tests for topology, selections and the phase-3B features (fillet, chamfer, revolve, hole, shell,
//! push face, arrays, mirror). Every expected volume is computed by hand in the test.

mod common;
use common::*;
use qymcad_engine::*;
use std::f64::consts::PI;

fn n(s: &str) -> Num {
    Num::Expr(s.into())
}

/// A rectangle `w` × `l` centred at (`cx`, `cy`) on XY, extruded up by `h`. Returns the session and the body.
fn block_at(cx: Num, cy: Num, w: Num, l: Num, h: Num) -> (Session, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("block sketch")).unwrap();
    s.sketch_rect(sk, &cx, &cy, &w, &l, false).unwrap();
    let (id, r) = s.extrude(&extrude(sk, h, Op::Add)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    (s, id)
}

fn block(w: f64, l: f64, h: f64) -> (Session, Id) {
    block_at(0.0.into(), 0.0.into(), w.into(), l.into(), h.into())
}

fn extrude(sketch: Id, h: Num, op: Op) -> Extrude {
    Extrude {
        sketch,
        profiles: None,
        height: h,
        op,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: Some("block".into()),
    }
}

/// A cylinder of diameter `d` centred at (`cx`, `cy`), extruded up by `h` from XY.
fn cylinder(s: &mut Session, cx: f64, cy: f64, d: Num, h: Num, op: Op) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &cx.into(), &cy.into(), &d, false).unwrap();
    s.extrude(&extrude(sk, h, op)).unwrap().0
}

fn volume(s: &Session) -> f64 {
    let b = s.result_bodies();
    assert_eq!(b.len(), 1, "one result body: {b:?}");
    b[0].volume
}

fn bbox(s: &Session) -> [f64; 6] {
    s.result_bodies()[0].bbox
}

fn along_z() -> Sel {
    Sel::Along { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }
}

fn top() -> Sel {
    Sel::Extreme { axis: Axis::Z, max: true }
}

// ---------------------------------------------------------------------------------------------------------------
// Topology and selection

#[test]
fn topology_of_a_block() {
    let (mut s, b) = block(40.0, 30.0, 10.0);
    let t = s.topology(None, true).unwrap();
    assert_eq!(t.body, b);
    assert_eq!(t.faces.len(), 6);
    assert_eq!(t.edges.len(), 12);
    assert!(t.faces.iter().all(|f| f.kind == FaceKind::Plane && f.normal.is_some()));
    assert!(t.edges.iter().all(|e| e.kind == EdgeKind::Line && e.faces.is_some()));
    let top = t.faces.iter().find(|f| f.normal.unwrap()[2] > 0.99).expect("a face looking up");
    assert_close(top.centroid[2], 10.0, 1e-9, "top face z");
    assert_close(top.area, 1200.0, 1e-6, "top face area");
    assert_eq!(top.edges.as_ref().unwrap().len(), 4);
    let len: f64 = t.edges.iter().map(|e| e.length).sum();
    assert_close(len, 4.0 * (40.0 + 30.0 + 10.0), 1e-6, "total edge length");
}

#[test]
fn topology_classifies_round_faces_and_edges() {
    let (mut s, _) = block(40.0, 30.0, 10.0);
    s.fillet(None, &along_z(), &3.0.into(), None).unwrap();
    let t = s.topology(None, false).unwrap();
    let cyl: Vec<_> = t.faces.iter().filter(|f| f.kind == FaceKind::Cylinder).collect();
    assert_eq!(cyl.len(), 4, "four rounded corners");
    for f in &cyl {
        assert_close(f.radius.unwrap(), 3.0, 1e-9, "fillet face radius");
        let [_, dir] = f.axis.unwrap();
        assert_close(dir[2].abs(), 1.0, 1e-9, "fillet axis is vertical");
    }
    let arcs: Vec<_> = t.edges.iter().filter(|e| e.kind == EdgeKind::Arc).collect();
    assert_eq!(arcs.len(), 8, "four arcs on the top, four on the bottom");
    for e in arcs {
        assert_close(e.length, PI / 2.0 * 3.0, 1e-6, "quarter-circle length");
    }

    // A cylinder: a cylindrical side and two circles.
    let mut c = Session::new_part();
    cylinder(&mut c, 0.0, 0.0, 20.0.into(), 10.0.into(), Op::Add);
    let t = c.topology(None, false).unwrap();
    assert_eq!(t.faces.iter().filter(|f| f.kind == FaceKind::Cylinder).count(), 1);
    assert_eq!(t.faces.iter().filter(|f| f.kind == FaceKind::Plane).count(), 2);
    let circles: Vec<_> = t.edges.iter().filter(|e| e.kind == EdgeKind::Circle).collect();
    assert_eq!(circles.len(), 2);
    assert_close(circles[0].length, 2.0 * PI * 10.0, 1e-6, "circle length");
}

#[test]
fn selections_resolve_like_qymcad_will() {
    let (mut s, b) = block(40.0, 30.0, 10.0);
    let faces = |s: &mut Session, sel: &Sel| s.select(None, Element::Faces, sel).unwrap().1;
    let edges = |s: &mut Session, sel: &Sel| s.select(None, Element::Edges, sel).unwrap().1;
    assert_eq!(faces(&mut s, &Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 5.0 }).len(), 1);
    assert_eq!(faces(&mut s, &top()).len(), 1);
    assert_eq!(faces(&mut s, &Sel::Largest).len(), 2, "top and bottom tie");
    assert_eq!(faces(&mut s, &Sel::OfFeature { feature: b, role: Some(Role::Wall) }).len(), 4);
    assert_eq!(faces(&mut s, &Sel::OfFeature { feature: b, role: Some(Role::CapEnd) }), faces(&mut s, &top()));
    assert_eq!(edges(&mut s, &along_z()).len(), 4);
    assert_eq!(edges(&mut s, &Sel::EdgesOf(Box::new(top()))).len(), 4);
    let side = Sel::Facing { dir: [1.0, 0.0, 0.0], tol_deg: 5.0 };
    assert_eq!(edges(&mut s, &Sel::Between(Box::new(top()), Box::new(side.clone()))).len(), 1);
    let all_but = Sel::Minus(Box::new(Sel::EdgesOf(Box::new(top()))), Box::new(Sel::EdgesOf(Box::new(side))));
    assert_eq!(edges(&mut s, &all_but).len(), 3);
    let both = Sel::Union(vec![Sel::EdgesOf(Box::new(top())), along_z()]);
    assert_eq!(edges(&mut s, &both).len(), 8);
    let and = Sel::And(Box::new(Sel::EdgesOf(Box::new(top()))), Box::new(Sel::Along { dir: [1.0, 0.0, 0.0], tol_deg: 1.0 }));
    assert_eq!(edges(&mut s, &and).len(), 2);

    // After rounding the vertical corners, the top outline is one tangent chain of 4 lines and 4 arcs.
    s.fillet(None, &along_z(), &3.0.into(), None).unwrap();
    let seed = Sel::And(Box::new(Sel::EdgesOf(Box::new(top()))), Box::new(Sel::Along { dir: [1.0, 0.0, 0.0], tol_deg: 1.0 }));
    let one = edges(&mut s, &seed)[0];
    let chain = Sel::TangentChain { seed: Box::new(Sel::Ids(vec![one])), tol_deg: 5.0 };
    assert_eq!(edges(&mut s, &chain).len(), 8);

    // Nonsense combinations are refused with a hint.
    let e = s.select(None, Element::Edges, &Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 5.0 }).unwrap_err();
    assert!(e.to_string().contains("along"), "{e}");
    assert!(s.select(None, Element::Faces, &along_z()).is_err());
}

#[test]
fn topology_is_available_after_open() {
    let (mut s, _) = block(40.0, 30.0, 10.0);
    s.fillet(None, &along_z(), &3.0.into(), None).unwrap();
    let path = scratch("topology_after_open.qcad");
    s.save(Some(&path)).unwrap();
    let (mut o, _) = Session::open(&path).unwrap();
    let t = o.topology(None, false).unwrap();
    assert_eq!(t.faces.len(), 10);
    o.chamfer(None, &Sel::EdgesOf(Box::new(top())), &1.0.into(), None, None).unwrap();
}

// ---------------------------------------------------------------------------------------------------------------
// Fillet, chamfer

/// a·b·h − (4 − π)·r²·h for the four vertical edges.
#[test]
fn fillet_vertical_edges() {
    let (a, b, h, r) = (40.0, 30.0, 10.0, 3.0);
    let expected = a * b * h - (4.0 - PI) * r * r * h;
    // Descriptive.
    let (mut s, _) = block(a, b, h);
    s.fillet(None, &along_z(), &r.into(), Some("round")).unwrap();
    assert_close(volume(&s), expected, 1e-3, "fillet by description");
    // Explicit ids from topology.
    let (mut s, _) = block(a, b, h);
    let ids: Vec<u32> = s.topology(None, false).unwrap().edges.iter().filter(|e| (e.a[2] - e.b[2]).abs() > 1.0).map(|e| e.id).collect();
    let (body, r1) = s.fillet(None, &Sel::Ids(ids), &r.into(), None).unwrap();
    assert_eq!(r1.bodies[0].id, body, "the fillet's body is the new current body");
    assert_close(volume(&s), expected, 1e-3, "fillet by ids");
}

/// a·b·h − 4·(d²/2)·h; two distances on one edge: − (d1·d2/2)·h.
#[test]
fn chamfer_vertical_edges() {
    let (a, b, h, d) = (40.0, 30.0, 10.0, 2.0);
    let (mut s, _) = block(a, b, h);
    s.chamfer(None, &along_z(), &d.into(), None, None).unwrap();
    assert_close(volume(&s), a * b * h - 4.0 * d * d / 2.0 * h, 1e-3, "symmetric chamfer");

    let (mut s, _) = block(a, b, h);
    let one = Sel::And(Box::new(along_z()), Box::new(Sel::Extreme { axis: Axis::X, max: true }));
    let one = Sel::And(Box::new(one), Box::new(Sel::Extreme { axis: Axis::Y, max: true }));
    s.chamfer(None, &one, &2.0.into(), Some(&4.0.into()), None).unwrap();
    assert_close(volume(&s), a * b * h - 2.0 * 4.0 / 2.0 * h, 1e-3, "two-distance chamfer (descriptive)");

    let (mut s, _) = block(a, b, h);
    let id = s.select(None, Element::Edges, &one).unwrap().1;
    s.chamfer(None, &Sel::Ids(id), &2.0.into(), Some(&4.0.into()), None).unwrap();
    assert_close(volume(&s), a * b * h - 2.0 * 4.0 / 2.0 * h, 1e-3, "two-distance chamfer (ids)");
}

/// Fillet of the top rim of a cylinder (radius R, height h) by r: Pappus on the removed spandrel, area
/// A = r²(1 − π/4) with its centroid c = r(10 − 3π)/(3(4 − π)) in from the rim, swept on radius R − c.
fn cylinder_rim_fillet(rr: f64, h: f64, r: f64) -> f64 {
    let area = r * r * (1.0 - PI / 4.0);
    let c = r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    PI * rr * rr * h - 2.0 * PI * (rr - c) * area
}

/// A fillet chosen by description ("the edges of the top face of the cylinder") still rounds the rim after the
/// height (an upstream parameter) changes, and its radius follows its own parameter — in this server and through
/// QymCAD.app's rebuild path. (The edges are stored as persistent names, F-3B-2.)
#[test]
fn descriptive_fillet_survives_an_upstream_edit() {
    let mut s = Session::new_part();
    s.param_set("h", &Num::Value(10.0)).unwrap();
    s.param_set("r", &Num::Value(2.0)).unwrap();
    let cyl = cylinder(&mut s, 0.0, 0.0, 40.0.into(), n("h"), Op::Add);
    let rim = Sel::EdgesOf(Box::new(Sel::OfFeature { feature: cyl, role: Some(Role::CapEnd) }));
    s.fillet(None, &rim, &n("r"), None).unwrap();
    assert_close(volume(&s), cylinder_rim_fillet(20.0, 10.0, 2.0), 0.05, "rim fillet");
    let r = s.param_set("h", &Num::Value(16.0)).unwrap();
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_close(volume(&s), cylinder_rim_fillet(20.0, 16.0, 2.0), 0.05, "rim fillet after h=16");
    s.param_set("r", &Num::Value(3.0)).unwrap();
    assert_close(volume(&s), cylinder_rim_fillet(20.0, 16.0, 3.0), 0.05, "rim fillet after r=3");
    // The same through QymCAD's own GUI rebuild path.
    let path = scratch("rim_fillet_gui.qcad");
    s.save(Some(&path)).unwrap();
    let v = gui_edit_param(&path, "r", "4");
    assert_close(v, cylinder_rim_fillet(20.0, 16.0, 4.0), 0.05, "GUI edit r=4");
    let v = gui_edit_param(&path, "h", "12");
    assert_close(v, cylinder_rim_fillet(20.0, 12.0, 3.0), 0.05, "GUI edit h=12");
}

/// FINDINGS F-3B-2, the reason fillets store pick lists: the same document with the fillet's edges stored as a
/// QymCAD *query* (`Adjacent(OfFeature(cap end))`), reopened in the app and the radius edited, rounds EVERY edge
/// (both rims): the query is re-evaluated against an edge pool that a reopened document does not have. When this
/// test fails, QymCAD fixed it and stored edge queries can be reconsidered.
#[test]
fn stored_edge_query_rounds_everything_after_reopen_upstream_bug() {
    use qymcad_core::feature::FeatureKind;
    use qymcad_core::refs::{Query, Ref};
    let mut s = Session::new_part();
    s.param_set("r", &Num::Value(2.0)).unwrap();
    let cyl = cylinder(&mut s, 0.0, 0.0, 40.0.into(), 10.0.into(), Op::Add);
    let rim = Sel::EdgesOf(Box::new(Sel::OfFeature { feature: cyl, role: Some(Role::CapEnd) }));
    let (fillet, _) = s.fillet(None, &rim, &n("r"), None).unwrap();
    let path = scratch("stored_edge_query.qcad");
    s.save(Some(&path)).unwrap();
    // Rewrite the fillet's edges as a stored query, the way `add_fillet_ref` stores them.
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let node = project.timeline.iter_mut().find(|n| n.id == fillet).unwrap();
    let FeatureKind::Fillet { edges, .. } = &mut node.kind else { panic!("not a fillet") };
    let cap = Query::OfFeature { feature: cyl, role: Some(qymcad_core::names::Role::CapEnd) };
    *edges = Ref::many(Query::Adjacent(Box::new(cap)));
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let v = gui_edit_param(&path, "r", "4");
    let one_rim = PI * 400.0 * 10.0 - cylinder_rim_fillet(20.0, 10.0, 4.0);
    assert_close(v, PI * 400.0 * 10.0 - 2.0 * one_rim, 0.05, "both rims rounded (upstream bug)");
}

/// The server is immune to F-3B-2 on files it opens: `Session::open` restores the edge pool from the live B-reps, so
/// inspecting a document with a stored edge query changes nothing, and editing the fillet's radius rounds one rim.
#[test]
fn stored_edge_query_is_safe_to_inspect_and_edit_after_open() {
    use qymcad_core::feature::FeatureKind;
    use qymcad_core::refs::{Query, Ref};
    let mut s = Session::new_part();
    s.param_set("r", &Num::Value(2.0)).unwrap();
    let cyl = cylinder(&mut s, 0.0, 0.0, 40.0.into(), 10.0.into(), Op::Add);
    let rim = Sel::EdgesOf(Box::new(Sel::OfFeature { feature: cyl, role: Some(Role::CapEnd) }));
    let (fillet, _) = s.fillet(None, &rim, &n("r"), None).unwrap();
    let path = scratch("stored_edge_query_open.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let node = project.timeline.iter_mut().find(|n| n.id == fillet).unwrap();
    let FeatureKind::Fillet { edges, .. } = &mut node.kind else { panic!("not a fillet") };
    let cap = Query::OfFeature { feature: cyl, role: Some(qymcad_core::names::Role::CapEnd) };
    *edges = Ref::many(Query::Adjacent(Box::new(cap)));
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();

    let one_rim_r2 = cylinder_rim_fillet(20.0, 10.0, 2.0);
    let (mut o, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_close(volume(&o), one_rim_r2, 0.05, "opened as saved");
    o.topology(None, true).unwrap();
    o.select(None, Element::Edges, &Sel::Ids(vec![])).ok();
    assert_close(volume(&o), one_rim_r2, 0.05, "inspection does not change the geometry");
    o.param_set("r", &Num::Value(4.0)).unwrap();
    assert_close(volume(&o), cylinder_rim_fillet(20.0, 10.0, 4.0), 0.05, "radius edit rounds one rim");
    // and straight after opening, without an inspection first
    let (mut o, _) = Session::open(&path).unwrap();
    o.param_set("r", &Num::Value(4.0)).unwrap();
    assert_close(volume(&o), cylinder_rim_fillet(20.0, 10.0, 4.0), 0.05, "radius edit right after open");
}

// ---------------------------------------------------------------------------------------------------------------
// Revolve

fn revolve(sketch: Id, axis: AxisRef, angle: Num, direction: Direction, op: Op) -> Revolve {
    Revolve { sketch, profiles: None, axis, angle, direction, op, target: None, name: None }
}

/// Rectangle x ∈ [10, 20], y ∈ [0, 30] on XY about the sketch y axis: a tube π(R² − r²)h.
fn tube_sketch(s: &mut Session) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &15.0.into(), &15.0.into(), &10.0.into(), &30.0.into(), false).unwrap();
    sk
}

#[test]
fn revolve_tube_angles_and_axes() {
    let tube = PI * (400.0 - 100.0) * 30.0;
    for (axis, label) in [
        (AxisRef::SketchY, "sketch y"),
        (AxisRef::World(Axis::Y), "world Y"),
        (AxisRef::Through { origin: [0.0; 3], dir: [0.0, 1.0, 0.0] }, "datum"),
    ] {
        let mut s = Session::new_part();
        let sk = tube_sketch(&mut s);
        s.revolve(&revolve(sk, axis, 360.0.into(), Direction::Normal, Op::Add)).unwrap();
        assert_close(volume(&s), tube, 0.5, label);
    }
    // About a construction line of the sketch (the left side of a construction rectangle, x = 0).
    let mut s = Session::new_part();
    let sk = tube_sketch(&mut s);
    let lines = s.sketch_rect(sk, &5.0.into(), &15.0.into(), &10.0.into(), &30.0.into(), true).unwrap();
    s.revolve(&revolve(sk, AxisRef::Line(lines[3]), 360.0.into(), Direction::Normal, Op::Add)).unwrap();
    assert_close(volume(&s), tube, 0.5, "construction line");
    // A parametric angle.
    let mut s = Session::new_part();
    s.param_set("ang", &Num::Value(90.0)).unwrap();
    let sk = tube_sketch(&mut s);
    s.revolve(&revolve(sk, AxisRef::SketchY, n("ang"), Direction::Normal, Op::Add)).unwrap();
    assert_close(volume(&s), tube / 4.0, 0.5, "90°");
    s.param_set("ang", &Num::Value(270.0)).unwrap();
    assert_close(volume(&s), tube * 0.75, 0.5, "270° after param edit");
}

/// Direction semantics (F-3B-4): about +Y by the right-hand rule, +X turns towards −Z.
#[test]
fn revolve_direction() {
    for (dir, zmin, zmax) in [(Direction::Normal, -20.0, 0.0), (Direction::Reverse, 0.0, 20.0), (Direction::Symmetric, -20.0, 20.0)] {
        let mut s = Session::new_part();
        let sk = tube_sketch(&mut s);
        s.revolve(&revolve(sk, AxisRef::SketchY, 180.0.into(), dir, Op::Add)).unwrap();
        assert_close(volume(&s), PI * 300.0 * 30.0 / 2.0, 0.5, "half tube");
        let bb = bbox(&s);
        assert_close(bb[2], zmin, 0.05, &format!("{dir:?} zmin"));
        assert_close(bb[5], zmax, 0.05, &format!("{dir:?} zmax"));
    }
}

/// A groove cut by revolving x ∈ [15, 25], y ∈ [10, 20] out of the tube: − π(20² − 15²)·10.
#[test]
fn revolve_cut_groove() {
    let mut s = Session::new_part();
    let sk = tube_sketch(&mut s);
    s.revolve(&revolve(sk, AxisRef::SketchY, 360.0.into(), Direction::Normal, Op::Add)).unwrap();
    let g = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(g, &20.0.into(), &15.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    s.revolve(&revolve(g, AxisRef::SketchY, 360.0.into(), Direction::Normal, Op::Cut)).unwrap();
    assert_close(volume(&s), PI * 300.0 * 30.0 - PI * (400.0 - 225.0) * 10.0, 0.5, "grooved tube");
}

// ---------------------------------------------------------------------------------------------------------------
// Hole

fn hole(face: Sel, at: Option<[f64; 3]>, d: Num, depth: Option<Num>) -> Hole {
    Hole { body: None, face, at, diameter: d, depth, kind: HoleKind::Plain, dia2: None, depth2: None, name: None }
}

#[test]
fn holes_plain_blind_and_through() {
    let (a, b, h) = (40.0, 30.0, 10.0);
    let (mut s, _) = block(a, b, h);
    s.hole(&hole(top(), None, 6.0.into(), None)).unwrap();
    assert_close(volume(&s), a * b * h - PI * 9.0 * h, 1e-3, "through hole");
    // A second, blind hole beside it at a world position (z is projected onto the face).
    s.hole(&hole(top(), Some([12.0, 5.0, 0.0]), 4.0.into(), Some(3.0.into()))).unwrap();
    assert_close(volume(&s), a * b * h - PI * 9.0 * h - PI * 4.0 * 3.0, 1e-3, "plus a blind hole");
    let t = s.topology(None, false).unwrap();
    let wall = t.faces.iter().find(|f| f.kind == FaceKind::Cylinder && (f.radius.unwrap() - 2.0).abs() < 1e-6).expect("the d4 wall");
    let [o, _] = wall.axis.unwrap();
    assert_close(o[0], 12.0, 1e-6, "hole x");
    assert_close(o[1], 5.0, 1e-6, "hole y");
    // Each hole wall closes with a seam line; descriptions such as "along z" include it, QymCAD's fillet
    // ignores it (the rounded corners alone are removed).
    assert_eq!(t.edges.iter().filter(|e| e.seam).count(), 2);
    let v = volume(&s);
    s.fillet(None, &along_z(), &3.0.into(), None).unwrap();
    assert_close(v - volume(&s), (4.0 - PI) * 9.0 * h, 1e-3, "corner fillet next to seams");
}

/// Counterbore: d6 through + d10 × 3. Countersink: d6 through + cone d10 → d6 over 2.
#[test]
fn holes_counterbore_and_countersink() {
    let (a, b, h) = (40.0, 30.0, 10.0);
    let (mut s, _) = block(a, b, h);
    s.hole(&Hole { kind: HoleKind::Counterbore, dia2: Some(10.0.into()), depth2: Some(3.0.into()), ..hole(top(), None, 6.0.into(), None) })
        .unwrap();
    assert_close(volume(&s), a * b * h - PI * 9.0 * h - PI * (25.0 - 9.0) * 3.0, 1e-3, "counterbore");

    let (mut s, _) = block(a, b, h);
    s.hole(&Hole { kind: HoleKind::Countersink, dia2: Some(10.0.into()), depth2: Some(2.0.into()), ..hole(top(), None, 6.0.into(), None) })
        .unwrap();
    let frustum = PI * 2.0 / 3.0 * (25.0 + 15.0 + 9.0);
    assert_close(volume(&s), a * b * h - PI * 9.0 * h - (frustum - PI * 9.0 * 2.0), 1e-3, "countersink");

    let (mut s, _) = block(a, b, h);
    let e = s.hole(&Hole {
        kind: HoleKind::Counterbore,
        dia2: Some(5.0.into()),
        depth2: Some(3.0.into()),
        ..hole(top(), None, 6.0.into(), None)
    });
    assert!(matches!(e, Err(Error::Invalid(_))), "dia2 smaller than the hole: {e:?}");
}

#[test]
fn hole_diameter_follows_its_parameter() {
    let (a, b, h) = (40.0, 30.0, 10.0);
    let (mut s, _) = block(a, b, h);
    s.param_set("d", &Num::Value(6.0)).unwrap();
    s.hole(&hole(Sel::OfFeature { feature: s.info().timeline[1].id, role: Some(Role::CapEnd) }, None, n("d"), Some(n("d")))).unwrap();
    assert_close(volume(&s), a * b * h - PI * 9.0 * 6.0, 1e-3, "d6 × 6");
    s.param_set("d", &Num::Value(8.0)).unwrap();
    assert_close(volume(&s), a * b * h - PI * 16.0 * 8.0, 1e-3, "d8 × 8 after param edit");
    // A stored face query is safe in the app: reopened, it resolves against the faces stored in the file.
    let path = scratch("hole_gui.qcad");
    s.save(Some(&path)).unwrap();
    let v = gui_edit_param(&path, "d", "5");
    assert_close(v, a * b * h - PI * 6.25 * 5.0, 1e-3, "GUI edit d=5");
    // And in this server after reopening.
    let (mut o, _) = Session::open(&path).unwrap();
    o.param_set("d", &Num::Value(4.0)).unwrap();
    assert_close(volume(&o), a * b * h - PI * 4.0 * 4.0, 1e-3, "reopened, d=4");
}

// ---------------------------------------------------------------------------------------------------------------
// Shell, push face

/// Open at the top, wall t inside: a·b·h − (a − 2t)(b − 2t)(h − t).
#[test]
fn shell_open_top() {
    let (a, b, h, t) = (40.0, 30.0, 20.0, 2.0);
    let (mut s, _) = block(a, b, h);
    s.shell(None, Some(&top()), &t.into(), Side::Inward, None).unwrap();
    assert_close(volume(&s), a * b * h - (a - 2.0 * t) * (b - 2.0 * t) * (h - t), 1e-3, "inward shell");

    let (mut s, _) = block(a, b, h);
    let id = s.select(None, Element::Faces, &top()).unwrap().1;
    s.shell(None, Some(&Sel::Ids(id)), &t.into(), Side::Outward, None).unwrap();
    // Outward: the block becomes the cavity; walls grow outside (rounded outer edges are not hand-computable
    // exactly, so check the cavity through the bbox and a lower bound).
    let bb = bbox(&s);
    assert_close(bb[3] - bb[0], a + 2.0 * t, 0.05, "outward shell width");
    assert_close(bb[5], h, 0.05, "outward shell top stays");
}

#[test]
fn push_face_moves_the_top() {
    let (a, b, h) = (40.0, 30.0, 10.0);
    let (mut s, _) = block(a, b, h);
    s.param_set("lift", &Num::Value(5.0)).unwrap();
    s.push_face(None, &top(), &n("lift"), None).unwrap();
    assert_close(volume(&s), a * b * (h + 5.0), 1e-3, "pushed out by 5");
    s.param_set("lift", &Num::Value(-3.0)).unwrap();
    assert_close(volume(&s), a * b * (h - 3.0), 1e-3, "pulled in by 3");
}

// ---------------------------------------------------------------------------------------------------------------
// Arrays, mirror

#[test]
fn linear_arrays_of_a_boss() {
    let boss = PI * 25.0 * 5.0;
    let mut s = Session::new_part();
    cylinder(&mut s, 0.0, 0.0, 10.0.into(), 5.0.into(), Op::Add);
    let d1 = ArrayDir { dx: 20.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: 3.0.into() };
    s.linear_array(None, &d1, None, None).unwrap();
    assert_close(volume(&s), 3.0 * boss, 1e-3, "3 copies");
    assert_close(bbox(&s)[3], 45.0, 0.05, "last copy at x = 40");

    let mut s = Session::new_part();
    s.param_set("k", &Num::Value(2.0)).unwrap();
    cylinder(&mut s, 0.0, 0.0, 10.0.into(), 5.0.into(), Op::Add);
    let d2 = ArrayDir { dx: 0.0.into(), dy: 25.0.into(), dz: 0.0.into(), count: n("k") };
    s.linear_array(None, &d1, Some(&d2), None).unwrap();
    assert_close(volume(&s), 6.0 * boss, 1e-3, "3 × 2 grid");
    s.param_set("k", &Num::Value(3.0)).unwrap();
    assert_close(volume(&s), 9.0 * boss, 1e-3, "3 × 3 after the count parameter changed");
}

/// Step = 360/count for a full turn, angle/count otherwise (F-3B-5).
#[test]
fn circular_arrays_of_a_boss() {
    let boss = PI * 25.0 * 5.0;
    let mut s = Session::new_part();
    cylinder(&mut s, 30.0, 0.0, 10.0.into(), 5.0.into(), Op::Add);
    s.circular_array(None, &4.0.into(), &360.0.into(), None, None).unwrap();
    assert_close(volume(&s), 4.0 * boss, 1e-3, "4 around");
    let bb = bbox(&s);
    assert_close(bb[0], -35.0, 0.05, "a copy at 180°");

    let mut s = Session::new_part();
    cylinder(&mut s, 30.0, 0.0, 10.0.into(), 5.0.into(), Op::Add);
    s.circular_array(None, &3.0.into(), &90.0.into(), None, None).unwrap();
    assert_close(volume(&s), 3.0 * boss, 1e-3, "3 over 90°");
    // Copies at 0°, 30°, 60° (not 0/45/90): the top is the 60° copy, 30·sin60 + 5.
    assert_close(bbox(&s)[4], 30.0 * (PI / 3.0).sin() + 5.0, 0.05, "last copy at 60°");

    // About the axis of a cylindrical face of another body (a hub at the origin): the array follows the hub.
    let mut s = Session::new_part();
    let hub = cylinder(&mut s, 0.0, 0.0, 10.0.into(), 5.0.into(), Op::Add);
    let side = s.topology(Some(hub), false).unwrap().faces.iter().find(|f| f.kind == FaceKind::Cylinder).unwrap().id;
    cylinder(&mut s, 30.0, 0.0, 10.0.into(), 5.0.into(), Op::NewBody);
    let (arr, r) =
        s.circular_array(None, &4.0.into(), &360.0.into(), Some(&AxisRef::FaceAxis { body: Some(hub), face: side }), None).unwrap();
    let v = r.bodies.iter().find(|b| b.id == arr).unwrap().volume;
    assert_close(v, 4.0 * boss, 1e-3, "4 around the hub's axis");
}

#[test]
fn mirror_keeps_or_replaces() {
    // x ∈ [0, 10]: mirrored about YZ.
    let (mut s, _) = block_at(5.0.into(), 0.0.into(), 10.0.into(), 20.0.into(), 5.0.into());
    s.mirror(None, &PlaneRef::Base(BaseName::YZ), true, None).unwrap();
    assert_close(volume(&s), 2.0 * 1000.0, 1e-3, "keep: both halves");
    let bb = bbox(&s);
    assert_close(bb[0], -10.0, 0.05, "image reaches x = -10");

    let (mut s, _) = block_at(5.0.into(), 0.0.into(), 10.0.into(), 20.0.into(), 5.0.into());
    s.mirror(None, &PlaneRef::Base(BaseName::YZ), false, None).unwrap();
    assert_close(volume(&s), 1000.0, 1e-3, "image only");
    let bb = bbox(&s);
    assert_close(bb[3], 0.0, 0.05, "image only: x ≤ 0");

    // About a planar face (x = 10): the image is x ∈ [10, 20].
    let (mut s, b) = block_at(5.0.into(), 0.0.into(), 10.0.into(), 20.0.into(), 5.0.into());
    let face = s.select(None, Element::Faces, &Sel::Extreme { axis: Axis::X, max: true }).unwrap().1[0];
    s.mirror(None, &PlaneRef::Face { body: b, face }, true, None).unwrap();
    assert_close(volume(&s), 2000.0, 1e-3, "mirror about a face");
    assert_close(bbox(&s)[3], 20.0, 0.05, "image reaches x = 20");
}

// ---------------------------------------------------------------------------------------------------------------
// Contract

#[test]
fn too_big_fillet_is_rolled_back_with_the_reason() {
    let (mut s, _) = block(20.0, 20.0, 20.0);
    let before = s.info().timeline.len();
    let e = s.fillet(None, &along_z(), &15.0.into(), None).unwrap_err();
    let Error::Rebuild(lines) = &e else { panic!("expected a rebuild error, got {e}") };
    assert!(!lines.is_empty() && !lines[0].is_empty(), "{e}");
    eprintln!("too-big fillet: {e}");
    assert_eq!(s.info().timeline.len(), before, "nothing added");
    assert_close(volume(&s), 8000.0, 1e-6, "volume unchanged");
    // The document still takes a sensible fillet afterwards.
    s.fillet(None, &along_z(), &2.0.into(), None).unwrap();
}

#[test]
fn stale_and_foreign_ids_are_clear_errors() {
    let (mut s, b) = block(40.0, 30.0, 10.0);
    let top_id = s.select(None, Element::Faces, &top()).unwrap().1[0];
    let edge = s.topology(None, false).unwrap().edges[0].id;
    s.fillet(None, &along_z(), &2.0.into(), None).unwrap();
    // The old body is consumed now.
    let e = s.hole(&Hole { body: Some(b), ..hole(Sel::Ids(vec![top_id]), None, 3.0.into(), None) }).unwrap_err();
    assert!(e.to_string().contains("consumed"), "{e}");
    // An id that is not on the current body.
    let e = s.hole(&hole(Sel::Ids(vec![123456]), None, 3.0.into(), None)).unwrap_err();
    assert!(matches!(e, Error::NotFound(_)) && e.to_string().contains("topology"), "{e}");
    let e = s.fillet(None, &Sel::Ids(vec![edge, 99]), &1.0.into(), None).unwrap_err();
    assert!(e.to_string().contains("99]"), "{e}");
    // A selection that matches nothing is refused (QymCAD would round every edge, F-3B-3).
    let e = s.fillet(None, &Sel::EdgesOf(Box::new(Sel::OfFeature { feature: 999_999, role: None })), &1.0.into(), None).unwrap_err();
    assert!(e.to_string().contains("matched no edge"), "{e}");
    // A face selection that is ambiguous is refused for one-face features.
    let e = s.push_face(None, &Sel::Largest, &1.0.into(), None).unwrap_err();
    assert!(e.to_string().contains("exactly one"), "{e}");
}

/// Review #14: "nan", "inf" and "1e400" parse as non-finite numbers; they must be refused before they reach
/// QymCAD, and the document must stay as it was.
#[test]
fn non_finite_dimensions_are_refused() {
    let (mut s, _) = block(40.0, 30.0, 10.0);
    let before = (s.info().timeline.len(), volume(&s));
    for bad in ["nan", "inf", "-infinity", "1e400"] {
        let e = s.fillet(None, &along_z(), &n(bad), None).unwrap_err();
        assert!(matches!(e, Error::Expr(_)) && e.to_string().contains("finite"), "radius {bad}: {e}");
        let e = s.push_face(None, &top(), &n(bad), None).unwrap_err();
        assert!(e.to_string().contains("finite"), "push {bad}: {e}");
        let d = ArrayDir { dx: 20.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: n(bad) };
        let e = s.linear_array(None, &d, None, None).unwrap_err();
        assert!(e.to_string().contains("finite"), "count {bad}: {e}");
    }
    assert_eq!((s.info().timeline.len(), volume(&s)), before, "nothing changed");
}

/// Review #15: an extrude into a consumed body would branch a ghost chain (F-009); new_body with a target is
/// contradictory. Both are refused and nothing changes.
#[test]
fn extrude_target_must_be_a_current_body() {
    let (mut s, block_id) = block(40.0, 30.0, 10.0);
    s.fillet(None, &along_z(), &2.0.into(), None).unwrap();
    let before = (s.info().timeline.len(), volume(&s));
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &5.0.into(), false).unwrap();
    let cut = |target, op| Extrude {
        sketch: sk,
        profiles: None,
        height: 5.0.into(),
        op,
        direction: Direction::Normal,
        through: false,
        target,
        name: None,
    };
    let e = s.extrude(&cut(Some(block_id), Op::Cut)).unwrap_err();
    assert!(e.to_string().contains("consumed"), "consumed target: {e}");
    let e = s.extrude(&cut(Some(999_999), Op::Cut)).unwrap_err();
    assert!(matches!(e, Error::NotFound(_)), "unknown target: {e}");
    let current = s.result_bodies()[0].id;
    let e = s.extrude(&cut(Some(current), Op::NewBody)).unwrap_err();
    assert!(e.to_string().contains("new_body"), "new_body with a target: {e}");
    let rv = Revolve { target: Some(current), ..revolve(sk, AxisRef::SketchY, 90.0.into(), Direction::Normal, Op::NewBody) };
    let e = s.revolve(&rv).unwrap_err();
    assert!(e.to_string().contains("new_body"), "revolve new_body with a target: {e}");
    assert_eq!((s.info().timeline.len(), volume(&s)), (before.0 + 1, before.1), "only the sketch was added");
}

/// Review #5: "largest" on edges means the longest edge by its true length, as `topology` reports it. QymCAD
/// scores edges by chord |b − a|, which is 0 for a full circle and made the 10 mm seam win over the rims.
#[test]
fn largest_edge_is_the_longest_by_true_length() {
    let mut s = Session::new_part();
    cylinder(&mut s, 0.0, 0.0, 20.0.into(), 10.0.into(), Op::Add);
    let t = s.topology(None, false).unwrap();
    let len = |id: u32| t.edges.iter().find(|e| e.id == id).unwrap().length;
    let rims = s.select(None, Element::Edges, &Sel::Largest).unwrap().1;
    assert_eq!(rims.len(), 2, "both rims tie: {rims:?}");
    for r in &rims {
        assert_close(len(*r), 2.0 * PI * 10.0, 1e-9, "rim length");
    }
    // Inside a composition too: the longest edge of the top face is the top rim.
    let top_rim = Sel::And(Box::new(Sel::EdgesOf(Box::new(top()))), Box::new(Sel::Largest));
    let got = s.select(None, Element::Edges, &top_rim).unwrap().1;
    assert_eq!(got.len(), 1, "{got:?}");
    let e = t.edges.iter().find(|e| e.id == got[0]).unwrap();
    assert_close(e.mid[2], 10.0, 1e-9, "top rim z");
    // Faces are still ranked by area: a Ø20 disc (314.16) beats... the side (628.3) wins.
    let f = s.select(None, Element::Faces, &Sel::Largest).unwrap().1;
    assert_eq!(f.len(), 1);
    assert_eq!(t.faces.iter().find(|x| x.id == f[0]).unwrap().kind, FaceKind::Cylinder);
}

/// Review #27: QymCAD's expression parser recurses per '(' level, per unary sign and per '^'; an agent string
/// deep enough overflows the stack and kills the server. Expressions are bounded in length and nesting before
/// they reach it, for feature dimensions and for parameters alike. (Shallow over-limit strings here, so this
/// test fails cleanly rather than crashing when the gate is missing.)
#[test]
fn over_deep_or_long_expressions_are_refused() {
    let (mut s, _) = block(40.0, 30.0, 10.0);
    let deep = format!("{}1{}", "(".repeat(65), ")".repeat(65));
    let signs = format!("{}1", "-".repeat(65));
    let carets = format!("1{}", "^1".repeat(65));
    let long = format!("1{}", "+0".repeat(500));
    for bad in [&deep, &signs, &carets, &long] {
        let e = Num::Expr(bad.clone()).eval(&Default::default()).unwrap_err();
        assert!(matches!(e, Error::Expr(_)), "eval {}: {e}", &bad[..20]);
        let e = s.fillet(None, &along_z(), &n(bad), None).unwrap_err();
        assert!(matches!(e, Error::Expr(_)), "fillet {}: {e}", &bad[..20]);
        let e = s.param_set("x", &n(bad)).unwrap_err();
        assert!(matches!(e, Error::Expr(_)), "param_set {}: {e}", &bad[..20]);
    }
    assert!(s.params().is_empty());
    // At the limits it still works.
    let ok = format!("{}1{}", "(".repeat(64), ")".repeat(64));
    assert_eq!(Num::Expr(ok).eval(&Default::default()).unwrap(), 1.0);
    assert_eq!(Num::Expr(format!("{}1", "-".repeat(64))).eval(&Default::default()).unwrap(), 1.0);
}

/// Review #8: ids nested anywhere in a selection are checked against the body, as faces or as edges depending
/// on where they stand; a foreign id must not be dropped silently.
#[test]
fn nested_ids_are_validated() {
    let (mut s, _) = block(40.0, 30.0, 10.0);
    let t = s.topology(None, false).unwrap();
    let (edge, face) = (t.edges[0].id, t.faces[0].id);
    let ids = |v: Vec<u32>| Sel::Ids(v);
    let e = s.select(None, Element::Edges, &Sel::Union(vec![ids(vec![edge]), ids(vec![424242])])).unwrap_err();
    assert!(e.to_string().contains("424242"), "union: {e}");
    let e = s.select(None, Element::Edges, &Sel::EdgesOf(Box::new(ids(vec![edge])))).unwrap_err();
    assert!(e.to_string().contains("faces") && e.to_string().contains(&edge.to_string()), "an edge id where a face is expected: {e}");
    let e = s.select(None, Element::Edges, &Sel::TangentChain { seed: Box::new(ids(vec![face])), tol_deg: 5.0 }).unwrap_err();
    assert!(e.to_string().contains("edges"), "a face id as a chain seed: {e}");
    let e = s.select(None, Element::Faces, &Sel::Minus(Box::new(top()), Box::new(ids(vec![7])))).unwrap_err();
    assert!(e.to_string().contains("[7]"), "minus: {e}");
    // Valid nested ids still work.
    assert_eq!(s.select(None, Element::Edges, &Sel::EdgesOf(Box::new(ids(vec![face])))).unwrap().1.len(), 4);
}

/// Review #9: a union of many selections must not become a query ladder so deep that the saved document no
/// longer opens (RON's recursion limit; upstream refs.rs warns about `Union(Union(..))` ladders).
#[test]
fn a_wide_union_saves_and_reopens() {
    let (a, b, h, t) = (40.0, 30.0, 20.0, 2.0);
    let (mut s, _) = block(a, b, h);
    // 300 children: a left-deep ladder failed to save ("Exceeded recursion limit"); 150 still worked.
    let wide = Sel::Union((0..300).map(|_| Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 5.0 }).collect());
    s.shell(None, Some(&wide), &t.into(), Side::Inward, None).unwrap();
    let expected = a * b * h - (a - 2.0 * t) * (b - 2.0 * t) * (h - t);
    assert_close(volume(&s), expected, 1e-3, "shell open at the top");
    let path = scratch("wide_union.qcad");
    s.save(Some(&path)).unwrap();
    let (o, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_close(volume(&o), expected, 1e-3, "after reopening");
}

/// Review #9: selections have a size and depth budget, so no agent-written nesting can build a ladder either.
#[test]
fn selection_budget() {
    let (mut s, _) = block(40.0, 30.0, 10.0);
    let mut deep = top();
    for _ in 0..60 {
        deep = Sel::Minus(Box::new(deep), Box::new(Sel::Facing { dir: [0.0, 0.0, -1.0], tol_deg: 5.0 }));
    }
    let e = s.select(None, Element::Faces, &deep).unwrap_err();
    assert!(e.to_string().contains("nested"), "{e}");
    let wide = Sel::Union((0..600).map(|_| top()).collect());
    let e = s.select(None, Element::Faces, &wide).unwrap_err();
    assert!(e.to_string().contains("parts"), "{e}");
    // Many ids are one flat list, not a tree: a 600-id union is fine.
    let id = s.select(None, Element::Faces, &top()).unwrap().1[0];
    let ids = Sel::Union((0..600).map(|_| Sel::Ids(vec![id])).collect());
    assert_eq!(s.select(None, Element::Faces, &ids).unwrap().1, vec![id]);
}

/// Review #7: an array is bounded to 1000 copies in total, at creation and when a parameter edit changes a
/// count expression (that edit is refused and rolled back).
#[test]
fn array_copies_are_bounded() {
    let boss = PI * 1.0 * 2.0; // Ø2 × 2
    let mut s = Session::new_part();
    s.param_set("k", &Num::Value(2.0)).unwrap();
    cylinder(&mut s, 0.0, 0.0, 2.0.into(), 2.0.into(), Op::Add);
    let dir = |count: Num, dx: f64, dy: f64| ArrayDir { dx: dx.into(), dy: dy.into(), dz: 0.0.into(), count };
    // 33 × 33 = 1089 > 1000.
    let e = s.linear_array(None, &dir(33.0.into(), 3.0, 0.0), Some(&dir(33.0.into(), 0.0, 3.0)), None).unwrap_err();
    assert!(e.to_string().contains("1000"), "creation: {e}");
    let e = s.circular_array(None, &1001.0.into(), &360.0.into(), None, None).unwrap_err();
    assert!(e.to_string().contains("1000"), "circular: {e}");
    s.linear_array(None, &dir(n("k"), 3.0, 0.0), None, None).unwrap();
    assert_close(volume(&s), 2.0 * boss, 1e-6, "k = 2");
    let e = s.param_set("k", &Num::Value(1001.0)).unwrap_err();
    assert!(e.to_string().contains("1000"), "param_set: {e}");
    assert_eq!(s.params()[0].value, 2.0, "parameter rolled back");
    assert_close(volume(&s), 2.0 * boss, 1e-6, "geometry unchanged");
    s.param_set("k", &Num::Value(4.0)).unwrap();
    assert_close(volume(&s), 4.0 * boss, 1e-6, "k = 4 still fine");
}

/// Review #1: a face axis without a body means the face of the CURRENT body of the active part, like every
/// other default — not the last result body of the document, which in a multi-part document can belong to
/// another part.
#[test]
fn face_axis_default_body_is_the_current_body_of_the_active_part() {
    // Part A (active): a Ø10 hub on the world Z axis. Part B, added after it: a 5 mm box.
    let mut s = Session::new_part();
    let hub = cylinder(&mut s, 0.0, 0.0, 10.0.into(), 30.0.into(), Op::Add);
    let path = scratch("two_parts.qcad");
    s.save(Some(&path)).unwrap();
    {
        let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
        let part_a = project.active_component;
        let shapes = {
            let _g = qymcad_kernel::kernel_gate();
            breps.into_iter().filter_map(|(id, b)| qymcad_kernel::Shape::from_brep_bytes(&b).map(|sh| (id, sh))).collect()
        };
        project.set_active_component(Some(project.root));
        let part_b = project.add_part("B");
        project.set_active_component(Some(part_b));
        project.add_box(5.0, 5.0, 5.0);
        let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, shapes);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        for (id, f) in report.built {
            project.set_body_faces(id, f);
        }
        project.set_active_component(part_a);
        let breps: Vec<_> = {
            let _g = qymcad_kernel::kernel_gate();
            shapes.iter().filter_map(|(id, sh)| sh.to_brep_bytes().map(|b| (*id, b))).collect()
        };
        qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    }
    let (mut s, _) = Session::open(&path).unwrap();
    let last = s.result_bodies().last().unwrap().id;
    assert_ne!(last, hub, "setup: the document's last result body is part B's box");
    let side = s.topology(None, false).unwrap().faces.iter().find(|f| f.kind == FaceKind::Cylinder).unwrap().id;
    // A tube revolved about the hub's axis, the axis given by the face alone.
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XZ), None).unwrap();
    s.sketch_rect(sk, &15.0.into(), &15.0.into(), &10.0.into(), &30.0.into(), false).unwrap();
    let axis = AxisRef::FaceAxis { body: None, face: side };
    let (tube, r) = s.revolve(&revolve(sk, axis, 360.0.into(), Direction::Normal, Op::NewBody)).unwrap();
    let v = r.bodies.iter().find(|b| b.id == tube).unwrap().volume;
    assert_close(v, PI * (400.0 - 100.0) * 30.0, 0.5, "tube about the hub's axis");
}

/// Review #20: a rolled-back feature leaves every existing body bit-identical. The failed edit makes the rebuild
/// retry with everything dirty (F-005); after a parameter edit a full rebuild does not reproduce the bodies
/// exactly (F-017: the pocket comes out 0.001 mm different), so keeping the retried shapes would move old
/// geometry although "nothing changed".
#[test]
fn a_rolled_back_feature_leaves_old_bodies_bit_identical() {
    let mut s = Session::new_part();
    s.param_set("t", &Num::Value(6.0)).unwrap();
    let plate = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(plate, &0.0.into(), &0.0.into(), &60.0.into(), &40.0.into(), false).unwrap();
    s.extrude(&extrude(plate, n("t"), Op::Add)).unwrap();
    let (top_plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &n("t"), None).unwrap();
    let pocket = s.sketch_create(&PlaneRef::Plane(top_plane), None).unwrap();
    s.sketch_rect(pocket, &0.0.into(), &0.0.into(), &30.0.into(), &16.0.into(), false).unwrap();
    s.extrude(&Extrude { direction: Direction::Reverse, ..extrude(pocket, 3.0.into(), Op::Cut) }).unwrap();
    s.param_set("t", &Num::Value(10.0)).unwrap();
    let before: Vec<(Id, u64)> = s.result_bodies().iter().map(|b| (b.id, b.volume.to_bits())).collect();
    let e = s.fillet(None, &along_z(), &100.0.into(), None).unwrap_err();
    assert!(matches!(e, Error::Rebuild(_)), "{e}");
    let after: Vec<(Id, u64)> = s.result_bodies().iter().map(|b| (b.id, b.volume.to_bits())).collect();
    let show = |v: &[(Id, u64)]| v.iter().map(|(i, b)| format!("{i}: {}", f64::from_bits(*b))).collect::<Vec<_>>();
    assert_eq!(after, before, "volumes after rollback {:?} vs before {:?}", show(&after), show(&before));
}
