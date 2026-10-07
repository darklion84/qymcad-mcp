# U7 and U2 review

## U2 capability investigation (stopped on feature-pattern implementation)

The pinned QymCAD has no native array of an existing feature or independently saved cut tool. Native `FeatureKind::LinearArray` / `CircularArray` store a source **body** (`qymcad-core/src/feature.rs:1187-1230`). Adders `model/timeline.rs:1271-1291` accept `src` and transforms/counts only. Preparers `model/regen.rs:2230-2290` call `Kernel::pattern_named` on that body. The kernel clones the complete source `Shape` at each transform and unites the copies (`qymcad-kernel/src/kernel.rs:1006-1041`). `Combine` builds its cut from sketch profiles (`model/timeline.rs:967-975`), not from an existing standalone tool-body input.

Options:
1. Current supported workaround: sketch circles at expression-driven centres (linear `i*spacing`, circular `r*cos(angle)` / `r*sin(angle)` with coordinates retaining their sign), then one `extrude` with `op=cut`, `through=true`. Count is the number of sketch entities; changing a parameter does not add/remove circles automatically.
2. Native sketch-driven multi-hole: `add_hole_from_sketch` (`model/timeline.rs:1345-1359`), `prep_hole` (`model/regen.rs:2081-2093`), `Kernel::holes` (`kernel.rs:997-1005`). Creates one hole per isolated sketch point in one boolean, supports the normal hole parameters, but has no count/spacing expressions or a seed feature. Exposing it requires a point-entity API and explicit scope/design, so it is an option, not silently substituted for a feature pattern.
3. An upstream native feature-pattern node could store a seed operation plus count and transforms and implement GUI-safe regeneration. Engine-only expansion into repeated nodes would not change cardinality when the saved file's count is edited in QymCAD.app. That does not satisfy server AND GUI parameter requirements and was refused.

Changed `crates/mcp/src/tools/features.rs` array descriptions: explicit entire body including holes/cuts, and sketch circles + through-cut workaround. Documentation-only change; no behavioural failing/mutation test applicable. Parent regenerates docs/TOOLS.md and integrates F-027 evidence / CHANGELOG notes.

## U7a parameter users

Files: `crates/engine/src/params.rs`, new `crates/engine/tests/datum_users.rs`.
Tests: `datum_parameter_users_identify_sketch_dependencies` asserts the guard label, absence of a false feature label, and preserved parameter-delete refusal.
Failure before: pending first build.
Mutation: pending.

## U7b structural GUI limitation

File: `crates/mcp/src/tools/params.rs` (`param_set` description).
The tool docs now direct agents to reopen and save through the server after GUI re-hosting, new datum sketches, or replacing datum distance expressions, before further GUI parameter edits. Parent adds CHANGELOG note and regenerated docs/TOOLS.md. Existing ADR 0007 documents the design; no new decision introduced.

## U7c/d top-face area units and bound

Files: `crates/engine/tests/common/mod.rs`, `crates/engine/tests/golden_datum.rs`.
Tests: a synthetic planar-face regression rejects area 249 mm² when the formula yields 30*10 - 10*5 = 250 mm². Formula and geometry checks remain on the real golden-plate fixture.
Failure before: pending first build.
Mutation: pending.

## U7e both-datum mode and guard verification

File: `crates/engine/tests/golden_datum.rs`.
Both mode uses parent offset t/2 and child offset t/2. Its total offset is t/2+t/2 = t, 6 at t=6 and 10 at t=10. Cap positions and invariant W*L*H volume are checked through server, native GUI, and reopened-server paths.
Failure before: pending first build.
Mutation: pending.
Strengthen the existing guard-refresh regression with exact reserved guard map assertions and replaced-parameter server/GUI movement checks. MiMo's report claims stale removal is undetected, but the tester explicitly reports a removal mutation already fails the existing geometry test; direct map checks remove that ambiguity without changing the engine workaround.

## U7f/g documentation claims

Files: `crates/mcp/tests/protocol.rs`, `docs/adr/0007-datum-sketch-dependencies.md`, `docs/FINDINGS.md` (F-017 evidence paragraph only).
Added a one-line literal-pocket comment referencing the expression-covered golden fixture. Narrowed disabling-tracking claim to literal-pocket placement and datum-only cap-position regressions; expression-driven dimensions mask the scheduler defect.

## Verification

Pending targeted fmt/clippy/tests; parent owns final scripts/check.sh and total test count in review-u1.md. No commits.
