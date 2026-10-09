# Backlog

Known follow-ups that were consciously deferred. Each item says where it came from.

Open (low, from the round-J live test by Codex, 236 calls, all items OK, 2026-10-09):

- The `between` suggestion in empty-corner hints was never offered in the tester's joined-extrusion and
  plate+boss cases, although a manual `between` works; check the ancestry rule (≥2 extrude/revolve) on add-joined
  extrusions, or drop the suggestion.
- `topology` rounds radii and lengths to 4 decimals (radius 1.2346 for q=1.23456789…); say so in its description,
  or report input-ready values at full precision like sketch_info.
- `plane_offset.base` schema admits a face reference, but the call refuses ("offset from a face is not supported
  yet"); narrow the schema or the description. Wish: a `{"seam": true}` selection filter.

Items closed without a code change in backlog round J (2026-10-09), with the reason:

- **0.001 mm entry clearance of one-sided cuts on internal planes** — native QymCAD behaviour; compensating
  geometry would change native recipes, and the pinned `Extent` has no flag to disable it. The exact-floor
  recipe (sketch at the top, cut in reverse) is documented in the extrude/plane_offset descriptions (F-061).
- **Project clone in the rebuild guard** — measured on the shelf: ~0.15 ms per clone (~0.07% of a parameter
  edit); ~0.9 ms for an 8× scaled payload. Negligible; the safety snapshot stays.
- **`Patch` edge queries and the unrestorable-edge guard** — the engine never creates Patch features. The guard
  carries a comment saying what to add if it ever does.
- **Duplicate EDGE ids after seam repair** — not reproduced (the F-068 fixture and rebuilds at 0.25/0.5/0.75 mm
  have unique edge ids). A reproducing fixture would justify mirroring the face policy.
- **MiMo client observations (stale render images, truncated doc_save arguments)** — not reproducible through
  the protocol: repeated renders after parameter edits return identical image bytes and matching captions
  (`repeated_renders_after_parameter_edits_match_image_bytes_and_captions`). Client-side.
- **Advertised schemas do not admit JSON strings** — schemas stay typed; the server instructions and
  ARCHITECTURE say objects/arrays may also be sent as JSON strings.
- **Torus face kind / which face takes an asymmetric chamfer's `dist`** — the pinned kernel exposes neither
  per face (F-060, F-036); documented in the topology and chamfer descriptions.

Parameter solve optimization was corrected after J6 review: native blanket rebuild dirtying stays, with
face-derived datum sketch scheduling barriers (F-017). Four→zero explicit shelf solves remains, but the
corrected median edits are ~249–256 ms versus ~253 ms with pre-J6 propagation (no reliable total speedup), rather
than J6 HEAD's ~222 ms. Reproduce with the ignored test `parameter_and_project_clone_shelf_timings`. Named feature dimensions
referring to other named dimensions retain the native scope limitation documented in F-071. Non-kind
composed face queries remain dynamic, with the documented shell missing-opening residual (ADR 0013).
