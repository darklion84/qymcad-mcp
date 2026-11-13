# Package J review log (0.1.1-dev.20261026.15 + primitives)

Subject: `primitive` (ADR 0056), branch medium-j on top of 0.1.1-dev.20261026.15. Principle: every tool is one existing QymCAD app action; only a silently wrong native result is refused, nothing is worked around, and a refusal comes with a FINDINGS entry.

| Round | Subject | Verdict |
|---|---|---|
| Phase 0 | Native probes `j0_probes` (core API, pin f25f27d), results in ADR 0056 "Probes" | done, one reconnaissance expectation disproved (probe 8) |
| Implementation | `1bece3e` (engine `primitive.rs`, MCP tool, tests, `docs/TOOLS.md`) and the docs commit | built |
| Round 1 | three independent reviews of `80bf6bc` (Codex, Opus, Sonnet; `tasks/review-*.md`), 14 decisions in `tasks/FIXES.md` | fixed, see "Round 1" |

## What the probes changed against the reconnaissance

| Expectation | Observed | Done |
|---|---|---|
| A formula that stopped evaluating is taken silently as the stored number (`eval_dim`) | The rebuild reports `UnknownName` / `UnexpectedEnd`; the server's `param_delete` already refuses a used parameter | No code, tests only (probe 8). |
| The core's missing minimum / maximum gives wrong bodies | Wrong only BELOW about 1e-5 mm (a third of the volume, 2 faces, `errors: []`); exact up to 1e12 mm | The fields' minimums are the protection (F-180); the upper 1e5 and the torus gap are kept as the app's field policy, said so in the refusal text and ADR. |
| `n` out of range is a silent coercion | Confirmed: rounds half away, clamps to 3, no upper limit (F-181) | Whole 3..=64 on every edit. |
| Cone `r1 == r2` is an unknown | Visible `OpFailed(Cone)` down to 1e-7 apart (F-182) | Clear refusal at 1e-7 or less. |
| A disjoint union is a candidate for refusal | One body, two solids, correct volumes | Allowed and described. |
| New: suppressing a united primitive | Unbuilds the union; the Part's body disappears, reported in `unbuilt` (F-183) | Accepted, tested, described. |
| New: several bodies in a Part | The primitive is united with the last body only (F-184) | Accepted, tested, described. |

## Decisions that a reviewer should look at first

- The fields' limits are enforced on every edit, not only at creation, through `check_evaluated_features` (the global guard). A file with a primitive already outside the fields opens with a warning; the exemption is per size with its original value (`open_primitive_faults`), kept by undo and carried by `part_copy` (round 1; the first version exempted the whole node).
- The cone's `r1` lower bound is 0.01 at creation and 0 on edit, copied from the app's two fields (`start.rs`, `reopen.rs`); not run in the GUI.
- `place` takes the point's or the vertex's coordinates as a pure translation; the primitive's reference point is its base centre (box, cylinder, cone, prism) or its centre (sphere, torus). A placement on a plane or a face is not offered.
- The undo label and the batch `created` entry follow `feature`, not `body` (`body` is the union node for the second primitive).

## Evidence

- Targeted (observed): `j0_probes` (8), `golden_primitives` (12), the unit test in `primitive.rs`, `mcp/tests/j1_primitive` (5), `protocol`, `batch`, `docs_fresh` and `coverage_matrix` pass; `cargo fmt` and `cargo clippy --workspace --all-targets -D warnings` clean.
- `scripts/check.sh` on `007fc88`: OK, 1494 passed, 0 failed, 3 ignored (fmt, clippy `-D warnings`, `cargo test --workspace`; the sum of the `test result` lines of the run).
- Not run: QymCAD.app itself (the GUI); every app-side claim is a native core-API probe or read in source, as marked in FINDINGS. Round 1 is below.

## Round 1

Reviewers: Codex (read-only, no runs: the sandbox refused a target directory), Opus and Sonnet (ran probes against `80bf6bc`). Findings that two or three of them reported are merged.

