# Review log: sketch move/copy/rotate, sketch patterns, DXF/SVG import (ADR 0035–0037, 0.1.1-dev.20261026.6)

Authors: sol61 (gpt-6.1-sol through OmniRoute/OpenCode) for stages B0–B4b; Claude (Opus 5.5) accepted every stage
(report against the task items, tests asserting success values, its own production mutation, full check.sh) and wrote
B4c and the documentation fixes. Reviews follow the incremental policy: each round reviews only its own diff and the
contracts it touches.

| Round | Scope | Verdict | Findings | Fixed in |
|---|---|---|---|---|
| B0 security (sol61) | B0 diff | ACCEPT | none | — |
| B4 review (sol61) | 96ed3b2..34db8b2 | REJECT | 2 major (pattern update by the returned id impossible; sketch_import accepted arbitrarily large files), 1 limitation (revolve/sweep not rebound) | d8135f5 |
| live MCP test 1 (sol61, binary 34db8b2) | transforms, patterns, import, refusals, save/reopen | FAIL | count edit on an explicit full pick shrank the extrusion to one copy (400 instead of 2000); same id-update defect | d8135f5 |
| acceptance of B4b (Claude) | B4b diff | fixed | a test pinned a wrong fixture value (204.7 for a 200 mm³ solid: circle not nested); revolve test asserted only "grew" | d8135f5 |
| automated security note + Claude probe | d8135f5 | fixed | `param_set` raising a pattern count to 100000 hung the MCP server: the native parameter pass regenerates instances before any cap check | 639cf11 |
| live MCP test 2 (sol61, binary d8135f5) | same + explicit picks, update by id, 16 MiB refusal | features PASS | reported FAILs were tester misuse (plate merged by `op add`, `dimension_set` aimed at a sketch name, undo consumed); dimension_set by pattern id rechecked by Claude through MCP | — |

Checks: check.sh 980 passed, 3 ignored on 06417fd. Manual QymCAD.app check (2026-10-31, five files, ChatGPT computer
use): explicit profile pick h 5→8, DXF import h 3→6 (hole kept), SVG import h 4→7 — PASS. Sketch pattern n 3→5: the
sketch shows five squares but the app keeps only the source block (F-123, upstream; the engine rebinds and gives five).
Copy + rotate with w 20→25: the copy follows w because copied internal dimensions keep their formulas — app and
engine agree; the check's expectation was wrong and the docs now state it.
