# ADR 0046: Active result bodies and feature suppression

- Status: accepted (2026-11-03).
- Related: ADR 0008, ADR 0020, ADR 0038, ADR 0041, ADR 0042; F-008, F-024, F-074, F-128, F-134.

## Context

Native suppression has two successful outcomes: a modifier passes its source through under the same output body id, while a base feature and its dependent bodies become intentionally unbuilt. A downstream persistent pick can instead fail, even while native regeneration supplies a pass-through solid. Separately, the rollback marker limits the active timeline prefix, and retained live import originals can outlast their document body. Filtering all suppressed nodes or treating every live shape as a result confuses these states.

## Active result bodies

`Session::result_bodies` considers only timeline nodes before `Project::rollback_place()`, existing `project.bodies`, and unconsumed outputs. A suppressed modifier's native identity-transform output remains eligible under its original body id. Suppressed bases and their suppression cascades contribute no result and are not rebuild errors. Live shapes without a current document body never become reported, measured or exported results; retained import originals remain private rebuild inputs.

This membership drives `doc_info` bodies, current-body choice, topology/select defaults, measure, export and render. Explicit body operands retain each tool's existing current-result restrictions. Open's missing-B-rep repair detection uses the active buildable set, so intentionally unbuilt base cascades and rollback tails do not trigger a false repair failure. The groundwork honors saved rollback state; E7 will expose changing the marker through an MCP tool.

## Feature suppression

1. Expose `feature_suppress {feature: ObjRef, suppressed: bool}`. Both fields are required, unknown fields are refused, and the boolean is an explicit desired state rather than a toggle. Resolve and validate before calling the native setter, which does not reject an invalid timeline index.
2. Accept body-producing timeline features. Refuse sketches, datum planes, parameters, unknown/ambiguous references, and mesh-born Moves (F-128) before mutation. Preserve feature and output ids.
3. For a real state change call `Project::set_feature_suppressed(ti, on)`. The setter marks the node and the entire suffix dirty; assigning the flag directly leaves downstream geometry stale. Rebuild through `with_rebuild_copies`, retaining the existing dependency closure and checked regeneration.
4. Every native rebuild error, including a downstream `EdgesNotFound`, refuses the edit. A pass-through shape is insufficient evidence of success (F-008). Keep F-024's persisted-pick/legacy empty-pool guards and F-074's geometric acceptance check. Do not repair a disappeared pick by silently choosing another edge.
5. Restore the exact pre-edit Project, live B-rep handles, shape absence and diagnostics on failure, without a repair rebuild (ADR 0038/0041). Normal tool/batch history retains ADR 0020's monotonic allocator behavior.
6. Requesting the current flag returns `changed:false`, does not rebuild and adds no undo entry, using rename's history mechanism. A real change adds one undo step; an enclosing batch supplies its single history boundary. Undo restores both flag and geometry, and a failed later batch step restores the pre-batch state.
7. Return `{"feature":{"id":N,"name":...},"suppressed":bool,"changed":bool,"unbuilt":[...],"rebuild":{"bodies":[...],"warnings":[...],"errors":[...]}}`. `unbuilt` identifies bodies intentionally omitted by the suppression cascade, not native failures. `doc_info` timeline nodes expose `suppressed:bool`.

## Consequences and evidence

T6 is partial: body-feature suppression is exposed; rollback remains pending E7. Native source and retained executed probes are recorded in F-134 and `tasks/research-medE/history/`. These probes ran the pinned native kernel and saved-file app parameter scheduling; they do not claim manual GUI acceptance.

Acceptance lives in `crates/engine/tests/golden_suppress.rs` and `crates/mcp/tests/e56_suppress.rs`. Independent formulas include a centered 10 × 20 × 20 prism of volume 4000 mm³; 20 × 20 × 10 stock with a 5 × 5 × 10 cut gives 3750 mm³, or 4000 when bypassed, and an independent 2 × 2 × 10 cut gives 3710 or 3960 mm³ respectively. Stage execution and production mutations are recorded in `tasks/E56-report.md` and `tasks/E56-logs/`.
