//! Contract of the engine API: atomic features, clear errors, parameter bookkeeping.

mod common;
use common::*;
use qymcad_engine::*;

fn n(s: &str) -> Num {
    Num::Expr(s.into())
}

fn block() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("a", &Num::Value(20.0)).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &n("a"), &n("a"), false).unwrap();
    let (b, _) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: n("a"),
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("block".into()),
        })
        .unwrap();
    (s, b)
}

#[test]
fn cut_without_a_body_is_refused() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
    let e = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: 5.0.into(),
            op: Op::Cut,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap_err();
    assert!(matches!(e, Error::Invalid(_)), "{e}");
}

#[test]
fn a_failing_feature_leaves_the_document_unchanged() {
    let (mut s, _) = block();
    let before = s.info().timeline.len();
    // A cut that misses the body entirely: QymCAD reports CutRemovedNothing.
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &100.0.into(), &100.0.into(), &5.0.into(), false).unwrap();
    let e = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: 5.0.into(),
            op: Op::Cut,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap_err();
    assert!(matches!(e, Error::Rebuild(_)), "{e}");
    assert_eq!(s.info().timeline.len(), before + 1, "only the sketch remains, the cut was rolled back");
    assert_close(s.result_bodies()[0].volume, 8000.0, 1e-6, "volume unchanged");
}

#[test]
fn invalid_parameter_names_and_expressions() {
    let mut s = Session::new_part();
    assert!(matches!(s.param_set("2x", &Num::Value(1.0)), Err(Error::Invalid(_))));
    assert!(matches!(s.param_set("x", &n("nope*2")), Err(Error::Expr(_))));
    assert!(s.params().is_empty(), "a failed set leaves nothing behind");
}

#[test]
fn a_used_parameter_cannot_be_deleted() {
    let (mut s, _) = block();
    let e = s.param_delete("a").unwrap_err();
    assert!(e.to_string().contains("used by"), "{e}");
    s.param_set("unused", &Num::Value(1.0)).unwrap();
    s.param_delete("unused").unwrap();
}

#[test]
fn a_parameter_change_that_breaks_the_model_is_rolled_back() {
    let (mut s, _) = block();
    let e = s.param_set("a", &Num::Value(-5.0)).unwrap_err();
    assert!(matches!(e, Error::Rebuild(_) | Error::Invalid(_)), "{e}");
    assert_eq!(s.params()[0].value, 20.0);
    assert_close(s.result_bodies()[0].volume, 8000.0, 1e-6, "volume restored");
}

#[test]
fn objects_resolve_by_name_or_id() {
    let (s, b) = block();
    assert_eq!(s.resolve("block").unwrap(), b);
    assert_eq!(s.resolve(&b.to_string()).unwrap(), b);
    assert!(matches!(s.resolve("missing"), Err(Error::NotFound(_))));
}
