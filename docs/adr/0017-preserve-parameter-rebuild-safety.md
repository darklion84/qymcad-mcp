# ADR 0017: Optimize parameter solves without narrowing rebuild safety

- Status: accepted (2026-10-09)

## Context
J6 reduced both sketch solves and feature rebuild dirtying to changed-expression reachability. The pinned
scheduler omits datum sketch inputs (F-017). An opened native document can contain OffsetFace datums whose
constant distance and hosted sketch dimensions never mention the parameter moving the source feature.
The consumer then keeps stale geometry. Restoring blanket feature dirtying alone can still capture the old
frame during parallel preparation when the sketch has no expression and therefore was never solved/dirty.

Named reference dimensions also change after solving, through native update_driven_dims. A single snapshot
before solving misses downstream sketches reached by these measurements.

## Decision
Retain native mark_param_dependents_dirty for every parameter edit, independently of solve reachability.
Dirty sketches on face-derived datums (including OffsetPlane chains ending at OffsetFace) as scheduling
barriers, without solving them. Bound ancestry traversal by the number of planes. This supplements ADR 0007;
it does not persist synthetic face-source parameter expressions or change native recipes.

Keep explicit solves targeted. Stabilize named driving expressions, solve sketches reached by changed scope
values, and compare the scope again after solving to propagate newly measured reference names. Bound solve
rounds to the number of sketches plus one, refusing and rolling back a non-stabilizing reference cycle.
Honor stabilized-scope evaluation errors and recheck array limits after propagation before rebuilding.

## Consequences
Geometry safety takes priority over J6's measured rebuild speedup. Four-to-zero explicit shelf solves remains,
but corrected medians ~249–256 ms versus ~253 ms with pre-J6 propagation establish no reliable total speedup.
The native GUI still leaves constant sketches on face-derived datums stale; this engine barrier cannot fix
its running scheduler. Named feature dimensions depending on named dimensions retain native scope limits
(F-071). Non-kind composed face queries stay dynamic under ADR 0013.

Evidence: golden_param_propagation checks cap positions and reference-driven prism volumes, including native
GUI characterization. Separate blanket/barrier/follow-up mutation tests fail their formula assertions.
Unit tests cover three reference hops in reverse sketch pool order and exact rollback of a reference cycle.
