# ADR 0050: Checked native feature reorder

- Status: accepted (2026-11-06).
- Related: ADR 0008, 0020, 0036, 0041, 0046, 0047; F-024, F-136, F-137, F-140, F-141, F-142.
- Contract: `tasks/research-medT.md` §4, with the implementation task's explicit overrides.

## Decisions

`feature_reorder {feature, direction:"up"|"down"}` moves an exact body-producing timeline node across the nearest body-producing node with the same parent. Names use the normal resolver. Consumed, suppressed and rollback-tail nodes remain addressable. Sketches/datums are skipped as neighbours but remain in the timeline. Secondary output ids are not normalized to their producer. Targets and neighbours must each produce one B-rep output; mesh-born and multi-output movers are refused.

Use the app's native call forms: Up raises the selected node; Down raises its next neighbour. First rewrite a source-byte-free candidate through `Project::reorder_feature`. Validate the rewritten graph, including native required inputs and omitted hole-marker, projection, measured-value and datum placement edges. Driving names available through settled scope are not geometric ordering edges. A forward relation the file already has is not caused by the move and does not refuse; only a new one does (review R2). Never reorder dependencies manually or implement another chain-rewrite algorithm.

A real move uses the established whole-timeline independent B-rep copies and checked rebuild/settlement path. All native errors, lost picks, projection loss, domain failures and ambiguous/recreated multi-output identities refuse. Unrelated patterns/projections and stable downstream pieces are supported. A move that would release any live projection refuses, not only one of pattern geometry (review R1). Reordering that changes a live projected spline's geometry refuses with the F-141 recovery instruction. That is decided by comparing the rebuilt curve with the original, because the source id can change with identical geometry (an identity Move) and can stay the same with different geometry (an instance of a reordered part) (review R3, R4). No automatic re-pick or flattening is introduced.

Keep the exact numeric rollback field. Crossing it can change which recipe nodes are active and remove body records. Report actual lost non-default names/visibility as `lost_body_metadata`; native resume recreates defaults. Suppression stays attached to its node. Results contain the requested feature, direction, changed flag, diagnostic indexes, complete node order, changed native inputs, changed face hosts, canonical rollback state, metadata loss and current rebuild bodies/diagnostics.

Failures restore the exact Project, original shape handles/absence and diagnostics through the existing transaction boundary, without a repair rebuild. A direct engine real move owns one labelled undo entry; MCP and batch reuse their outer boundary. Boundary no-ops do no regeneration and add no standalone entry. Semantic recipe change, not native bool, determines changed. Batches retain the established one-entry policy even if all steps are no-ops; a later no-op never cancels a real edit. Batch validates arguments/direction before beginning and returns compact reorder action metadata.

A selection stored as a query stays a query. After a move it is evaluated on the new source, so a hole picked with `facing +z` and moved after an array refuses, because both tops match, while the same hole picked by id succeeds. We do not convert the query to an id or repick silently. Refusal is atomic; the native message says the face went stale.

## Persistence and limits

Save the native ordered nodes and remapped references, with normal dependency reconciliation, numeric rollback and suppression. No format fields, pin change or server-only timeline are added. Native saved-file forced rebuilding and parameter scheduling are tested separately from Session rebuilding. Native F-137 pattern source-only scheduling and F-141 general projection limitations remain; reorder does not fix the app.

Excluded: arbitrary index/before/after placement, sketch/datum targets, cross-parent moves, mesh-born/multi-output movers, general sketch-based Cut/Add chain rewriting, dependency relocation, forced unsuppression and silent pick repair. Visible-app verification is unavailable in this stage. Executed acceptance and mutation evidence, including any remaining limitations, belongs in `tasks/T-report.md`.