| # | Finding (who) | Done |
|---|---|---|
| 1 | `param_set` judged `n` and sizes after the rebuild: `k` = 20000 took over 75 s and 700 MB, a billion sides would not return (Codex, Sonnet run) | `check_primitive_nodes` runs before the rebuild; the check after the rebuild stays. (Round 1 put it before propagation; that was inconsistent, see round 2: it now runs after propagation.) Test `a_huge_side_count_through_a_parameter_is_refused_before_the_rebuild` (k = 1e6, under 10 s; in practice milliseconds). |
| 2 | The exemption of a faulty file covered the whole node: `n` 2.5 to 3.5 and `dy` = 0.0001 passed (all three, Opus run) | Per `(node, size)` with its original value; an unchanged size passes, another size is judged, moving the tolerated size is allowed only into the field and drops the exemption. ADR Known limits rewritten. Test `the_exemption_of_a_file_is_per_size_and_holds_only_for_the_original_value`. |
| 3 | Undo and a rolled-back batch lost the exemption (all three, Opus and Sonnet run) | `open_node_faults` and `open_primitive_faults` are in `Snapshot` and `restore_snapshot`. Test `undo_and_a_rolled_back_batch_restore_the_exemption_of_a_file` (fails when the restore line is removed: observed). |
| 4 | `part_copy` of a Part with a tolerated size: the copy's new node ids were not exempt, every later `param_set` refused (Sonnet run) | The exemption is carried to the copied nodes by the node map of the copy. Test `a_part_copy_carries_the_exemption_to_the_copied_nodes` (an independent `param_set` passes, a new fault on the copy is refused). |
| 5 | A primitive over a prior body that is not built gives an empty result without an error (Opus run rp3/rp4) | Refused (F-185, new); probe `p14` shows the app's silent result; test `a_primitive_over_a_prior_body_that_is_not_built_is_refused_f185` (suppressed box, and the F-183 chain). F-183 reworded: the suppression is reported, the following primitive was silent, the tree claim is marked as inferred from code. |
| 6 | The limits were stricter than the field's tolerance (`0.3 - 0.2`, torus 5.1/5.099, `n` = `0.1*3*10`) and warned falsely on open (Opus run, Sonnet) | `lo - 1e-9 ..= hi + 1e-9`, `n` within 1e-9 of a whole number and then rounded; the same function judges opening, so no false warning. Tests `the_fields_tolerance_accepts_float_noise_and_does_not_warn_on_open`, `the_torus_and_prism_limits_take_the_fields_tolerance_at_creation`. |
| 7 | Torus upper bound: the field is `(major - 0.001).max(minor.lo)`, the server refused `{major 0.1005, minor 0.1}` (Codex, Opus, Sonnet) | Same bound; ADR, schema and `docs/TOOLS.md` fixed. |
| 8 | `body` of the first placed primitive is its Move, not equal to `feature` and no `joined` (all three) | Three variants documented (tool description, ADR, doc comment). Tests: engine `the_first_placed_primitive_is_its_move_and_a_point_beyond_the_rollback_bar_is_refused`, MCP `the_first_placed_primitive_is_its_move_and_survives_undo_save_and_open`. |
| 9 | The refusal text cited F-180 for the upper limits too | Lower limit: with F-180. Upper limit and torus gap: "the app's field limit" (and that the core would build it). |
| 10 | `place.vertex` accepted a vertex on a closed or smooth edge, which the app's click skips (`pick_vertex_pos`); the refusal said "datum point's vertex" (Opus run, Sonnet) | Refused with the reason (closed, or smooth); the text of the other-Part refusal no longer says "datum point's" (it is shared with `datum_point`, whose test strings do not contain it). A hidden datum point: a line in the ADR, no refusal. Test `a_vertex_the_apps_click_cannot_reach_is_refused` (a circle's rim, a cylinder's seam line). |
| 11 | `place.point` beyond the rollback bar was accepted (Opus run) | Refused: not built in the active prefix. Test in the same engine test as 8. |
| 12 | Test gaps: volume tolerance 20 on 8 mm3, no `doc_info` warning check, no undo, `part_copy`, first placed, early `n` | The tolerance is now exact (728 mm3 +/- 1e-3, the cube only touches the block); `doc_info` warnings are asserted in the open test; the missing tests are those listed above. The existing test that expected the core's own text for a zero size through `param_set` now expects the field's text (the check is earlier). |
| 13 | Nit: F-182 threshold and `CONE_EQUAL`; `primitive_faults` over suppressed or rolled-back nodes | New probe `p5b`: the core fails at a difference of 1e-7 and builds at 1.01e-7, so the threshold is 1e-7 and `CONE_EQUAL` equals it (F-182, constant's comment). `primitive_faults` left conservative and said so in the ADR (Known limits). |
| 14 | This log, COVERAGE, CHANGELOG, TOOLS | Done; `docs/TOOLS.md` regenerated. |

Not done or changed against the decisions: nothing skipped. Not run: the GUI; the Codex review made no runs, its three majors were confirmed by the Opus and Sonnet runs and are now covered by tests.

Evidence, round 1 (observed): `scripts/check.sh` on `fd451f7` (the code is that of `36c71a5`; this log was written after): OK, 1506 passed, 0 failed, 3 ignored (the sum of the `test result` lines). Targeted runs before it: `primitive::tests` (6), `golden_primitives` (16), `j0_probes` (10), `mcp/tests/j1_primitive` (6). Mutation seen: removing the restore of `open_primitive_faults` in `restore_snapshot` fails `undo_and_a_rolled_back_batch_restore_the_exemption_of_a_file`. Not run: the GUI; not mutated: the part_copy and early-check tests.

## Round 2

Reviewers: Codex (code-read only) and Opus (ran probes against `a009769`; `tasks/review2-codex.md`, `tasks/review2-opus.md`). Decisions: `tasks/FIXES2.md`, 7 points. Findings reported by both are merged.

| # | Finding (who) | Done |
|---|---|---|
| 1 | The round-1 check in `param_set` ran before `propagate_params`, on a scope that is stale for named reference dimensions: a false refusal (`dx: "diagonal - w + 0.5"`, w = 20: -9.06 against the settled 0.72) and a bypass (prism `n: "round(diagonal)-7"`, w = 2000 built 1993 sides; `n: "L"` with L measured, 1e6) (Codex M1, M2; Opus M1, run) | `check_primitive_nodes` moved to after `propagate_params` and before `with_rebuild_copies`, as `dimension_set` does; the early call is removed. ADR, CHANGELOG, this log reworded (no more "before propagation"). Tests `param_set_judges_the_sizes_on_the_settled_scope`, `a_side_count_through_a_reference_dimension_is_refused_before_the_rebuild` (both measured quantities; run against the old code: the first failed with the false refusal, the second did not return within 120 s). |
| 2 | An evaluation error of a tolerated key ended the judging of the node (`dx: "missing_name"` then `dy = 1e-6` passed) (Codex M3) | `primitive_faults` collects an unevaluable size as its own fault and judges the others (`judge` takes `Option` values). Test `an_exempt_size_that_does_not_evaluate_does_not_hide_the_nodes_other_sizes`. |
| 3 | Torus exemption keyed on `minor` only (Codex m4, Opus n1) | The upper-bound fault is keyed on `(minor, major)` with their original values, as the cone. Test `the_torus_exemption_holds_for_the_pair_minor_and_major` (a worse gap by `major` is refused). |
| 4 | F-185 refused any unbuilt prior, a mesh piece too, where the app gives a visible error (Opus m1, run) | Refused only when the prior is in `suppression_unbuilt` (suppressed or cascaded); a mesh piece passes to the app: `Body A was not built`, rolled back. Test `a_primitive_over_a_mesh_body_is_left_to_the_app` (fails on the previous code). Texts, ADR, F-185, tool description. |
| 5 | `place.vertex` was judged by the edge: the end of a smooth edge at a sharp neighbour's end is clickable in the app (Opus m2, code) | Judged by the point: allowed when any reachable (neither closed nor smooth) edge of the body ends there; else refused. Test `a_smooth_edge_end_where_a_sharp_edge_ends_is_a_vertex_the_click_gives` (a filleted box, all 8 smooth edges, both ends; fails on the previous code); the cylinder rim and seam refusals stay. |
| 6 | Nits: the F-182 threshold is from one radius pair (Opus n2); doubled `invalid request:` prefix in the `param_set` primitive refusal (Opus n3); ADR line on `open_node_faults` stale (Codex) | F-182 marks the threshold as inferred from one pair (the literal `10 - 1e-7` is 9.99999994e-8); the prefix is single (`node_refusal`, asserted in a test); ADR rewritten. The same doubling in `dimension_set` and in the point-coordinates refusal of `param_set` was left: not primitive code, no test depends on it. |
| 7 | This log, full `scripts/check.sh` | See below. |

Not done: nothing skipped. Not run: the GUI. The IGES prior (Opus h) was not given a test.

Evidence, round 2 (observed): `scripts/check.sh` on `dd59d4b`: OK, 1512 passed, 0 failed, 3 ignored (the sum of the `test result` lines). The first run on `4d8d618` stopped at clippy (`clone` on a Copy type in a new test), fixed in `dd59d4b`. Mutations seen: the previous code fails the settled-scope test (false refusal) and hangs on the side-count test (1993 sides built), and fails the vertex and mesh tests; the unit tests for points 2 and 3 were not run against the old code.
