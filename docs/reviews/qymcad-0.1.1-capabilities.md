# Review log: the QymCAD 0.1.1 capabilities (0.1.1-dev.20261026.1, ADR 0022)

Authors: stage 1 Codex (gpt-6.1-sol); stage 2 drafted by Codex, finished by Claude; stages 3–4 Claude.
Reviewers who did not write the code: MiMo (all stages), Claude (stage 1), Codex (stages 2–4).

## Round 1

### MiMo (REJECT: 1 major, 3 minor, 3 nit)

| # | Severity | Finding | Verdict | Evidence | Action |
|---|---|---|---|---|---|
| 1 | major | `sketch_edit` corner refuses a cut that leaves freedom in a fully defined sketch; a chamfer does not hold its cut point on a spline leg | confirmed | new test `a_chamfer_on_a_spline_corner_is_applied_and_reports_its_freedom` failed with "did not preserve full definition: DOF (1, 0)" | freedom is reported (ADR 0022); only added redundancy is refused |
| 2 | minor | a line–line distance appends `Parallel` but `indices` lists only the dimension | confirmed | `sketch_behaviour::a_distance_between_lines_makes_them_parallel` now asserts both rows | `indices` covers every appended row |
| 3 | minor | the polygon side-count check fails on any unevaluable formula | partly: the edit is refused anyway by the formula evaluation, but the message blamed the polygon | `golden_figures_1::an_unrelated_broken_named_formula_is_reported_as_itself_in_polygon_documents` (red on the old code) | the check skips formulas a side count does not read and names the side count when it is the one that fails |
| 4 | minor | tangent circle radius below the native 0.01 mm minimum gives a solver error | confirmed (native `r.max(0.01)`) | `golden_tangent_circle_1` refusal case | preflight refuses below 0.01 with the reason |
| 5 | nit | `dof.0 >= 0` is a tautology | confirmed | — | asserts no redundancy instead |
| 6 | nit | F-086 "the copy follows its source" overclaims for a carried size dimension | confirmed | `golden_mirror_point_2` SetDriver case | wording in FINDINGS, ADR, tool and doc comment |
| 7 | nit | ADR 0023 / F-081 say imported item names are "editable"; renames are refused | confirmed | `dimensions.rs` | wording |
| — | coverage | no functional batch/undo test for sweep and loft | agreed | — | `solids_3::sweep_and_loft_run_in_a_batch_and_undo_as_one_step` |

### Claude on stage 1 (no critical/major)
- minor: rectangle and ellipse turn formulas are stored with a `+0/sqrt(t*(180-t))` domain guard, visible in the
  app's dimension box. Kept: it turns an edit outside the supported half-plane into a visible fault instead of a
  silent mirrored solution; documented in the tool text.

After the fixes: `check.sh` OK, 529 passed.

### Codex on stages 2–4 (REJECT: 2 major, 1 minor; snapshot d8fdad9)

| # | Severity | Finding | Verdict | Evidence | Action |
|---|---|---|---|---|---|
| 1 | major | the point mirror matched sources by coordinates; coincident points of different curves (concentric centres) both tied to the first | confirmed | `golden_mirror_point_2::concentric_sources_keep_their_own_copies` failed (copy of B did not follow X) | copies are paired with their source curves in the native copy order; points paired per curve |
| 2 | major | loft ambiguity checked only top-level contours, so a section with concentric circles silently took the outer one | confirmed | `golden_sweep_loft_3::a_loft_section_with_nested_contours_must_name_one` failed | every closed contour counts; a nested one can be named (or picked with `profile_at`) |
| 3 | minor | dropped formulas matched by expression text only, so the wrong dimension was reported | confirmed | `golden_curve_edits_2::a_dropped_formula_is_the_one_that_was_removed` reported index 7 for 0 | match by kind, points and text first, then kind and text; each surviving row accounts for one |

After the fixes: `check.sh` OK, 532 passed; mutations M5/M6 still red.

## Live MCP test (Codex, black box, 2026-10-26; binary from c0fce64)

214 calls over six saved parts; 118 independent arithmetic checks pass, every save/reopen keeps its volumes, every
refusal left the document unchanged. Report: `3d_modeling/tmp/mcp-test/features-011/REPORT.md`.

