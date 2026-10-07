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
- **Curved-junction fillets are hard to select** (live final test): `{"concave": true}` covers straight edges
  between planar faces only, so a boss-to-plate fillet needs `{"between": [{"of_feature": X, "role": "wall"},
  {"of_feature": Y, "role": "cap_end"}]}`. Hint at it when a concave/convex selection is empty, or add the
  example to the fillet description; or extend concave/convex to curved junctions.
- **Stale-geometry warning on open compares volume only** (live final test): a stored sealed void vs a rebuilt open
  pocket differs by 0.48 mm³ and reads as noise. Also compare shell/solid/face counts and run the void check on the
  stored body.
- **`export` does not repeat document warnings** (e.g. a sealed void) in its result.
- **`undo` echoes the undone call's arguments**; also show the restored values (e.g. parameter t back to 5).
- **Topology and sketch contour areas are tessellated** (~0.1–0.2 % off analytic); say so in the descriptions.

