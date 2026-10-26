//! Native trim, extend and break with boundary associations and independent area oracles.
mod common;
use common::*;
use qymcad_engine::*;
fn v(x: f64) -> Num {
    Num::Value(x)
}
fn e(x: &str) -> Num {
    Num::Expr(x.into())
}
fn line(s: &mut Session, sk: Id, a: [Num; 2], b: [Num; 2], construction: bool, dimensioned: bool) -> Added {
    s.sketch_line(sk, &LineSpec { x1: a[0].clone(), y1: a[1].clone(), x2: b[0].clone(), y2: b[1].clone(), construction, dimensioned })
        .unwrap()
}
#[test]
fn trim_circle_to_half_disk_tracks_boundary_and_radius_in_app() {
    let mut s = Session::new_part();
    s.param_set("R", &v(5.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let c = s.sketch_circle(sk, &v(0.), &v(0.), &e("2*R"), false).unwrap();
    line(&mut s, sk, [v(-20.), v(0.)], [v(20.), v(0.)], true, true);
    let out = s.sketch_trim(sk, &CurvePickSpec { curve: EntityRef::Id(c), at: [v(0.), v(-5.)] }).unwrap();
    assert!(!out.created_entities.is_empty());
    assert_eq!(out.dof.1, 0, "the trim leaves no redundant constraint");
    let arc = s.sketch_detail(sk).unwrap().entities.into_iter().find(|x| x.kind == "arc").unwrap();
    let d = s.sketch_detail(sk).unwrap();
    for id in &arc.points[1..] {
        assert!(
            d.constraints.iter().any(|c| c.kind == "point_on_line" && c.points.contains(id)),
            "trimmed end {id} stays on the cutting line"
        );
    }
    let a = d.points.iter().find(|p| p.id == arc.points[1]).unwrap();
    let b = d.points.iter().find(|p| p.id == arc.points[2]).unwrap();
    line(&mut s, sk, [v(a.x), v(a.y)], [v(b.x), v(b.y)], false, false);
    let (_, r) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: v(2.),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert_close(r.bodies[0].volume, std::f64::consts::PI * 25., 1e-6, "half disk extrusion");
    let path = scratch("stage2-trim.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_close(r.bodies[0].volume, std::f64::consts::PI * 25., 1e-6, "trim reopen");
    let r = opened.param_set("R", &v(6.)).unwrap();
    assert!(r.errors.is_empty());
    assert_close(r.bodies[0].volume, std::f64::consts::PI * 36., 1e-6, "trim radius edit");
    assert_close(gui_edit_param(&path, "R", "6"), std::f64::consts::PI * 36., 1e-6, "trim app radius edit");
}
#[test]
fn extend_free_end_attaches_to_formula_boundary_and_refuses_held_end() {
    let mut s = Session::new_part();
    s.param_set("L", &v(20.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let a = line(&mut s, sk, [v(0.), v(0.)], [v(10.), v(0.)], false, false);
    let b = line(&mut s, sk, [e("L"), v(-5.)], [e("L"), v(10.)], true, true);
    let out = s
        .sketch_extend(
            sk,
            &ExtendSpec {
                curve: EntityRef::Id(a.entities[0]),
                at: [v(10.), v(0.)],
                target: Some(EntityRef::Id(b.entities[0])),
                both: false,
            },
        )
        .unwrap();
    assert!(!out.free_points.is_empty());
    let d = s.sketch_detail(sk).unwrap();
    let p = d.points.iter().find(|p| p.id == a.points[1]).unwrap();
    assert_close(p.x, 20., 1e-6, "extended x");
    assert!(d.constraints.iter().any(|c| c.kind == "point_on_line" && c.points.contains(&p.id)), "extended end stays on the boundary line");
    let held = line(&mut s, sk, [v(30.), v(0.)], [v(40.), v(0.)], false, true);
    assert!(s
        .sketch_extend(sk, &ExtendSpec { curve: EntityRef::Id(held.entities[0]), at: [v(40.), v(0.)], target: None, both: false })
        .is_err());
}
#[test]
fn break_line_keeps_original_id_and_reports_free_cut_point() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let a = line(&mut s, sk, [v(0.), v(0.)], [v(10.), v(0.)], false, true);
    let out =
        s.sketch_break(sk, &BreakSpec { curve: EntityRef::Id(a.entities[0]), at: Some([v(4.), v(0.)]), intersections: false }).unwrap();
    assert_eq!(out.created_entities.len(), 1);
    assert!(s.sketch_detail(sk).unwrap().entities.iter().any(|x| x.id == a.entities[0]));
    assert!(out.dof.0 > 0);
    assert!(!out.free_points.is_empty());
}

fn extrude_volume(s: &mut Session, sk: Id) -> f64 {
    let (_, r) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: v(2.),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r.bodies[0].volume
}

/// Area of the part of a circle of radius r above the chord y = c: r²·acos(c/r) − c·√(r² − c²).
fn segment(r: f64, c: f64) -> f64 {
    r * r * (c / r).acos() - c * (r * r - c * c).sqrt()
}

#[test]
fn a_trimmed_circle_follows_its_cutting_line_after_open_and_in_app() {
    let mut s = Session::new_part();
    s.param_set("R", &v(5.)).unwrap();
    s.param_set("Cut", &v(1.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let c = s.sketch_circle(sk, &v(0.), &v(0.), &e("2*R"), false).unwrap();
    // The cutter is a real line, so the trimmed arc and the kept part of the cutter close the profile.
    let cutter = line(&mut s, sk, [v(-20.), e("Cut")], [v(20.), e("Cut")], false, true);
    let out = s.sketch_trim(sk, &CurvePickSpec { curve: EntityRef::Id(c), at: [v(0.), v(-5.)] }).unwrap();
    assert!(out.dropped_formulas.is_empty(), "the radius formula survives the trim: {:?}", out.dropped_formulas);
    // Trim the cutter's two tails outside the circle so one closed region is left.
    for x in [-15., 15.] {
        s.sketch_trim(sk, &CurvePickSpec { curve: EntityRef::Id(cutter.entities[0]), at: [v(x), v(1.)] }).ok();
    }
    let d = s.sketch_detail(sk).unwrap();
    let remaining_lines: Vec<_> = d.entities.iter().filter(|x| x.kind == "line").collect();
    assert_eq!(remaining_lines.len(), 1, "only the chord of the cutter is left: {remaining_lines:?}");
    assert_eq!(d.summary.dof, (0, 0), "trim keeps a fully defined sketch fully defined");
    assert_close(extrude_volume(&mut s, sk), 2. * segment(5., 1.), 1e-6, "circular segment above y=1");
    let path = scratch("stage2-trim-follow.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_close(
        opened.param_set("Cut", &v(2.)).unwrap().bodies[0].volume,
        2. * segment(5., 2.),
        1e-6,
        "server: the arc follows the cutter",
    );
    assert_close(gui_edit_param(&path, "Cut", "2"), 2. * segment(5., 2.), 1e-6, "app: the arc follows the cutter");
    assert_close(gui_edit_param(&path, "R", "6"), 2. * segment(6., 1.), 1e-6, "app: the radius formula still drives the arc");
}

#[test]
fn an_extended_end_follows_its_boundary_when_the_boundary_moves() {
    let mut s = Session::new_part();
    s.param_set("L", &v(20.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let a = line(&mut s, sk, [v(0.), v(0.)], [v(10.), v(0.)], false, false);
    line(&mut s, sk, [e("L"), v(-5.)], [e("L"), v(10.)], true, true);
    s.sketch_extend(sk, &ExtendSpec { curve: EntityRef::Id(a.entities[0]), at: [v(10.), v(0.)], target: None, both: false }).unwrap();
    let path = scratch("stage2-extend-follow.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, _) = Session::open(&path).unwrap();
    opened.param_set("L", &v(26.)).unwrap();
    let d = opened.sketch_detail(sk).unwrap();
    let p = d.points.iter().find(|p| p.id == a.points[1]).unwrap();
    assert_close(p.x, 26., 1e-6, "the extended end stays on the moved boundary");
}

#[test]
fn a_broken_ellipse_keeps_its_area_and_its_axis_formula() {
    let mut s = Session::new_part();
    s.param_set("A", &v(8.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let el = s
        .sketch_ellipse(
            sk,
            &EllipseSpec { cx: v(0.), cy: v(0.), rx: e("A"), ry: v(4.), turn: v(0.), construction: false, dimensioned: true },
        )
        .unwrap();
    let out =
        s.sketch_break(sk, &BreakSpec { curve: EntityRef::Id(el.entities[0]), at: Some([v(0.), v(4.)]), intersections: false }).unwrap();
    assert!(!out.created_entities.is_empty(), "the ellipse is split into pieces");
    assert!(out.dropped_formulas.is_empty(), "no formula is lost: {:?}", out.dropped_formulas);
    let pi = std::f64::consts::PI;
    assert_close(extrude_volume(&mut s, sk), 2. * pi * 8. * 4., 1e-6, "pi*a*b*h of the split ellipse");
    let path = scratch("stage2-break-ellipse.qcad");
    s.save(Some(&path)).unwrap();
    assert_close(gui_edit_param(&path, "A", "10"), 2. * pi * 10. * 4., 1e-6, "app: the axis formula drives both pieces");
}

#[test]
fn breaking_at_intersections_splits_a_circle_into_its_pieces() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let c = s.sketch_circle(sk, &v(0.), &v(0.), &v(10.), false).unwrap();
    line(&mut s, sk, [v(-10.), v(0.)], [v(10.), v(0.)], true, true);
    line(&mut s, sk, [v(0.), v(-10.)], [v(0.), v(10.)], true, true);
    let out = s.sketch_break(sk, &BreakSpec { curve: EntityRef::Id(c), at: None, intersections: true }).unwrap();
    let arcs = s.sketch_detail(sk).unwrap().entities.into_iter().filter(|x| x.kind == "arc").count();
    assert_eq!(arcs, 4, "two lines cross the circle four times: {out:?}");
    assert!(s.sketch_break(sk, &BreakSpec { curve: EntityRef::Id(c), at: Some([v(5.), v(0.)]), intersections: true }).is_err());
}

#[test]
fn a_formula_dimension_removed_by_an_edit_is_reported() {
    let mut s = Session::new_part();
    s.param_set("R", &v(5.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let c = s.sketch_circle(sk, &v(0.), &v(0.), &e("2*R"), false).unwrap();
    // No other curve crosses the circle, so trim removes it whole, with its diameter.
    let out = s.sketch_trim(sk, &CurvePickSpec { curve: EntityRef::Id(c), at: [v(5.), v(0.)] }).unwrap();
    assert_eq!(out.removed_entities, vec![c]);
    let dropped: Vec<_> = out.dropped_formulas.iter().map(|d| d.expr.as_deref()).collect();
    assert_eq!(dropped, vec![Some("2*R")], "{:?}", out.dropped_formulas);
}

/// Two circles share the formula "d"; trimming away the first reports the first one's diameter, with its own
/// index, not the survivor's (Codex review r1).
#[test]
fn a_dropped_formula_is_the_one_that_was_removed() {
    let mut s = Session::new_part();
    s.param_set("d", &v(6.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let first = s.sketch_circle(sk, &v(0.), &v(0.), &e("d"), false).unwrap();
    s.sketch_circle(sk, &v(20.), &v(0.), &e("d"), false).unwrap();
    let before = s.sketch_detail(sk).unwrap();
    let centre = before.entities.iter().find(|x| x.id == first).unwrap().points[0];
    let expected = before.constraints.iter().find(|c| c.kind == "diameter" && c.points.contains(&centre)).unwrap().index;
    let out = s.sketch_trim(sk, &CurvePickSpec { curve: EntityRef::Id(first), at: [v(3.), v(0.)] }).unwrap();
    let dropped: Vec<usize> = out.dropped_formulas.iter().map(|c| c.index).collect();
    assert_eq!(dropped, vec![expected], "{:?}", out.dropped_formulas);
}

#[test]
fn the_pieces_of_a_broken_line_stay_on_the_line() {
    // Live test D1: the split point of a line held by distances (not a horizontal) was free to leave the line.
    let mut s = Session::new_part();
    s.param_set("W", &v(20.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let a = line(&mut s, sk, [v(0.), v(0.)], [e("W"), v(10.)], false, true);
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    let out =
        s.sketch_break(sk, &BreakSpec { curve: EntityRef::Id(a.entities[0]), at: Some([v(10.), v(5.)]), intersections: false }).unwrap();
    assert_eq!(out.dof, (1, 0), "only the place along the line is free");
    let cut = out.created_points[0];
    let spec = ConstrainSpec {
        kind: ConstraintKind::Distance,
        refs: vec![SketchRef::Frame(FrameRef::Origin), SketchRef::Id(cut)],
        value: Some(e("W/4")),
        axis: DistAxis::X,
        reference: false,
    };
    s.sketch_constrain(sk, &spec).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    s.param_set("W", &v(40.)).unwrap();
    let p = s.sketch_detail(sk).unwrap().points.into_iter().find(|p| p.id == cut).unwrap();
    assert_close(p.x, 10.0, 1e-9, "cut point x");
    assert_close(p.y, 2.5, 1e-9, "cut point stays on the line to (40,10)");

    // At intersections each new point is held by the crossing curve and by the line itself: nothing comes free.
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let a = line(&mut s, sk, [v(0.), v(0.)], [e("W"), v(10.)], false, true);
    line(&mut s, sk, [v(10.), v(-5.)], [v(10.), v(20.)], false, true);
    line(&mut s, sk, [v(30.), v(-5.)], [v(30.), v(20.)], false, true);
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    let out = s.sketch_break(sk, &BreakSpec { curve: EntityRef::Id(a.entities[0]), at: None, intersections: true }).unwrap();
    assert_eq!(out.created_points.len(), 2);
    assert_eq!(out.dof, (0, 0));
}

#[test]
fn a_line_broken_at_a_driven_point_is_refused_rather_than_bent() {
    // Review round 2 (Codex): breaking at an existing point welded the pieces to it with no tie to the line, so a
    // later edit of that point's driver bent the line while the sketch reported full definition.
    let mut s = Session::new_part();
    s.param_set("K", &v(5.)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let a = line(&mut s, sk, [v(0.), v(0.)], [v(20.), v(10.)], false, true);
    line(&mut s, sk, [v(10.), e("K")], [v(10.), v(30.)], true, true);
    let before = s.sketch_detail(sk).unwrap();
    let error = s
        .sketch_break(sk, &BreakSpec { curve: EntityRef::Id(a.entities[0]), at: Some([v(10.), v(5.)]), intersections: false })
        .unwrap_err();
    assert!(error.to_string().contains("would bend"), "{error}");
    assert_eq!(s.sketch_detail(sk).unwrap(), before);

    // A point already tied to the line keeps the pieces straight, and the break is accepted.
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let a = line(&mut s, sk, [v(0.), v(0.)], [v(20.), v(10.)], false, true);
    let b = line(&mut s, sk, [v(10.), v(5.)], [v(10.), v(30.)], false, false);
    let foot = b.points[0];
    let spec = ConstrainSpec {
        kind: ConstraintKind::PointOnCurve,
        refs: vec![SketchRef::Id(foot), SketchRef::Id(a.entities[0])],
        value: None,
        axis: DistAxis::Aligned,
        reference: false,
    };
    s.sketch_constrain(sk, &spec).unwrap();
    let out =
        s.sketch_break(sk, &BreakSpec { curve: EntityRef::Id(a.entities[0]), at: Some([v(10.), v(5.)]), intersections: false }).unwrap();
    assert!(out.created_points.is_empty(), "the pieces meet at the existing point");
}

#[test]
fn a_horizontal_line_broken_at_a_driven_point_is_refused_both_ways() {
    // Review round 3 (Codex): the horizontal exemption let a break at intersections weld the pieces to a driven point
    // (the far-end horizontal row does not hold them), and a picked break there left a redundant row.
    for intersections in [true, false] {
        let mut s = Session::new_part();
        s.param_set("K", &v(5.)).unwrap();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let a = line(&mut s, sk, [v(-10.), v(5.)], [v(10.), v(5.)], false, true);
        line(&mut s, sk, [v(0.), e("K")], [v(0.), v(20.)], true, true);
        // Hold the line by a horizontal instead of its far end's height.
        let height =
            s.sketch_detail(sk).unwrap().constraints.iter().find(|c| c.kind == "distance_y" && c.points[0] == a.points[1]).unwrap().index;
        s.sketch_remove_constraint(sk, height).unwrap();
        let horizontal = ConstrainSpec {
            kind: ConstraintKind::Horizontal,
            refs: vec![SketchRef::Id(a.entities[0])],
            value: None,
            axis: DistAxis::Aligned,
            reference: false,
        };
        s.sketch_constrain(sk, &horizontal).unwrap();
        let before = s.sketch_detail(sk).unwrap();
        assert_eq!(before.summary.dof, (0, 0));
        let spec = BreakSpec { curve: EntityRef::Id(a.entities[0]), at: (!intersections).then(|| [v(0.), v(5.)]), intersections };
        assert!(s.sketch_break(sk, &spec).is_err(), "intersections: {intersections}");
        assert_eq!(s.sketch_detail(sk).unwrap(), before);
    }
}
