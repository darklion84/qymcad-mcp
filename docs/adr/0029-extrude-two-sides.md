# ADR 0029: Native extrude Two sides

- Status: accepted (2026-10-29).
- Related: ADR 0024, F-008, F-024, F-074, F-104, F-111.

## Context

Pinned QymCAD distinguishes Symmetric from Two sides. Its public Extrude and Combine adders both accept `down`, but native regeneration takes its absolute value, ignores lengths at or below 1e-9 mm, and ignores the second side with BothWays or through-all. Passing a seed alone loses later parameter edits.

## Decisions

1. Extend `extrude` with optional `down: Num`. Absence preserves existing one-sided behavior. Presence means native Two sides: normal spans [-down, height] relative to the sketch normal; reverse spans [-height, down]. Symmetric still divides height equally in both directions.
2. Refuse explicit `down` with symmetric or through-all. Require its evaluated value to be finite and strictly greater than 1e-9 mm at creation and every evaluated edit, rebuild and open. `dimension_set down` obeys the same rule, including zero. Recreate the feature to return to one side.
3. Pass the evaluated length to both `add_extrude_multi` and `add_combine_multi_op`, covering add, new body, cut and intersect. Store its native `down` formula through the same formula path as height. Preserve numeric precision (F-104) and native named feature dimensions; canonicalize plain numeric strings to full-precision decimal formulas. Repeat the domain check after edits even if the same node already had a native rebuild error.
4. Keep the existing result shape, owner context, profile selection, atomicity, batch and undo paths. Retain F-024 stored-query preflight and F-074 downstream fillet validation. Recipe writes need no new kernel gate; self-gating native doors must never be nested under a caller gate.

## Consequences

Two independently editable sides survive save/load and native app parameter scheduling. Native app inputs themselves still have native abs/ignore behavior; server refusal is a boundary policy, not an upstream change. Independent tests use a 10×6 profile, h=7 and d=3: V=600 mm³ with forward z=[-3,7] or reverse z=[-7,3]. A centered cut inside 20×16×20 stock removes 600 mm³, leaving 5800 mm³; cut cap positions distinguish direction even when volumes match.

Evidence: stage-0 probes in `tasks/research-medA.md` section 1; implementation tests `crates/engine/tests/golden_extrude_two_sides.rs`; failing-first and mutation results in `tasks/A1-report.md`.
