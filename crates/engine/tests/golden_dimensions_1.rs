//! Native named drivers retain formulas and rebuild through server and app edit paths.
mod common;
use common::{assert_close, scratch};
use qymcad_engine::*;

fn stock() -> (Session, Id, DimensionTarget, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("Profile")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    let ci = s.project().sketches[0].constraints.iter().position(|c| qymcad_core::model::Project::dim_value_of(c) == Some(10.0)).unwrap();
    let target = DimensionTarget::Sketch { sketch: sk, constraint: ci };
    let (feature, _) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: 2.0.into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("Prism".into()),
        })
        .unwrap();
    (s, sk, target, feature)
}

#[test]
fn named_sketch_driver_literal_formula_save_and_app_setdriver() {
    let (mut s, sk, target, _) = stock();
    let edit = s.dimension_set(&target, Some(&12.0.into()), Some("Width")).unwrap();
    assert_eq!(edit.dimension.name, "Width");
    assert_eq!(edit.dimension.dof, Some((0, 0)));
    assert_close(edit.rebuild.bodies[0].volume, 12.0 * 6.0 * 2.0, 1e-6, "literal driver rebuild");
    s.param_set("Base", &8.0.into()).unwrap();
    s.dimension_set(&target, Some(&"Base*2".into()), None).unwrap();
    assert_eq!(s.info().named_dimensions[0].expression.as_deref(), Some("Base*2"));
    assert_close(s.param_set("Base", &9.0.into()).unwrap().bodies[0].volume, 18.0 * 6.0 * 2.0, 1e-6, "parameter reaches named formula");
    let path = scratch("named-sketch-driver.qcad");
    s.save(Some(&path)).unwrap();
    let (loaded, _) = Session::open(&path).unwrap();
    assert_eq!(loaded.info().named_dimensions, s.info().named_dimensions);
    assert_close(loaded.result_bodies()[0].volume, 18.0 * 6.0 * 2.0, 1e-6, "saved named sketch geometry");
    assert_eq!(loaded.sketch_info(sk).unwrap().dof, (0, 0));
    assert_close(common::gui_edit_param(&path, "Base", "10"), 20.0 * 6.0 * 2.0, 1e-6, "GUI parameter edit");
    assert_close(common::gui_edit_driver(&path, "Width", "22"), 22.0 * 6.0 * 2.0, 1e-6, "GUI literal SetDriver solves and dirties target");
    assert_close(common::gui_edit_driver(&path, "Width", "Base*3"), 27.0 * 6.0 * 2.0, 1e-6, "GUI formula SetDriver");
}

