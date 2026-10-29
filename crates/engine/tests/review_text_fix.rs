use qymcad_engine::*;

fn text_part(height: f64) -> (Session, Id, Id) {
    text_part_with_depth(height, 2.)
}

fn text_part_with_depth(height: f64, depth: f64) -> (Session, Id, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let text = s
        .sketch_text(sk, &TextSpec { text: "ABCDEFGHIJ".into(), at: [3., 4.], height, angle: 0., font: None, construction: false })
        .unwrap();
    let (body, _) = s
        .extrude(&Extrude {
            down: None,
            sketch: sk,
            profiles: None,
            height: depth.into(),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    (s, text, body)
}

#[test]
fn review_text_height_edit() {
    let (mut edited, text, body) = text_part(10.);
    let before = edited.shape(body).unwrap().volume();
    let result = edited.dimension_set(&DimensionTarget::Feature { feature: text, key: "height".into() }, Some(&20.0.into()), None);
    println!("height edit result: {result:?}");
    let actual = edited.shape(body).unwrap().volume();
    let (fresh, _, fresh_body) = text_part(20.);
    let expected = fresh.shape(fresh_body).unwrap().volume();
    println!("before={before} edited={actual} fresh={expected}");
    assert!(result.is_err() || (actual - expected).abs() < 1e-5, "fixed-height policy bypass: accepted edit changes glyph regions");
}

fn snapshot(s: &Session) -> String {
    serde_json::to_string(s.project()).unwrap()
}

#[test]
fn every_text_item_dimension_key_is_fixed_or_unavailable() {
    let (mut s, text, body) = text_part(10.);
    let before = snapshot(&s);
    let volume = s.shape(body).unwrap().volume().to_bits();
    // Only `height` is a native text item value. Include placement aliases, rotation, font and label
    // keys so future native additions cannot accidentally widen the fixed-text boundary.
    for key in ["height", "angle", "turn", "x", "y", "at", "font", "font_index", "text"] {
        let err = s
            .dimension_set(&DimensionTarget::Feature { feature: text, key: key.into() }, Some(&20.0.into()), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("text geometry is fixed") || err.contains("no numeric dimension"), "{key}: {err}");
        assert_eq!(snapshot(&s), before, "{key} changed the recipe");
        assert_eq!(s.shape(body).unwrap().volume().to_bits(), volume, "{key} changed the B-rep");
    }
}

#[test]
fn text_is_not_constraint_geometry_and_refusals_are_atomic() {
    let (mut s, text, body) = text_part(10.);
    let sk = s.project().sketches[0].id;
    let before = snapshot(&s);
    let volume = s.shape(body).unwrap().volume().to_bits();
    for kind in [ConstraintKind::Distance, ConstraintKind::Angle, ConstraintKind::Diameter, ConstraintKind::Fix] {
        let err = s
            .sketch_edit(sk, |s| {
                s.sketch_constrain(
                    sk,
                    &ConstrainSpec {
                        kind,
                        refs: vec![SketchRef::Id(text)],
                        value: Some(20.0.into()),
                        axis: DistAxis::Aligned,
                        reference: false,
                    },
                )
            })
            .unwrap_err()
            .to_string();
        assert!(err.contains("no point or entity"), "{kind:?}: {err}");
        assert_eq!(snapshot(&s), before);
        assert_eq!(s.shape(body).unwrap().volume().to_bits(), volume);
    }
}

fn imported_height_formula(from_dimension: bool) -> (Session, Id, Id) {
    let (mut s, text, body) = text_part(10.);
    let expression = if from_dimension {
        s.dimension_set(&DimensionTarget::Feature { feature: body, key: "height".into() }, None, Some("depth")).unwrap();
        "depth*5"
    } else {
        s.param_set("letter_height", &10.0.into()).unwrap();
        "letter_height"
    };
    let dir = std::env::temp_dir().join(format!("qymcad-text-fix-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("inherited-text-height-{from_dimension}.qcad"));
    s.save(Some(&path)).unwrap();
    let mut loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    loaded.project.feat_dims.entry(text).or_default().insert("height".into(), expression.into());
    qymcad_io::save_project_with_brep(&loaded.project, path.to_str().unwrap(), &loaded.breps).unwrap();
    (Session::open(&path).unwrap().0, text, body)
}

#[test]
fn imported_text_height_formula_cannot_be_changed_by_parameter_or_other_dimension() {
    for from_dimension in [false, true] {
        let (mut s, _, body) = imported_height_formula(from_dimension);
        let before = snapshot(&s);
        let volume = s.shape(body).unwrap().volume().to_bits();
        let err = if from_dimension {
            s.dimension_set(&DimensionTarget::Feature { feature: body, key: "height".into() }, Some(&4.0.into()), None).unwrap_err()
        } else {
            s.param_set("letter_height", &20.0.into()).unwrap_err()
        }
        .to_string();
        assert!(err.contains("text") && err.contains("geometry is fixed") && err.contains("recreate"), "{err}");
        assert_eq!(snapshot(&s), before, "indirect text edit changed recipe");
        assert_eq!(s.shape(body).unwrap().volume().to_bits(), volume, "indirect text edit changed live shape");
    }
}

#[test]
fn ordinary_parameter_dimension_and_sketch_edits_match_a_fresh_text_build() {
    let (mut s, text, body) = text_part(10.);
    let original = s.sketch_info(s.project().sketches[0].id).unwrap().texts[0].clone();
    s.param_set("depth", &2.0.into()).unwrap();
    s.dimension_set(&DimensionTarget::Feature { feature: body, key: "height".into() }, Some(&"depth".into()), None).unwrap();
    s.param_set("depth", &3.0.into()).unwrap();
    // A construction circle gives sketch_constrain real geometry beside the fixed text.
    let sk = s.project().sketches[0].id;
    let (circle, _) = s.sketch_edit(sk, |s| s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), true)).unwrap();
    s.sketch_edit(sk, |s| {
        s.sketch_constrain(
            sk,
            &ConstrainSpec {
                kind: ConstraintKind::Radius,
                refs: vec![SketchRef::Id(circle)],
                value: None,
                axis: DistAxis::Aligned,
                reference: true,
            },
        )
    })
    .unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().texts.iter().find(|t| t.id == text).unwrap(), &original);
    let (fresh, _, fresh_body) = text_part_with_depth(10., 3.);
    assert!((s.shape(body).unwrap().volume() - fresh.shape(fresh_body).unwrap().volume()).abs() < 1e-7);
    let actual = s.shape(body).unwrap().bbox().unwrap();
    let expected = fresh.shape(fresh_body).unwrap().bbox().unwrap();
    assert!(actual.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-7));
}

#[test]
fn opening_a_stale_text_item_formula_cannot_resize_existing_text() {
    let (mut s, text, _) = text_part(10.);
    let path = std::env::temp_dir().join(format!("qymcad-stale-text-{}.qcad", std::process::id()));
    s.save(Some(&path)).unwrap();
    let mut loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    loaded.project.feat_dims.entry(text).or_default().insert("height".into(), "20".into());
    qymcad_io::save_project_with_brep(&loaded.project, path.to_str().unwrap(), &loaded.breps).unwrap();
    let opened = Session::open(&path);
    assert!(opened.is_err(), "opening evaluated an inherited text height formula and resized the stored text");
    let error = opened.err().unwrap().to_string();
    assert!(error.contains("text") && error.contains("geometry is fixed"), "{error}");
    std::fs::remove_file(path).unwrap();
}
