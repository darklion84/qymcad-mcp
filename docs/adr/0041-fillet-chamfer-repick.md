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

## Consequences

Successful results report `feature`, `body`, `name`, `selected_edges` and rebuild diagnostics; chamfer results also report retained mode/reference/flip. T2 remains partial: RemoveFace, fillet and chamfer re-picks are implemented; shell and push_face are next.

Evidence: `crates/engine/tests/golden_repick.rs`, `crates/mcp/tests/d1_repick.rs`, and generated `docs/TOOLS.md`. Acceptance mutations (append instead of replace, a stored query instead of picks, no F-074 check, no retained-adjacency check) each turned a named test red.
