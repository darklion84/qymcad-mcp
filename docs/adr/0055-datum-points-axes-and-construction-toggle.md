# ADR 0055: Datum points, datum axes and the construction toggle

- Status: accepted (2026-11-12).
- Related: ADR 0031 (datum planes), 0042 (names), 0052-0054 (style: one tool = one app action, refuse only silent wrong results); F-168-F-177.
- Probes (`crates/engine/tests/i0_probes.rs`, native pin f25f27d) and their decisions are in the section "Probes" below.
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

- Both or neither of `at`/`vertex`; an edge that is not in the body's current edges; a body that is consumed or in another Part (the
  app itself takes a vertex only on the current Part's bodies, `viewport_3d.rs:569-577`).
- **A body under a vertex point cannot be suppressed (F-169).** Natively the point keeps its last position and nothing turns red.
  Deleting is already refused by the closure (`cascade` takes the point); suppression has no cascade, so it is refused outright.
- **`dimension_set` on `x`/`y`/`z` of a vertex point (F-174):** the app ignores the typed value.
- **An edit that removes the point's edge while the body stays (F-176):** judged after the rebuild of every edit, rolled back.
- **The assembly root as the active context:** a datum would be owned by the root assembly, which is not covered (the app's assembly
  bench has its own datum commands); `part_select` a Part first.
- **`part_copy` of a Part with a vertex point (F-175):** the app's copy does not remap the point's body and edge.

## 2. `datum_axis`: the app's Datum axis command

### App behaviour (source, pin f25f27d)

- Modes «By edge or face», «Two points», «By hand» (`feat_bar.rs:609-620`). `AxisDef::{Manual, TwoPoints, FromEdge, FromFace}`; the
  GUI offers only straight, non-smooth edges (`picks.rs:316-339`), the core takes any. Two vertex clicks make a NON-associative manual
  axis in world coordinates (`copies.rs:58-60`); a click on an existing axis makes a manual copy.
- Caches of non-manual axes are not saved; after the app's open path they are resolved anyway (probe P2).

### Tool contract

`datum_axis {edge:{edge,body?} | face:{face,body?} | points:[a,b] | origin+dir, name?}`, exactly one form. `edge`: straight, not a seam,
not smooth (`edges_full_smooth`), on a current body of the active Part. `face`: cylinder or cone (`face_axis`). `points`: two different
datum points of the active Part, associative. `origin`/`dir`: numbers only, within ±1e7, nonzero direction. Result: `{axis, name, def, owner_part?, origin, dir,
source?, rebuild}` after the rebuild; `dir` is always a unit vector (a manual axis keeps what was typed in the app; replies and
`doc_info` normalise it and turn negative zero into zero). The two-vertex form and a click on an existing axis are not offered (make datum points).
`revolve` and `circular_array` take `{"datum": id}` as before; the axis must belong to the active Part (a document with several Parts).
`fixed_axis` (their `origin` + `dir`) never reuses an axis made by `datum_axis` (marked `feat_dims[axis]["__explicit"]`), so deleting the
explicit axis cannot reach a feature that only asked for a line.

### Refusals

- **Formulas on a manual axis (F-170):** the app ignores them (the axis does not move). `origin`/`dir` take numbers and `dimension_set`
  on an axis coordinate is refused. A numeric edit of a manual axis is therefore not covered either: delete the axis and make a new
  one (or use `points` through two datum points whose coordinates are formulas).
- **A body or point of another Part:** an edge or face of another Part's body, or a point of another Part, is refused. This is the
  server's limit, not the app's: the app's axis pick offers any visible body (`pick_axis_at`), but the server stores every datum in the
  active Part's frame and does not convert between frames (a datum point's vertex, by contrast, is the app's own limit).
