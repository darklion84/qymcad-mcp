# ADR 0011: Report native frames and document bounding-box tolerances

- Status: accepted (2026-10-07)

## Context
Agents need the world location and axes of face sketches, meaningful extrusion operation labels, and cylinder
axis points near the queried face. QymCAD already resolves sketch frames, including datum/face hosts, shifted
origins and component ownership. OCCT bounds can include tolerance padding that changes after reopening a B-rep
(F-016); a geometry-reporting change must not introduce a different modelling or kernel pipeline.

## Decision
Expose `world_frame` in the compact sketch report and detailed `sketch_info`. Read `Project::sketch_frame` and
apply the owning component's world transform; do not reproduce native frame construction in the engine. An
unresolved native frame is reported as null so the rest of the sketch remains inspectable.

Label `Combine` sketch extrusions as `extrude (cut/add/intersect)`. Preserve the raw native label for other
node kinds. Project each cylinder face centroid
onto its existing axis to locate the reported axis point within the face's axial extent; leave cone apex
reporting unchanged.

Document OCCT bbox padding and the tighter bounds that stored B-reps can have after `doc_open`, using the
existing F-016 allowance of about 0.05 mm. A new tight-bounds kernel implementation is unnecessary for this
reporting task. Prefer volume for size checks and topology positions for placement.

## Consequences
Reporting follows native sketch host resolution and component placement. This changes output fields/labels,
not modelling geometry or saved native data. Hosted frames remain correct after server and native GUI parameter
edits; regression tests check formula-derived frame positions, boss volume, and the GUI-built cap. The cylinder
axis point lies on the axis, not on the cylindrical surface.

Evidence: `crates/engine/tests/usability_info.rs`, `crates/mcp/tests/usability_reporting.rs`, and U6 source
references in `tasks/u6-integration.md`.

BBox reporting policy superseded by [ADR 0015](0015-tight-reporting-bounds.md) after phase-4 acceptance exposed unmeshed torus enclosure inflation (F-066).
