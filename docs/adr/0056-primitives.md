# ADR 0056: Primitives

- Status: accepted (2026-11-13).
- Related: ADR 0055 (style: one tool = one app action, refuse only silent wrong results), 0052-0054, 0026 (`body_boolean`), 0050 (`feature_reorder`), 0047 (rollback); F-180-F-185.
- Probes (`crates/engine/tests/j0_probes.rs`, native pin f25f27d) and their decisions are in the section "Probes" below.
- Principle: the tool is the app's action with the app's field limits. Where the native result is silently wrong, only that case is refused and recorded in FINDINGS; no workaround is written.

## 1. `primitive`: the app's Box, Cylinder, Sphere, Cone, Torus and Prism commands

### App behaviour (source, pin f25f27d)

- Six commands `part.box|cylinder|sphere|cone|torus|prism` (`bars/commands.rs:111-170`) with one help page (`part/19-primitives.md`). The bar holds only the size fields; there is no operation choice. Fields (`start.rs:380-417`, numbers or formulas): box `dx dy dz` 0.1..1e5; cylinder `r` 0.05..1e5, `h` 0.1..1e5; sphere `r` 0.05..1e5; cone `r1` 0.01..1e5, `r2` 0..1e5, `h` 0.1..1e5; torus `major` 0.1..1e5, `minor` 0.1..`(major-0.001).max(0.1)` (`sync_torus_params`); prism `r` 0.1..1e5, `h` 0.1..1e5, `n` whole 3..64. The edit field (`reopen.rs:405-437`) differs only in the cone's `r1` (lower bound 0).
- Standing without a click: box, cylinder, cone and prism on z = 0 centred on XY; sphere and torus centred on the origin.
- A click on a vertex or a datum point places the primitive by a pure translation (`aims.rs:167-175`); a click on a plane or a face gives an oriented frame, which this server does not offer. Nothing is stored about the point: the 12 numbers of a `Move` node are all.
- `apply_prim_cmd` (`apply/solids.rs:599-621`) is one action: the primitive node (id = body id), the formulas through `store_cmd_exprs` (a formula stands on `feat_dims[node][key]`, a plain number clears the key), a `Move` when the frame is not the identity, and `finish_base_body(placed, 1)`: a union with the LAST unconsumed non-sheet body of the same Part when there is one. A Part is one body in the app; there is no cut, intersect or new-body option.
- Rebuild (`prep_box3..prep_prism`): the core checks only `size > 1e-9`; a torus `minor >= major` is `TorusThroughItself`; `n = round(dim).max(3)`. The properties panel writes any expression into the node with no limits (`panels_props.rs:759-770`), which is also the path of `dimension_set`.

### Tool contract

