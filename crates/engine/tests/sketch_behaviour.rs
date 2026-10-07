//! Contract of sketch editing: `sketch_constrain` (refuses over-constraining, rolls back), references, removal,
//! rebuild of features built from an edited sketch.

mod common;
use common::*;
use qymcad_engine::*;

fn n(s: &str) -> Num {
    Num::Expr(s.into())
}

fn v(x: f64) -> Num {
    Num::Value(x)
}

fn c(kind: ConstraintKind, refs: &[SketchRef], value: Option<Num>) -> ConstrainSpec {
    ConstrainSpec { kind, refs: refs.to_vec(), value, axis: DistAxis::Aligned, reference: false }
}

fn id(x: Id) -> SketchRef {
    SketchRef::Id(x)
}

const ORIGIN: SketchRef = SketchRef::Frame(FrameRef::Origin);

/// An undimensioned right triangle (0,0) (30,0) (0,20): 6 degrees of freedom. Returns (session, sketch,
/// lines [bottom, hypotenuse, left], points [p0, p1, p2]).
fn free_triangle() -> (Session, Id, Vec<Id>, Vec<Id>) {
    let mut s = Session::new_part();
    s.param_set("a", &v(30.0)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let pts = vec![Xy(v(0.0), v(0.0)), Xy(v(30.0), v(0.0)), Xy(v(0.0), v(20.0))];
    let added = s.sketch_polyline(sk, &PolylineSpec { points: pts, closed: true, construction: false, dimensioned: false }).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (6, 0));
    (s, sk, added.entities, added.points)
}

fn extrude(s: &mut Session, sk: Id) -> f64 {
    let (_, r) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: v(2.0),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    r.bodies[0].volume
}

/// Fix the triangle at the origin with a horizontal base and a vertical side: 2 freedoms left (the sizes).
fn anchored() -> (Session, Id, Vec<Id>, Vec<Id>) {
    let (mut s, sk, l, p) = free_triangle();
    s.sketch_constrain(sk, &c(ConstraintKind::Coincident, &[id(p[0]), ORIGIN], None)).unwrap();
    s.sketch_constrain(sk, &c(ConstraintKind::Horizontal, &[id(l[0])], None)).unwrap();
    let r = s.sketch_constrain(sk, &c(ConstraintKind::Vertical, &[id(l[2])], None)).unwrap();
    assert_eq!(r.dof, (2, 0));
    (s, sk, l, p)
}

#[test]
fn constraints_and_dimensions_fully_define_a_free_sketch() {
    let (mut s, sk, l, _) = anchored();
    let r = s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l[0])], Some(n("a")))).unwrap();
    assert_eq!(r.dof, (1, 0));
    let r = s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l[2])], Some(v(20.0)))).unwrap();
    assert_eq!(r.dof, (0, 0));
    let info = s.sketch_detail(sk).unwrap();
    let dim = &info.constraints[r.index.unwrap()];
    assert_eq!((dim.kind.as_str(), dim.value), ("distance", Some(20.0)));
    assert_close(extrude(&mut s, sk), 600.0, 1e-3, "triangle");
    let rb = s.param_set("a", &v(45.0)).unwrap();
    assert_close(rb.bodies[0].volume, 900.0, 1e-3, "a=45");
}

#[test]
fn a_dimension_moves_free_geometry() {
    let (mut s, sk, l, p) = anchored();
    s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(p[0]), id(p[1])], Some(v(40.0)))).unwrap();
    let d = s.sketch_detail(sk).unwrap();
    let p1 = d.points.iter().find(|q| q.id == p[1]).unwrap();
    assert_close(p1.x, 40.0, 1e-6, "p1 moved to the new length");
    // An angle between the base and the hypotenuse.
    let r = s.sketch_constrain(sk, &c(ConstraintKind::Angle, &[id(l[0]), id(l[1])], Some(v(30.0)))).unwrap();
    assert_eq!(r.dof, (0, 0));
    let d = s.sketch_detail(sk).unwrap();
    let p2 = d.points.iter().find(|q| q.id == p[2]).unwrap();
    assert_close(p2.y, 40.0 * 30f64.to_radians().tan(), 1e-6, "height from the 30° angle");
}

