mod common;
use common::*;
use qymcad_engine::*;

fn box_body(s: &mut Session, cx: Num, cy: Num, width: Num, height: Num, name: &str) -> Id {
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some(&format!("{name} sketch"))).unwrap();
    s.sketch_rect(sketch, &cx, &cy, &width, &height, false).unwrap();
    s.extrude(&Extrude {
        sketch,
        profiles: None,
        height: 20.0.into(),
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: Some(name.into()),
    })
    .unwrap()
    .0
}

fn boolean(a: Id, b: Id, op: BooleanOp, names: Option<Vec<&str>>) -> BodyBooleanOptions {
    BodyBooleanOptions {
        a,
        b,
        op,
        name: Some("boolean result".into()),
        piece_names: names.map(|v| v.into_iter().map(str::to_owned).collect()),
    }
}

fn check_box(body: &BodyInfo, volume: f64, bounds: [f64; 6]) {
    assert_close(body.volume, volume, 1e-5, "boolean volume");
    for (actual, expected) in body.bbox.into_iter().zip(bounds) {
        assert_close(actual, expected, 0.001, "boolean bbox");
    }
}

#[test]
fn overlapping_box_booleans_have_hand_computed_geometry_and_consumed_inputs() {
    for (op, expected_volume, expected_bbox) in [
        (BooleanOp::Cut, 4000.0, [0., 0., 0., 10., 20., 20.]),
        (BooleanOp::Union, 12000.0, [0., 0., 0., 30., 20., 20.]),
        (BooleanOp::Intersect, 4000.0, [10., 0., 0., 20., 20., 20.]),
    ] {
        let mut s = Session::new_part();
        let a = box_body(&mut s, 10.0.into(), 10.0.into(), 20.0.into(), 20.0.into(), "A");
        let b = box_body(&mut s, 20.0.into(), 10.0.into(), 20.0.into(), 20.0.into(), "B");
        let result = s.body_boolean(&boolean(a, b, op, None)).unwrap();
        assert_eq!(result.consumed, [a, b]);
        assert_eq!(result.body, result.feature);
        assert_eq!(result.op, op);
        assert_eq!(result.pieces.len(), 1);
        check_box(&result.pieces[0], expected_volume, expected_bbox);
        assert!(result.rebuild.errors.is_empty());
        assert!(s.project().consumed_bodies().contains(&a));
        assert!(s.project().consumed_bodies().contains(&b));
    }
}

#[test]
fn cut_returns_named_pieces_and_refuses_changed_piece_count_atomically() {
    let mut s = Session::new_part();
    s.param_set("x", &0.0.into()).unwrap();
    let a = box_body(&mut s, 0.0.into(), 0.0.into(), 30.0.into(), 10.0.into(), "base");
    let b = box_body(&mut s, Num::Expr("x".into()), 0.0.into(), 2.0.into(), 12.0.into(), "tool");
    let result = s.body_boolean(&boolean(a, b, BooleanOp::Cut, Some(vec!["east", "west"]))).unwrap();
    assert_eq!(result.pieces.len(), 2);
    assert_eq!(s.resolve("east").unwrap(), result.pieces[0].id);
    assert_eq!(s.resolve("west").unwrap(), result.pieces[1].id);
    assert_eq!(result.pieces.iter().map(|piece| piece.name.as_str()).collect::<Vec<_>>(), vec!["east", "west"]);
    let path = scratch("body-boolean-pieces.qcad");
    s.save(Some(&path)).unwrap();
    let (mut reopened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    let before = reopened.info();
    let error = reopened.param_set("x", &15.0.into()).unwrap_err().to_string();
    assert!(error.contains("output count changed") && error.contains("recreate the boolean"), "{error}");
    assert_eq!(reopened.info(), before);
}

#[test]
fn both_operands_rebuild_after_save_open_and_app_path_edit() {
    for edit_b in [false, true] {
        let mut s = Session::new_part();
        s.param_set("w", &20.0.into()).unwrap();
        s.param_set("x", &20.0.into()).unwrap();
        let a_width = if edit_b { 20.0.into() } else { Num::Expr("w".into()) };
        let b_center = if edit_b { Num::Expr("x".into()) } else { 20.0.into() };
        let a = box_body(&mut s, 10.0.into(), 10.0.into(), a_width, 20.0.into(), "A");
        let b = box_body(&mut s, b_center, 10.0.into(), 20.0.into(), 20.0.into(), "B");
        let result = s.body_boolean(&boolean(a, b, BooleanOp::Cut, None)).unwrap();
        check_box(&result.pieces[0], 4000.0, [0., 0., 0., 10., 20., 20.]);
        let path = scratch(if edit_b { "boolean-edit-b.qcad" } else { "boolean-edit-a.qcad" });
        s.save(Some(&path)).unwrap();
        let (mut reopened, report) = Session::open(&path).unwrap();
        assert!(report.errors.is_empty());
        if edit_b {
            reopened.param_set("x", &24.0.into()).unwrap();
            check_box(&reopened.result_bodies()[0], 5600.0, [0., 0., 0., 14., 20., 20.]);
            let (volume, bounds, _) = gui_edit_param_faces(&path, "x", "24");
            assert_close(volume, 5600.0, 1e-5, "app-path edit of B");
            assert_close(bounds[3], 14.0, 0.001, "app-path edited B bound");
        } else {
            reopened.param_set("w", &24.0.into()).unwrap();
            check_box(&reopened.result_bodies()[0], 4800.0, [-2., 0., 0., 10., 20., 20.]);
            let (volume, bounds, _) = gui_edit_param_faces(&path, "w", "24");
            assert_close(volume, 4800.0, 1e-5, "app-path edit of A");
            assert_close(bounds[3], 10.0, 0.001, "app-path edited A bound");
        }
    }
}

#[test]
fn body_boolean_refuses_invalid_and_noop_inputs_without_mutation() {
    let mut s = Session::new_part();
    let a = box_body(&mut s, 0.0.into(), 0.0.into(), 10.0.into(), 10.0.into(), "A");
    let b = box_body(&mut s, 30.0.into(), 0.0.into(), 10.0.into(), 10.0.into(), "B");
    let before = s.info();
    assert!(s.body_boolean(&boolean(a, a, BooleanOp::Union, None)).is_err());
    assert!(s.body_boolean(&boolean(a, 999999, BooleanOp::Cut, None)).is_err());
    assert!(s.body_boolean(&boolean(a, b, BooleanOp::Cut, None)).is_err());
    assert!(s.body_boolean(&boolean(a, b, BooleanOp::Intersect, None)).is_err());
    assert_eq!(s.info(), before);
}
