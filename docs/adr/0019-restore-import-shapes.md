# ADR 0019: Restore missing import shapes before deciding an open needs regeneration

- Status: accepted (2026-10-11).

## Context
Imported STEP/IGES timeline bodies have no modelling recipe. They retain their original file in
`Project.sources`, while saved B-reps are optional. Native `regen_import` only tessellates an existing
live shape; without one it preserves the display mesh and returns success
(`qymcad-core/src/model/regen.rs:1805-1816`, pinned checkout `d2949f0`). Our open path previously loaded
only stored B-reps, then marked the timeline dirty if any body lacked a shape. Separate STEP and IGES
regressions reproduce an import with no live B-rep and no regeneration error. Missing, empty or unreadable
sources also lacked a warning, including documents that retained usable stored import B-reps. After
adding source validation, a dirty import's retessellation cleared the warning without repairing the source;
the same stored-B-rep regression reproduced that failure. Stored-B-rep geometry already passes
open/edit/save/undo controls (F-072).

## Decision
- Keep valid stored B-reps authoritative. After deserializing them, restore only Import bodies absent
  from the live shape map, before computing whether any body still requires regeneration.
- Group imports by embedded source and parse every source once to validate availability and solid indices,
  including imports with stored live shapes. Restore only absent shapes by the stored solid index and
  apply the stored uniform import scale. Follow the pinned app's source/solid/scale behavior
  (`qymcad/src/gui.rs:374-409`). All calls and restoration code stay in `crates/engine`.
- Implement the equivalent of `qymcad_testkit::restore_import_shapes` in a focused engine module.
  Testkit is already a runtime dependency, but its helper ignores import scale, stages each source under
  a shared source-id filename, performs native reads without taking the kernel gate, and silently ignores
  failures (`qymcad-testkit/src/lib.rs:54-84`). Use exclusively created process/counter temporary files,
  remove them after reading, and hold the native gate around parsing and scale transforms. Add no crates.
- Report missing, empty, unreadable sources, absent solid indices and failed scale transforms as advisory
  warnings naming the import body and embedded source. Distinguish a retained stored B-rep from unavailable
  exact geometry. Keep the timeline, display body and available source data intact so saving cannot discard
  them merely because a source is unavailable. Preserve import advisories across retessellation while their
  nodes remain: a live shape cannot repair its unavailable embedded original.
- Keep regeneration scheduling, dirty marking, save and history behavior unchanged. Existing rebuilds pass
  the live shape map to the kernel; the native Import branch re-tessellates a seeded shape without replacing it
  (`qymcad-kernel/src/kernel.rs:1111-1113`). No demonstrated failure justifies excluding imports from
  dirty marking or changing their history representation.

## Alternatives considered
- **Call the testkit helper directly:** simpler reuse, but its scale, temporary-file, gate and diagnostic
  limitations do not meet the restoration requirements above.
- **Restore every import from its source:** would overwrite usable saved geometry and make otherwise
  healthy documents depend on successful reparsing. Only missing shapes require restoration.
- **Force a recipe rebuild:** cannot recreate an imported shape and returns misleading success when
  only its display mesh exists.
- **Reject opening when a source is unavailable:** would prevent inspection and preservation of the
  remaining document. A named warning leaves the unavailable exact geometry explicit.

## Consequences
Older or interrupted saves that omit import B-reps can regain exact STEP/IGES bodies from embedded
originals before regeneration planning. Source failures remain visible across retessellation while document
data survives save. Documents with valid stored B-reps retain their exact geometry, even when validating
their originals fails. Validating sources introduces one native parse per embedded import source on open;
this cost supplies the required unavailable-original warning even when a stored B-rep remains usable.
Source parsing uses temporary files because the pinned exact-reader API accepts paths; these files are
isolated between sessions and removed after each read. F-072 and `crates/engine/tests/imports.rs` record
the failing-before/passing-after cases and stored-geometry controls. Mutation and full-workspace check
results are recorded in `tasks/review-imports.md`: all 11 import tests pass, five independent mutations
fail their regression assertions, and `scripts/check.sh` is green.
