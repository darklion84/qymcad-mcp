# ADR 0025: Include sketch projections in engine dependencies and refuse source deletion

- Status: implemented (2026-10-28, cover stage 1b).

## Context
Pinned QymCAD v0.1.1-dev.20261026 follows sketch projections during regeneration even when native
`dependents` and `regen_plan` omit the source-body dependency. Parameter, sketch and dimension edits
therefore rebuilt a cut in independent stock outside the retained shape snapshot. A later failure restored
the Project but left changed B-reps. The same omission bypassed F-024's stored-query preflight.

Native projection disappearance has two outcomes: a missing edge on a live source releases the association
and retains sketch geometry/constraints; missing source geometry is `Unknown` and retains the association,
even after source-feature deletion. Tests in `golden_projections.rs` characterize both paths (F-107).

## Decision
- Extend native reachability in one engine-only helper with source-body → projecting-sketch edges.
  Traverse to a fixed point, including native outputs/consumers and projected sources' consuming lineage
  above the sketch, matching native `body_seen_from`.
- Use that closure for parameter, dimension and sketch snapshot/retry plans, deletion reachability,
  and both planned-query and unnamed-ancestor checks in F-024. Keep native regeneration and upstream unchanged.
- Refuse deleting any source reached by a deletion, including `cascade=true`. Identify projecting sketches
  in the error, before any mutation. The caller can explicitly delete those sketches first and retry.

## Alternatives considered
- Report automatic detachment: native absent-body behavior does not reliably detach, so claiming that would
  misreport the result. Implementing explicit detach and a new report contract belongs with projection tools.
- Cascade projected sketches away: this would remove geometry and dependent features merely to remove a source.
  Require explicit sketch deletion instead.
- Snapshot every body on every edit: avoids the symptom but leaves dependency planning and F-024 incomplete.

## Consequences
Successful native projection following remains unchanged; failure rollback preserves serialized Project
and every B-rep byte-for-byte. Deleting a projection source is stricter than native deletion, with an actionable
error. The guard also protects imported/native projections before a server projection tool exists.
Evidence and independent mutations: `tasks/cover-1-evidence.md`.
