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