#[test]
fn a_conflicting_dimension_is_refused_and_rolled_back() {
    let (mut s, sk, l, p) = anchored();
    s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l[0])], Some(v(30.0)))).unwrap();
    s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l[2])], Some(v(20.0)))).unwrap();
    let before = s.sketch_detail(sk).unwrap();
    // The hypotenuse is already determined (√(30²+20²) ≈ 36.06): 50 contradicts, 36.06 duplicates.
    let e = s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(p[1]), id(p[2])], Some(v(50.0)))).unwrap_err();
    assert!(matches!(&e, Error::Invalid(m) if m.contains("over-constrains") && m.contains("reference")), "{e}");
    assert_eq!(s.sketch_detail(sk).unwrap(), before, "nothing changed");
    // As a reference dimension it is accepted and measures.
    let mut spec = c(ConstraintKind::Distance, &[id(l[1])], None);
    spec.reference = true;
    let r = s.sketch_constrain(sk, &spec).unwrap();
    assert_eq!(r.dof, (0, 0));
    let dim = &s.sketch_detail(sk).unwrap().constraints[r.index.unwrap()];
    assert!(dim.reference);
    assert_close(dim.value.unwrap(), (30f64.powi(2) + 20f64.powi(2)).sqrt(), 1e-6, "measured");
}

#[test]
fn a_contradicting_geometric_constraint_is_refused() {
    let (mut s, sk, l, _) = anchored();
    s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l[0])], Some(v(30.0)))).unwrap();
    s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l[2])], Some(v(20.0)))).unwrap();
    let before = s.sketch_detail(sk).unwrap();
    let e = s.sketch_constrain(sk, &c(ConstraintKind::Horizontal, &[id(l[1])], None)).unwrap_err();
    assert!(e.to_string().contains("contradicted"), "{e}");
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
}

#[test]
fn an_implied_constraint_is_not_added() {
    let (mut s, sk, l, _) = anchored();
    let n_before = s.sketch_detail(sk).unwrap().constraints.len();
    let r = s.sketch_constrain(sk, &c(ConstraintKind::Horizontal, &[id(l[0])], None)).unwrap();
    assert_eq!(r.index, None, "{r:?}");
    assert!(r.note.unwrap().contains("already implied"));
    assert_eq!(s.sketch_detail(sk).unwrap().constraints.len(), n_before);
}

#[test]
fn bad_references_are_reported() {
    let (mut s, sk, l, p) = free_triangle();
    let e = s.sketch_constrain(sk, &c(ConstraintKind::Horizontal, &[id(9999)], None)).unwrap_err();
    assert!(matches!(e, Error::NotFound(_)), "{e}");
    let e = s.sketch_constrain(9999, &c(ConstraintKind::Horizontal, &[id(l[0])], None)).unwrap_err();
    assert!(matches!(e, Error::NotFound(_)), "{e}");
    let e = s.sketch_constrain(sk, &c(ConstraintKind::Angle, &[id(p[0]), id(p[1])], Some(v(30.0)))).unwrap_err();
    assert!(matches!(&e, Error::Invalid(m) if m.contains("[line, line]")), "{e}");
    let e = s.sketch_constrain(sk, &c(ConstraintKind::Horizontal, &[id(l[0])], Some(v(1.0)))).unwrap_err();
    assert!(e.to_string().contains("takes no value"), "{e}");
    let e = s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l[0])], None)).unwrap_err();
    assert!(e.to_string().contains("needs a `value`"), "{e}");
    let e = s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l[0])], Some(n("nope")))).unwrap_err();
    assert!(matches!(e, Error::Expr(_)), "{e}");
    assert!(matches!(s.sketch_remove_entity(sk, 9999), Err(Error::NotFound(_))));
    assert!(matches!(s.sketch_remove_constraint(sk, 9999), Err(Error::NotFound(_))));
    assert_eq!(s.sketch_info(sk).unwrap().dof, (6, 0), "failed calls changed nothing");
}

