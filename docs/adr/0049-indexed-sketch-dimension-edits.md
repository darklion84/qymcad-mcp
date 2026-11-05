# ADR 0049: Indexed sketch dimension edits versus native names

- Status: accepted (2026-11-04).
- Related: ADR 0008, ADR 0020; F-112. Research: `tasks/research-medF.md` section F2.

## Context

Generic sketch pins dimension a point's x and y from the origin with two `Distance` rows on the same point pair.
Native `DimTarget::Sketch` stores only the reference set, and native `dim_index`/SetDriver resolve the first
set-equal row, so a refs-only edit of the y row changes x. The server converted the caller's exact constraint
index to that refs-only target and therefore refused every such edit as ambiguous. The app's inline dimension
editor instead writes the selected `(sketch, constraint)` row (`qymcad-sketch/src/editors.rs:623-699`,
`dims.rs:24-58`); only naming goes through the refs-only target.

## Decisions

1. `dimension_set {target:{sketch, constraint}}` with a value or formula writes exactly that row, with the app's
   `set_dim_value` variant semantics (DistancePL keeps its side sign) and the formula on the same row. Validation,
   solve, propagation, pattern settlement, checked rebuild and undo/batch boundaries are unchanged.
2. A name (including the empty name) on a row whose references are shared is refused before any change:
   `constraint {ci} in sketch {sketch} has ambiguous native driver references; value/formula can be edited by index, but this dimension cannot be named independently in QymCAD 0.1.1`.
   A native name on the first row of an imported shared set stays bound to that first row. A formula on another row of that set may read this name: it is checked as an unnamed formula, while the first row itself keeps the native self-reference refusal.
3. The result reports the selected row: `target` is the given selector, `value`/`expression` come from that row,
   and `name` is reported only when the row is the native first hit.
4. Rows with unique references keep the native SetDriver path. No file-format change. Constraint indices are a
   transient selector and shift after `sketch_remove`.

Not done: distinct names for x/y pins would need a different pin representation (axis DistancePL, as explicit
points already use, F-112) and a migration; separate scope.

## Evidence

`crates/engine/tests/golden_dimension_index.rs`: both circle-centre rows edit only their own axis; triangle
(3,4),(13,4),(13,14) × 2 goes from 100 to 70 mm³ when the second vertex's y row becomes 7, also after save/reopen;
formulas on either row follow their parameter; naming refused with the project unchanged; undo restores the row
and solid. All fail on the previous code. `golden_dimensions_1` keeps the naming refusal with the new message.
