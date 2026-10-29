//! Names must survive the resolver's id-first interpretation (review F5).
use qymcad_engine::*;

fn stock(s: &mut Session) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &10.0.into(), &10.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    s.extrude(&Extrude {
        down: None,
        sketch: sk,
        profiles: None,
        height: 20.0.into(),
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: Some("stock".into()),
    })
    .unwrap()
    .0
}

fn numeric_names() -> Vec<String> {
    vec!["99999".into(), "+1".into(), " 99999 ".into(), "00001".into(), "0".into(), Id::MAX.to_string()]
}

#[test]
fn numeric_piece_names_are_refused_atomically() {
    for name in numeric_names() {
        let mut s = Session::new_part();
        let body = stock(&mut s);
        let before = s.info();
        let result = s.split(&SplitOptions {
            body: Some(body),
            plane: PlaneRef::Base(BaseName::XY),
            offset: 10.0.into(),
            name: Some("cut".into()),
            piece_names: Some(vec![name.clone(), "upper".into()]),
        });
        let err = result.expect_err(&format!("piece name {name:?} is interpreted as an id")).to_string();
        assert!(err.contains("name") && err.contains("id"), "{err}");
        assert_eq!(s.info(), before, "refusal must leave all pieces and names unchanged");
    }
}

#[test]
fn numeric_sketch_names_are_refused_before_mutation() {
    for name in numeric_names() {
        let mut s = Session::new_part();
        let before = s.info();
        let result = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some(&name));
        let err = result.expect_err(&format!("sketch name {name:?} is interpreted as an id")).to_string();
        assert!(err.contains("name") && err.contains("id"), "{err}");
        assert_eq!(s.info(), before);
    }
}

#[test]
fn numeric_feature_names_are_refused_atomically() {
    for name in numeric_names() {
        let mut s = Session::new_part();
        let body = stock(&mut s);
        let before = s.info();
        let err = s.plane_offset(&PlaneRef::Base(BaseName::XY), &1.0.into(), Some(&name)).expect_err("numeric plane name");
        assert!(err.to_string().contains("id"));
        assert_eq!(s.info(), before);
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_circle(sk, &40.0.into(), &0.0.into(), &5.0.into(), false).unwrap();
        let before = s.info();
        let err = s
            .extrude(&Extrude {
                down: None,
                sketch: sk,
                profiles: None,
                height: 10.0.into(),
                op: Op::NewBody,
                direction: Direction::Normal,
                through: false,
                target: None,
                name: Some(name.clone()),
            })
            .expect_err("numeric extrude name");
        assert!(err.to_string().contains("id"));
        assert_eq!(s.info(), before);
        let err = s.mirror(Some(body), &PlaneRef::Base(BaseName::YZ), false, Some(&name)).expect_err("numeric mirror name");
        assert!(err.to_string().contains("id"));
        assert_eq!(s.info(), before);
        let err = s
            .split(&SplitOptions {
                body: Some(body),
                plane: PlaneRef::Base(BaseName::XY),
                offset: 10.0.into(),
                name: Some(name.clone()),
                piece_names: Some(vec!["lower".into(), "upper".into()]),
            })
            .expect_err("numeric split feature name");
        assert!(err.to_string().contains("id"));
        assert_eq!(s.info(), before);
    }
}

#[test]
fn names_outside_the_id_grammar_still_resolve() {
    let overflow = ((Id::MAX as u128) + 1).to_string();
    for name in ["-1", "1e3", "part 99999", overflow.as_str()] {
        let mut s = Session::new_part();
        let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some(name)).unwrap();
        assert_eq!(s.resolve(name).unwrap(), sketch);
    }
}
