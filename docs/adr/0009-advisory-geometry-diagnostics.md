# ADR 0009: Advisory shell and stored-geometry diagnostics

- Status: accepted (2026-10-07)

## Context
A successful cut can create a sealed internal void without a regeneration error. Opening stale B-reps can
silently rebuild into different geometry. Both need diagnostics while intentional hollow models remain valid.
F-016 bounds include edge-tolerance padding; exact volume is the size check.

## Decision
After successful rebuilt bodies receive their faces, compare native `Shape::shell_count - solid_count`
(saturating at zero) with the previous body and its consumed source. Warn, naming the node, when a result
gains excess shells relative to either baseline. Each disconnected solid contributes an outer shell, so
arrays and other disconnected results with shells == solids remain silent. Do not turn this into an error. Rebuilding a cavity keeps its warning;
features inheriting an unchanged cavity do not add duplicate warnings at every node.

On open, retain each stored live body's volume/bbox and native shell/solid/face counts before regeneration
and compare afterward. Counts compare exactly (face counts when available), independently of metric changes. Use 0.05 mm
per bound (F-016) and a separate volume tolerance `max(1e-6 mm³, 1e-9 * max(abs(old),abs(new)))` for numerical
roundoff. Converting bbox padding into a volume allowance hides small internal cavities and is rejected.
Warn with the body id/name and four-decimal before/after volume. Show bounds only when a bound changes by
more than 0.05 mm, without Rust debug formatting. State that rebuilt geometry is in use and saving updates
the file. Report shell/solid/face differences explicitly, including a stored sealed-void indication when
shells exceed solids. Inspect stored bodies for excess shells even when no rebuild or metric change occurs;
a clean reopen of a sealed body therefore warns. Unchanged geometry without cavities stays silent.

Keep these warnings in the Session and expose them in rebuild reports and `doc_info`. Export repeats the
current document warning list; render includes the same warnings in its text alongside the image. Rollback and undo
restore the diagnostics with the Project/shapes. They are session advisories, not serialized recipe data;
stored-body inspection reconstructs cavity diagnostics on clean reopen.

## Consequences
Excess shells are an advisory indication of a cavity, not a geometry error. Counts also expose topology
changes with equal volume/bounds; equal metrics and counts still cannot prove identical geometry. Evidence is in
`usability_warnings`, including unchanged dirty boolean rebuilds, same-bbox stale pockets and failed-edit
restoration, legacy sealed-pocket repair (2 → 1 shells, 12 → 11 faces, delta V=0.48 mm³), stored-void checks,
face-count-only changes and disconnected-solid counts. MCP `backlog_output_history` checks export/render
warning propagation. This changes diagnostics, not geometry or the native document format.
