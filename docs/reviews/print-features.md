# Review log: thread, split and sketch text (ADR 0024, 0.1.1-dev.20261026.3)

Authors: Codex (gpt-6.1-sol) for stages 1–3 and the fixes of rounds 1 and 3; round-2 fixes started by Codex and
finished by Claude (Opus 5.5); the live-test message fix by Claude. Reviewers who did not write the code: fresh Codex
sessions (read-only reports, their own probes) and Claude. MiMo was dropped on 2026-10-27 (subscription ended).
Every fix began with a failing test and was checked with a production mutation.

| Round | Scope | Verdict | Findings | Fixed in |
|---|---|---|---|---|
| 1 | whole branch 5115f45 | REJECT | 3 major (text height editable #347; undersized internal bore buries the groove; split pieces swap identity #350), 2 minor (custom angle ≤ 1°; numeric piece names), 1 nit (unlabelled thread error) | dbc13b4 |
| 2 | whole branch dbc13b4 | REJECT | 3 major (custom angle > 148.969° clamped; infeasible depth changes the flank; shallow profile cut on the wrong side #353), 1 minor (global split tolerance); plus an audit of every thread input against native clamps (runouts, radii, pitch/depth epsilons, buttress, angle/depth on standard profiles) and dimension_set keeping 4 decimals (F-103, F-104) | c565725 |
| 3 | whole branch c565725 | REJECT | 5 major (crest/root radii matched across kinds; numeric strings rounded; internal Tr/ACME/Rd diameters reported wrong; second unnamed split refused; bitmap-only glyph dropped), 1 minor (ISO internal radius refused on the nominal bore) | 37fe17d |
| 4 (narrow, owner's choice after three rounds) | 01f7f08..37fe17d | ACCEPT | none; reviewer reversed the fixes: 11 tests fail | — |
| live MCP test (Codex, binary 37fe17d) | bolt/nut M8, Tr16x4 screw/sleeve, split box, raised/engraved label | no P1/P2 | P3: a split edit missing the body gave an opaque, duplicated message | 37d4351 |
| mini-review of 37d4351 | 37fe17d..37d4351 | REJECT | the hint claimed a missed plane as the only cause | 34975f5 |
| mini-review of 34975f5 | 37fe17d..34975f5 | ACCEPT | — | — |

Checks: check.sh 651 passed, 3 ignored on 34975f5. Live test: every volume against a hand formula, ISO 68-1/724 and
ISO 2904 basic diameters, parameter edits, save/reopen, eight 3MF exports closed (every edge on two triangles). App
path emulated (load with B-rep, Parameters edit, dirty rebuild) on the five check files: no errors, volumes equal to
the formulas and to the server. The owner chose to merge without the manual QymCAD.app check.
Upstream reports: #347 (text height), #350 (split identity), #353 (shallow thread side).
