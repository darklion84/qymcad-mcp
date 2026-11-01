# ADR 0038: Native RemoveFace and focused re-pick

- Status: accepted (2026-11-01).
- Related: ADR 0008, ADR 0019, ADR 0020, ADR 0021; F-008, F-010, F-023.

## Context

QymCAD's RemoveFace operation heals an existing body after deleting a selected set of faces. The native operation takes `FaceKey` witnesses and reports a native healing failure even when the failed timeline node exposes a pass-through shape. The app edits an existing RemoveFace node by replacing its complete face set.

## Decisions

1. Expose `remove_face` as a modelling tool. `body` defaults to the current body, and `faces` uses the existing face selection syntax. All selections resolve against one body before mutation; empty and foreign/stale selections are refused.
2. Convert every selected face to the native `FaceKey` containing persistent id, centroid and normal. Positional indices are never used as the stored identity.
3. Treat any native rebuild error as failure, including `RemoveFacesFailed`. Return the native reason and selected ids; `Session::atomic` restores the exact recipe and live B-reps.
4. Support focused T2 re-pick through the same tool with `feature`. It replaces the entire persisted face set on the existing `RemoveFace` node, preserves the node id, rebuilds downstream nodes, and uses the same rollback path. Empty sets are refused. Other feature re-picks remain outside this stage.
5. Results expose feature/body id and name, selected face ids, rebuild bodies with volume/bbox, and diagnostics. Topology loss or ambiguity remains a named rebuild error; no nearest-face substitution is allowed.

## Consequences

Native persistent face names and witnesses survive the tested save/reopen and app-style parameter edit path. A blind-hole wall-only heal remains an error with the native reason `cad-neighbours-not-extendable`, even though QymCAD supplied a pass-through shape. The implementation is batchable and undoable through the existing registry boundary. P12 is covered; T2 remains partial because only RemoveFace re-pick is implemented.

Evidence: `crates/engine/tests/golden_remove_face.rs`, the real-kernel probe in `tasks/research-medC-probes.md`, and the generated `remove_face` schema in `docs/TOOLS.md`.
