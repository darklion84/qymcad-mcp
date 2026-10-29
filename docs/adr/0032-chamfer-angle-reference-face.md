# ADR 0032: Native chamfer Leg and angle with support-face selection

- Status: accepted (2026-10-29).
- Related: ADR 0024, ADR 0031, F-024, F-036, F-064, F-074, F-103, F-104, F-114, F-115.

## Context

Pinned QymCAD exposes symmetric, TwoDist and DistAngle chamfers through `Project::add_chamfer_ex`. DistAngle stores degrees in native `d2`; native 181 degrees aliases 1 degree without an error. Native support-face selection can silently fall back to its automatic side when the chosen face is not adjacent to an edge.

## Decisions

1. Extend `chamfer` with optional `angle:Num`, `reference_face:Sel` and `flip:bool=false`. `angle` and `d2` are exclusive. Neither means symmetric; `d2` selects TwoDist; `angle` selects DistAngle. Return `{body,rebuild,chamfer:{mode,reference_face,flip}}`, using null for automatic reference and native persistent face id otherwise.
2. Use public `add_chamfer_ex` and the app's `store_cmd_exprs` equivalent, `write_feat_formula(..., PlainNumber::Stands)`. Store angle under native `d2`, never an invented angle key. Preserve full precision and formulas. Require positive finite `dist` and TwoDist `d2`, and strict `0 < angle < 90` degrees at creation and every evaluated edit/rebuild/open. The interval excludes aliases; it does not guarantee a finite solid can accept every angle in it.
3. Resolve an explicit reference selection to exactly one source face, adjacent to every selected non-seam edge. Asymmetric edges need two distinct adjacent faces. Refuse a reference face or true flip in symmetric mode. Preserve native automatic side/flip when no reference is supplied; a picked support face takes native precedence over flip.
4. Persist picked edges and reference face ids. Recheck support existence and adjacency against rebuilt source topology. Refuse upstream edits atomically when that association is lost, restoring Project, original B-reps and diagnostics. Do not silently adopt a different support side. Tangent expansion beyond explicit picks remains native (F-064).
5. Make `dimension_set d2` branch-aware: mm for TwoDist, degrees for DistAngle, refused even for naming in symmetric mode. Report chamfer dimension `mode` and `unit` (`mm` or `deg`) in edited and named dimension metadata. Preserve batch/undo, F-024 and F-074; do not apply the fillet overlap validator to valid intersecting chamfer planes.

## Consequences

Face areas and centroids distinguish support sides where volumes coincide. The oblique golden uses interior angle 135 degrees, leg a=2, angle theta=30 degrees: the remaining triangle angle is 15 degrees, b=a sin(theta)/sin(15 degrees), section area=ab sin(135 degrees)/2=1+sqrt(3). For edge length 10 the independent expected volume is 11875−10(1+sqrt(3)). The rectangular right-angle formula is not used for that case.

Saved files retain native global/named dimension scheduling. Server input/association guards do not change what the QymCAD app itself accepts. Evidence: stage-0 `research-medA/extrude-chamfer` probes and `tasks/research-medA.md` section 2; `crates/engine/tests/golden_chamfer_angle.rs`, `golden_a2_dimensions.rs`, `crates/mcp/tests/stage_a2.rs`; `tasks/A2-report.md`.