#[test]
fn feature_driver_scope_formula_and_atomic_refusals() {
    let (mut s, sk, width, feature) = stock();
    s.param_set("Base", &4.0.into()).unwrap();
    s.dimension_set(&width, None, Some("Width")).unwrap();
    let depth = DimensionTarget::Feature { feature, key: "height".into() };
    let edit = s.dimension_set(&depth, Some(&"Width/4".into()), Some("Depth")).unwrap();
    assert_eq!(edit.dimension.value, Some(2.5));
    assert_close(edit.rebuild.bodies[0].volume, 10.0 * 6.0 * 2.5, 1e-6, "named feature dimension");
    let edit = s.dimension_set(&width, Some(&12.0.into()), None).unwrap();
    assert_close(edit.rebuild.bodies[0].volume, 12.0 * 6.0 * 3.0, 1e-6, "named feature follows sketch driver");
    let before = serde_json::to_value(s.project()).unwrap();
    let bodies = serde_json::to_value(s.result_bodies()).unwrap();
    for (target, value, name) in [
        (&width, Some(Num::from("Depth*2")), None),
        (&depth, Some(Num::from("missing+1")), None),
        (&depth, None, Some("width")),
        (&depth, None, Some("Width")),
        (&depth, None, Some("Base")),
        (&depth, None, Some("base")),
        (&depth, Some(Num::from("Depth+1")), None),
        (&depth, Some(Num::from("width+1")), None),
        (&depth, None, Some("pi")),
        (&depth, None, Some("2Bad")),
        (&depth, Some(Num::Value(f64::INFINITY)), None),
        (&depth, Some(Num::from("1e400")), None),
        (&depth, Some(Num::from("1/0")), None),
        (&depth, Some(Num::from(0.0)), Some("InvalidDepth")),
        (&width, Some(Num::from(-4.0)), None),
    ] {
        assert!(s.dimension_set(target, value.as_ref(), name).is_err(), "must refuse {target:?} {value:?} {name:?}");
        assert_eq!(serde_json::to_value(s.project()).unwrap(), before, "failed edit is atomic");
        assert_eq!(serde_json::to_value(s.result_bodies()).unwrap(), bodies, "failed edit preserves live bodies");
    }
    assert!(s.dimension_set(&DimensionTarget::Feature { feature, key: "unknown".into() }, Some(&1.0.into()), None).is_err());
    assert!(s.dimension_set(&DimensionTarget::Sketch { sketch: 0, constraint: 0 }, Some(&1.0.into()), None).is_err());
    assert!(s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: usize::MAX }, Some(&1.0.into()), None).is_err());
    let geometric = s.project().sketches[0].constraints.iter().position(|c| qymcad_core::model::Project::dim_refs(c).is_none()).unwrap();
    assert!(s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: geometric }, Some(&1.0.into()), None).is_err());
    assert!(s.dimension_set(&depth, None, None).is_err());
    let path = scratch("named-feature-driver.qcad");
    s.save(Some(&path)).unwrap();
    let (loaded, _) = Session::open(&path).unwrap();
    assert_eq!(loaded.info().named_dimensions, s.info().named_dimensions);
    assert_close(loaded.result_bodies()[0].volume, 12.0 * 6.0 * 3.0, 1e-6, "saved named feature geometry");
    assert_close(common::gui_edit_driver(&path, "Depth", "Width/3"), 12.0 * 6.0 * 4.0, 1e-6, "GUI feature formula SetDriver");
    s.dimension_set(&depth, None, Some("")).unwrap();
    assert_eq!(s.info().named_dimensions.len(), 1, "empty name clears it");
}

