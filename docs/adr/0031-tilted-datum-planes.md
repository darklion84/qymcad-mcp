# ADR 0031: Native tilted datum planes and ancestor barriers

- Status: accepted (2026-10-29).
- Related: ADR 0007, ADR 0029, F-017, F-069, F-073, F-104, F-113.

## Context

The app's datum bar exposes distance, Tilt X, Tilt Y and Turn on the same native WorkPlane. The pinned scheduler can update an ancestor and child datum while leaving a child sketch's solid at its old position, without errors. An invariant volume does not reveal that defect.

## Decisions

1. Extend `plane_offset` with optional `tilt_x`, `tilt_y` and `turn`, each a Num in degrees defaulting to zero. Retain base XY/XZ/YZ or datum, and the result `{plane,rebuild}`. Face-based creation remains unsupported. Keep the legacy engine method as a zero-angle wrapper.
2. Use the app's native offset adder and WorkPlane tilt/rotation fields. Store expressions through `write_feat_formula` with `PlainNumber::Stands`, preserving literal precision and native named dimension keys `dist`, `tilt_x`, `tilt_y`, `turn`. Native regeneration owns frame math: X then Y about the original support axes; Turn rotates sketch axes about the resulting normal. A child inherits the normal, not the parent's Turn. The sketch origin is the projection of world zero on its plane.
3. Require finite angles within inclusive ±360 degrees and distance within inclusive ±100000 mm. Refuse outside values without normalization at creation, parameter/dimension edits, rebuild and open. Repeat the guard after evaluated edits even when the same node already had a rebuild error. Legacy zero-angle documents retain their placement.
4. Reconcile reserved native sketch feature-dimension barriers for all four fields and their named drivers across the entire datum ancestry. Refresh after formula/name edits, parameter propagation, open and before rebuild. Remove obsolete reserved guards while preserving unrelated entries. This extends the persisted ADR 0007 barrier, so saved global and named driver edits use the unmodified app scheduler.
5. Preserve existing snapshot/retry, stored-query preflight, downstream fillet checks, batch and undo paths. Native regeneration and import restoration acquire their own kernel gate; callers hold no outer gate.

## Consequences

Direct planes and a two-deep chain are tested with independent transformed prism corners, volume and saved native app global/named edits. GUI structural edits to placement formulas/driver names or sketch hosts still require reopen/save through the server to reconcile persisted barriers, as in ADR 0007. This is a saved recipe fix for the tested parameter paths, not an upstream scheduler patch.

Evidence: stage-0 `tasks/research-medA.md` section 4 and `research-medA/plane/{probe.rs,output.txt}`; `crates/engine/tests/golden_datum_tilt.rs`; failing-first and restored mutation logs in `tasks/A2-report.md`.
