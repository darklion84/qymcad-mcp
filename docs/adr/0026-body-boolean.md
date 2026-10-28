# ADR 0026: Native body boolean tool

- Status: accepted (2026-10-28).
- Related: ADR 0024, ADR 0025, F-008, F-095, F-108.

## Context

QymCAD v0.1.1-dev.20261026 exposes `Project::add_body_boolean(a, b, op)`: cut, union and intersect consume both current bodies and rebuild parametrically from both recipe inputs. Cut and intersect can dynamically produce multiple output bodies; union can produce one compound body containing multiple solids.

## Decisions

1. Add `body_boolean({a,b,op,name?,piece_names?})` as a separate modelling tool. `a` and `b` are required object references, must be distinct current bodies in the same part, and `op` is `cut`, `union` or `intersect` with `cut` as the default. Create the node in the operands' owning part with the existing active-context helper, then restore the user's prior active part.
2. Rely on native regeneration to validate booleans; do not call `ask::boolean_gives_body` as a separate self-gating preflight. Source trace shows `run.rs` calls the same `Shape::boolean_checked` operation that `ask::boolean_gives_body` delegates to, so the preflight duplicates kernel work. `atomic_with_post` rejects native rebuild errors and restores the pre-edit Project and shapes, including pass-through no-op shapes. Always inspect `RegenReport.errors`.
3. Preserve the native recipe and dynamic output semantics. Both operands are consumed. Output `BodyInfo` entries include every native output body; union remains one body even when it has multiple solids.
4. Output piece names are assigned atomically. Explicit `piece_names` must match the measured output count; every nonblank entry must be distinct, nonnumeric, without surrounding whitespace and unused. Blank entries and unnamed outputs receive the feature name plus the next unused positive suffix.
5. The server refuses later edits that change output-body count, change result solid count, or make surviving piece identity ambiguous under global bounding-box overlap matching. The refusal is atomic and tells the caller to recreate the boolean. This is a server safety policy; QymCAD.app's native dynamic allocation remains unchanged.
6. Boolean outputs participate in normal id/name resolution, export, doc_info, batch and undo. Both operands remain in the dependency graph, so parameter edits to either operand rebuild the boolean.

## Consequences

Stable-topology parameter edits work through save/open and the app-path emulation. Topology-changing edits are deliberately conservative because native piece ids can disappear or be reassigned. A future identity policy may relax the guard after evidence for downstream references and app behavior exists.

Evidence: `tasks/cover-2-evidence.md`; tests `crates/engine/tests/golden_body_boolean.rs` and `crates/mcp/tests/body_boolean.rs`.
