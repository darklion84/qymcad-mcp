use qymcad_engine::*;

fn u_part() -> (Session, SplitResult) {
    let mut s = Session::new_part();
    s.param_set("right_end", &22.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_polyline(
        sk,
        &PolylineSpec {
            points: vec![
                Xy((-10.).into(), 0.0.into()),
                Xy(10.0.into(), 0.0.into()),
                Xy(10.0.into(), "right_end".into()),
                Xy(6.0.into(), "right_end".into()),
                Xy(6.0.into(), 4.0.into()),
                Xy((-6.).into(), 4.0.into()),
                Xy((-6.).into(), 20.0.into()),
                Xy((-10.).into(), 20.0.into()),
            ],
            closed: true,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    let src = s
        .extrude(&Extrude {
            down: None,
            sketch: sk,
            profiles: None,
            height: 10.0.into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    let result = s
        .split(&SplitOptions {
            body: Some(src),
            plane: PlaneRef::Base(BaseName::XZ),
            offset: 10.0.into(),
            name: Some("U split".into()),
            piece_names: Some(vec!["bridge".into(), "left".into(), "right".into()]),
        })
        .unwrap();
    (s, result)
}

fn breps(s: &Session) -> Vec<(Id, Vec<u8>)> {
    let _gate = qymcad_kernel::kernel_gate();
    s.project().timeline.iter().flat_map(|n| n.kind.bodies()).map(|id| (id, s.shape(id).unwrap().to_brep_bytes().unwrap())).collect()
}

#[test]
fn a_same_count_split_edit_cannot_swap_left_and_right() {
    let (mut s, result) = u_part();
    let before = serde_json::to_value(s.info()).unwrap();
    let bytes = breps(&s);
    let left = s.resolve("left").unwrap();
    let right = s.resolve("right").unwrap();
    assert!(result.pieces.iter().find(|b| b.id == left).unwrap().bbox[3] < 0.0);
    let edited = s.param_set("right_end", &18.0.into());
    println!("after limb edit: {edited:?}");
    let error = edited.expect_err("same-count split edit must refuse native identity swap").to_string();
    for text in ["identity", "left", "right", &left.to_string(), &right.to_string()] {
        assert!(error.contains(text), "missing {text} in {error}");
    }
    assert_eq!(serde_json::to_value(s.info()).unwrap(), before, "recipe, names, formula values and diagnostics restored");
    assert_eq!(breps(&s), bytes, "every source, split and downstream B-rep is unchanged");
}

#[test]
fn split_pieces_can_move_without_reordering() {
    let (mut s, result) = u_part();
    let ids = [s.resolve("bridge").unwrap(), s.resolve("left").unwrap(), s.resolve("right").unwrap()];
    s.dimension_set(&DimensionTarget::Feature { feature: result.feature, key: "offset".into() }, Some(&12.0.into()), None).unwrap();
    s.param_set("right_end", &24.0.into()).unwrap();
    let bodies = s.result_bodies();
    // U bridge: (20*4+2*4*(12-4))*10; legs: width4 times remaining height times depth10.
    for ((body, id), volume) in bodies.iter().zip(ids).zip([1440.0, 320.0, 480.0]) {
        assert_eq!(body.id, id);
        assert!((body.volume - volume).abs() < 1e-7);
    }
    assert!(bodies[1].bbox[3] < 0.0);
    assert!(bodies[2].bbox[0] > 0.0);
    assert_eq!(bodies[1].bbox[1], 12.0);
    assert_eq!(bodies[2].bbox[4], 24.0);
}

#[test]
fn a_preexisting_split_error_cannot_exempt_an_identity_swap() {
    let (mut s, split) = u_part();
    let path = std::env::temp_dir().join(format!("qymcad-error-split-{}.qcad", std::process::id()));
    s.save(Some(&path)).unwrap();
    let mut loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    // Current right_end22 gives offset2, two pieces (invalid stored count3).
    // Editing it to18 restores offset10/count3 but native swaps the two limbs.
    loaded.project.feat_dims.entry(split.feature).or_default().insert("offset".into(), "(22-right_end)*2+2".into());
    loaded.project.timeline.iter_mut().find(|n| n.id == split.feature).unwrap().dirty = true;
    qymcad_io::save_project_with_brep(&loaded.project, path.to_str().unwrap(), &loaded.breps).unwrap();
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.iter().any(|e| e.node == split.feature), "fixture must have an existing count error");
    let before = s.info();
    let bytes = breps(&s);
    let edited = s.param_set("right_end", &18.0.into());
    assert!(edited.is_err(), "preexisting count error hid a new split identity refusal");
    assert_eq!(s.info(), before);
    assert_eq!(breps(&s), bytes);
    s.dimension_set(&DimensionTarget::Feature { feature: split.feature, key: "offset".into() }, Some(&10.0.into()), None).unwrap();
    assert!(s.current_rebuild().errors.is_empty(), "safe split repair must remain possible");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_split_edit_that_misses_the_body_says_why_once() {
    let mut s = Session::new_part();
    s.param_set("h", &10.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &10.0.into(), &10.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let src = s
        .extrude(&Extrude {
            down: None,
            sketch: sk,
            profiles: None,
            height: 20.0.into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    s.split(&SplitOptions {
        body: Some(src),
        plane: PlaneRef::Base(BaseName::XY),
        offset: "h".into(),
        name: Some("divide".into()),
        piece_names: None,
    })
    .unwrap();
    let before = s.info();
    let text = s.param_set("h", &45.0.into()).unwrap_err().to_string();
    assert!(text.contains("no longer divides the body"), "{text}");
    assert_eq!(text.matches("divide (").count(), 1, "the failing node is named once: {text}");
    assert_eq!(s.info(), before);
}
