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
