# Review log: feature re-pick for fillet, chamfer, shell, push face (ADR 0041, 0.1.1-dev.20261026.8)

Authors: sol61 (gpt-6.1-sol through OmniRoute/OpenCode) for stages D1–D2; Claude (Opus 5.5) accepted every stage
(report against the task items, tests read one by one against the items, its own production mutations, full check.sh)
and made the fixes below. Reviews follow the incremental policy: each round reviews only its own diff.

| Round | Scope | Verdict | Findings | Fixed in |
|---|---|---|---|---|
| acceptance of D1 (Claude) | fillet, chamfer re-pick | fixed | `edges` doc dropped from the schema; a FINDINGS entry recorded our own policy, not QymCAD behaviour; unclear refusal text; dead `debug_assert`; own mutations (ref_face not written, node not marked dirty) red | 68b02c2 |
| acceptance of D2 (Claude) | shell, push_face re-pick | fixed | PushFace reference built twice (creation and re-pick) — one `push_face_ref`; own mutation "descriptive shell re-pick stored as picks" survived, test strengthened; "no dirty on push_face re-pick" red | 46394d4 |
| review D (sol61) | 914bf97..46394d4 | ACCEPT | none | — |
| live MCP test (sol61, binary 46394d4) | fillet/chamfer/shell/push_face re-pick, param edits, undo, refusals, batch rollback, save/open | checks 1–7 PASS | two refusals were sent with extra arguments and never reached the code under test; Claude reran them over stdio: wrong kind for fillet/chamfer/push_face on a shell and an empty shell selection are refused, doc_info unchanged; shell re-pick by name works | — |

Checks: check.sh 1036 passed, 3 ignored on 46394d4. Live MCP test: fillet 3991.416 → 3965.664 (4 edges) → r=1
3991.416, undo back; chamfer 3990 → 3980; shell 1084 → tube 760 → t=2 1440; push_face 6000 → +x 5000 → d=2 4400;
failed batch rolled back the re-pick; save/open matches — PASS.
