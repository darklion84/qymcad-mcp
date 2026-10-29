//! Formula-derived golden coverage for native 0.1.1 sketch figures.
mod common;
use common::*;
use qymcad_engine::*;

fn n(v: f64) -> Num {
    Num::Value(v)
}
fn e(v: &str) -> Num {
    Num::Expr(v.into())
}
fn sketch(s: &mut Session) -> Id {
    s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap()
}
fn extrude(s: &mut Session, sk: Id) -> Id {
    let (body, r) = s
        .extrude(&Extrude {
            down: None,
            sketch: sk,
            profiles: None,
            height: n(3.0),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    body
}
fn name_expression(s: &mut Session, sk: Id, expression: &str, name: &str) {
    let constraint = s.sketch_detail(sk).unwrap().constraints.iter().find(|c| c.expr.as_deref() == Some(expression)).unwrap().index;
    s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint }, None, Some(name)).unwrap();
}
fn ellipse_spec() -> EllipseSpec {
    EllipseSpec { cx: n(20.0), cy: n(30.0), rx: e("A"), ry: e("B"), turn: e("Theta"), construction: false, dimensioned: true }
}
#[test]
fn ellipse_axes_turn_formula_survives_server_open_and_app_edits() {
    let mut s = Session::new_part();
    for (k, v) in [("A", 8.0), ("B", 4.0), ("Theta", 30.0)] {
        s.param_set(k, &n(v)).unwrap();
    }
    let sk = sketch(&mut s);
    let added = s.sketch_ellipse(sk, &ellipse_spec()).unwrap();
    assert_eq!(added.entities.len(), 1);
    assert_ne!(added.entities[0], added.points[0], "ellipse adder returns a centre, entity id must be resolved");
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    let body = extrude(&mut s, sk);
    assert_close(s.shape(body).unwrap().volume(), 96.0 * std::f64::consts::PI, 1e-6, "ellipse pi*a*b*t");
    name_expression(&mut s, sk, "A", "Major");
    let path = scratch("figures_ellipse.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_close(opened.shape(body).unwrap().volume(), 96.0 * std::f64::consts::PI, 1e-6, "ellipse roundtrip");
    s.param_set("A", &n(10.0)).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    assert_close(s.shape(body).unwrap().volume(), 120.0 * std::f64::consts::PI, 1e-6, "ellipse axis formula edit");
    assert_close(gui_edit_param(&path, "A", "10"), 120.0 * std::f64::consts::PI, 1e-6, "ellipse app axis edit");
    assert_close(gui_edit_driver(&path, "Major", "10"), 120.0 * std::f64::consts::PI, 1e-6, "ellipse app SetDriver axis edit");
    let (_, bbox) = gui_edit_param_body(&path, "Theta", "60");
    let a = 60f64.to_radians();
    let dx = (64.0 * a.cos().powi(2) + 16.0 * a.sin().powi(2)).sqrt();
    let dy = (64.0 * a.sin().powi(2) + 16.0 * a.cos().powi(2)).sqrt();
    for (got, want) in bbox.into_iter().zip([20.0 - dx, 30.0 - dy, 0.0, 20.0 + dx, 30.0 + dy, 3.0]) {
        assert_close(got, want, 0.05, "ellipse rotated bbox");
    }
}
#[test]
fn rectangle_native_anchor_turn_and_size_survive_app_edits() {
    for anchor in [RectAnchor::Centre, RectAnchor::Corner] {
        let mut s = Session::new_part();
        for (k, v) in [("W", 20.0), ("H", 10.0), ("Theta", 30.0)] {
            s.param_set(k, &n(v)).unwrap();
        }
        let sk = sketch(&mut s);
        s.sketch_rectangle(sk, &RectSpec { cx: n(40.0), cy: n(50.0), w: e("W"), h: e("H"), turn: e("Theta"), anchor, construction: false })
            .unwrap();
        let si = s.project().sketch_index(sk).unwrap();
        assert_eq!(s.project().sketches[si].rects().len(), 1);
        assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
        let body = extrude(&mut s, sk);
        assert_close(s.shape(body).unwrap().volume(), 600.0, 1e-6, "rectangle w*h*t");
        name_expression(&mut s, sk, "W", "Width");
        let path = scratch(&format!("figures_rectangle_{anchor:?}.qcad"));
        s.save(Some(&path)).unwrap();
        let (opened, r) = Session::open(&path).unwrap();
        assert!(r.errors.is_empty());
        assert_close(opened.shape(body).unwrap().volume(), 600.0, 1e-6, "rectangle roundtrip");
        s.param_set("W", &n(24.0)).unwrap();
        assert_close(s.shape(body).unwrap().volume(), 720.0, 1e-6, "rectangle width formula");
        assert_close(gui_edit_param(&path, "W", "24"), 720.0, 1e-6, "rectangle app width formula");
        assert_close(gui_edit_driver(&path, "Width", "24"), 720.0, 1e-6, "rectangle app SetDriver width edit");
        let (vol, bbox) = gui_edit_param_body(&path, "Theta", "60");
        assert_close(vol, 600.0, 1e-6, "rotating does not change volume");
        let (c, t) = (60f64.to_radians().cos(), 60f64.to_radians().sin());
        let expected = match anchor {
            RectAnchor::Centre => {
                [40.0 - 10.0 * c - 5.0 * t, 50.0 - 10.0 * t - 5.0 * c, 0.0, 40.0 + 10.0 * c + 5.0 * t, 50.0 + 10.0 * t + 5.0 * c, 3.0]
            }
            RectAnchor::Corner => [40.0 - 10.0 * t, 50.0, 0.0, 40.0 + 20.0 * c, 50.0 + 20.0 * t + 10.0 * c, 3.0],
        };
        for (got, want) in bbox.into_iter().zip(expected) {
            assert_close(got, want, 1e-5, "rectangle rotated bbox and anchor");
        }
    }
}
#[test]
fn polygon_sides_formula_keeps_first_side_and_points_in_app() {
    let mut s = Session::new_part();
    s.param_set("N", &n(6.0)).unwrap();
    s.param_set("R", &n(10.0)).unwrap();
    s.param_set("Theta", &n(30.0)).unwrap();
    let sk = sketch(&mut s);
    let spec = PolygonSpec {
        cx: n(20.0),
        cy: n(30.0),
        sides: 6,
        r: Some(e("R")),
        angle: Some(e("Theta")),
        vertex: None,
        construction: false,
        dimensioned: true,
    };
    let added = s.sketch_polygon_sides(sk, &spec, &e("N")).unwrap();
    let first = added.entities[1];
    let oldpoints = added.points.clone();
    let body = extrude(&mut s, sk);
    let path = scratch("figures_polygon.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_close(
        opened.shape(body).unwrap().volume(),
        3.0 * 6.0 * 100.0 * (std::f64::consts::TAU / 6.0).sin() / 2.0,
        1e-6,
        "polygon roundtrip",
    );
    s.param_set("N", &n(8.0)).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    let si = s.project().sketch_index(sk).unwrap();
    let native = &s.project().sketches[si];
    assert!(native.entity(first).is_some());
    for &p in &oldpoints {
        assert!(native.points.iter().any(|q| q.id == p));
    }
    let expected = 3.0 * 8.0 * 100.0 * (std::f64::consts::TAU / 8.0).sin() / 2.0;
    assert_close(s.shape(body).unwrap().volume(), expected, 1e-6, "polygon n*R^2*sin(2pi/n)/2");
    assert_close(gui_edit_param(&path, "N", "8"), expected, 1e-6, "polygon native app count edit");
    s.param_set("N", &n(5.0)).unwrap();
    s.param_set("R", &n(12.0)).unwrap();
    s.param_set("Theta", &n(60.0)).unwrap();
    let si = s.project().sketch_index(sk).unwrap();
    let native = &s.project().sketches[si];
    assert!(native.entity(first).is_some());
    for &id in oldpoints.iter().take(6) {
        assert!(native.point(id).is_some());
    }
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    let area = |r: f64| 3.0 * 5.0 * r * r * (std::f64::consts::TAU / 5.0).sin() / 2.0;
    assert_close(s.shape(body).unwrap().volume(), area(12.0), 1e-6, "count decrease retains radius formula");
    let point = native.point(oldpoints[1]).unwrap();
    assert_close(point.x, 26.0, 1e-6, "count edit retains first vertex angle");
    assert_close(point.y, 30.0 + 6.0 * 3f64.sqrt(), 1e-6, "retained turn formula after count decrease");
    let path = scratch("polygon_after_count_decrease.qcad");
    s.save(Some(&path)).unwrap();
    assert_close(gui_edit_param(&path, "R", "14"), area(14.0), 1e-6, "app retains radius after count decrease");
}

#[test]
fn ellipse_arc_span_formula_drives_segment_area_in_server_and_app() {
    for ccw in [true, false] {
        let mut s = Session::new_part();
        for (key, value) in [("A", 8.0), ("B", 4.0), ("Theta", 30.0), ("Start", 30.0), ("End", 150.0)] {
            s.param_set(key, &n(value)).unwrap();
        }
        let sk = sketch(&mut s);
        let added = s
            .sketch_ellipse_arc(
                sk,
                &EllipseArcSpec {
                    cx: n(20.0),
                    cy: n(30.0),
                    rx: e("A"),
                    ry: e("B"),
                    turn: e("Theta"),
                    start_angle: e("Start"),
                    end_angle: e("End"),
                    ccw,
                    construction: false,
                    dimensioned: true,
                },
            )
            .unwrap();
        let detail = s.sketch_detail(sk).unwrap();
        assert_eq!(detail.summary.dof, (0, 0));
        let entity = detail.entities.iter().find(|row| row.id == added.entities[0]).unwrap();
        assert_eq!(entity.kind, "ellipse_arc");
        assert_eq!(entity.ccw, Some(ccw));
        let point = |id| detail.points.iter().find(|p| p.id == id).unwrap();
        let (from, to) = (point(added.points[3]), point(added.points[4]));
        s.sketch_line(sk, &LineSpec { x1: n(from.x), y1: n(from.y), x2: n(to.x), y2: n(to.y), construction: false, dimensioned: false })
            .unwrap();
        assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
        let body = extrude(&mut s, sk);
        let volume = |start: f64| {
            let span = (150.0 - start).to_radians();
            let short = 8.0 * 4.0 * (span - span.sin()) / 2.0;
            3.0 * if ccw { short } else { 8.0 * 4.0 * std::f64::consts::PI - short }
        };
        assert_close(s.shape(body).unwrap().volume(), volume(30.0), 1e-6, "ellipse sector minus triangle");
        name_expression(&mut s, sk, "A", "Major");
        let path = scratch(&format!("ellipse_arc_{ccw}.qcad"));
        s.save(Some(&path)).unwrap();
        let (opened, report) = Session::open(&path).unwrap();
        assert!(report.errors.is_empty());
        assert_close(opened.shape(body).unwrap().volume(), volume(30.0), 1e-6, "ellipse arc roundtrip");
        s.param_set("Start", &n(45.0)).unwrap();
        assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
        assert_close(s.shape(body).unwrap().volume(), volume(45.0), 1e-6, "ellipse eccentric angle edit");
        assert_close(gui_edit_param(&path, "Start", "45"), volume(45.0), 1e-6, "ellipse arc app angle edit");
        // Editing this native axis driver replaces its A formula; endpoint polar dimensions
        // still read A=8, so compute the new eccentric span from those retained directions.
        let start = (10.0 * 30f64.to_radians().sin()).atan2(8.0 * 30f64.to_radians().cos());
        let span = std::f64::consts::PI - 2.0 * start;
        let short = 10.0 * 4.0 * (span - span.sin()) / 2.0;
        let edited = 3.0 * if ccw { short } else { 40.0 * std::f64::consts::PI - short };
        assert_close(gui_edit_driver(&path, "Major", "10"), edited, 1e-6, "ellipse arc app SetDriver axis edit retains polar dimensions");
    }
}

#[test]
fn unsupported_ellipse_and_rect_turns_are_atomic() {
    let mut s = Session::new_part();
    for (k, v) in [("A", 8.0), ("B", 4.0), ("Theta", 30.0)] {
        s.param_set(k, &n(v)).unwrap();
    }
    let sk = sketch(&mut s);
    let before = s.sketch_detail(sk).unwrap();
    for turn in [-30.0, 181.0, 270.0] {
        let mut spec = ellipse_spec();
        spec.turn = n(turn);
        let error = s.sketch_ellipse(sk, &spec).unwrap_err();
        assert!(error.to_string().contains("0..180"), "{error}");
        assert_eq!(s.sketch_detail(sk).unwrap(), before);
    }
    s.sketch_ellipse(sk, &ellipse_spec()).unwrap();
    let before = s.sketch_detail(sk).unwrap();
    for turn in [-30.0, 0.0, 180.0, 270.0] {
        let error = s.param_set("Theta", &n(turn)).unwrap_err().to_string();
        assert!(error.contains("`Theta` of a rotated sketch figure must stay strictly between 0 and 180 degrees"), "{error}");
        assert_eq!(s.sketch_detail(sk).unwrap(), before);
    }
    let sk = sketch(&mut s);
    s.sketch_rectangle(
        sk,
        &RectSpec { cx: n(50.0), cy: n(50.0), w: n(10.0), h: n(5.0), turn: e("Theta"), anchor: RectAnchor::Centre, construction: false },
    )
    .unwrap();
    let before = s.sketch_detail(sk).unwrap();
    assert!(s.param_set("Theta", &n(270.0)).is_err());
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
}

#[test]
fn native_item_values_do_not_replace_driving_dimension_targets() {
    let mut s = Session::new_part();
    for (k, v) in [("A", 8.0), ("B", 4.0), ("Theta", 30.0)] {
        s.param_set(k, &n(v)).unwrap();
    }
    let sk = sketch(&mut s);
    let added = s.sketch_ellipse(sk, &ellipse_spec()).unwrap();
    let mut p = s.project().clone();
    let si = p.sketch_index(sk).unwrap();
    let centre = added.points[0];
    p.write_feat_formula(&qymcad_core::model::FeatNumber { node: centre, key: "w" }, "24", qymcad_core::model::PlainNumber::Stands)
        .unwrap();
    assert!(p.eval_parameters().is_empty());
    assert_close(p.kept_number(centre, "w").unwrap(), 24.0, 1e-6, "item formula moves raw axes");
    p.solve_sketch(si);
    assert_close(p.kept_number(centre, "w").unwrap(), 16.0, 1e-6, "fully driving dimensions restore their targets");
    assert_eq!(p.sketch_dof(si), (0, 0));
    let polygon_sk = sketch(&mut s);
    let polygon = s
        .sketch_polygon(
            polygon_sk,
            &PolygonSpec {
                cx: n(50.0),
                cy: n(50.0),
                sides: 6,
                r: Some(n(10.0)),
                angle: Some(e("Theta")),
                vertex: None,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    let mut p = s.project().clone();
    let si = p.sketch_index(polygon_sk).unwrap();
    let centre = polygon.points[0];
    p.write_feat_formula(&qymcad_core::model::FeatNumber { node: centre, key: "turn" }, "60", qymcad_core::model::PlainNumber::Stands)
        .unwrap();
    assert!(p.eval_parameters().is_empty());
    assert_close(p.kept_number(centre, "turn").unwrap(), 60.0, 1e-6, "item formula turns raw polygon");
    p.solve_sketch(si);
    assert_close(p.kept_number(centre, "turn").unwrap(), 30.0, 1e-6, "direction driver restores polygon turn");
    assert_eq!(p.sketch_dof(si), (0, 0));
}

#[test]
fn polygon_count_formulas_refuse_nonintegral_and_excessive_transitive_edits() {
    let mut s = Session::new_part();
    s.param_set("N", &n(4.0)).unwrap();
    s.param_set("Count", &e("N+2")).unwrap();
    let sk = sketch(&mut s);
    let spec = PolygonSpec {
        cx: n(20.0),
        cy: n(30.0),
        sides: 6,
        r: Some(n(10.0)),
        angle: Some(n(30.0)),
        vertex: None,
        construction: false,
        dimensioned: true,
    };
    for value in [n(2.0), n(6.5), n(100000.0)] {
        let before = s.sketch_detail(sk).unwrap();
        let error = s.sketch_polygon_sides(sk, &spec, &value).unwrap_err();
        assert!(error.to_string().contains("3..=64"), "{error}");
        assert_eq!(s.sketch_detail(sk).unwrap(), before);
    }
    s.sketch_polygon_sides(sk, &spec, &e("Count")).unwrap();
    let body = extrude(&mut s, sk);
    s.param_set("N", &n(6.0)).unwrap();
    assert_close(
        s.shape(body).unwrap().volume(),
        3.0 * 8.0 * 100.0 * (std::f64::consts::TAU / 8.0).sin() / 2.0,
        1e-6,
        "transitive count formula rebuild",
    );
    for value in [4.5, 0.0, 100000.0] {
        let before = s.sketch_detail(sk).unwrap();
        let volume = s.shape(body).unwrap().volume();
        let error = s.param_set("N", &n(value)).unwrap_err();
        assert!(error.to_string().contains("3..=64"), "{error}");
        assert_eq!(s.sketch_detail(sk).unwrap(), before);
        assert_eq!(s.shape(body).unwrap().volume().to_bits(), volume.to_bits());
    }
}

#[test]
fn literal_cardinal_turns_retain_the_requested_ellipse_and_rectangle_branch() {
    for turn in [0.0, 90.0, 180.0] {
        let mut s = Session::new_part();
        let sk = sketch(&mut s);
        let ellipse = s
            .sketch_ellipse(
                sk,
                &EllipseSpec { cx: n(20.0), cy: n(30.0), rx: n(8.0), ry: n(4.0), turn: n(turn), construction: true, dimensioned: true },
            )
            .unwrap();
        let d = s.sketch_detail(sk).unwrap();
        assert_eq!(d.summary.dof, (0, 0));
        let major = d.points.iter().find(|p| p.id == ellipse.points[1]).unwrap();
        assert_close(major.x, 20.0 + 8.0 * turn.to_radians().cos(), 1e-6, "ellipse cardinal branch x");
        assert_close(major.y, 30.0 + 8.0 * turn.to_radians().sin(), 1e-6, "ellipse cardinal branch y");
        let sk = sketch(&mut s);
        s.sketch_rectangle(
            sk,
            &RectSpec { cx: n(40.0), cy: n(50.0), w: n(20.0), h: n(10.0), turn: n(turn), anchor: RectAnchor::Corner, construction: true },
        )
        .unwrap();
        assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
        let si = s.project().sketch_index(sk).unwrap();
        let p = &s.project().sketches[si];
        let rect = &p.rects()[0];
        let first = p.point(rect.corners[0]).unwrap();
        let next = p.point(rect.corners[1]).unwrap();
        assert_close(first.x, 40.0, 1e-6, "rect corner stays x");
        assert_close(first.y, 50.0, 1e-6, "rect corner stays y");
        assert_close(next.x - first.x, 20.0 * turn.to_radians().cos(), 1e-6, "rect cardinal direction x");
        assert_close(next.y - first.y, 20.0 * turn.to_radians().sin(), 1e-6, "rect cardinal direction y");
    }
}

/// A broken formula elsewhere in the document (here a named diameter whose expression reads a name that no longer
/// exists) is reported as itself; the polygon side-count check only refuses a side count it cannot evaluate.
#[test]
fn an_unrelated_broken_named_formula_is_reported_as_itself_in_polygon_documents() {
    let mut s = Session::new_part();
    s.param_set("N", &n(6.0)).unwrap();
    s.param_set("R", &n(10.0)).unwrap();
    let sk = sketch(&mut s);
    let spec = PolygonSpec {
        cx: n(20.0),
        cy: n(30.0),
        sides: 6,
        r: Some(e("R")),
        angle: Some(n(30.0)),
        vertex: None,
        construction: false,
        dimensioned: true,
    };
    s.sketch_polygon_sides(sk, &spec, &e("N")).unwrap();
    let other = sketch(&mut s);
    s.sketch_circle(other, &n(80.0), &n(0.0), &n(6.0), false).unwrap();
    let diameter = s.sketch_detail(other).unwrap().constraints.iter().find(|c| c.kind == "diameter").unwrap().index;
    s.dimension_set(&DimensionTarget::Sketch { sketch: other, constraint: diameter }, None, Some("Dia")).unwrap();
    let path = scratch("polygon-unrelated-broken.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let si = project.sketch_index(other).unwrap();
    for c in &mut project.sketches[si].constraints {
        if let qymcad_core::model::Constraint::Diameter { expr, .. } = c {
            *expr = "Gone+1".into();
        }
    }
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (mut opened, _) = Session::open(&path).unwrap();
    // The edit is refused because of the broken formula, and the message names it, not the polygon.
    let error = opened.param_set("R", &n(12.0)).unwrap_err().to_string();
    assert!(error.contains("Gone") && !error.contains("polygon"), "{error}");
}

#[test]
fn a_concentric_ellipse_is_its_own_curve_and_fully_defined() {
    // Review round 2 (Codex): the second ellipse on a shared centre reported, and dimensioned, the first one.
    let mut s = Session::new_part();
    s.param_set("Rx", &n(4.0)).unwrap();
    let sk = sketch(&mut s);
    let spec =
        |rx: Num, ry: f64| EllipseSpec { cx: n(20.0), cy: n(30.0), rx, ry: n(ry), turn: n(0.0), construction: false, dimensioned: true };
    let outer = s.sketch_ellipse(sk, &spec(n(8.0), 6.0)).unwrap();
    let inner = s.sketch_ellipse(sk, &spec(e("Rx"), 3.0)).unwrap();
    assert_ne!(outer.entities[0], inner.entities[0]);
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    s.param_set("Rx", &n(5.0)).unwrap();
    let d = s.sketch_detail(sk).unwrap();
    let major = d.points.iter().find(|p| p.id == inner.points[1]).unwrap();
    assert_close(major.x, 25.0, 1e-9, "inner major-axis end follows Rx");
    assert_close(d.points.iter().find(|p| p.id == outer.points[1]).unwrap().x, 28.0, 1e-9, "outer stays");
}

#[test]
fn an_item_value_on_a_shared_centre_is_refused() {
    // Review round 3 (Codex): concentric free ellipses share their centre, and the native item lookup takes the
    // first, so an edit meant for the inner one resized the outer one.
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let spec = |rx: f64, ry: f64| EllipseSpec {
        cx: n(20.0),
        cy: n(30.0),
        rx: n(rx),
        ry: n(ry),
        turn: n(0.0),
        construction: false,
        dimensioned: false,
    };
    s.sketch_ellipse(sk, &spec(8.0, 6.0)).unwrap();
    let inner = s.sketch_ellipse(sk, &spec(4.0, 3.0)).unwrap();
    let before = serde_json::to_value(s.project()).unwrap();
    let target = DimensionTarget::Feature { feature: inner.points[0], key: "w".into() };
    let error = s.dimension_set(&target, Some(&n(24.0)), None).unwrap_err();
    assert!(error.to_string().contains("centre of 2 curves"), "{error}");
    assert_eq!(serde_json::to_value(s.project()).unwrap(), before);
}

#[test]
fn item_values_on_a_shared_centre_are_set_where_one_item_answers() {
    // Review round 4 (Codex): the shared-centre refusal counted curves regardless of the key, so a polygon's
    // `sides` and an ellipse's `w` about one centre, or the width of an ellipse broken into arcs, were refused; and
    // clearing a name was refused too.
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let ellipse = EllipseSpec { cx: n(20.0), cy: n(30.0), rx: n(8.0), ry: n(6.0), turn: n(0.0), construction: false, dimensioned: false };
    let el = s.sketch_ellipse(sk, &ellipse).unwrap();
    let polygon = PolygonSpec {
        cx: n(20.0),
        cy: n(30.0),
        sides: 6,
        r: Some(n(3.0)),
        angle: Some(n(30.0)),
        vertex: None,
        construction: false,
        dimensioned: false,
    };
    let pg = s.sketch_polygon(sk, &polygon).unwrap();
    let centre = el.points[0];
    assert_eq!(pg.points[0], centre, "the polygon shares the ellipse's centre");
    s.dimension_set(&DimensionTarget::Feature { feature: centre, key: "sides".into() }, Some(&n(8.0)), None).unwrap();
    s.dimension_set(&DimensionTarget::Feature { feature: centre, key: "w".into() }, Some(&n(20.0)), None).unwrap();
    let turn = s.dimension_set(&DimensionTarget::Feature { feature: centre, key: "turn".into() }, Some(&n(10.0)), None).unwrap_err();
    assert!(turn.to_string().contains("centre of 2 curves with a `turn`"), "{turn}");
    s.dimension_set(&DimensionTarget::Feature { feature: centre, key: "w".into() }, None, Some("")).unwrap();

    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let el = s.sketch_ellipse(sk, &ellipse).unwrap();
    s.sketch_break(sk, &BreakSpec { curve: EntityRef::Id(el.entities[0]), at: Some([n(28.0), n(30.0)]), intersections: false }).unwrap();
    // The arcs of one ellipse are one item here; their own end points may still hold the width (another refusal).
    let w = s.dimension_set(&DimensionTarget::Feature { feature: el.points[0], key: "w".into() }, Some(&n(20.0)), None);
    assert!(w.as_ref().err().is_none_or(|e| !e.to_string().contains("centre of")), "{w:?}");
}
