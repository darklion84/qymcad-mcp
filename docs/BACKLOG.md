# Backlog

Known follow-ups that were consciously deferred. Each item says where it came from.

- **Re-solve only the sketches a parameter edit reaches.** `Session::propagate_params` (crates/engine/src/params.rs)
  re-solves every sketch that has any expression on every parameter edit. Solving only the sketches whose
  expressions mention the changed name (directly or through other parameters) is a performance optimisation for
  large documents; it must keep F-001 (case-insensitive matching) and F-002 in mind. Raised in the phase 3A review.
- **`Patch` edge queries are not gated by the unrestorable-edge guard** (MiMo, final review): the guard in
  `Session::rebuild_retrying` covers fillet/chamfer stored queries; extend it if the engine ever creates Patch
  features with edge queries.
- **The rebuild guard clones the `Project` on every public rebuild** (MiMo, final review): fine for small parts;
  make the retry-plan check incremental if large documents get slow.
- **Persist face-kind selections in modifiers** (C1/E7 native-capability gate): previews support kind filters,
  but `refs::Query` has no native kind variant. Choose between lowering to fixed persistent face ids (does not
  discover newly created faces) and adding upstream native query support. Until a decision, face modifiers
  refuse kind filters; edge modifiers already store resolved ids. Evidence/options: ADR 0013 and tasks/review-c1.md.
- **Low items from the E1-E9 review** (MiMo): `corner_planar_normal` should reject `face_sphere` itself (today only
  its caller does, so the unit test cannot prove the guard); the shared rollback helper is not byte-for-byte the
  former semantics of both callers (document the difference or align); the `to_query` kind error is worded for
  faces even for edges; `empty_corner_hint` recomputes `classified_corners` up to three times.

