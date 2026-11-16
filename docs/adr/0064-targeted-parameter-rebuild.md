# ADR 0064: Targeted parameter rebuild with derived-datum dependencies

- Status: accepted (2026-11-16). Supersedes the "blanket dirtying" half of ADR 0017; its face-datum sketch barriers stay.
- Related: ADR 0017, FINDINGS F-002, F-017, F-202. Package W.

## Context

ADR 0017 kept the native `mark_param_dependents_dirty()` on every `param_set`: every node with any formula in its
feature dimensions was rebuilt, whatever name changed. Measured in release on a 31-node plate with formula heights: an
edit of a parameter nothing reads took 134 ms, the same as an edit that reaches every node (133 ms). The app itself no
longer does this: the pinned GUI marks only what mentions the changed names (`mark_changed_params_dirty`,
qymcad-ui-state `lib.rs:426-435`, calling `mark_param_dependents_dirty_for`).

Removing the blanket call alone broke two golden tests. The native rebuild plan follows `inputs`, and `inputs` omit
what a derived datum is read off: a two-point axis's points, an edge or face axis's body, a vertex point's body, a
face-offset plane's body, an offset plane's parent; and a revolve's `axis_datum`. The blanket call had hidden this by
rebuilding everything (F-202).

## Decision

`param_set` marks only what the changed names reach (the existing `propagate_params` loop: feature dimensions, sketch
dimensions, named reference rounds), keeps the face-datum sketch barriers of ADR 0017, then marks dirty, to a fixed
point, every derived datum whose source is in the rebuild set and every revolve whose datum axis is
(`mark_derived_datums_dirty` in `params.rs`). The native plan then carries their consumers (a circular array about the
axis, a mirror or split about the plane) through `inputs`.

The rebuild set is the engine's `rebuild_nodes` (the native plan closed over projections, sketch-placed holes, named
dimension owners and native dependents), not the native `regen_plan`: the native plan misses a body extruded from a
projection of the edited body, a hole placed by an edited sketch, and the body a suppressed modifier passes through
(`regen_plan` skips suppressed nodes), although the engine rebuilds all three. A datum on such a body moved while its
array stayed about the old axis (review of the first version of this ADR). Each node is marked at most once; suppressed
nodes and nodes past the rollback bar are not built, so they are not marked.

The rule lives in `propagate_params_unsettled`, shared by `param_set`, `dimension_set` and `move_body` (a Move whose
named shift other formulas read); all three lost the blanket dirtying and all three get the rule.

## Consequences

- An edit of a parameter nothing reads: 134 ms → 11 ms on the 31-node plate; an edit that reaches every node is
  unchanged (~133 ms).
- Tests: `a_parameter_edit_plans_only_what_reads_the_changed_names` (fails with the blanket call back),
  the two golden datum-axis tests, `a_mirror_about_a_face_derived_datum_follows_the_face_on_a_parameter_edit`,
  `an_axis_through_vertex_points_follows_the_vertices_on_a_parameter_edit`; each fails without its rule.
  `an_axis_on_an_extruded_projection_follows_the_projected_block_on_a_parameter_edit`,
  `an_axis_on_a_suppressed_modifier_body_follows_its_source_on_a_parameter_edit` and
  `..._on_a_dimension_edit` fail with the native plan in place of `rebuild_nodes`.
  The revolve rule has no test: the server cannot make a revolve about a datum axis (only GUI files hold one).
- `dimension_set` and `move_body` go through the same `propagate_params_unsettled` and follow this ADR. Other edits
  (`sketch_edit`, re-picks) are unchanged by it.