`primitive {kind: box | cylinder | sphere | cone | torus | prism, <sizes>, place?, name?}`. A tagged argument (`kind`), every variant `deny_unknown_fields`: a cylinder with `dx` is a bad-arguments error. Sizes are required (the app's defaults are on-screen only), numbers or formulas; the keys are the field names (`dx dy dz`, `r h`, `r`, `r1 r2 h`, `major minor`, `r h n`), the same keys `dimension_set` takes with `feature` = the primitive's node. `place` is `{point: <datum point id or name>}` or `{vertex: {edge, end, body?}}`, a pure translation into the point's or the vertex's coordinates of the active Part, no association (as the click); omitted, the origin. A plane or face placement is not offered. `name` labels the primitive's node.

The action is atomic and one undo step (batch-allowed): node, formulas, optional `Move`, union. Result: `{feature, body, kind, name, sizes, placed_at?, joined?: {prior, union}, rebuild}`. `feature` is the primitive's node (target of `dimension_set`); `body` is the Part's body after the action, one of three: the primitive's own id when it stands unplaced in an empty Part; its placement `Move` node when it is placed into an empty Part (`feature != body`, no `joined`); the union node when the Part already had a body (`joined`). The undo label and the batch `created` entry follow `feature` (the union node would carry a default name).

Two primitives united, apart or touching at a point, are one body of two pieces with both volumes (probe 9), as in the app.

### Refusals

- **The app's field limits at creation and on every edit (F-180, F-181, F-182).** Creation judges the evaluated sizes before anything is added; `param_set` judges them right after the parameters evaluate, before propagation and the rebuild (a million-sided prism is refused at once, not built; the polygon side count has the same early place), `dimension_set` before its rebuild; the check after the rebuild stays, with the other `check_evaluated_features` guards, and rolls back. The field's own tolerance holds: `lo - 1e-9 ..= hi + 1e-9`, `n` within 1e-9 of a whole number and then rounded (`tool_frame/mod.rs`, read), also when a file is opened (no false warning on float noise). A size below its minimum is refused with F-180 (the core builds it silently wrong); one above 1e5 or past the torus gap is refused as "the app's field limit" (the core would build it): a policy, not a protection. The reason is the core's silent result below the fields (a thin extrusion with a third of its volume, `errors: []`) and the silent rounding of `n`; the upper bound 1e5 is the field's policy (the core builds exact bodies far above it) and the torus's `minor <= max(major - 0.001, 0.1)` is the field's too (the core builds exact bodies up to the touching of the tube). The cone's `r1` may be 0 on edit, as in the app's edit field. Equal cone radii get a clear refusal instead of the core's bare `OpFailed(Cone)`.
- **A primitive over a body that is not built (F-185):** the Part's last body (the one the app would unite with) is suppressed or cascaded out (the F-183 chain); the app gives an empty result without an error, so the primitive is refused until it is unsuppressed.
- **`place` beyond the app's reach:** a datum point after the rollback bar is not in the active history and is refused; a vertex only on an edge the app's click picks (`pick_vertex_pos` skips closed edges, whose single vertex is a seam, and smooth junctions such as a cylinder's seam line), others refused with that reason. A datum point hidden in the app cannot be clicked there either; it is accepted here (not refused: the placement is the same translation).
- **A file made in the app that is already outside the fields** (its properties panel has no limits) opens with a warning in the reply and in `doc_info`. The exemption is per size: `Session::open_primitive_faults` holds `(node, size) -> the value it had`; that size passes while unchanged, any other size of the node is judged as usual, and moving the tolerated size is allowed only into the field (the exemption then goes). It is part of the undo snapshot (undo and a rolled-back batch restore it) and `part_copy` carries it to the copied nodes. Datum nodes keep the whole-node set `open_node_faults` (`open_datum_faults` renamed).
- **An assembly (root or nested) as the active context:** the app would create or reuse a Part for the primitive (`body_parent`); not covered here, `part_select` a Part first.
- **A datum point or vertex of another Part, a consumed or foreign body, an edge that is not in the body:** as for `datum_point` (the coordinates are in the active Part's frame).
- A formula that does not evaluate, and a parameter under a primitive formula, are stopped by the existing paths (`param_delete` refuses a used parameter; probe 8).

## 2. Other changes

- `modelling_tool` and the batch allowlist know `primitive`; the batch description names it.
- `dimension_set` on primitives needs nothing new (`kept_number` knows the keys). `doc_info.timeline[].kind` shows `box3`, `cylinder`, `sphere`, `cone`, `torus`, `prism`, `move`, `bodyboolean`.
- `Session::open_datum_faults` is now `open_node_faults` (datum and primitive nodes that were faulty when the file opened).

## Tests

`golden_primitives` (hand volumes, boxes and face counts of all six including the apex cone, formulas and `param_set`, `dimension_set` of sizes and `n`, creation and edit refusals with unchanged documents, the cone's `r1` = 0 on edit, the torus fat tube, union with an overlapping and an apart body, commutativity of the union, placement at a vertex and a datum point, no association, other Part and root refusals, delete/suppress/reorder/rollback, a parameter under a formula, `part_copy`/`part_instance`, save and open, several bodies in a Part), `primitive::tests` (a file outside the fields: per-size exemption, undo and a rolled-back batch, `part_copy`, tolerances, the early check of a million-sided prism), the primitive over an unbuilt prior, the first placed primitive, placement refusals (rollback bar, closed or smooth edge), `j0_probes` (native), and `mcp/tests/j1_primitive` (strict arguments, undo label, batch, rollback of a batch, string leniency, timeline kinds, the root, the first placed primitive with undo, save and open).

## Not done

- Placing a primitive on a plane or a face (the app's frame: `a = (1,0,0)` or `(0,1,0)` when `|n.x| >= 0.9`, `u = norm(a x n)`, `v = n x u`; on the XY plane this turns the box 90 degrees against the unplaced one, source-read, not run). Use `move` or `part_place`.
- Operations other than the app's union (cut, intersect, new body): the app has none for primitives.
- A mesh piece or an imported body as the Part's last body (`prior` of the union): not tried.
- The app's GUI itself was not run; every app-side claim is a native core-API probe or read in source, as marked.

## Known limits

- `place` is not associative: the datum point or vertex may move later and the primitive stays.
- With several bodies in a Part the primitive is united with the last one only (F-184).
- Suppressing a united primitive hides the Part's body (F-183); a primitive made over such a body is refused (F-185).
- `primitive_faults` walks the whole timeline, suppressed and rolled-back primitives included: one of them outside the fields (e.g. its formula follows a parameter) blocks `param_set` until it is fixed, conservatively; it is not skipped.
- A tolerated size of a file is compared by its evaluated value: a formula that moves it to another faulty value is a new fault.

## Probes

Executable in `crates/engine/tests/j0_probes.rs` (the app's data path: `Project` and its regeneration through the testkit, no `Session`) and `golden_primitives` (the server path). Observed = executed; inferred = read in source only.

| # | Question | Observation | Decision |
|---|---|---|---|
| 1 | A size between 1e-9 and the field minimum | Observed: exact volumes down to 1e-5 mm (box, cylinder, prism, torus, sphere); at 1e-6 a thin box, cylinder or prism has 1/3 of its volume and 2 faces, `errors: []`; prism `r` 1e-5 has 0.72; smaller: visible `OpFailed(ExtrudeContour)` or `EmptyResult`. | The fields' minimums at creation and on every edit; F-180. |
| 2 | A size above 1e5 | Observed: exact volumes up to 1e12 (box, cylinder, sphere, torus, prism), `errors: []`. | Not a wrong result. The field's 1e5 is kept as policy (stated in the refusal text and the tool description). |
| 3 | `n` outside 3..64 or not whole | Observed: 2, 2.5, 3.4, 0, -4 build a triangle; 3.5 a square; 5.5 a hexagon; 65 and 1000 build n-gons; `errors: []`. | Whole and 3..=64 on every edit; F-181. |
| 4 | Zero and negative sizes | Observed on all six kinds: `PrimitiveSizeNotAboveZero`, no body. | Nothing beyond the fields: every edit path gives the field's text, before the rebuild. |
| 5 | Cone `r1 == r2` | Observed: `OpFailed(Cone)` (also 1e-7 apart; `p5b`: fails at d = 1e-7, builds at 1.01e-7); 1e-5 apart is exact; both zero `PrimitiveSizeNotAboveZero`. | A clear refusal for a difference of 1e-7 or less (the core's own threshold); F-182. |
| 6 | Cone `r1 = 0` or 1e-6, `r2 > 0` | Observed: apex cone, 2 faces, exact volume. | Creation `r1 >= 0.01`, edit `r1 >= 0` as the app's two fields. |
| 7 | Torus `minor` near `major` | Observed: 12/11.9, 11.999, 11.9999999 exact (2 pi^2 R r^2), `errors: []`; `minor >= major`: `TorusThroughItself`. | The field's `major - 0.001` kept (the core is wider and correct). |
| 8 | A formula that no longer evaluates | Observed: a VISIBLE `UnknownName` / `UnexpectedEnd` at rebuild, not a fallback to the stored number (the reconnaissance expected a silent fallback). The server's `param_delete` refuses a used parameter. | Nothing to add. |
| 9 | Union with a disjoint body | Observed: one body, 2 solids and 2 shells, volumes add, `errors: []`, no warning; touching at a point the same; overlapping gives 1 solid. | Allowed and described, as in the app. |
| 10 | Several bodies in a Part | Observed on the server path: the primitive is united with the last body only. A mesh piece was not tried. | Allowed and described; F-184. |
| 11 | Active assembly root | Observed on the server path: refused before the core runs. | Refusal, as for datums. |
| 12 | Delete, suppress, reorder, rollback, `part_copy`, `part_instance` with a primitive chain | Observed: delete needs `cascade` and the chain goes in order; suppressing a united primitive unbuilds the union and the Part's body disappears (reported in `unbuilt`); reorder keeps the volume; a primitive made under the rollback bar lands there and the later union is rewired (same body at the end); copies follow global parameters. | Accepted app behaviour with tests; F-183. |
| 13 | Placement on a plane or a face | Not probed (source). | Not offered. |
| 14 | A primitive united with a suppressed prior body (`p14`) | Observed: the union has no shape, no volume, `errors: []`. | Refused; F-185. |
