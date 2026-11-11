# Package G review log (0.1.1-dev.20261026.13)

Subject: draft, thicken (ADR 0052) and part instances and copies (ADR 0053), branch medium-g on top of 0.1.1-dev.20261026.12. Principle: every tool is an existing QymCAD app action with the GUI's limits; where native behaviour is silently wrong the tool refuses, and the defect is filed upstream.

| Round | Subject | Verdict |
|---|---|---|
| Acceptance | 9d6ec8a draft/thicken | one deviation from the app (neutral among the faces refused); fixed in 565f58f |
| 1 | `ad9d509..b476e65`, independent Claude reviewer (Codex out of quota until 2026-11-12) | FIX: 3 major, 4 minor; fixed in ef6f1e0 |
| GUI | QymCAD.app, Claude Desktop computer use | the Draft edit panel opened empty; fixed in 29ac5da and e1db954 |

## Round 1 findings, fixed in ef6f1e0

- `part_instance` during a history rollback bypassed the single-body guard, and the clone missed the source's later features (native: clone inserted at the bar, F-152). Now refused while a rollback is active; `part_copy` during a rollback is allowed (nothing silently wrong).
- A bodiless-source refusal was stricter than the app; removed. Rolling back to start with clones works. Suppressing a cloned source's base stays refused: native keeps a stale clone body with no error (F-153).
- Removing a clone: the app deletes the whole Part; the refusal now says so and points to undo. `part_copy` placement of a grounded copy is refused like `part_place`. Descriptions now state verbatim copied names and that a copy of a clone is a clone.

## GUI findings, fixed in 29ac5da and e1db954

- The app's edit panels restore faces only from id picks (`live_picks`, `face_desc_of`). Faces given as descriptions were stored as queries, so «Править» opened Draft with no faces and no neutral (an OK there would have broken the feature). `draft`, `push_face`, `shell` and face-placed `hole` now store `Ref::picks`/`Ref::one` as the app does, for ids and descriptions alike.

## Evidence

- `scripts/check.sh` on e1db954: OK, 1333 passed, 0 failed, 3 ignored.
- Live stdio run of the release binary: 56/56 PASS (draft −3° and thicken +2 against hand volumes, the ±60° and thickness refusals, a knob clone that follows `kd`, refusals inside a clone and during a rollback, a cabinet with a copied six-plate drawer on two as_built Sliders, save/reopen).
- QymCAD.app, 2026-11-09..11: files 1–3 open with Draft/Thicken nodes and values, the clone follows `kd`, both drawers drag on their own Sliders and follow `fh`. After the picks fix the Draft, push_face and shell edit panels show their faces. A server hole that is last in the tree reopens like a hole drawn in the app (marker yes, face counter only after a new feature is built). A hole on an intermediate body shows its preview only after a rebuild; its saved record is identical in form to the app's.
- Upstream issues: QymIs-Tech/QymCAD#376 (F-146), #377 (F-148), #378 (F-149), #379 (F-151), #380 (F-152), #381 (F-153); F-150 is #301.

## Known limits

- No part delete tool; a clone is removed by undo.
- `part_instance` needs a single-body source (F-146); use `part_copy` for multi-body Parts.
- The app's hole edit panel shows the face counter only after a feature is built in the session; not investigated further.
