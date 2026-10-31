# ADR 0030: Native isolated sketch points

- Status: accepted (2026-10-29).
- Related: ADR 0006, ADR 0008, ADR 0020, F-112.

## Context

The app creates a sketch Point through `Project::sketch_point_at(si, x, y, 1e-6)`. Points have no entity or contour of their own, can be reused by later geometry, and add no material. Two ordinary origin-to-point Distance dimensions share the same native reference set, making native naming through `dimension_set` ambiguous.

## Decisions

1. Add `sketch_add` item `{type:"point", x?:Num, y?:Num, dimensioned?:bool}`. Coordinates default to zero and dimensioned defaults to true. Deny unknown fields. Use the app's native point adder and the established settled solve/rebuild transaction.
2. Dimension a new point with DistancePL to the y axis for x and to the x axis for y. Reverse the y-axis reference order so positive x has a positive native driver. These reference sets are distinct and independently nameable. A literal zero coordinate uses PointOnLine on its axis; expressions retain formulas, including expressions evaluating to zero. Apply the existing coordinate sign/zero-crossing policy, retaining the guard when later geometry reuses the point. Validate all magnitude writes before mutation; negative magnitudes refuse atomically, zero retains the stored side. After the native formula check, write accepted Point values/formula text directly, bypassing SetDriver’s abs/clear fast path while preserving literal-versus-formula semantics and legacy side flags. The same evaluated Point guard runs on open, rebuild and parameter/dimension edits, including previously failing sketches; diagnostics identify the sketch and point. For initially negative coordinates reverse the corresponding axis and store a nonnegative magnitude: the native `d < 0` side flag loses its sign at zero. Axis orientation survives zero and native app edits. Normalize numeric strings to full-precision decimal formulas because the native expression parser rejects exponent notation.
3. Return the requested point id even on reuse, reporting `reused:true`. Reuse within native tolerance adds no dimensions. A point at the origin gets its own user id; system points are never adopted as geometry.
4. Extend sketch inspection with isolated-point eligibility and dimension status, preserving existing entity, point and constraint data. Entity ownership includes every native entity kind and construction geometry; constrained points remain isolated if no entity uses them.
5. Extend `sketch_remove` with exclusive `point:<id>`. Refuse system points and points owned by any entity, identifying that entity for removal instead. Delete isolated points through native `Project::delete_points`, within the ordinary atomic, undoable and batch-capable sketch edit path.
6. Persist newly created explicit Point intent, preserving the existing no-edit reuse contract, in native `feat_dims[point]["explicit_point"]="1"`. Deletion/helper cleanup must preserve these points and coordinate drivers independently of entity membership or hole use. Retain Point records across native curve cleanup, offset-solving/regeneration and the loader's sketch settling; restore before acceptance and reschedule consumers if a native pass pruned them. Legacy isolated points remain user geometry except positively identified rectangle/angle/midpoint helpers; old intent already absorbed into an entity/helper is not recoverable from the untagged native data. Deliberate point/constraint removal keeps its existing semantics. No dummy geometry or constraints. R4 tests/mutations are recorded in `tasks/R4-report.md`.
7. Keep profile behavior native: isolated points do not form contours for extrude, revolve or profile selection. Hole by sketch remains a later stage; this ADR does not expose that operation or promise its scheduler semantics.

## Consequences

Agents can place persistent dimensioned markers, edit both named coordinates in the app, inspect eligibility and remove markers safely. Existing curves that reuse a marker make it ineligible for isolated removal. Literal-zero PointOnLine rows are geometric constraints rather than nameable numeric dimensions, matching existing sketch pinning.

Evidence: `tasks/research-medA.md` section 3 and `research-medA/point-hole` probes; `crates/engine/tests/golden_sketch_point.rs`; failing-first, app-path and mutation results in `tasks/A1-report.md`.

R1 evidence: `crates/engine/tests/golden_medium_r1.rs`, `sketch::r1_point_guards`, and `tasks/R1-report.md` (failing-first and production mutation logs).
