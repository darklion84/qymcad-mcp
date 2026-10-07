# Phase 3 review log

Phase 3 (3A sketches, 3B topology and features, 3C export and render) was reviewed by two independent models,
working read-only on identical snapshots: OpenAI Codex (gpt-6.1-sol, high reasoning) and Xiaomi MiMo
(MiMo-V2.6-Pro). Claude (Opus 5.5) orchestrated: it evaluated every finding against the code, decided what
to fix, and verified each fix. The rule was: a change is accepted only when two models that did not write it
accept it. Every behavioural fix came with a test that failed before the fix, and a mutation of the fixed code
that turned the test red again.

| Round | Scope | Codex | MiMo | Outcome |
|---|---|---|---|---|
| 1 | each phase branch | 3A/3B/3C: majors found | 3A/3B/3C: majors found (different ones) | fixes by the phase implementers and by Claude |
| 2 | each phase branch after fixes | REJECT (3A: 3 items, 3B: 2, 3C: 1) | 3A ACCEPT, 3B ACCEPT, 3C REJECT (1 item) | branches merged into one integration branch; fixes implemented by Codex (A1–B6, C1–C19), finished and verified by Claude |
| 3 | integrated phase 3 | REJECT (2 items: unsafe rebuild after failed edge restoration; parameter rollback rebuilt committed geometry) | ACCEPT | fixes D1–D7 by Codex; MiMo then rejected one residual (D1 via later rebuilds), fixed (E1) and accepted |

Highlights the reviews caught (all with regression tests):
- inspecting a reopened document could round every edge of a stored edge query (F-023, F-024);
- `"nan"`/`"inf"` passed as numbers; an over-deep expression crashed the server (F-031);
- parametric angles across quadrants broke sketches (F-045); a coordinate crossing zero is refused (ADR 0006);
- OpenCASCADE wrote to stdout and corrupted the protocol (F-019, ADR 0005);
- rollbacks rebuilt old bodies instead of restoring them (F-034);
- exports could write through links (docs/SECURITY.md).

Rejected findings (with the reasons accepted by the reviewers): the protocol test need not decode PNGs
(engine tests do); mirrored placements need no winding flip (QymCAD placements are rigid); engine argument types
need no JSON schema derives (MCP argument structs carry the schemas). Deferred items are in docs/BACKLOG.md.
