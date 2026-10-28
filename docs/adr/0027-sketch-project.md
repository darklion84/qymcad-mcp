# ADR 0027: Associative sketch projection through MCP

- Status: implemented (2026-10-28, cover stage 3).
- Related: ADR 0025, F-107, F-109.

## Context

Pinned QymCAD can project a persistent body edge or a complete face outline into a sketch. The API preserves associative, driven geometry and allows dimensions and constraints to reference it, but the server had no projection tool or inspection state. Native projection disappearance distinguishes a missing edge on live geometry (detached) from missing source geometry (unknown).

## Decision

- Expose `sketch_project` with exactly one edge selection or face selection. Resolve selectors to unique persistent ids before editing; face mode requires the sketch host face to match the selected face and its live body, following the app's `live_body(host)` lookup.
- Default the body to the latest result body, accept body/sketch names or ids, and return projection records plus all projected curve and point ids with geometry for immediate constraints.
- Treat duplicate body/source projections as idempotent. Requests return the selected projection records and their geometry, including existing ids in mixed existing/new requests.
- Make projection, solve and dependent rebuild one rollback-safe engine edit. Refuse absent/stale/ambiguous topology, empty projection, unsolved sketch or rebuild errors.
- Refuse source bodies owned by another part (`project only bodies of the sketch's own part`): native `projected_curves` reads the source's local shape and projects it into the target sketch frame without applying component placement. Refuse sources whose producing feature is at or below the sketch (`body must be built above the sketch`): regeneration resolves projections once in timeline order, so later source changes would leave projected geometry stale. `Session::open` applies this order guard to stored projections, preventing an external reorder/edit from silently introducing a forward dependency.
- Accept stored cross-part projections on open, preserving the app's local-coordinate behavior. Do not transform their geometry or refuse these files. Every cross-part record produces a rebuild warning identifying the sketch, projection, source body and both owner parts: "projected in the source part's local coordinates; its placement is ignored, as in the QymCAD app". The same owner-comparison helper serves creation refusal, warnings and inspection. Warnings are derived from current records, so later rebuilds retain them and deletion removes them.
- Mark associated cross-part entities with `cross_part: true` in `sketch_info`, independently of attached/unknown source state; ordinary and same-part entities omit the flag.
- Mark attached entities as driven in `sketch_info`; report attached, detached or unknown with projection id, source and body. Detached geometry remains inspectable and constrainable; unknown retains the association. Provenance of released entities is session-only (retained by rollback and undo); a file already saved without a projection record reopens as ordinary geometry, matching the app.
- Include `sketch_project` in batch and normal modelling history; batch owns the one enclosing history snapshot.

The exposed tools cannot reorder timeline nodes, reassign existing feature inputs, re-host sketches or retarget projections. Dimension edits change numeric values; deletion refuses projection sources; undo restores complete snapshots; batch uses the same handlers. External structural edits enter through `doc_open`: stored forward references are refused, while stored cross-part projections are accepted with local-coordinate warnings and inspection flags. See the source audit in `tasks/fix1-report.md`.

## Consequences

Agents can constrain projected entities without a follow-up topology lookup. Edge disappearance does not silently report a live association; source deletion remains prohibited by ADR 0025. Golden and protocol tests cover geometry, selection, idempotence, driven inspection, batch and server/app-path parameter movement. Evidence: `tasks/cover-3-evidence.md`. The reviewer's placed-import fixture, constructed with public `Project::add_sketch_projection`, is covered by `golden_review_r2`: four per-record warnings, cross-part inspection flags, unchanged local X=-5..5 and a 200 mm³ extrusion. MCP stdio and independent mutation evidence: `tasks/fix2-report.md`.
