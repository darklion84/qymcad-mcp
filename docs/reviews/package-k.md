# Package K review log (0.1.1-dev.20261026.16 + split faces)

Subject: `split_face` (ADR 0057), branch medium-k on top of 0.1.1-dev.20261026.16. Principle: every tool is one existing QymCAD app action; only a silently wrong native result is refused, nothing is worked around, and a refusal comes with a FINDINGS entry.

| Round | Subject | Verdict |
|---|---|---|
| Phase 0 | Native probes `k0_probes` (core API, pin f25f27d), results in ADR 0057 "Probes" | done; one probe added in the implementation (14c) |
| Implementation | `2c3515f` (one plane resolver), `311e82e` (engine), `bcda602` (MCP tool, tests, `docs/TOOLS.md`) and the docs commit | built, not reviewed |
| Round 1 | not started | - |

## What the implementation found against the probes

| Expectation | Observed | Done |
|---|---|---|
| Repeated ids come from a second split BELOW the first (probe 14, 14b) | Every CROSSING pair repeats ids as well (XY then XZ or YZ and back: 16 faces, 14 distinct ids); the clean order of parallel planes follows the axis (along x the other way) | Probe 14c; F-189 widened; the tool description and the refusal text name both causes. A cross cut of one body cannot be made with this tool. |
| The bbox overlap of a cut id is judged with 1e-6 | The mesh holds f32 coordinates (F-063); 1e-6 is below their noise at coordinates of tens of mm, and a touching pair could pass | Tolerance `max(1e-4, 1e-6 * scale)`; a flat axis is not compared. |
| An assembly root as the active context is refused | `default_body` takes the only body of another Part; a named body works for `split` | Refused only when no `body` is named. |
| The plane's offset is the same at 1e-5 above the bottom face | Splits normally (the silent failure is only 1e-6 over the bottom; 1e-6..1e-5 under the top) | Asserted in `golden_split_face`; F-187 states it. |
| An F-185-style check for an unbuilt source is reachable | `source_body` already refuses an unbuilt default body; an explicit unbuilt body reached only `is not built in the active document` | The suppressed-chain check runs first on an explicit body and says why. |
| A plane following a face that moves with its body is safe from F-188 | A piece that moves by more than its own extent has no overlap (box 5 mm tall, plane 3 -> 6) and is refused | Accepted as the conservative rule and stated in F-188 and the ADR. |
| `k0_probes::p19` opens every saved case | The file with repeated ids is now refused on the full rebuild that reading topology needs (it stores no faces) | The probe asserts the refusal for that case. |

## Decisions that a reviewer should look at first

- The checks hang on the existing `split_identity_refusals` places: `rebuild_passes` and `with_rebuild_copies`. They skip a node the app itself failed (any error mark except `Kernel`, which is what the server's own refusals write).
- A before-snapshot exists only for a node whose result already had more faces than its source (a suppressed or never built node is a pass-through and is not compared), so suppress and unsuppress are never refused by F-188.
- F-188 compares ids and boxes of the cut faces only (faces missing from the source or whose area differs); an upstream edit that adds faces on the cut walls is refused as well.
- The pre-check `ask::splits_faces` is asked in the body's local frame, like `split`.

## Evidence

- Targeted (observed): `golden_split_face` (20), `split_face::tests`, `k0_probes` (28), `mcp/tests/k1_split_face` (6), `print_split`, `golden_split_print`, `review_r3`; `cargo fmt` and `cargo clippy --workspace --all-targets -D warnings` clean.
- `scripts/check.sh`: see below.
- Not run: QymCAD.app itself (the GUI); every app-side claim is a native core-API probe or read in source, as marked in FINDINGS.
