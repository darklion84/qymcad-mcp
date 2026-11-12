# ADR 0055: Datum points, datum axes and the construction toggle

- Status: accepted (2026-11-12).
- Related: ADR 0031 (datum planes), 0042 (names), 0052-0054 (style: one tool = one app action, refuse only silent wrong results); F-168-F-173.
- Plan: `tasks/SPEC.md` (binding decisions). Research: `tasks/RECON.md`; probes and their decisions: `tasks/PROBES.md`
  (`crates/engine/tests/i0_probes.rs`, native pin f25f27d).
- Principle: each tool is one existing QymCAD app action with the GUI's limits. Where the native result is silently wrong,
  only that case is refused and recorded in FINDINGS; no workaround is written.

## 1. `datum_point`: the app's Datum point command

### App behaviour (source, pin f25f27d)

- `part.datum-point` (`bars/commands.rs:49-58`): X, Y, Z fields within ±1e7 with formulas (`store_cmd_exprs`, `apply/copies.rs:38-43`), or
  «To a vertex» (`add_point_at_vertex(at, body, edge, end)`), vertices of the current Part's bodies only (`viewport_3d.rs:569-577`).
  One timeline node plus a pool record share the id (`model/features.rs:42-53`). `PointDef::AtVertex` is resolved from `kernel.edges`
  on every rebuild. The app uses a point only as an end of `AxisDef::TwoPoints` and as a snap target; no sketch, plane, joint or
  measure takes one.

### Tool contract

`datum_point {at:[x,y,z]} | {vertex:{edge, end:"start"|"end", body?}}, name?`. Exactly one of the two. Coordinates are numbers or
formulas stored on `feat_dims` keys `x, y, z` (`dimension_set` edits them; evaluated values are held to ±1e7 on every edit). A vertex
point stores (body, persistent edge id, end), takes `body` from the active Part's current bodies and refuses another Part's. Result:
`{point, name, def: manual|vertex, at, vertex?, rebuild}`; `at` is read after the rebuild. Batch-allowed, one undo step.

### Refusals

- Both or neither of `at`/`vertex`; an edge that is not in the body's current edges; a body that is consumed or in another Part.
- **A body under a vertex point cannot be suppressed (F-169).** Natively the point keeps its last position and nothing turns red.
  Deleting is already refused by the closure (`cascade` takes the point).

## 2. `datum_axis`: the app's Datum axis command

### App behaviour (source, pin f25f27d)

- Modes «By edge or face», «Two points», «By hand» (`feat_bar.rs:609-620`). `AxisDef::{Manual, TwoPoints, FromEdge, FromFace}`; the
  GUI offers only straight, non-smooth edges (`picks.rs:316-339`), the core takes any. Two vertex clicks make a NON-associative manual
  axis in world coordinates (`copies.rs:58-60`); a click on an existing axis makes a manual copy.
- Caches of non-manual axes are not saved; after the app's open path they are resolved anyway (probe P2).

### Tool contract

`datum_axis {edge:{edge,body?} | face:{face,body?} | points:[a,b] | origin+dir, name?}`, exactly one form. `edge`: straight, not a seam,
not smooth (`edges_full_smooth`), on a current body of the active Part. `face`: cylinder or cone (`face_axis`). `points`: two different
datum points, associative. `origin`/`dir`: numbers only, within ±1e7, nonzero direction. Result: `{axis, name, def, origin, dir,
source?, rebuild}` after the rebuild. The two-vertex form and a click on an existing axis are not offered (make datum points).
`revolve` and `circular_array` take `{"datum": id}` as before; the axis must belong to the active Part (a document with several Parts).
`fixed_axis` (their `origin` + `dir`) never reuses an axis made by `datum_axis` (marked `feat_dims[axis]["__explicit"]`), so deleting the
explicit axis cannot reach a feature that only asked for a line.

### Refusals

- **Formulas on a manual axis (F-170):** the app ignores them (the axis does not move). `origin`/`dir` take numbers and `dimension_set`
  on an axis coordinate is refused.
- **Two-point axis with coincident or missing points (F-172):** judged on every edit; the app keeps the old line without an error.
- Deleting an axis a feature uses, or a point under an axis, is refused without `cascade` (F-173: the app turns the feature about another
  axis; the closure already covered the array, the probe confirmed it).

## 3. `sketch_edit op:"construction"`: the app's toggle

### App behaviour (source, pin f25f27d)

- `toggle_construction(si, eids)` (`model/sketch/entities.rs:904-919`): the whole set takes ONE state, construction while any member is
  ordinary, ordinary once all are construction. The button/X key wrap it in one undo step; texts and isolated points are not switched
  (probe P12). Constraints and dimensions stay.

### Tool contract

`sketch_edit {op:"construction", sketch, entities:[ids], to:bool}`. `to` is required: when the app's rule would give the other state
(all already in `to`, or a mixed set going to ordinary) the call is refused unchanged, so a repeated call cannot invert the state.
Result: the standard edit report plus `construction: [{id, construction}]`. Runs through `Session::sketch_edit` (rollback if a
feature that built before now fails).

### Refusals

- Empty set, ids that are not curves of the sketch, texts, isolated points, instances of a sketch pattern.
- **A contour that consumers hold by id changes (F-168):** contour ids are reused by position; a surviving contour with another area or
  other holes is a different profile for its consumers and the app rebuilds them with no error (probe P11). Consumers that lose their
  contour entirely fail in the rebuild and the edit rolls back.

## 4. Names

`set_node_name` and `rename` write a datum node's name to its pool record too (F-171); the app's tree shows the pool name. `plane_offset`
names follow for the same reason.

## 5. Other changes

- `doc_info.datums`: `points {id, name, def, at, vertex?}`, `axes {id, name, def, origin, dir, source?}`, `errors` (omitted when the document
  has no datums).
- Batch allows both tools; the undo label is the created node's name.

## Tests

`golden_sketch_construction`, `golden_datum_point`, `golden_datum_axis` (hand-computed boxes and volumes: arrays about manual, edge,
face and two-point axes, edge and point following a parameter, revolve ring about a datum axis, save/reopen of every definition, refusals
with unchanged documents, deletion order), `golden_datum_lifecycle` (rollback, reorder/suppress refusals, Part copy and delete), `i0_probes`
(native app data path, pins F-168-F-173), and `mcp/tests/i1_construction`, `i2_datum_point`, `i3_datum_axis` (strict arguments, batch,
batch rollback, undo, Part ownership).

## Not done

- Datums at assembly level (the commands exist in the assembly bench), the snap targets for primitives, and the manual-copy of an
  existing axis; the app's right-panel drag that silently turns an edge axis manual and the rewind of an `AtVertex` point's `at` edit
  (RECON 5.8) were not probed.