#[test]
fn a_line_ending_on_an_arc_can_be_made_tangent() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let arc = s
        .sketch_arc(
            sk,
            &ArcSpec {
                cx: v(0.0),
                cy: v(0.0),
                r: Some(v(10.0)),
                start_angle: Some(v(0.0)),
                end_angle: Some(v(90.0)),
                start: None,
                end: None,
                ccw: true,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    // A line from the arc's start (10, 0) going down, slightly off tangent.
    let line = s
        .sketch_line(sk, &LineSpec { x1: v(10.0), y1: v(0.0), x2: v(10.5), y2: v(-20.0), construction: false, dimensioned: false })
        .unwrap();
    let dof = s.sketch_info(sk).unwrap().dof;
    let r = s.sketch_constrain(sk, &c(ConstraintKind::Tangent, &[id(line.entities[0]), id(arc.entities[0])], None)).unwrap();
    assert!(r.index.is_some());
    assert_eq!(r.dof.0, dof.0 - 1);
    let d = s.sketch_detail(sk).unwrap();
    let end = d.points.iter().find(|p| p.id == line.points[1]).unwrap();
    assert_close(end.x, 10.0, 1e-6, "the tangent at (10, 0) is vertical");
}

#[test]
fn editing_a_sketch_rebuilds_its_features() {
    let (mut s, sk, l, _) = anchored();
    assert_close(extrude(&mut s, sk), 600.0, 1e-3, "before");
    let (_, r) = s.sketch_edit(sk, |s| s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l[0])], Some(v(45.0))))).unwrap();
    let r = r.expect("the extrude depends on the sketch");
    assert_close(r.bodies[0].volume, 900.0, 1e-3, "after the dimension");
    assert_close(s.result_bodies()[0].volume, 900.0, 1e-3, "kept");
}

#[test]
fn removing_entities_and_constraints() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let lines = s.sketch_rect(sk, &v(5.0), &v(5.0), &v(10.0), &v(10.0), false).unwrap();
    let c = s.sketch_circle(sk, &v(30.0), &v(0.0), &v(4.0), false).unwrap();
    for l in &lines {
        s.sketch_remove_entity(sk, *l).unwrap();
    }
    let d = s.sketch_detail(sk).unwrap();
    assert_eq!(d.entities.len(), 1, "only the circle is left");
    let circle_pts = 1;
    assert_eq!(d.points.iter().filter(|p| p.role.is_none()).count(), circle_pts, "the rectangle's centre went with it: {:?}", d.points);
    assert_eq!(d.summary.dof, (0, 0));
    // Remove the circle's diameter: one freedom appears.
    let di = d.constraints.iter().find(|k| k.kind == "diameter").unwrap().index;
    s.sketch_remove_constraint(sk, di).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (1, 0));
    // The frame's anchors stay.
    let fixed = s.sketch_detail(sk).unwrap().constraints.iter().find(|k| k.kind == "fix").unwrap().index;
    assert!(matches!(s.sketch_remove_constraint(sk, fixed), Err(Error::Invalid(_))));
    s.sketch_remove_entity(sk, c).unwrap();
    assert!(s.sketch_detail(sk).unwrap().points.iter().all(|p| p.role.is_some()));
}

#[test]
fn constant_names_are_not_parameters() {
    let mut s = Session::new_part();
    for bad in ["e", "PI", "tau"] {
        let e = s.param_set(bad, &v(1.0)).unwrap_err();
        assert!(e.to_string().contains("built-in constant"), "{e}");
    }
}

