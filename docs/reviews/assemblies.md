# Package A review log (0.1.1-dev.20261026.12)

Subject: root assemblies (ADR 0051, F-143..F-147), branch medium-a on top of 0.1.1-dev.20261026.11.

| Round | Subject | Verdict |
|---|---|---|
| Acceptance | `66ab945..2208af5` (author: Codex, stopped by usage limits before its final check) | three defects found and fixed in 2208af5 |
| 1 | `66ab945..2208af5`, independent Claude reviewer (Codex was out of quota until 2026-11-12) | FIX: F1, F2, F3 medium; F4 low |

## Acceptance findings, fixed in 2208af5

- Rebuild results carried an `assembly` block in every document; it now appears only with joints, mates or relations.
- Plain single-Part documents gained placement fields on bodies, nodes and sketches; they now appear only with several Parts or a placement.
- A sketch on another Part's datum plane silently built new bodies into that Part (the live cabinet test put a drawer front into the carcass). It is refused while a Part is active; face sketches keep body-owner routing and report `owner_part`.

## Round 1 findings, fixed in a5cedf4

- **F1:** `ground` accepted grounding a Slider's moving Part or ungrounding a joint's anchor Part, which locked the drawer or made its formula slide the carcass. It now refuses both; the chained-slider refusal no longer advises grounding the moving Part.
- **F2:** a conflicting joint or drive was reported as lost anchors with advice to delete joints, and solve refusals could list only empty causes. Refusals now name the actual cause.
- **F3:** `hole.at` is documented as world coordinates but was applied Part-local; it is now transformed into the body's Part frame.
- **F4:** documentation side of F1; ADR, tool description and TOOLS.md updated.

## Evidence

- `scripts/check.sh` on a5cedf4: OK, 1289 passed, 0 failed, 3 ignored.
- The new tests for the datum refusal and for F1, F2 and F3 fail without their fixes. The reviewer disabled each acceptance change and the existing tests caught it.
- Live stdio cabinet: a grounded carcass and two drawer Parts on Sliders driven by `pa`/`pb`; each parameter moves only its drawer; numeric drive, release, undo, save, STEP export and reopen keep the formulas driving. The reviewer's repro scripts pass on a5cedf4.
- Live ownership probes with the drawer Part active: sketch hole, chamfer, thread, revolve, sweep and loft build into the source Part; import_mesh and recognise create a new Part; the active Part is unchanged.
- QymCAD.app check, 2026-11-08, run by Claude Desktop computer use: 2/2 PASS. Both files open without errors, with carcass grounded and Sliders `slide_a`/`slide_b` showing formulas `pa`=400 and `pb`=250 at DOF 0; drawers sit at Y −400 and −250; `pa` 0 moves only drawer A; in the released file the Slider handle drags drawer A along Y only (to −664), and undo restores it. Source hashes still match.

## Known limits

- Slider travel has no limits; MCP does not edit joint limits yet. In the app a drawer can be dragged out past the carcass.
- Both files open in the app marked as modified; the cause was not investigated.
- Not covered: GLB readback, a file saved by the app itself.
