# ADR 0035: One-shot sketch curve transforms

- Status: accepted (2026-10-31).
- Related: ADR 0008, ADR 0020, ADR 0030, ADR 0034; F-024, F-074, F-121.

## Context

QymCAD exposes native move, copy and rotate operations for selected sketch curves. They mutate sketch geometry directly and do not create timeline drivers. The app carries whole figures and copies selected geometry without links to its source.

## Decisions

1. Extend `sketch_edit` with `move`, `copy` and `rotate`. Inputs are finite `Num` values evaluated once from the current parameter scope. They are one-shot geometry edits and results report `parametric: false`.
2. Accept a non-empty list of curve entity ids from the selected sketch. Text is refused because the pinned native transform API accepts curve entity ids only.
3. Call `Project::move_entities`, `copy_entities` and `rotate_entities`. Copy reports `linked: false` and created entity ids; move and rotate report changed ids. The sketch edit report compares constraints and reports dropped formulas, every removed/released constraint or dimension, and released offsets.
4. Preserve the existing sketch transaction, dependent rebuild, solver residual and exact rollback rules. A native solver result that does not satisfy the requested transformed geometry, including a nonzero transform that becomes a no-op, is refused atomically with the holding constraints/dimensions named.
5. Preserve explicit Points and sketch-hole dependencies through the existing native retention and dependency closure paths.

## Consequences

Move, copy and rotate match the app's public core path and are batchable and undoable through the existing modelling history. Transforms are intentionally not drivers; users who need later parameter edits must use named dimensions or constraints after the edit.

Evidence: native API source and probe in `tasks/research-medB.md` S8; engine and MCP tests in `crates/mcp/tests/sketch_edits_2.rs`; final verification in `tasks/B1-report.md`.
