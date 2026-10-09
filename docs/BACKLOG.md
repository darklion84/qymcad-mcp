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

- **Low items from the G1-G6 live tests** (mcp-tester4/5):
  - the `between` suggestion appears on any body with two or more features in its chain, even with no named
    junction to offer;
  - the same body's bbox differs by the curved-body padding (~0.02 mm) between the feature result and a later
    rebuild;
  - `fillet R0.50 failed on 1 edges` gives no reason or suggestion;
  - the `along` error lists the axis strings but not the accepted [x, y, z] vector;
  - `doc_open` returns stored and rebuilt bboxes without saying which is which;
  - origin/frame points appear in `sketch_info` only after the first `sketch_add`;
  - the 0.001 mm cut entry clearance on internal planes cannot be turned off.
- **Phase 4 acceptance findings** (Sonnet subagent building collet_test/hanging_shelf from the build123d scripts, 2026-10-08):
  - Mouth-circle chamfer via of_feature also returns the seam line; the agent fell back to explicit ids.
- **Low items from the H1-H4 review** (MiMo):
  - the revolve-chamfer advice fires on any chamfer rebuild failure with revolve ancestry (also a genuinely
    oversized chamfer); the ancestry walk follows only boolean operand `a`.
  - UPGRADING.md: the pinning test `upstream_sphere_fit_accepts_a_single_coplanar_floor_mesh` must be removed
    when QymCAD fixes the fit.
- **Low items from the H1-H4 live test** (mcp-tester6, 2026-10-08):
  - wish: kind "torus" (fillet corner tori are "other", like NURBS); asymmetric chamfer result does not say
    which face took `dist`.
- **Remaining MiMo client observations** (2026-10-08): client truncated doc_save arguments; typed schemas
  could improve opaque selection/axis discoverability. Unverified claim: 2-3 of 5 render images did not match
  the current body while the caption did (may be the client attaching images). Structured JSON-string
  argument interop is fixed centrally.