#[test]
fn free_ellipse_item_formula_follows_globals_but_app_named_item_driver_keeps_old_body() {
    let mut s = Session::new_part();
    s.param_set("R", &5.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let ellipse = s
        .sketch_ellipse(
            sk,
            &EllipseSpec {
                cx: 0.0.into(),
                cy: 0.0.into(),
                rx: 5.0.into(),
                ry: 3.0.into(),
                turn: 30.0.into(),
                construction: false,
                dimensioned: false,
            },
        )
        .unwrap();
    let target = DimensionTarget::Feature { feature: ellipse.points[0], key: "w".into() };
    let before = serde_json::to_value(s.project()).unwrap();
    let error = s.dimension_set(&target, Some(&"R*2".into()), Some("AxisW")).unwrap_err().to_string();
    assert!(error.contains("SetDriver does not dirty its owning sketch"), "{error}");
    assert_eq!(serde_json::to_value(s.project()).unwrap(), before);
    s.dimension_set(&target, Some(&"R*2".into()), None).unwrap();
    s.extrude(&Extrude {
        sketch: sk,
        profiles: None,
        height: 2.0.into(),
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap();
    let expected = |rx: f64| std::f64::consts::PI * rx * 3.0 * 2.0;
    assert_close(s.param_set("R", &6.0.into()).unwrap().bodies[0].volume, expected(6.0), 1e-6, "free item global edit");
    let path = scratch("free-ellipse-item-driver.qcad");
    s.save(Some(&path)).unwrap();
    // Construct an imported native named item to characterize the app bug without advertising creation.
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    assert!(project.name_dim("AxisW".into(), qymcad_core::model::DimTarget::Feature { node: ellipse.points[0], key: "w".into() }));
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (loaded, _) = Session::open(&path).unwrap();
    assert_eq!(loaded.info().named_dimensions[0].name, "AxisW");
    assert_close(loaded.result_bodies()[0].volume, expected(6.0), 1e-6, "free item reload");
    assert_close(common::gui_edit_param(&path, "R", "7"), expected(7.0), 1e-6, "free item app parameter edit");
    // Native SetDriver dirties an item id rather than its owner sketch: the old body remains.
    assert_close(
        common::gui_edit_driver(&path, "AxisW", "16"),
        expected(6.0),
        1e-6,
        "native app item SetDriver stale-body characterization",
    );
    assert_close(
        s.dimension_set(&target, Some(&16.0.into()), None).unwrap().rebuild.bodies[0].volume,
        expected(8.0),
        1e-6,
        "server item value edit rebuilds owner",
    );
}

#[test]
fn item_side_driver_refuses_fractional_or_out_of_range_counts_atomically() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let polygon = s
        .sketch_polygon(
            sk,
            &PolygonSpec {
                cx: 0.0.into(),
                cy: 0.0.into(),
                r: Some(5.0.into()),
                sides: 6,
                angle: None,
                vertex: None,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    let target = DimensionTarget::Feature { feature: polygon.points[0], key: "sides".into() };
    let before = serde_json::to_value(s.project()).unwrap();
    for count in [6.5, 2.0, 65.0] {
        let error = s.dimension_set(&target, Some(&count.into()), None).unwrap_err().to_string();
        assert!(error.contains("integral side count in 3..=64"), "{error}");
        assert_eq!(serde_json::to_value(s.project()).unwrap(), before);
    }
}

#[test]
fn fully_dimensioned_ellipse_refuses_an_item_edit_that_solver_would_discard() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let ellipse = s
        .sketch_ellipse(
            sk,
            &EllipseSpec {
                cx: 0.0.into(),
                cy: 0.0.into(),
                rx: 8.0.into(),
                ry: 4.0.into(),
                turn: 30.0.into(),
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    let before = serde_json::to_value(s.project()).unwrap();
    for (key, value) in [("w", 20.0), ("h", 12.0), ("turn", 60.0)] {
        let target = DimensionTarget::Feature { feature: ellipse.points[0], key: key.into() };
        let error = s.dimension_set(&target, Some(&value.into()), None).unwrap_err().to_string();
        assert!(error.contains("driving constraints"), "{error}");
        assert_eq!(serde_json::to_value(s.project()).unwrap(), before);
    }
}

#[test]
fn ambiguous_native_dimension_references_are_refused_before_editing() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &3.0.into(), &4.0.into(), &2.0.into(), false).unwrap();
    // Native targets carry only entity ids. The centre's x and y distances share their pair.
    let constraints = &s.project().sketches[0].constraints;
    let ci = constraints
        .iter()
        .enumerate()
        .find_map(|(i, c)| {
            let refs = qymcad_core::model::Project::dim_refs(c)?;
            let refs = qymcad_core::model::Project::dim_key_pub(&refs);
            (constraints
                .iter()
                .filter(|c| qymcad_core::model::Project::dim_refs(c).is_some_and(|r| qymcad_core::model::Project::dim_key_pub(&r) == refs))
                .count()
                > 1)
            .then_some(i)
        })
        .expect("ambiguous x/y coordinate dimensions");
    let before = serde_json::to_value(s.project()).unwrap();
    let error = s
        .dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: ci }, Some(&7.0.into()), Some("CentreX"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("ambiguous native dimension references"), "{error}");
    assert_eq!(serde_json::to_value(s.project()).unwrap(), before);
}

#[test]
fn a_non_positive_hole_size_is_refused_by_name() {
    // Live test D3: a negative hole diameter set through dimension_set answered only "operation failed: Hole".
    let (mut s, _, _, _) = stock();
    let hole = Hole {
        body: None,
        face: Sel::Extreme { axis: Axis::Z, max: true },
        at: None,
        offset: None,
        diameter: 2.0.into(),
        depth: Some(1.0.into()),
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: Some("Vent".into()),
    };
    let (feature, _) = s.hole(&hole).unwrap();
    let before = serde_json::to_value(s.project()).unwrap();
    for key in ["diameter", "depth"] {
        let error = s.dimension_set(&DimensionTarget::Feature { feature, key: key.into() }, Some(&(-1.0).into()), None).unwrap_err();
        assert_eq!(error.to_string(), format!("invalid request: {key} must be positive, got -1"));
        assert_eq!(serde_json::to_value(s.project()).unwrap(), before);
    }
}
