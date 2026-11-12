# Package H review log (0.1.1-dev.20261026.14)

Subject: part_delete, joint limits and Revolute hinges (ADR 0054), branch medium-h on top of 0.1.1-dev.20261026.13. Principle: every tool is an existing QymCAD app action with the GUI's limits; silently wrong native results are refused and filed upstream.

| Round | Subject | Verdict |
|---|---|---|
| Research | native delete, limits, Revolute (tasks/research-H) | six defect candidates |
| 1 | `0bcab6a..b167dd6`, two independent reviewers in parallel (Claude Opus 5.5 and Claude Sonnet 5.5 high) | FIX: Opus 1 blocker, 4 major, 4 minor; Sonnet 1 major, 6 minor; fixed in 547a35d |
| 1b | face anchors of Rigid/Slider (Sonnet's note) | stale anchor confirmed, also in .12/.13; fixed in 1835a39 |
| GUI 1 | QymCAD.app, Claude Desktop computer use | 15/23: hinge files opened with "anchor not found" |
| GUI 2 | hinge made in the app itself, save, reopen | the app's own hinge loses its anchor too: QymCAD defect F-166 (#388); a save-time workaround (c7cafd4) was reverted in 7896e49, a note is returned instead |

## Round 1 findings, fixed in 547a35d

- A joint from a file with values the app accepts (min > max, angle bound out of range) locked every edit of the document; validation is now scoped to the joints an edit touches, other faults are reported in `limit_faults`.
- A ticked-off bound keeps its formula dormant, as the app does; dormant formulas are not evaluated and do not block `param_delete`.
- Clamps of numeric drives by a limit formula, and clamps inside compact batch steps, are reported; `part_delete`'s `native_leftovers` reach batch steps.
- The ±100000 range applies to typed numbers only; angle bounds use (−180, 180].
- A hinge on a body consumed by a later feature kept the old pivot while reporting healthy (F-160): edge anchors now follow the anchored body and refuse edits that move the edge. `at_limit` has one format.

## Fix 1b (1835a39)

- Face anchors of Rigid/Slider had the same F-160 weakness in released .12/.13: a push_face of the anchored face left the Slider on the old face. Edits that move an anchored face are refused atomically.

## Evidence

- `scripts/check.sh` on the release commit: OK, 1393 passed, 0 failed, 3 ignored.
- Live stdio run of the release binary: 96/96 PASS (hinged door at −90 against hand bbox, limit [−110, 0] clamp reports, formula drive, refusals with unchanged document, batch clamp, save/reopen; Slider with a formula limit and dormant bound; part_delete with leftovers, undo, clone/source/rollback rules).
- QymCAD.app: limits, clamp marks, formula drive and the drawer stop work; a hinge works after Edit → Rebuild all; the app-made hinge reproduces F-166.
- Upstream issues: QymIs-Tech/QymCAD#383 (F-154), #384 (F-156), #385 (F-157), #386 (F-158), #387 (F-160), #388 (F-166); F-159 is #263.

## Known limits

- F-166: after opening a file with an edge-anchored joint in QymCAD.app, use Edit → Rebuild all; `doc_save` and `joint_create` say so.
- Edits that move an anchored face or edge (including fillet/chamfer of its edges) are refused.
- Dragging the door or drawer by mouse in the app was not checked (background drag rotated the camera).
