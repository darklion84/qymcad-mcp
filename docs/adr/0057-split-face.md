# ADR 0057: Split faces

- Status: accepted (2026-11-14).
- Related: ADR 0056 (style: one tool = one app action, refuse only silent wrong results), 0024 (`split`), 0053 (clones), 0050 (`feature_reorder`), 0047 (rollback); F-100 (`split` piece identity), F-185; F-187-F-194.
- Probes (`crates/engine/tests/k0_probes.rs`, native pin f25f27d) and their decisions are in the section "Probes" below.
- Principle: the tool is the app's action with its limits. Where the native result is silently wrong, only that case is refused and recorded in FINDINGS; no workaround is written.

## 1. `split_face`: the app's Split faces command

### App behaviour (source, pin f25f27d)

- Command `part.split-face` (`Launch::Feat(PartTool::SplitFace)`, help `part/14-split-face`). The bar has one field, the offset (default 0, limits +-1e5, formulas allowed), and no option: no mode, no list of faces, no names of pieces. The target is the selected body or the active Part's body. The plane is clicked: a face under the cursor first (ANY face, curved ones too), else a base plane or a datum plane.
- One node `SplitFace { src, plane, datum, offset, body, face }` (`add_split_face`, plus `set_op_face` for a face; the id of the node is the id of the body it makes; the source is consumed). The offset formula stands in `feat_dims[node]["offset"]`. One undo step.
- Rebuild (`prep_splitface`): the plane is the base plane (XZ along +Y), the datum's origin and normal, or the CURRENT centre and normal of the face (`resolve_face`, so it follows the face), moved by `offset` along its normal; the kernel (`qym_shape_split_faces`) sections the body with the INFINITE plane and splits every face the section crosses, curved ones and hole walls included. The body stays one piece, the volume does not change. Untouched faces keep their ids; of a cut face the first piece keeps the id and the others get fresh ids.
- Before the node is made the app asks the kernel on a copy (`ask::splits_faces`) and refuses "plane splits nothing"; its own pre-check measures in the world frame while the build cuts in the body's local frame (F-194).

### Tool contract

`split_face {body?, plane, offset?, name?}`, every field strict (`deny_unknown_fields`). `plane` is `"XY" | "XZ" | "YZ"`, `{plane: <datum id or name>}` or `{body, face: <one PLANAR face selection>}` (the same argument as `split`); `offset` is a number or a formula along the plane's normal (XY +Z, XZ +Y, YZ +X), default 0; `name` labels the node. There is no choice of faces, no mode and no piece names: the app has none.

One action, atomic, batch-allowed, one undo step (the reply's `body` is the node's id, so the undo label and the batch `created` entry follow it without a special branch). Reply: `{feature, body, name, offset, faces: {before, after}, added_faces, rebuild}`; `feature` and `body` are the same id; `offset` is evaluated; `added_faces` are the ids the result has and the source did not, ascending. The pieces of one cut face share their normal and are told apart by centroid or position after `topology` or `select`.

The tool description says what the tool does, not only what it refuses: it cuts EVERY face the plane crosses (curved faces and hole walls), the body stays one piece, nothing is removed, it is not `split`; base planes and offsets are local to the body's Part; the face-plane follows its face; the ids of the pieces; and the accepted behaviour of the app (F-192, F-193).

### Refusals

At creation, and on every later rebuild of an existing node (`param_set`, `dimension_set`, an edit upstream, reorder): the same checks hang on the places where `split_identity_refusals` already runs, after a rebuild with rollback (`rebuild_passes`, and `with_rebuild_copies` for the edits that rebuild on copies). A node the app itself failed keeps the app's own text.

