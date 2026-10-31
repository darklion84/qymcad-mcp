# Review log: two-sided extrude, sketch points, tilted datums, chamfer angle, move, sketch holes (ADR 0029–0034, 0.1.1-dev.20261026.5)

Authors: Codex (gpt-6.1-sol) for stages A1–A4 and the fixes of rounds 1–5. Reviewers who did not write the code:
fresh Codex sessions for rounds 1–5 (read-only reports, their own probes); round 6 by sol61 through OmniRoute and by
Claude (Opus 5.5), both on the round diff only. Every fix began with a failing test and was checked with a production
mutation; Claude added its own mutation per round.

| Round | Scope | Verdict | Findings | Fixed in |
|---|---|---|---|---|
| 1 | whole branch 9079196..32a6e16 | REJECT | 3 major (negative literal Point driver silently made absolute; open/rebuild skipped Point side validation; deletion closure missed named coordinate sources), 1 minor (scientific numeric strings in sketch-hole sizes) | 99099d1 |
| 2 | whole branch | REJECT | 2 major (deleting a named dimension's owner left another Move with a dead formula; a named coordinate source bypassed the projected-support refusal) | d065f88 |
| 3 | whole branch | REJECT | 3 major (open/rebuild did not recheck evaluated domains after regeneration; formulas reading reference measurements stayed stale in the engine; a later world-plane source left a saved sketch hole stale) | 72ee12e |
| 4 | whole branch | REJECT | 4 major (sketch_remove of an unrelated entity pruned explicit Points and their bores; same-sketch reference measurement bypassed the order check; settling validated an intermediate scope; repeat pass built arrays over the 1000-copy cap) | bdc37cd |
| 5 | whole branch | REJECT | 3 major (native offset cleanup deleted a free explicit Point in the saved app path; transitive reference ordering; open validated before settling) — closed as classes: no reference measurement in a sketch-hole source graph, native retention of explicit Points, settle before validating | c7f3183 |
| 6 | bdc37cd..c7f3183 only | ACCEPT | none | — |
| live MCP test (sol61, binary c7f3183) | two-sided plate, two/three sketch holes moved by a parameter (face centroids), line removal keeps points, tilted datum, chamfer leg+angle, move, refusals, save/reopen/edit, STEP | PASS | none | — |

Rounds 1–5 reviewed the whole branch and used most of a weekly Codex limit; from round 6 a round reviews only its own
diff and the contracts it touches (owner's decision, 2026-10-31).

Checks: check.sh 945 passed, 3 ignored on c7f3183. Manual QymCAD.app check (2026-10-31, five files, ChatGPT computer
use): two-sided down 5→8, point holes px 15→25 (free hole kept), tilt 15→25, chamfer 3→5, move shift 30→50 — all
rebuilt without errors, source hashes unchanged. A free explicit Point shows its retention reference distance as a
grey dimension from the origin.
