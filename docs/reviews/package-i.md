# Package I review log (0.1.1-dev.20261026.14 + datums)

Subject: `sketch_edit op:"construction"`, `datum_point` and `datum_axis` (ADR 0055), branch medium-i on top of 0.1.1-dev.20261026.14. Principle: every tool is one existing QymCAD app action; only a silently wrong native result is refused, nothing is worked around, and a refusal comes with a FINDINGS entry.

| Round | Subject | Verdict |
|---|---|---|
| Implementation | S2, D4, D3, docs (`d590c84..2ede273`) | built, one allowlist test fixed in `4182404` |
| 1 | `2ede273`, two independent reviewers in parallel (Claude Opus 5.5 and Claude Sonnet 5.5) | FIX: Opus 1 blocker, 4 major, minor and nits; Sonnet 4 major, 6 minor, 5 notes; fixed in `dde4ad8..d09d0ac` and the docs commit |

## Round 1 findings and what was done

| Finding | Reviewer | Done |
|---|---|---|
| `dimension_set` x/y/z on a vertex point is written and ignored (blocker) | Opus 1, Sonnet M2 | Refused (F-174), test, native probe P14. |
| `part_copy` with a vertex point: the copy stands on the SOURCE body, a cascade of the source deletes it (major) | Opus 2, Sonnet M1 | `part_copy` refused (F-175, native probe P15). `part_instance` copies no datum node, so it needs no refusal: tested with datums. Edge, face and two-point axes are remapped by the app: tested. |
| An edit that removes the edge or face under a point or axis keeps the old place (major) | Opus 3, Sonnet m3 | Judged after the rebuild of every edit, rolled back (F-176, native probe P13); tests with a hexagon 6 -> 4 (point and edge axis) and a bore closing up (face axis). A consumed body is not an edit of its edge (tested). |
| The F-168 guard compares every contour and its premise is wrong (the app matches by provenance and centroid) (major) | Opus 4, Sonnet M3, m4, m5 | Guard rewritten: only contours that features hold, only when some but not all of a feature's profiles vanish; hole to construction is allowed. F-168, ADR section 3, the help path (`docs/help/en/sketch/23-construction.md`) and "reused by position" corrected. Tests: hole passes (volume grows), two islands refused, an unused ring passes, the only profile rolls back. |
| `datum_axis points` from another Part accepted (major) | Opus 5, Sonnet M4 | Both points must belong to the active Part. Test. |
| Datum made with the assembly root active is created silently (parent = root) | Sonnet n5 | Refused as not covered, both tools. Tests. |
| `Session::open` refuses an app-saved file whose two-point axis lost a point | Sonnet m2 | Open warns (doc_info and the open reply); the datums faulty at opening do not block other edits; edits that make a datum faulty are refused. Test. |
| Texts: edge or face of another Part claims a GUI limit; the `current_part_body` comment; the `__explicit` refusal mentions F-170; datum_point says suppress is refused only "without cascade" | Opus minor/nit, Sonnet m1, n1 | A point's vertex is the app's limit, an axis's edge or face is the server's (the Part frame); messages, the comment, ADR section 2 and the descriptions corrected; the marker has its own text. |
| Manual axis `dir` not normalised in replies; negative zero | Opus minor | Unit vector and zero cleaned in replies and doc_info; the app keeps what was typed. Test. |
| `doc_info.datums` lacks the owner Part | Opus minor | `owner_part` on points and axes in a multi-Part document. Tests. |
| F-170 also blocks numeric edits of a manual axis | Opus minor | Not changed: stated as "not covered, delete and create again" in ADR, COVERAGE and the tool description. |
| The probes pin the server's guard, not the app (F-168, F-171); a face axis is missing from the save/open test | Opus minor, Sonnet n4, m6 | Native probes P11, P7 through the core API only; the face axis joined `every_definition_survives_save_and_open...`. |
| Tests: only `is_err`, `contains(a) \|\| contains(b)`, a tautology in the MCP test, no MCP vertex/edge/face tests, nothing for a suppressed body under an edge/face axis, a revolve with another Part's axis, `param_set` collapsing axis points, `part_instance` with datums | Opus, Sonnet n3 | All added or tightened (texts asserted, alternatives removed, the `changed_entities` assertion made specific, the unused `other` removed). |
| ADR refers to untracked `tasks/*.md`; the PROBES table belongs in the ADR | Opus nit, Sonnet | The table is the ADR section "Probes"; no `tasks/` reference is left in the ADR. |
| The server is stricter than the GUI on edges (`EdgeKind::Line` against `is_straight_poly`, visibility ignored); the `__explicit` marker may be lost on a GUI edit | Opus nit, Sonnet n2 | ADR "Known limits"; the marker is in BACKLOG. |
| Side note: `sketch_remove` of a circle under an extrude passes, volume unchanged, errors `[]` | Opus | Probe P16: "volume unchanged" is NOT reproduced (the volume follows the geometry); an island of a several-profile feature does drop out silently with `errors: []`, like F-168 through the delete door. Not fixed here: F-177 and BACKLOG. |

## Evidence

- Targeted: the datum, construction and probe test files and the MCP `i1`, `i2`, `i3` and `docs_fresh` tests pass (observed); the whole engine suite passed once on an intermediate state (1164 passed, 0 failed, 3 ignored).
- `scripts/check.sh`: see the closing entry below.
- New findings: F-174 (typed coordinate on a vertex point ignored), F-175 (Part copy leaves a vertex point on the source), F-176 (vanished edge or face under a datum), F-177 (`sketch_remove` drops one profile of several); F-168 rewritten.
- Not run: QymCAD.app itself (the GUI); every app-side claim is a native core-API probe or read in source, as marked in FINDINGS.

## Known limits

- A numeric edit of a manual axis is not covered (F-170): delete and create again.
- The server judges edges by `EdgeKind::Line`, not as the app's picker does, and does not consider visibility.
- F-177 is open.
