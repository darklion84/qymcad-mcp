//! Generated dimension formulas must fit the same budget as user expressions.

use qymcad_engine::*;
use std::collections::HashMap;

fn padded(value: &str) -> Num {
    // Parentheses contribute 2 characters: total 999 < 1000, even after normalization.
    Num::Expr(format!("({value}{})", " ".repeat(999 - value.len() - 2)))
}

fn nested(value: &str, depth: usize) -> Num {
    Num::Expr(format!("{}{value}{}", "(".repeat(depth), ")".repeat(depth)))
}

fn refusal(expr: Num, expected: &str, edit: impl FnOnce(&mut Session, Id, Num) -> Result<()>) {
    // The uncomposed input is valid, so refusal must come from the generated formula.
    expr.eval(&HashMap::new()).unwrap();
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let before = s.sketch_detail(sk).unwrap();
    let result = edit(&mut s, sk, expr);
    assert!(
        matches!(&result, Err(Error::Expr(message)) if message.contains(expected)),
        "expected composed-expression {expected} refusal, got {result:?}"
    );
    assert_eq!(s.sketch_detail(sk).unwrap(), before, "refused composition changes nothing");
}

fn circle(s: &mut Session, sk: Id, x: Num) -> Result<()> {
    s.sketch_circle(sk, &x, &0.0.into(), &2.0.into(), false).map(|_| ())
}

fn slot(s: &mut Session, sk: Id, width: Num) -> Result<()> {
    s.sketch_slot(
        sk,
        &SlotSpec { x1: 0.0.into(), y1: 0.0.into(), x2: 10.0.into(), y2: 0.0.into(), width, construction: false, dimensioned: true },
    )
    .map(|_| ())
}

fn polygon(s: &mut Session, sk: Id, angle: Num) -> Result<()> {
    s.sketch_polygon(
        sk,
        &PolygonSpec {
            cx: 0.0.into(),
            cy: 0.0.into(),
            sides: 3,
            r: Some(10.0.into()),
            angle: Some(angle),
            vertex: None,
            construction: false,
            dimensioned: true,
        },
    )
    .map(|_| ())
}

#[test]
fn negative_coordinate_composition_obeys_length_limit() {
    // -(e) adds 3 characters: 999 + 3 = 1002 > 1000.
    refusal(padded("-2"), "longer than 1000", circle);
}

#[test]
fn negative_coordinate_composition_obeys_nesting_limit() {
    // -(e) adds one level: 64 + 1 = 65 > 64.
    refusal(nested("-2", 64), "nested too deeply", circle);
}

#[test]
fn slot_radius_composition_obeys_length_limit() {
    // (e)/2 adds 4 characters: 999 + 4 = 1003 > 1000.
    refusal(padded("2"), "longer than 1000", slot);
}

#[test]
fn slot_radius_composition_obeys_nesting_limit() {
    // (e)/2 adds one level: 64 + 1 = 65 > 64.
    refusal(nested("2", 64), "nested too deeply", slot);
}

#[test]
fn direction_composition_obeys_length_limit() {
    // (10)*(e)*pi/180 adds 14 characters: 999 + 14 = 1013 > 1000.
    refusal(padded("30"), "longer than 1000", polygon);
}

#[test]
fn direction_composition_obeys_nesting_limit() {
    // A negative turn stores (10)*((e)-(-360))*pi/180: two enclosing levels,
    // so 63 input levels (below the limit) become 65 > 64.
    refusal(nested("-30", 63), "nested too deeply", polygon);
}