- **An edit that removes the edge or the round face an axis follows while the body stays (F-176).**
- **The assembly root as the active context**, as for points.
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
- **A profile of a several-profile feature that would vanish (F-168):** features hold contours by id and the app finds a contour again
  by its boundary entities, then by place and size (`replace_contours`, `model/sketch/contours.rs:12-17`). A held contour that only
  changes shape (a hole circle turned construction under a plate extrude) stays the same profile and the consumer follows: a legal
  action, not refused. A held contour that is gone is an error in the app for a feature with one profile ("profile was not found";
  the edit rolls back), but a feature with several profiles is built from the rest and the island silently drops out. That case is
  refused (some, not all, of a feature's held contours vanish). Contours no feature holds are not judged.

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
  were not probed.
- A numeric edit of a manual axis (F-170): only delete and create again.

## Known limits

- The server is stricter than the app about edges: it accepts `EdgeKind::Line` edges that are not seams and not smooth, the app
  filters by `is_straight_poly` and by what is visible; the server does not consider visibility (a hidden body's edge is accepted),
  and the app's id filter (`id != 0`) is not mirrored.
- The marker `feat_dims[axis]["__explicit"]` (an axis made by `datum_axis`, never reused by a feature that gives a line) can be lost when
  the axis is edited in the app's GUI; `fixed_axis` could then reuse it and deleting it would reach that feature. BACKLOG.
- Opening a file that already holds a faulty datum (a two-point axis that lost a point, a vertex whose edge is gone) is not refused:
  `open` reports a warning, and the datums that were faulty at opening do not block other edits. Edits that make a datum faulty are refused.

## Probes

Executable in `crates/engine/tests/i0_probes.rs` (native app data path through `common::native_*` where the question is about the app;
the server's `Session` where it is about the server). Observed = executed, inferred = read in source only.

| # | Question | Observation | Decision |
|---|---|---|---|
| 1 | Do formulas in a Manual axis move the axis? | Observed: no. `feat_dims[axis].ox = "px"`, `px` 30 -> 20, dirty rebuild: `origin()` stays (30,0,0), the array stays centred on x = 30. | `datum_axis` origin/dir are numbers only; `dimension_set` on an axis coordinate refused; F-170. |
| 2 | Axis from edge/face, two points, vertex points after save and open without regeneration | Observed (edge, two points, vertex points, manual-with-formula): after the app's open path the caches are already resolved and the next parameter edit gives the same box as an in-memory edit. The face axis is covered by the server's save/open test only. | Nothing to repair; no open/save note. Server save/open test for every definition, face axis included. |
| 3 | Delete the body under an `AtVertex` point | Observed: `errors: []`, the points keep their last position. | Deleting is refused by the closure (cascade takes the point); suppressing refused outright; F-169. |
| 4 | Two vertex clicks give a non-associative world-space axis | Not probed (source only). | Not offered; two datum points instead. |
| 5 | Click on an existing axis gives a manual copy | Not probed. | Not offered. |
| 6 | Delete an axis under revolve/circular_array | Observed on the server: refused ("has dependents"), the closure already knows `CircularArray.axis`. The app's silent degradation is source-read. | Test added; F-173, no code change. |
| 7 | Name in the pool or in the node | Observed: node name set, pool keeps `name-plane`; the tree reads the pool (source). Native (P7 native): the app's own `rename_plane` writes the pool and leaves the node. | Names are written to both for plane, point and axis (creation and `rename`); F-171. |
| 8 | Right-panel drag / `at` edit of an `AtVertex` point | Not probed. | Listed under "Not done". |
| 10 | Undo of the context-menu toggle without `begin_edit` | Not probed (GUI path). | The server follows the button path: one undo step through the tool boundary (tested). |
| 11 | A hole/island turned construction under a consumer (native, `p11_native_*`) | Observed natively: a hole circle under a plate extrude: volume 937.17 -> 1000, `errors: []` (the consumer follows its held outer contour). Two profiles (square + disc, 892.70): the disc turned construction gives volume 500, `errors: []` (the island drops out). One profile: "profile was not found". | Refuse only the several-profile island that vanishes; hole-to-construction is a legal action; F-168 (rewritten). |
| 12 | Text and the toggle | Observed natively: `toggle_construction` on a text id returns false and the text stays. | Text refused with an explanation. |
| 13 | An edge under a vertex point and an edge axis disappears, the body stays (native `p13`) | Observed: `errors: []`, the point and the axis keep their last place. | Refuse after the rebuild, rollback; F-176. |
| 14 | A coordinate typed on a vertex point (native `p14`) | Observed: stored, ignored by the rebuild, `errors: []`. | `dimension_set` refused; F-174. |
| 15 | Part copy of a Part with a vertex point (native `p15`) | Observed: the copy's point stands on the source body (body and edge not remapped). | `part_copy` refused; F-175. Edge/face/two-point axes are remapped (server test). |
| 16 | `sketch_remove` of a curve under an extrude (`p16`, server path) | Observed: a hole follows (937.17 -> 1000); one profile of two drops out with `errors: []` (892.70 -> 500); the only profile fails and rolls back. The reviewer's "volume unchanged" was not reproduced. | Not fixed in this package; F-177, BACKLOG. |