/// Review #2: a parameter edit that leaves a sketch unsolvable is refused and rolled back. A line's angle to the
/// x axis is driven by `a`; 200° is beyond what an angle between lines can be (0..180).
#[test]
fn a_parameter_edit_that_breaks_a_sketch_is_refused() {
    let mut s = Session::new_part();
    s.param_set("a", &v(45.0)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let l =
        s.sketch_line(sk, &LineSpec { x1: v(0.0), y1: v(0.0), x2: v(10.0), y2: v(10.0), construction: false, dimensioned: false }).unwrap();
    s.sketch_constrain(sk, &c(ConstraintKind::Angle, &[id(l.entities[0]), SketchRef::Frame(FrameRef::XAxis)], Some(n("a")))).unwrap();
    let before = s.sketch_detail(sk).unwrap();
    let e = s.param_set("a", &v(200.0)).unwrap_err();
    assert!(e.to_string().contains("does not solve"), "{e}");
    assert_eq!(s.params()[0].value, 45.0, "parameter restored");
    assert_eq!(s.sketch_detail(sk).unwrap(), before, "sketch restored");
}

/// Review #9: a document written elsewhere may hold a parameter named `e`; it cannot be set, but it can be deleted.
#[test]
fn an_old_parameter_with_a_constant_name_can_be_deleted() {
    let mut p = qymcad_core::model::Project::default();
    p.new_document();
    p.parameters.push(qymcad_core::model::Param { name: "e".into(), expr: "3".into(), value: 3.0 });
    let path = scratch("param_e.qcad");
    qymcad_io::save_project_guarded_with_brep(&p, path.to_str().unwrap(), &[]).unwrap();
    let (mut s, _) = Session::open(&path).unwrap();
    assert!(s.param_set("e", &v(4.0)).is_err());
    s.param_delete("e").unwrap();
    assert!(s.params().is_empty());
}

/// Review #5: QymCAD's `delete_entities` keeps only points that entities use, so removing any entity used to drop
/// the control points of every spline in the sketch. Removing a circle must leave a spline intact.
#[test]
fn removing_an_entity_keeps_splines() {
    use qymcad_core::geom::Point2;
    let mut p = qymcad_core::model::Project::default();
    p.new_document();
    let sid = p.add_sketch("S", vec![], None);
    p.add_sketch_node(sid, "S".to_string());
    let si = p.sketch_index(sid).unwrap();
    let pts = vec![Point2::new(0.0, 0.0), Point2::new(10.0, 5.0), Point2::new(20.0, 0.0)];
    p.add_spline(si, pts, qymcad_core::feature::Ends::Open, qymcad_core::feature::Purpose::Real);
    let path = scratch("spline_keep.qcad");
    qymcad_io::save_project_guarded_with_brep(&p, path.to_str().unwrap(), &[]).unwrap();
    let (mut s, _) = Session::open(&path).unwrap();
    let circle = s.sketch_circle(sid, &v(40.0), &v(0.0), &v(6.0), false).unwrap();
    s.sketch_remove_entity(sid, circle).unwrap();
    let sk = &s.project().sketches[si];
    assert_eq!(sk.splines.len(), 1);
    for id in &sk.splines[0].points {
        assert!(sk.points.iter().any(|q| q.id == *id), "spline control point {id} was dropped");
    }
}

/// Review #6: a distance between two lines is only meaningful when they are parallel. The line (0,5)-(5,0)
/// crosses the x axis; "distance 5 to the x axis" must make it parallel at y = 5, not measure one endpoint.
#[test]
fn a_distance_between_lines_makes_them_parallel() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let l =
        s.sketch_line(sk, &LineSpec { x1: v(0.0), y1: v(5.0), x2: v(5.0), y2: v(0.0), construction: false, dimensioned: false }).unwrap();
    let r = s
        .sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l.entities[0]), SketchRef::Frame(FrameRef::XAxis)], Some(v(5.0))))
        .unwrap();
    assert_eq!(r.dof, (2, 0), "4 freedoms − parallel − distance");
    let d = s.sketch_detail(sk).unwrap();
    for p in &l.points {
        let q = d.points.iter().find(|q| q.id == *p).unwrap();
        assert_close(q.y, 5.0, 1e-6, "both ends at y = 5");
    }
    // Already parallel: no second parallel is added.
    let r = s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l.entities[0]), SketchRef::Frame(FrameRef::XAxis)], Some(v(5.0))));
    assert!(r.is_err(), "the same distance again over-constrains");
    // A reference distance between non-parallel lines is refused (it would have to add a constraint).
    let l2 =
        s.sketch_line(sk, &LineSpec { x1: v(0.0), y1: v(-5.0), x2: v(5.0), y2: v(-9.0), construction: false, dimensioned: false }).unwrap();
    let mut spec = c(ConstraintKind::Distance, &[id(l2.entities[0]), SketchRef::Frame(FrameRef::XAxis)], None);
    spec.reference = true;
    let e = s.sketch_constrain(sk, &spec).unwrap_err();
    assert!(e.to_string().contains("parallel"), "{e}");
}

