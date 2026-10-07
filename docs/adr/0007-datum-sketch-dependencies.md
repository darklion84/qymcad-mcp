# ADR 0007: Persist datum dependencies on sketch nodes

- Status: accepted (2026-10-07)

## Context
The pinned QymCAD scheduler misses sketch→datum and child-datum→parent-datum dependencies. Parallel solid
preparation can capture an old sketch frame before resolving its offset plane. A top pocket becomes a sealed
internal void after a thickness edit, without a regeneration error (FINDINGS F-017).

The engine already creates offset planes exactly as QymCAD.app does: an OffsetBase/OffsetPlane definition and
`feat_dims[plane]["dist"]`. The native sketch placement stores only a datum id; it has no expression/dependency
field that can repair the scheduler. Changing only server regeneration would leave saved files unsafe to edit
in the pinned GUI. Vendor sources are read-only and the release remains pinned.

## Decision
Retain native offset-plane storage. Copy each parameterized ancestor's distance expression into the dependent
sketch node's native `feat_dims`, with a reserved `datum_dist_<plane id>` key. This is a scheduling dependency:
the expression does not alter sketch geometry. QymCAD's generic parameter marking scans every dimension key,
so both the server and GUI dirty the sketch before preparing its consumers. The pending sketch declaration
blocks preparation until its timeline position, after its datum chain has resolved.

Reconcile these entries when creating a sketch and opening a document. Add/update missing expressions, remove
obsolete reserved entries, preserve unrelated dimensions, and dirty only changed sketches. Ancestor traversal
is bounded by the plane count. Saving uses the existing native file format; no schema or QymCAD upgrade is needed.

Rejected alternatives:
- Moving the plane expression: its current storage already matches the app and evaluates correctly.
- Pre-resolving planes or disabling parallel regeneration in the server: the GUI would retain the defect.
- Adding `0*(distance)` to a sketch dimension or adding dummy geometry: changes user-facing sketch definitions.
- Accepting a larger volume tolerance: hides a positional error and an internal void.

## Consequences
Saved server documents support native GUI parameter edits for direct and chained datum sketches, even with
literal sketch dimensions. Legacy documents receive the dependencies when opened through the server. Removing
or replacing dependencies on reopen prevents stale guards from retaining obsolete parameter references.

The upstream GUI still cannot maintain these reserved entries after structural edits such as reattaching a
sketch or changing the distance expression to a different parameter. Reopen/save through the server before
subsequent GUI parameter edits in that case. App-created new datum sketches do not gain this workaround
automatically. An upstream scheduler correction should replace this workaround on a future verified upgrade.

Evidence: `golden_plate.rs` checks open-pocket floor/opening/topology and formula volume; `golden_datum.rs`
checks caps of an otherwise unchanged literal extrusion. Both cover server, GUI, and reopened parameter edits;
legacy/open-refresh tests cover persisted dependency repair. Disabling datum tracking turns these tests red.
