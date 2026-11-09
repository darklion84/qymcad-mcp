# ADR 0041: Re-pick for fillet and chamfer

- Status: accepted (2026-11-01).
- Related: ADR 0038; F-008, F-010, F-024, F-074, F-104.

## Context

The QymCAD app edits an existing fillet or chamfer node by replacing its complete picked edge set while retaining the node id and dimension formulas. The first server-side focused feature edit is RemoveFace (ADR 0038). Edge selections must remain persistent picks because app regeneration of stored edge queries is unsafe under F-024.

## Decisions

1. `fillet` and `chamfer` accept an optional `feature`. With it, `edges` resolves against the node's own source body through the normal creation path, seam handling included, and replaces the complete stored edge set.
2. Re-pick preserves the node id, name, dimensions, chamfer mode, d2, flip, downstream timeline and one-step history boundary. `body`, numeric values, mode controls and name are refused with guidance to use `dimension_set` for values.
3. Fillet variable-radius vertex tables are refused. The normal F-074 acceptance check and all rebuild errors apply to the replacement set. Chamfer retained reference-face adjacency is checked before mutation; an explicit new reference face replaces it.
4. Every edit rebuilds on independent B-rep copies and restores the exact recipe, diagnostics and live shapes on failure. Re-pick is legal in batch and is undoable.

## Shell and push_face

`Shell` and `PushFace` use the same focused `feature` update surface. The complete face selection is resolved against the node's own source body, ids are stored as native picks, descriptions retain the creation query, and PushFace refreshes the centroid/normal hint. Body, side, thickness/dist and name stay unchanged and are editable with `dimension_set`. Independent B-rep copies, downstream rebuild checks, batch and one-step undo preserve the atomic rollback contract. Hole re-placement is out of scope: move a hole by editing its sketch/offset dimensions.

## Consequences

Successful results report `feature`, `body`, `name`, `selected_edges` or `selected_faces` and rebuild diagnostics; chamfer results also report retained mode/reference/flip. T2 is covered for RemoveFace, fillet, chamfer, shell and push_face re-picks.

Evidence: `crates/engine/tests/golden_repick.rs`, `crates/engine/tests/golden_repick_faces.rs`, `crates/mcp/tests/d1_repick.rs`, `crates/mcp/tests/d2_repick_faces.rs`, and generated `docs/TOOLS.md`. Acceptance mutations (append instead of replace, an old PushFace hint, no planar check, no refusal on a downstream rebuild error) each turned a named test red.

## 2026-11-09 addendum — descriptions stored as picks

Shell and PushFace re-picks (and creations) now store a description exactly as an id selection: the resolved
face ids as the app's picks, PushFace with the centroid/normal hint. "Descriptions retain the creation query"
above no longer holds: the app's edit mode reads only picked ids (`live_picks`) and reopened such nodes empty.
See ADR 0013 addendum 2026-11-09; evidence `crates/engine/tests/golden_app_face_picks.rs`.
