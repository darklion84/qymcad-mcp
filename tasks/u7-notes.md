# U7 / U2 work in progress

- [x] Read AGENTS, architecture, findings (large first combined read truncated; re-read F-010–F-049 in chunks), backlog and MiMo review.
- [ ] Add failing datum user-label and one-sided top-area regressions; record failures.
- [ ] Implement labels, correct units and area bound; mutation-check and restore.
- [ ] Add both-datum mode, strengthen guard-refresh assertions, comment literal MCP pocket.
- [ ] Narrow F-017 / ADR 0007 claims; surface structural GUI limitation in tool descriptions (parent owns CHANGELOG).
- [ ] Investigate U2 native feature/cut-tool pattern and document options; improve whole-body array descriptions.
- [ ] Run targeted verification; write tasks/review-u7.md; provide parent integration notes.

## Integration additions for parent

F-027 evidence addition (existing finding):
- Native arrays cannot pattern one existing feature or an independent cut tool. Source `qymcad-core/src/feature.rs:1187-1230`, `model/timeline.rs:1271-1291`, `model/regen.rs:2230-2290` and `qymcad-kernel/src/kernel.rs:1006-1041`: array source is the whole body, cloned and united at each transform.
- Related native option: `Project::add_hole_from_sketch` (`timeline.rs:1345-1359`, `regen.rs:2081-2093`, `kernel.rs:997-1005`) makes a hole at each isolated sketch point in one boolean. It has no count/spacing expressions or seed-operation reference. No general feature-pattern tool added; options in review-u7.md.
- Current supported repeated-cut recipe: sketch circles at expression-driven centres then extrude-cut with `through=true`; cardinality is sketch entity count and is not parameter driven.

CHANGELOG additions:
- `param_users` labels reserved datum scheduling guards as "datum dependency of sketch …" while continuing to block deleting the live parameter.
- After GUI structural edits (creating/re-hosting a datum sketch or replacing a datum distance expression), reopen and save through the server before further GUI parameter edits so persisted guards refresh (ADR 0007 limitation). Surfaced in the `param_set` tool description.
- Linear/circular array tool docs explicitly describe whole-body copying (including cuts) and suggest sketch circles + through cut for repeated holes; pinned QymCAD has no general feature-pattern capability.
- Datum regression coverage includes both parameterized ancestors and exact refreshed-guard keys; top-area check allows only tessellation inflation (plus numerical roundoff).

No new ADR: user-label presentation, test precision and explicit docs apply existing ADR 0007/F-027 decisions; new pattern semantics were deliberately stopped per user instruction.