1. **A curved face as the plane** (creation; F-191). The server is stricter than the app, as `split`.
2. **No face gained** (F-187): after the build the result must have more faces than its source; a plane 1e-6..1e-5 mm from a face or through a torus's axis is green and splits nothing in the app, and near a face it damages the body.
3. **Repeated face ids** (F-189): fewer distinct ids than faces. The app does this for a split that crosses an earlier split of the same body and for parallel splits after the first two or so (2 rising along z and y, 1 falling; 4 falling along x, 1 rising, on a block): a body takes only a few cuts in a row. The text promises no order and names `split`.
4. **The plane's face is gone** (F-190): after a rebuild the face's id is no longer on the body; the app keeps cutting at the place of creation.
5. **Another set of cut faces** (F-188, edits only): the node's pieces as last ACCEPTED are kept in `Session::split_face_witness` (id, local bounding box, cut flag, the point its plane passed through): updated after each accepted rebuild, kept while the node is suppressed or past the rollback bar, in the undo snapshot, rolled back with a refused edit, rebuilt by `part_copy` (the copy's ids are its own) and recorded at `doc_open` (a node whose topology cannot be read gets none and refuses edits that rebuild it). Only the faces this node cut are compared: the set of cut ids must be the same, and each stays on its piece: positive bounding-box overlap along every axis in which either box extends, or the same after moving the old box by the movement of the plane's point (a plane that follows a face of a growing body); the axis of a flat face is compared by neither. Tolerance `max(1e-4, 1e-6 * scale)` mm (the mesh holds f32 coordinates). Conservative: a false refusal is allowed (a piece that moves by more than its own extent without its plane), a silent move of an id is not. A change of faces the node did not cut is the business of the nodes above it. The edit is rolled back; recreate the split.
6. **General paths:** no body, a mesh body, a body that a suppressed feature leaves unbuilt, a consumed body, a clone made by `part_instance` (`atomic`, ADR 0053), an assembly as the active context with no `body` named (a named body works, as for `split`), a non-finite offset, a formula that does not evaluate, a sheet body, a plane that splits nothing or misses the body (the kernel's own question, asked in the local frame of the body; for a face the text says the offset runs along the OUTWARD normal), a plane that the native rebuild rejects (the app's own failure with the server's hint: the plane no longer meets the body, undo or change the offset). A clone is refused as a clone before the plane is looked at.

### Accepted, described in the tool

- Suppressing the node gives its consumers (a `push_face` on a piece) the WHOLE face; unsuppressing returns the pieces (F-192); `feature_suppress` says so in its notes.
- A fillet on the edge of one piece rounds the whole chain of that edge (F-193).
- The frame is the body's local frame (F-194).

## 2. Other changes

- `Session::resolve_op_plane`: the one place that turns a plane reference into the stored code, datum or face witness and the plane in the body's frame; `split` uses it too (behaviour unchanged; the text for a curved face now names the app's averaged plane).
- `modelling_tool` and the batch allowlist know `split_face`; the batch description names it. `doc_info.timeline[].kind` shows `splitface`.
- At `doc_open` F-187, F-189 and F-190 are judged on every active Split faces node by its stored geometry (nodes the open did not rebuild too); a warning in the reply and in `doc_info` names the node and the way out (feature_suppress or feature_delete it), the file is not changed; the refusals on such a node and the repeated-id texts of `topology` and selections give the same way out. The `plane` argument names its forms when it is wrong.
- `k0_probes::p19_save_then_open`: the case with repeated ids is now refused on the full rebuild that reading a file's topology needs (the file stores no faces).

## Tests

`golden_split_face` (the block 6 -> 10 faces, 12 -> 20 edges, volume and bounds unchanged; a cylinder; a datum plane and a face plane that follows its face; a formula, `param_set`, `dimension_set`; each refusal with the document byte-identical, at creation and by edit; F-188 on the U body; F-190 on two boxes; the app's own error kept; suppress, rollback, delete, reorder, save and open, `part_copy`, a clone, the assembly root; F-192 and F-193), suppress then edit then unsuppress, rollback then edit then roll forward, the first edit after `doc_open` (with and without stored faces), a file already wrong, a cut that moves with its plane (a boss on a block), a change upstream, a fillet below the split, a third parallel cut, the hint for a plane off the body, the face-plane offset text, a clone of a placed Part, the text of a refused creation, the note of `feature_suppress`, a mesh body; `golden_split_print` (a curved face as the plane of `split`); `split_face::tests` (the piece overlap rule, a sheet body), `k0_probes` (native), `mcp/tests/k1_split_face` (strict arguments, reply, undo label, batch and its rollback, timeline kind, the description's phrases).

## Not done

