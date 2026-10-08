# ADR 0015: Tight approximate reporting bounds from independent tessellation

- Status: accepted (2026-10-08)

## Context
Phase-4 acceptance found that the shelf's stored B-rep reports x ±135.236 for true x ±134, suggesting
it exceeds a 270 mm printer bed. Its outer rim tori are bounded by coarse OCCT enclosures after open:
saved B-reps omit triangulations, and native `BRepBndLib::Add` chooses different algorithms for unmeshed
and meshed shapes (F-066). The pinned Rust kernel exposes no `AddOptimal` getter. This supersedes the
bbox-padding policy in ADR 0011 without altering the modelling kernel or pinned dependencies.

## Decision
For result bodies and stored/rebuilt metric comparisons, bound the vertices of a fresh merged tessellation
of an independently serialized/deserialized B-rep. Request nominal deflection max(0.005 mm, 1e-5 of body
diagonal), retaining .005 mm for ordinary parts up to 500 mm diagonal and controlling curved-mesh growth
on large imports. Mesh vertices retain native f32 coordinate rounding. Require complete native-face coverage
and finite vertices; if measurement fails, retain conservative native padded bounds.

Use a separate shape because native tessellation cleans/replaces triangulations even through `&Shape`.
Leave display meshes, recipes, live shape triangulations and exact volume integrals untouched. doc_info,
doc_open, render captions and undo share result-body reporting. Explain nominal accuracy, size dependence,
possible slight under-bounding of curve extrema and the native fallback in the tool description.

## Consequences
Open/rebuild/save reporting is consistent for the acceptance shelf; tight measurements no longer inherit
torus enclosure inflation. This costs serialization and tessellation on reads. Mesh deflection is an
engineering approximation, not certified containment: native rescue meshing can disable surface-deflection
control. Formula-derived sphere and shelf tests check ordinary and large-part accuracy. An independently
loaded native bbox and exact B-rep bytes verify observer isolation; copy-removal mutation fails that guard.
The existing test helper that collected result bodies while holding kernel_gate now collects them first,
avoiding nested lock acquisition.

Evidence: F-066; `golden_acceptance.rs`, `session::tests::reporting_sphere_bounds_follow_nominal_accuracy_at_part_and_large_import_scales`,
and `backlog_selection::opened_shelf_reports_tight_bounds_in_both_fields_and_render_caption`.
