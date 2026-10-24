# QymCAD v0.1.1 upgrade review log

The upgrade happened in three stages:
- **Stage A, the API port, commit 13cdf13.** Written by OpenAI Codex (gpt-6.1-sol, high).
- **Stages B and C.** Policies, removal of obsolete workarounds, docs and version 0.2.0. Written by Claude (Opus 5.5) after Codex hit its usage limit.

Two models reviewed the work read-only on identical snapshots: Xiaomi MiMo (MiMo-V2.6-Pro) reviewed all of it, and Codex reviewed Claude's part. Claude assessed every finding against the code. Each behavioural fix came with a test that failed before the fix, and with a mutation of the fixed code that turned the test red again. Mutation evidence was recorded per round.

| Round | Snapshot | Codex | MiMo | Outcome |
|---|---|---|---|---|
| 1 | 82948c9 | — | cut off before its summary; three of its sub-audits finished | 9 fixes (9b68c36): fillet refusals persist; QymCAD 0.1.1 drops 0.1.0 splines (F-075, warned on open); case twins include named dimensions; formula problems in doc_info |
| 2 | 9b68c36 | — | REJECT: overlapping blends passed when a support face survived; a persisted refusal blocked every parameter edit | 557e6cb |
| 3 | 557e6cb | REJECT: the two-rail rule refused a valid limiting rim fillet; a clean app-saved overlap passed on open | ACCEPT | 70ae8dd: cusps are judged by source-edge adjacency; open checks kept fillets |
| 4 | 70ae8dd | REJECT: only `Ids` picks were mapped; f32 vertex positions; shallow overlap under the 1.5° flag | delta REJECT (same pick-mapping issue) | 2550078: every pick form, native vertex names, exact angle for cylinders |
| 5 | 2550078 | REJECT: a tangent convex/concave pair was refused (radial directions compared oriented); two far-from-origin f32 limits | — | 31e25ba: angles between surfaces compared as lines; the f32 limits recorded in F-074 |

The fillet acceptance rule (F-074) changed four times under review. Each earlier rule failed on a case the review produced:
- "no tangent edge" refused a valid quarter round;
- "two tangent rails" refused a limiting rim;
- f32 positions misjudged shared vertices;
- oriented radial vectors refused a valid S-shaped pair.

The final rule is in `modifiers::fillet_acceptance`, and its known gaps are listed in F-074.

Live test through MCP (Codex as a tester, binary 557e6cb): PASS 23 / FAIL 4. All four failures were bitwise volume comparisons after a parameter round trip, off by 1e-12 to 6e-11 mm³. They are floating-point noise, not defects. QymCAD 0.1.1's new warnings "n of the m edges could not be taken" on two legacy parts give the same volumes as the 0.1.0 server, which dropped those edges silently.

The user's seven parts were checked after each round. They open with identical volumes, bboxes and face/edge ids, and a full rebuild reproduces their volumes to 1e-11 with no errors.

Codex's three rounds ended with a major (round 5) that is fixed in 31e25ba but not re-reviewed. Under the review rule, accepting that fix is the user's decision.
