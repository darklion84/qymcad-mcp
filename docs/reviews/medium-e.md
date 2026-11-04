# Review log: rename, measure, appearance and colour export, suppression, rollback (ADR 0042–0047, 0.1.1-dev.20261026.9)

Authors: Codex (gpt-6.1-sol) for stages E1–E8; Claude (Opus 5.5) wrote the task files, accepted every stage
(report against the task items, tests read one by one against the items, its own production mutations, full check.sh)
and made the fixes below. Reviews follow the incremental policy: each round reviews only its own diff.

| Round | Scope | Verdict | Findings | Fixed in |
|---|---|---|---|---|
| acceptance of E1 (Claude) | rename | fixed | Codex rolled back the id allocator on a failed batch, changing ADR 0020 behaviour to satisfy a byte-equal archive check: reverted, the test normalises `next_id` as ADR 0020 allows; own mutation "duplicate by display name not diagnosed" survived, test added. New task rule: never change behaviour promised by an existing ADR or rewrite an old test to pass | 6b262af |
| acceptance of E2–E3 (Claude) | measure, appearance | accepted | own mutations red; one mutation pattern did not match the source and was skipped (reported, not counted) | bf73c92, 4593028 |
| acceptance of E4 (Claude) | colour in 3MF/GLB/STEP, `visible_only` | accepted after relaunch | first run stopped on a conflict with an E3 test (as the new rule requires); the task got an explicit exception and was rerun | 91a0d4a |
| acceptance of E5+E6 (Claude) | active result bodies, `feature_suppress` | accepted | — | 6e909e9 |
| acceptance of E7 (Claude) | `history_rollback` | accepted after relaunch | first run stopped on ADR 0033 (Move reuse); the task was changed to a 2×2 through cut and asserts that Move reuse stays | 0a3b89e |
| review E (Codex) | 392288f..0a3b89e | FIX | E-R1 refused measure may change face cache in the next save; E-R2 suppressing a cut with several output pieces; E-R3 rollback/suppress lose body names and visibility; E-R4 inactive chamfer blocks rollback to start; E-R5 identity guards reject intended suppression removal (all medium, all reproduced) | 8e89fe2 |
| review E8 (Codex) | 0a3b89e..8e89fe2 | ACCEPT | none | — |

E8 decisions: E-R1 is documented (same face-cache restore as `select`, only derived `faces/*.ron`); E-R2/E-R5 refuse
suppression changes involving multiple output bodies before mutation; E-R3 reports `lost_body_metadata` (F-136);
E-R4 limits geometry checks to built nodes.

Checks: check.sh 1149 passed, 0 failed, 3 ignored on 8e89fe2. Live MCP test over stdio (binary 8e89fe2): rename of
sketch/body with no-op and refusals; measure plane gap 20 and 3-4-5 = 5; manual colour 17/34/51 into 3MF
`displaycolor="#112233"` and STEP `COLOUR_RGB`; cut 4000 → 3920, suppress → 4000, enable → 3920; rollback after
plate → 4000, `doc_info.rollback`, back to end → 3920; save/open keeps colour, name and volume; h=30 → 5880, undo
→ 3920 — PASS.

Manual QymCAD.app check (ChatGPT with computer use, 2026-11-04): four files in `tmp/gui-check-medE`.
1-rename: names «Плита», drawing, Housing shown; h 20→30 gives 10×20×30 mm, names kept — PASS.
2-colour-hidden: block red (R 200, G 40, B 40, manual), hidden cylinder absent; the app's own 3MF export holds one
body (24 vertices, 12 triangles, `#C82828`) — PASS. 3-suppressed: hole2 suppressed, enabling it cuts the through
hole — PASS. 4-rollback: marker after plate and the block without a hole on open — PASS; dragging the marker with
computer use did not move it (three tries, cause not established: the tool's drag delivery or the app), the app's
context menu «Снять откат» moved it to the end and the hole appeared — counted 3/4 by the checker, the file itself
behaves as a native one. Files unchanged (SHA-256).