#[test]
fn a_line_distance_refuses_a_contradictory_rank_dependent_parallel() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let l =
        s.sketch_line(sk, &LineSpec { x1: v(0.0), y1: v(5.0), x2: v(0.0), y2: v(15.0), construction: false, dimensioned: false }).unwrap();
    s.sketch_constrain(sk, &c(ConstraintKind::Vertical, &[id(l.entities[0])], None)).unwrap();
    // The line length is 15 - 5 = 10; vertical + length leaves only its two translations free.
    s.sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l.entities[0])], Some(v(15.0 - 5.0)))).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (2, 0));
    let before = s.sketch_detail(sk).unwrap();
    let e = s
        .sketch_constrain(sk, &c(ConstraintKind::Distance, &[id(l.entities[0]), SketchRef::Frame(FrameRef::XAxis)], Some(v(5.0))))
        .unwrap_err();
    assert!(e.to_string().contains("parallel") && e.to_string().contains("contradicted"), "{e}");
    assert_eq!(s.sketch_detail(sk).unwrap(), before, "the entire constrain call is rolled back");
}

fn param_arc() -> (Session, Id, Added) {
    let mut s = Session::new_part();
    s.param_set("r", &v(20.0)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let arc = s
        .sketch_arc(
            sk,
            &ArcSpec {
                cx: v(0.0),
                cy: v(0.0),
                r: Some(n("r")),
                start_angle: Some(v(30.0)),
                end_angle: Some(v(120.0)),
                start: None,
                end: None,
                ccw: true,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    (s, sk, arc)
}

/// Review #8: QymCAD refreshes reference radius/diameter dimensions from circles only, so on an arc the shown
/// value would go stale after the arc changes.
#[test]
fn a_reference_radius_on_an_arc_does_not_go_stale() {
    let (mut s, sk, arc) = param_arc();
    let mut spec = c(ConstraintKind::Radius, &[id(arc.entities[0])], None);
    spec.reference = true;
    match s.sketch_constrain(sk, &spec) {
        Err(e) => assert!(e.to_string().contains("arc"), "{e}"),
        Ok(r) => {
            s.param_set("r", &v(25.0)).unwrap();
            let dim = &s.sketch_detail(sk).unwrap().constraints[r.index.unwrap()];
            assert_close(dim.value.unwrap(), 25.0, 1e-6, "reference radius after r = 25");
        }
    }
}

/// Review #3 (F-042): a circle drawn on an ordinary point (a polyline vertex) takes that point as its centre; a
/// second circle there gets a node of its own (one radius variable per centre). Either way the sketch stays fully
/// defined without redundancy.
#[test]
fn circles_on_a_vertex_share_or_get_their_centre() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let pl = s
        .sketch_polyline(
            sk,
            &PolylineSpec {
                points: vec![Xy(v(0.0), v(0.0)), Xy(v(10.0), v(0.0)), Xy(v(10.0), v(8.0))],
                closed: true,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    let c1 = s.sketch_circle(sk, &v(10.0), &v(0.0), &v(4.0), false).unwrap();
    let c2 = s.sketch_circle(sk, &v(10.0), &v(0.0), &v(2.0), false).unwrap();
    let d = s.sketch_detail(sk).unwrap();
    let centre = |e: Id| d.entities.iter().find(|x| x.id == e).unwrap().points[0];
    assert_eq!(centre(c1), pl.points[1], "the first circle's centre is the vertex");
    assert_ne!(centre(c2), centre(c1), "a second circle there has its own centre");
    assert_eq!(d.summary.dof, (0, 0));
}