| # | Severity | Finding | Verdict | Evidence | Action |
|---|---|---|---|---|---|
| D1 | P2 | a line broken at a point (or at intersections) loses its straightness: the split point has two free coordinates | confirmed; the app keeps it only for a horizontal or vertical (F-091) | `golden_curve_edits_2::the_pieces_of_a_broken_line_stay_on_the_line` failed with DOF (2, 0) | split points are put on the line's ends where independent |
| D2 | P2 | rounding a corner of an outline offset by the same distance is refused as redundant | confirmed; the native fillet centre is laid on the source corner point (F-090) | `golden_corner_2::a_corner_of_an_offset_outline_is_cut_without_redundancy` failed with DOF (0, 2) | the arc gets a centre point of its own; only rows the cut added move to it |
| D3 | P3 | a negative hole diameter through `dimension_set` says only "operation failed: Hole"; a turn of 180 shows the guard expression | confirmed | `golden_dimensions_1::a_non_positive_hole_size_is_refused_by_name`, `golden_figures_1` turn messages | named refusals |
| D4 | P3 | argument shapes hard to discover | partly: the schemas are complete but behind `$ref`, which that client did not show | — | corner example and size modes in the `sketch_edit` description |
| D5 | P3 | 3MF vertices are not shared between faces | not this branch: export is unchanged since 0.1.0 and uses QymCAD's own writer; the welded mesh is closed | — | none; candidate for a later look |

Sketch `contours[].area` is sampled, not exact (noted by the tester); unchanged, documented as approximate later if needed.

After the fixes (4bf5dcb): `check.sh` OK, 535 passed.

## Round 2

### Codex (REJECT: 4 major; snapshot b25becb)

| # | Severity | Finding | Verdict | Evidence | Action |
|---|---|---|---|---|---|
| 1 | major | the fillet-centre fix re-pointed every changed row off the shared point, including a parallel the cut had only rewritten; a later radius edit did not solve | confirmed | `golden_corner_2::a_fillet_centre_on_an_existing_point_leaves_that_points_rows_alone` failed on 4bf5dcb | the point is moved 1 µm aside during the cut and put back, so no row is re-pointed (F-090) |
| 2 | major | the mirror still paired copies against unselected curves, so a copy of B tied to A at the same centre | confirmed | `golden_mirror_point_2::a_mirrored_curve_is_tied_to_itself_not_to_an_unselected_one_in_the_same_place` failed | pairs taken from exactly the curves the native copy takes (selection plus the diagonals of whole rectangles), in sketch order |
| 3 | major | a line broken at an existing, driven point got no tie, so it bent when the driver changed | confirmed | `golden_curve_edits_2::a_line_broken_at_a_driven_point_is_refused_rather_than_bent` failed | such a break is refused unless the point is already tied to the line (F-091) |
| 4 | major | a concentric ellipse resolved to the first ellipse on its centre: wrong id, the new one undimensioned | confirmed | `golden_figures_1::a_concentric_ellipse_is_its_own_curve_and_fully_defined` failed | the new curve is the one the adder laid |

After the fixes: `check.sh` OK, 539 passed.

### MiMo (REJECT: 1 major, 2 minor; snapshot b25becb)

All round 1 and live-test fixes checked and confirmed with their tests; atomicity, formula retention, gate scopes,
batch/undo and exclusive alternatives held.

| # | Severity | Finding | Verdict | Evidence | Action |
|---|---|---|---|---|---|
| 1 | major | `dimension_set` accepts a non-positive sketch diameter/radius: it solves to a negative radius that the profile clamps to 0.01 mm | confirmed | `golden_dimensions_1::a_non_positive_sketch_diameter_is_refused_by_name` failed | sketch diameter, radius and arc length dimensions refuse values ≤ 0 by name |
| 2 | minor | the named refusal omits the hole's `dia2`/`depth2` | confirmed | the hole test now uses a counterbore and fails on the old code for `dia2`/`depth2` | added |
| 3 | minor | ADR 0022 says the loft's default `op` is `new_body`; the code defaults to `add` | confirmed | `features.rs` `Op` default | ADR corrected |

After the fixes: `check.sh` OK, 540 passed.
