# Review log: body booleans, imports and sketch projections (ADR 0025–0028, 0.1.1-dev.20261026.4)

Authors: Codex (gpt-6.1-sol) for the stages and the fixes of rounds 1–3 (sol61 through OmniRoute while Codex was
rate-limited, then Codex fix1b to complete it); the round-4 doc fix, the live-test fixes and the round-5 fix by Claude
(Opus 5.5). Reviewers who did not write the code: fresh Codex sessions (read-only reports, their own probes).
Every fix began with a failing test and was checked with a production mutation.

| Round | Scope | Verdict | Findings | Fixed in |
|---|---|---|---|---|
| 1 | whole branch 3847215 | REJECT | 4 major (forward-timeline projection source; cross-part projection in source-local coordinates #355; reported imported-body names did not resolve; boolean node made in the active part, not the operands'), 4 minor (default piece names allocated twice; union piece name hidden; Piece/MirrorPart marked covered; no-op test did not prove the preflight) | d84bd81 |
| 2 | d84bd81 against round 1 | REJECT | 1 major (opening a saved cross-part projection bypassed the guard), 1 minor (imported names with edge whitespace did not resolve) | 24d473e |
| 3 | 24d473e against round 2 | REJECT | 1 major (blank optional names refused instead of defaulting), 2 minor (undo dropped cross-part warnings; dimension names with edge whitespace — declined, ADR 0026 note: parameter/dimension names stay trimmed) | 6d47a6a |
| 4 | 6d47a6a against round 3 | ACCEPT | 1 minor (ADR 0026 piece-name wording) | 02751f0 |
| live MCP test (Codex, binary 02751f0) | bracket with STEP roundtrip, imported adapter with all booleans, face projection pocket, names/guards/undo/batch, save/open | no P1 | P2: default topology could not find the only imported body (B2); P2 render omission (B1) not reproduced (identical PNGs); P3 cylinder bbox 0.0025 mm under (B3) predates the branch | 00bfb30 |
| 5 | 02751f0..00bfb30 | REJECT | 1 major (fallback body dedup removed only adjacent duplicates) | 55bc641 |
| 6 | 00bfb30..55bc641 | ACCEPT | 1 minor (the fillet test compared only volume; every vertical edge removes the same volume) — the test now compares the saved recipe and topology, and a run on a different edge turns it red | release commit |

Checks: check.sh 732 passed, 3 ignored on the release commit. Live test: volumes against hand formulas, STEP/IGES roundtrip
with the source file deleted, projection following parameter edits, save/reopen. Manual QymCAD.app check
(2026-10-29, three files): `projected_pocket` width 80→60 and `boolean_union` width 80→70 rebuilt without errors and
the source hashes were unchanged; in `import_adapter` the app's tree folded AdapterUnion into the AdapterBlock row
(display only, F-108).
Upstream reports: #355 (cross-part projection ignores placement).
