# ADR 0009: Advisory shell and stored-geometry diagnostics

- Status: accepted (2026-10-07)

## Context
A successful cut can create a sealed internal void without a regeneration error. Opening stale B-reps can
silently rebuild into different geometry. Both need diagnostics while intentional hollow models remain valid.
F-016 bounds include edge-tolerance padding; exact volume is the size check.

## Decision
After successful rebuilt bodies receive their faces, compare native `Shape::shell_count` with the previous
body and its consumed source. Warn, naming the node, when a result gains multiple shells, or creates extra
shells relative to its source. The native count includes disconnected solids as well as inner shells, so the
message states both possibilities. Do not turn this into an error. Rebuilding a cavity keeps its warning;
features inheriting an unchanged cavity do not add duplicate warnings at every node.

On open, retain each stored live body's volume/bbox before regeneration and compare afterward. Use 0.05 mm
per bound (F-016) and a separate volume tolerance `max(1e-6 mm³, 1e-9 * max(abs(old),abs(new)))` for numerical
roundoff. Converting bbox padding into a volume allowance hides small internal cavities and is rejected.
Warn with the body id/name, before/after volume and bounds; unchanged rebuilt geometry stays silent.

Keep these warnings in the Session and expose them in rebuild reports and `doc_info`. Rollback and undo
restore the diagnostics with the Project/shapes. They are session advisories, not serialized recipe data;
a clean reopen with no rebuild does not reconstruct an earlier cavity warning.

## Consequences
Shell warnings are deliberately conservative: multiple disconnected solids can also trigger one. Volume
and bbox checks detect metric changes, not different topology with identical metrics. Evidence is in
`usability_warnings`, including unchanged dirty boolean rebuilds, same-bbox stale pockets and failed-edit
restoration. This changes diagnostics, not geometry or the native document format.
