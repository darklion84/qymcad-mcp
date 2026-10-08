# Backlog

Known follow-ups that were consciously deferred. Each item says where it came from.

- **GLB export test checks accessor metadata only** (Codex review, phase 3C): decode the binary positions and
  indices and use an asymmetric fixture to prove +Y vs −Y and winding. The writer is QymCAD's (`qymcad_io::export_glb`).
- **OBJ export with several bodies** (Codex and MiMo reviews, phase 3C): the single-body read-back (bbox, volume)
  is in `obj_is_the_same_solid_in_millimetres`; the per-body `o body_k` offsets across bodies are not tested.
- **Re-solve only the sketches a parameter edit reaches.** `Session::propagate_params` (crates/engine/src/params.rs)
  re-solves every sketch that has any expression on every parameter edit. Solving only the sketches whose
  expressions mention the changed name (directly or through other parameters) is a performance optimisation for
  large documents; it must keep F-001 (case-insensitive matching) and F-002 in mind. Raised in the phase 3A review.
- **One helper for "snapshot live shapes, rebuild on copies, restore on failure".** `Session::sketch_edit` and
  `Session::param_set` carry the same B-rep-copy snapshot logic (F-034); extract it so both change together.
  Raised by Claude in the round-3 verification.
- **`Patch` edge queries are not gated by the unrestorable-edge guard** (MiMo, final review): the guard in
  `Session::rebuild_retrying` covers fillet/chamfer stored queries; extend it if the engine ever creates Patch
  features with edge queries.
- **The rebuild guard clones the `Project` on every public rebuild** (MiMo, final review): fine for small parts;
  make the retry-plan check incremental if large documents get slow.
- **Document (or avoid) the 0.001 mm cut entry clearance on internal datum planes** (live test 2026-10-08): a cut
  sketched on a datum plane inside the stock starts 0.001 mm behind the plane (a void of 4.001 instead of 4, floor at
  z = 2.999). QymCAD offsets the cut tool to avoid coplanar faces (see F-017); harmless on a top face, a real
  geometry change inside. At least state it in FINDINGS and in the extrude description.
- **Empty-selection hint wording** (live test): say "0 concave (inward) edges; this body has N convex edges", drop the
  `between` example on a single-feature body, and repeat the hint in fillet/chamfer "matched no edge" errors.
- **`doc_save` without a path after switching models** (live test): after undo/delete into a different model, a bare
  `doc_save` overwrites the last file. Warn or refuse when the file holds a different model than the session's
  (e.g. a different first feature), unless a path is given.
- **Minor output consistency** (live test): stable edge order between bodies; `volume_mm3` (rounded) in rebuild
  results vs `volume` (full precision) in doc_info.

