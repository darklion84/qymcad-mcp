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
  - unnamed objects show raw localization keys as names: datum planes as "name-plane (plane 24)", bodies as
    "feat-name-extrude"/"feat-name-hole"…, while unnamed sketches read "Sketch 3";
  - number agreement: "omitted 1 uncertain edges", "1 edge … treat them";
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
  - a planar face whose mesh vertices are all cocircular could in theory pass the sphere-fit validation
    (zero residual). Not reproduced: circular pocket floors Ø6-Ø250, with and without a floor fillet, classify
    as plane on both the old and the new binary (checked 2026-10-08; the upstream singular-system cutoff
    rejects them). Cheap hardening: reject a fit when all face vertices are coplanar.
  - `kernel_gate` is not re-entrant and `reporting_bbox` always takes it; any future caller that holds the gate
    across `result_bodies`/`info`/`render` deadlocks. Add a precondition comment or a debug re-entrancy check.
  - the revolve-chamfer advice fires on any chamfer rebuild failure with revolve ancestry (also a genuinely
    oversized chamfer); the ancestry walk follows only boolean operand `a`.
  - UPGRADING.md: the pinning test `upstream_sphere_fit_accepts_a_single_coplanar_floor_mesh` must be removed
    when QymCAD fixes the fit.
- **Low items from the H1-H4 live test** (mcp-tester6, 2026-10-08):
  - revolve-chamfer error: "chamfer 0.50 too big" is the native reason but false here; "negative side of its axis"
    is undefined (it depends on the axis line's direction, not on sketch x); "(F-067)" means nothing to a user;
    the failure needs more than the orientation (all clean-block variants passed, an overlapping-cone case
    failed and reversing the axis line fixed it). Say concretely: draw the axis line toward sketch +y and the
    profile at larger x.
  - bbox formats differ inside one doc_open result: rebuild/feature results round to 0.001 (which can exceed the
    true extent, e.g. 36.173 > 36.1725), doc.bodies gives raw f32 noise (-3.5999999046325684, -2.78e-15); the
    render caption rounds to 0.1/0.01. Document or unify.
  - topology/sketch_info areas: "typically 0.1-0.2% low" understates small faces (-0.37% Ø8 floor, -0.38% full
    sphere, -0.72..-0.79% small spherical patches).
  - a cut that removes nothing reports the raw key "error-cut-removed-nothing" with no hint (e.g. "cut in reverse").
  - wish: kind "torus" (fillet corner tori are "other", like NURBS); asymmetric chamfer result does not say
    which face took `dist`.
- **Bug (found by Codex as a live tester, confirmed 2026-10-08):** a cone cut by a full-turn revolve whose profile
  lies on the negative side of an upward axis line, then chamfered 0.5 on both rims, succeeds but its chamfer faces
  are split in two and each half-pair shares one face id (11 faces, ids 1073741836 and 1073741837 twice); the top
  pair is reported as kind "sphere" r=3.0606 (two coaxial circles always lie on a common sphere, so the H2 vertex
  check passes). `select(faces=<id>)` returns two rows for one id. Reversing the axis line gives 9 unique faces,
  all cones. Repro: tmp/mcp-test/h1-codex/cone_negative_chamfer.qcad vs cone_negative_axis_reversed.qcad.
  Fix ideas: detect duplicate ids and warn; classify a face as sphere only when its vertices do not all lie on two
  or fewer circles about one axis (or ask the kernel's native surface type); duplicate names are an upstream
  candidate (QymCAD blend-face naming).
