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
  - **Investigate first (possible upstream bug):** a seat revolved (cut, 360°) from a profile drawn on the
    -x side of its own axis gave cone faces with axis [0,0,-1], and a later 0.5 chamfer on the mouth circles failed
    with "chamfer 0.50 too big"; the same profile on the +x side chamfered fine. Reproduce, find whether QymCAD's
    revolve or the chamfer is at fault, and either normalise the profile side or explain it in the error.
  - **doc_info bbox right after doc_open is inflated by 1.24 mm** on the shelf (x ±135.236 for a 268 mm part, which
    reads as larger than a 270 mm bed); after a rebuild it is the usual ~0.12 mm padding. Use a tight bbox.
  - The pocket cut from the internal plane z=10 left the floor at 9.999 (F-061 entry clearance), so the volume
    is 0.0088% below build123d (-42.0 mm3 = 0.001 x pocket area). The agent did not use the recommended
    "sketch on the top, cut reverse" pattern; consider stating in the extrude/plane_offset descriptions which
    pattern gives an exact floor.
  - **Bug (ours or native classification):** the shelf's flat pocket floor (all 12 edges at z=9.999) is reported by
    topology/select as kind "sphere", radius 533.76, centre z=-505.22, before and after rebuild; a sphere through
    the floor plane is impossible. `{"facing":"+z"}` still returns it although facing is documented as planar faces.
    Repro: tmp/mcp-test/phase4/hanging_shelf.qcad (face 1073741842). Confirmed by mcp-tester5.
  - The 1.236 mm bbox inflation after doc_open reproduces after a save/reopen round trip and also shows in the
    render caption; STL export of the same body is exact (±134 × ±85). The collet coupon has none, so the
    suspect is the shelf's B-spline blend corners (Bnd_Box from poles).
  - Mouth-circle chamfer via of_feature also returns the seam line; the agent fell back to explicit ids.
