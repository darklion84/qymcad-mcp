# ADR 0048: Modelling part when an assembly is active

- Status: accepted (2026-11-04).
- Related: ADR 0008, ADR 0020, ADR 0046, ADR 0047; F-139. Research: `tasks/research-medF.md` section F1.

## Context

QymCAD.app saves the navigation context in `active_component`. A user who saves while standing in the root
assembly produces a file whose active component is an assembly although all geometry lives in a part. The server
used the active component both as navigation and as modelling context: an existing part-owned sketch looked for
its implicit target body in the assembly ("there is no body to cut or intersect yet"), and a new base-plane sketch
was recorded in the assembly, where any feature on it reached a body of another component ("cross-component
input"). The app never models in an assembly: applying a feature to an existing sketch activates the sketch's
owning part first, and creating a sketch from an assembly calls native `part_to_draw_in` (F-139).

## Decisions

1. A sketch-based feature (extrude, revolve, sweep, loft by its first section) resolves its implicit body in the
   part that owns the sketch, as the app's apply does, and creates its node in that part. The user's
   `active_component` is restored afterwards; it is never rewritten by `doc_open`.
2. An explicit `target` in a different part than the sketch's part is refused before any change:
   `target body <id> belongs to part <id> '<name>'; sketch <id> belongs to part <id> '<name>'; cross-component inputs are not allowed`.
3. A new sketch on a base plane or datum: a part-owned datum supplies its part; otherwise the active part; with an
   assembly active, the document's only part. This sole-part rule is server policy (the app would create a new
   part). With several parts the call is refused before any change, listing them by id:
   `active component <id> '<name>' is an assembly; cannot choose a modelling part from <N> parts: <id> '<name>', ...; create a sketch on a body face or use a sketch owned by the intended part`.
   With no part at all, native `part_to_draw_in` creates and activates one, as the app does. A face sketch keeps
   its body's owning part. Navigation saved as `None` means the root (native `current_ctx`), exactly like `Some(root)`. Documents without components keep the previous behaviour.
4. No file-format change and no new metadata. Undo/batch boundaries are unchanged.

## Consequences and evidence

`crates/engine/tests/golden_assembly_active.rs`: the user's GUI file (`tests/data/f1-gui-assembly-active.qcad`)
cuts with an existing sketch implicitly and equals the explicit-target control; a new sketch goes to the sole part;
a minimal 10x20x5 stock with the assembly active loses exactly a 2x2x5 pocket (980 mm³), keeps its bbox and
reopens with the assembly still active; two parts refuse with an unchanged archive; a cross-part target is refused.
All five fail on the previous code. Not covered: nested assemblies and part-owned transformed datums (source only).
