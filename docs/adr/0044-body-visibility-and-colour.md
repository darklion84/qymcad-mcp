# ADR 0044: Body visibility and colour

- Status: accepted (2026-11-02).
- Related: ADR 0008, ADR 0020, ADR 0042; F-132.

## Context

The pinned app saves a visibility flag on each body and a manual RGB map keyed by the body's consuming-lineage root. These have different scopes: hiding one result does not hide a later consuming result, while its colour follows that lineage. Sketch hiding exists only in private GUI session state and has no persisted field in the pinned document format. Native colour setters silently ignore invalid indices, so the server must validate its target before calling them.

## Decisions

1. Add `appearance` with required `body: ObjRef`, optional `visible: bool` and optional `color`. Omitted colour leaves the colour unchanged; an RGB array sets it; explicit `null` calls native Reset. Require at least one of `visible` or `color` to be present and deny unknown fields. Decode RGB as exactly three integer bytes in 0..=255; refuse negative, oversized, fractional, string and wrong-length values before any mutation. Do not cast floats to bytes.
2. Resolve and validate a current, unconsumed body before any write. Refuse consumed bodies, sketches, datums, unknown references and ambiguous names. Sketches are not appearance targets: the pinned QymCAD format does not persist sketch visibility.
3. Resolve `Project::mesh_index(id)` and assign only that body's `visible` flag. Set colour through `Project::set_mesh_color(index, rgb)` and reset through `Project::reset_mesh_color(index)`. These native methods use `Project::lineage_root(id)` for `part_colors`; do not substitute the selected body's id. Preserve imported per-face and per-triangle colour overrides.
4. Return `{"body":{"id":N,"name":...,"body_name":...},"lineage_root":N,"visible":bool,"color_rgb":[r,g,b],"color_source":"manual|palette","scope":{"visibility":"body","color":"lineage"},"changed":bool}`. `color_rgb` comes from native `mesh_color`. `color_source` is `manual` exactly when `part_colors` contains the lineage root, including a manual RGB equal to the palette; otherwise it is `palette`.
5. Add `visible`, `color_rgb` and `color_source` to each `doc_info` body record. Preserve every existing field and its meaning, including `name` and `body_name` (ADR 0042).
6. Save appearance through the native body flag and `part_colors`, so opening the document and rebuilding the same body after a parameter edit retains it. A newly created downstream body defaults to `visible: true`, independently of a hidden source, and reads its lineage colour through the native rule.

## Metadata transaction and history

Reuse ADR 0042's metadata edit boundary: validate and detect no-ops before writing, use `begin_tool_edit` / `finish_tool_edit` for one undo step, and reuse an enclosing MCP or batch boundary. Do not mark a node dirty, rebuild or solve geometry, or change B-reps and diagnostics. A request whose flag and manual-colour state already match returns `changed: false` and skips history, preserving the previous undo entry. Reset with no root map entry is a colour no-op; explicitly assigning the palette RGB while no manual entry exists is a real change.

Allow appearance in batch. A failed step restores the pre-batch metadata, geometry and diagnostics without a repair rebuild. Preserve ADR 0020's monotonic allocator: archive rollback comparisons normalize only `next_id`. Undo restores the exact prior flag and map.

## Output compatibility and scope

In E3, `render` and `export` continue to show/write every current result body regardless of `visible`, without body colours. The appearance tool description states this explicitly. Colour in exports and an opt-in visibility filter belong to E4; this ADR does not change those existing output contracts. Sketch visibility remains unsupported until the native persisted format can represent it.

## Evidence and acceptance

F-132 cites native source and the retained executed probe in `tasks/research-medE/appearance/`, summarized in `tasks/research-medE.md`, T7. The probe's 10×20×h extrusion and consuming Move preserve hiddenness and RGB `[17,34,51]` through native save/load and app-path h=4→7 regeneration, with expected volume 10 × 20 × 7 = 1400 mm³. It also records a visible downstream body and native reset palette `[150,170,190]`. These are headless app-path observations; no GUI verification is claimed.

Stage acceptance is in `crates/engine/tests/golden_appearance.rs`, `crates/mcp/tests/e3_appearance.rs`, `tasks/E3-report.md` and `tasks/E3-logs/`, including refusal snapshots, history/batch behavior, override preservation and production mutations. The earlier native probe is separate evidence from those acceptance tests.

The existing ADR 0042 boundary restores independent native B-rep copies for live use and retains original handles for rollback/undo. Native restore can normalize a Move's serialized representation even without an edit (`tasks/E3-logs/history-copy-probe.log`, `history-copy-probe2.log`). Successful appearance tests therefore compare bytes to an independently restored pre-call B-rep, plus exact topology/recipe and hand-computed volume; refusal and undo compare raw originals exactly. E3 does not change that accepted boundary or claim raw pre-call/success serialization equality.
