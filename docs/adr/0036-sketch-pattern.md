# ADR 0036: Linked sketch patterns

- Status: accepted (2026-10-31).
- Related: ADR 0008, ADR 0020, ADR 0030, ADR 0035; F-033, F-074, F-120.

## Context

QymCAD has a native `SketchPattern` recipe whose source curves remain linked and whose instances are recreated when the pattern values change. The native item values are formula-bearing under the pattern id.

## Decisions

1. Expose `sketch_pattern` for create and update, with linear and circular forms. Create requires non-empty source curve ids. Update keeps the stored source and refuses a different `entities` list.
2. Store formulas under the native keys `count`, `dx`, `dy`, `count2`, `dx2`, `dy2`, and `angle` through `write_feat_formula`; omitted linear second-direction values are `count2=1`, `dx2=dy2=0`. Centre is an evaluated placement input, not a formula field.
3. Counts are strict integral values from 2 through 1000; total copies are at most 1000. Guards run before native work and through the existing rebuild, parameter, dimension and open checks. Refusals restore the exact archive.
4. Preserve native circular spacing: a full circle uses `total/count`; a sector uses `total/max(count,2)`.
5. Pattern instances are native derived entities. Editing an instance through `sketch_edit` or `dimension_set` is refused with the owning pattern id; source edits rebuild the linked instances.
6. A feature consuming a patterned sketch is rebound after every pattern regeneration. Explicit picks persist their pattern instance indices in feature dimensions, so full picks grow and shrink with the pattern while subset picks retain their indices; a removed subset index refuses atomically. Extrude, Combine, Revolve and Sweep profile consumers use this policy. The native app path remains characterized as stale after count edits (F-123); the engine repairs it.
7. Results report the pattern, source, created and removed instance ids, evaluated values, formulas, names, contours and rebuilt bodies. History, batch and undo use the existing registry boundaries.

## Consequences

The faithful app path still rebuilds the sketch instances but leaves a profile consumer stale on the source contour after the pattern replaces instances; the characterization test records the observed source-only volume. The engine repairs this known native scheduling gap by rebinding through native contour provenance.

Linked sketch arrays are editable and persist through QymCAD save/load and app-style parameter edits. Reference-measurement formulas follow the engine's settled dependency policy; unsupported non-convergence remains an atomic rebuild error.

Evidence: pinned native source and `tasks/research-medB.md` S9; implementation and acceptance evidence are recorded in `tasks/B2-report.md`.

## Addendum (B4c): cap before the native parameter pass

`Project::eval_parameters` imposes item values, so it regenerates every pattern instance from the new count at
once; a `param_set` raising a pattern count to 100000 hung the server before any later cap check ran (observed
through MCP, release build of d8135f5). Every engine call of the native evaluation goes through
`Session::eval_parameters_capped`: pattern count formulas are pinned to their current counts for a first pass,
the new parameter values are checked against the cap, and only then the native pass runs. Test:
`session::b0_tests::param_set_refuses_sketch_pattern_count_over_cap_before_instances`.

## Addendum (F5): settle instances after source solves

After every successful modelling call that solves a sketch, all its pattern instances must equal the final
solved source under the evaluated pattern transform, before dependent bodies rebuild and before returning.
Identical ordered single calls and `batch` calls produce identical geometry; no repair call is required.

Use one explicit guarded caller-level settlement step in the engine sketch infrastructure. It skips sketches
without patterns and refreshes only sketches actually solved by the call, through `refresh_sketch_patterns`.
Keep its pre-allocation count cap, contour provenance and consumer rebind. Dirty the full dependency closure
before and after refresh, including projected sketches and their consuming bodies. Keep independent B-rep
copies, the existing error restoration path, and one undo boundary per tool or batch. A failed batch restores
every archive byte except the monotonic `next_id` allocator (ADR 0020/F-056): refresh allocates replacement
entity ids, and keeping them past a rollback avoids any aliasing with native state keyed by id.

Profile provenance uses the native whole-figure copy membership, including carried construction curves
(F-122), while contour boundaries contain only real curves. Persist each picked contour's copy index,
owning pattern and source-curve positions in the existing feature dimensions. This preserves disconnected
source components and prefix subsets without turning them into full-pattern picks. Legacy picks infer
provenance from their old boundaries; an ambiguous legacy boundary is refused atomically.

The entry points are parameter propagation (including named-source propagation), dimension edits after their
final solve, structural sketch edits (additions sharing source points, constraints and constraint removal),
native sketch-edit operations and projection edits. The shared step is never called from `solve_settled`
or `finish_sketch_edit`: refresh itself uses `finish_sketch_edit`, so doing so would recurse.

Chained patterns are refused: a pattern source cannot contain another pattern's instance entities. Creation
checks before native allocation; settlement also checks existing/loaded recipes, including missing source
entities. A refusal restores the pre-call recipe and geometry. Independent patterns in one sketch remain
supported.

A reached source with competing native item formulas and fully driving sketch dimensions (F-081) is
refused when its evaluated item input differs from its solved geometry. This prevents a later native
evaluation from reimposing a different source after copies were made. The check is limited to the solved
pattern sources and uses the existing transaction rollback; it does not add support for competing drivers.

`doc_open` does not silently repair saved stale instance geometry. The next modelling call that solves that
sketch repairs it. The pinned app can leave instances stale after a source-only parameter edit (F-137), even
when it loads an engine-corrected file. This limitation is separate from F-123's stale consuming contour ids.

Evidence: `golden_pattern_source`, real stdio `f5_pattern_source`, and executed logs/report under
`tasks/F5-logs/` and `tasks/F5-report.md`.

## Addendum (package F review): projected pattern sources

A projected curve moves during the rebuild, after every solve of its sketch. `Session::rebuild_for_edit` compares the source points of patterned sketches holding a projection before and after each rebuild; when they moved it settles those patterns and rebuilds again, bounded by the sketch count and refused if it does not settle. Standalone and open rebuilds do not do this: `doc_open` still does not repair a stale file.
