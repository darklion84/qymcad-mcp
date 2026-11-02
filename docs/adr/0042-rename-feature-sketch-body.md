# ADR 0042: Rename feature, sketch and body

- Status: accepted (2026-11-02).
- Related: ADR 0008, ADR 0020, ADR 0038, ADR 0041; F-130.

## Context

QymCAD stores three independent labels: a timeline `FeatureNode.name`, a sketch record's `Sketch.name`, and a body's `Body.name`. The app edits each through a separate path, even when a sketch and its timeline node share an id. Native setters accept labels that the server's id-first resolver cannot address reliably, and a missing target can silently do nothing. Rename therefore needs an explicit target and server validation before any write.

## Decisions

1. Expose `rename` with required `target` and `name`. The target is exactly one of `{"feature": ObjRef}`, `{"sketch": ObjRef}` or `{"body": ObjRef}`; unknown fields and combinations are refused. Resolve the reference and validate the requested native kind before mutation. A feature target includes a sketch's timeline node.
2. Write exactly one native store: feature calls `Project::rename_node`; sketch assigns `project.sketches[i].name`; body resolves `Project::mesh_index(id)` and calls `Project::set_mesh_name`. Renaming one store does not synchronize any other label, including labels of already-created downstream bodies. Preserve all ids, feature kinds, formulas, `feat_dims`, named dimensions and persistent references.
3. Reject Unicode control characters in the raw input before trimming. Trim surrounding whitespace, refuse an empty result, then reuse creation's `check_object_name` numeric-id guard. Store the trimmed text with its case. Thus `"  Base  "` stores `"Base"`; `"123"`, `"+1"` and `"007"` are refused. These are server policies; native setters are more permissive (F-130).
4. Allow duplicate labels, as the app does. Results list the other ids with the same raw or localised label in `duplicates`, deduplicated and sorted. Later ambiguous text references retain the existing “several objects … use the id” refusal. A request that already matches the target's stored name returns `changed: false`, writes nothing and adds no undo entry.
5. Return `{"target":{"kind":"feature|sketch|body","id":N},"previous_name":...,"name":...,"display_name":...,"changed":bool,"duplicates":[...]}`. `previous_name` and `name` are stored text; `display_name` uses the existing localisation function. A manually supplied `feat-name-shell` therefore remains stored as that key and displays as `Shell`. Localisation keys remain allowed.
6. Extend `Session::resolve` with both raw and localised `Sketch.name` and `Body.name` aliases. Deduplicate matching aliases by id, retain existing current-body preference and preserve the existing ambiguity rule. New sketch labels work immediately and after save/open in every tool using the common resolver, including `sketch_add` and `extrude`. The former sketch record name stops being an alias; a separately unchanged timeline label remains an alias in its own right.
7. Keep existing body `name` reporting semantics, including the producing feature label for ordinary bodies. Add `body_name` for the localised native `Body.name` wherever body records are reported, including `doc_info`, topology and feature/rebuild results. This makes the native label visible without breaking clients that read `name`.

## Metadata transaction and history

Rename does not mark anything dirty, solve sketches or regenerate geometry, including when the document already has dirty nodes or diagnostics. It does not use `Session::atomic`, whose feature path rebuilds geometry. Validation, kind resolution and the same-name check happen before a write. Direct engine rename uses the existing `begin_tool_edit` / `finish_tool_edit` snapshot boundary for one undo step; an enclosing MCP call or batch reuses its boundary. Failure restores the original project, live shapes and diagnostics without a repair rebuild.

The MCP history boundary skips a successful same-name rename, preserving the previous undo entry. Batch permits rename, and subsequent steps resolve the new label immediately. A later failed step restores the complete pre-batch snapshot; a successful batch remains one undo step.

A failed batch that contains a rename is rolled back like any other failed batch: recipe, B-reps and diagnostics are restored and, per ADR 0020/F-056, the allocator keeps the ids it handed out. Acceptance compares every archive entry before/after a late batch failure with only the `next_id` line normalised. Undo restores the previous stored label exactly.

## Consequences and evidence

T4 is covered by one explicit tool for the three app label stores. Rename changes metadata and resolver aliases; it does not rename parameters, dimensions, export paths or embedded export hierarchy metadata.

Native source/probe evidence is recorded in F-130 and `tasks/research-medE/rename/` (`src/main.rs`, `rustc-run.log`, saved `.qcad` fixtures). Acceptance tests are `crates/engine/tests/golden_rename.rs` and `crates/mcp/tests/e1_rename.rs`. The rectangular fixture has unchanged volume 10 × 6 × 5 = 300 mm³ after renaming; saved-file app-path and engine parameter edits to h=8 give 10 × 6 × 8 = 480 mm³ while preserving all three labels. Stage verification and production-mutation transcripts are in `tasks/E1-report.md` and `tasks/E1-logs/`.