- Re-picking the plane of an existing node (the app's double click); delete and recreate. `dimension_set` on `offset` works.
- The app's GUI was not run; every app-side claim is a native core-API probe or read in source, as marked.
- Colouring a piece: the server colours whole bodies only, so the use of the split is a later tool on a piece (`push_face`, `draft`, `shell`, `hole`, `thicken`, `remove_face`, a sketch or a datum on it).

## Known limits

- Crossing cuts of one body, and more than a few parallel ones, are impossible while the app repeats ids for them (F-189); use `split` for separate bodies.
- F-188 refuses a piece that moves by more than its own extent without its plane moving with it; a plane that follows a face (or a datum that moves with the cut) is allowed. The flat axis of a face is not compared, so the move of a face to a parallel plane with the same extent is not seen.
- A node suppressed in a file when it is opened, or whose copy was suppressed, has no pieces to compare: the first edit that rebuilds it is refused until it is deleted.
- Which parallel pairs are clean was measured on a block; the rule is stated as observed.
- The refusals read the faces of the node after the rebuild: a node already red from the app is left to its own error.

## Probes

Executable in `crates/engine/tests/k0_probes.rs` (the app's data path: `Project`, `add_split_face` and its regeneration through the testkit, no `Session`); the server's behaviour is in `golden_split_face`. Observed = executed; inferred = read in source only. The block is 40 x 30 x 10 (x -20..20, y -15..15, z 0..10); the U body is a 40 x 30 x 20 block less a slot 20 wide and 10 deep from the top; the L body is the block less the upper half of its x > 0 side.

| # | Question | Observation | Decision |
|---|---|---|---|
| 1 | Do the pieces swap ids when the offset or the body changes within one topology | Observed: block 5 -> 3 -> 7, height 10 -> 14, U and L bodies: the old id stays on the same piece every time and agrees with a rebuild from scratch; which piece keeps it follows the axis (XY z and XZ y the lower, YZ x the higher). | No swap guard of its own; F-188 catches the other case. |
| 2 | An edit that changes how many faces are cut | Observed: U body 5 -> 15: 14 -> 18 faces, 14 ids stay, 4 are new, id 837 moves from a 400 mm2 wall piece to a 50 mm2 tower, a push on it moves 50 instead of 400 mm3; L body: an id vanishes and the consumer takes the whole wall. No error. | Refused on the edit; F-188. |
| 3 | A plane at or near a face, an edge, a tangent | Observed: at 0 and up to 5e-7 mm a visible failure; 1e-6..1e-5 mm under the top (and 1e-6 over the bottom) green with no split, volume down 8e-4..8e-3; 1e-4 splits. A cylinder's tangent plane fails at 0 and splits at -1e-6; a sphere's fails at 0 and splits already at -1e-9. | Refuse a node that gained no face; F-187. |
| 4 | A plane on an inner face (the step of the L body) | Observed: 8 -> 11 faces, unique ids, volume intact. | Allowed. |
| 5 | Curved faces cut | Observed: a cylinder across 3 -> 4, through the axis 3 -> 6 (x = 0: 3 -> 7), a chord 3 -> 7; a sphere's equator 1 -> 2; a cone 3 -> 4 and 3 -> 6; a torus z = 0 1 -> 2, x = 12 1 -> 5; through its axis (y = 0) 1 -> 1 green; a sphere's meridian a visible failure. Ids unique, volumes agree to 1e-6. | Allowed; the torus case is F-187. |
| 6 | A curved face as the plane | Observed: a reasonable-looking plane through the axis or an inclined one (cylinder x = 0, cone inclined, sphere x = 0, torus no-op), no error. | Refused; F-191. |
| 7 | Offsets 50, -50, 1e5, 1e6, 1e12, NaN, inf, a formula with an unknown name | Observed: a visible failure `OpFailed(SplitFaces)` and a copy of the source; an unknown name gives `UnknownName` while the body is built on the stored number. | Nothing to add beyond a finite offset. |
| 8 | The plane's face disappears after an edit | Observed: green, the plane stays at the place of creation; while the id lives the plane follows the face. | Refused; F-190. |
| 10 | A placed Part (z +100; a quarter turn and a shift) | Observed: the base plane at offset 5 cuts in the body's LOCAL frame (10 faces); the app's pre-check in the world frame (emulated) disagrees. | Local frame, described; F-194. |
| 11 | Suppress the node under a consumer of a piece | Observed: the consumer stays green and takes the whole wall; unsuppress returns the pieces. | Accepted, described; F-192. |
| 13 | Open a file whose node is red | Observed: `report.errors` shows `Split faces: operation failed: SplitFaces`, the body is a copy of the source. | Visible; nothing to add. |
| 14 | Two splits in a row (14, 14b, 14c) | Observed: 3 then 6: 14 faces, ids unique; 6 then 3 (and every second plane below the first along z): 14 faces, 10 distinct ids; every crossing pair (XY then XZ or YZ, XZ then XY, YZ then XZ): 16 faces, 14 distinct; the repeats survive save and open; `Rebuild.warnings` is empty. | Refused; F-189. |
| 14d | Three and more parallel splits | Observed (`p14d`): clean splits before the first repeat: along z and y rising 2, falling 1; along x falling 4, rising 1. | The limit is stated, no order is promised; F-189. |
| 15 | A fillet on a seam edge, on one piece of an edge | Observed: the seam gives `AllEdgesSmooth`; one piece of a corner edge rounds the whole edge. | Accepted, described; F-193. |
| 19 | Save and open | Observed: the same faces and ids, no warnings, except the repeated ids of probe 14 (shown only by `topology.warnings`). | Nothing to add; the repeats are refused (F-189). |
| 9, 12, 16-18, 20 | A face of another body, reorder and rollback, draft/shell/hole by a piece, `select` by feature, mesh and sheet bodies, export | Not probed natively; reorder, rollback, `part_copy`, clones and the root are `golden_split_face` tests; a mesh body is refused by the existing `source_body` path. | Not claimed. |
