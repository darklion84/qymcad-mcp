# ADR 0006: Refuse coordinate expressions that cross zero

- Status: accepted (2026-10-07)

## Context
QymCAD's axis Distance measures a magnitude and retains the point's original side. DistancePL also preserves
its stored sign when evaluating an expression. A coordinate expression crossing zero therefore cannot reliably
move the point across the axis in either the server or the pinned GUI (FINDINGS F-045). Accepting an unsettled
solve would commit incorrect geometry.

## Decision
Refuse a parameter edit whose coordinate dimensions cannot settle, and roll back the edit through the existing
residual check. Place the sketch origin so coordinate expressions retain their sign. Use directed ArcLength
dimensions for parametric directions, which need to cross quadrants.

Rejected alternatives:
- Splitting coordinate sums into positive terms works for only some expressions and adds geometry/dimensions
  without supporting arbitrary sign changes.
- Far anchor points shift dimensions to positive magnitudes but expand the sketch bounds and zoom the GUI out.

## Consequences
The server reports the failed solve instead of accepting the wrong side. A user editing such an expression in
the pinned GUI still gets an unsolved sketch; no vendor solver changes are introduced. Evidence:
`golden_sketch.rs::a_coordinate_that_would_cross_zero_is_refused` verifies refusal and rollback.
