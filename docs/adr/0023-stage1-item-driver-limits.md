# ADR 0023: Stage-1 native item-driver limits

- Status: accepted (2026-10-25), option 4: keep the refusal boundary.
- Related: accepted ADR 0022, findings F-080/F-081, tasks/review-features-1.md.

## Context

ADR 0022 requires native item formulas, fully defined creation geometry and app-compatible driver edits.
The pinned release cannot satisfy all three for ellipse axes/turn and polygon turn. Item imposition moves
raw points, then explicit dimension rows restore their own values during solve. Adding both formula sources
makes independently editable drivers compete. Tests prove width 16→24→16 and polygon turn 30→60→30.

App SetDriver on a named sketch item also fails to dirty its owning sketch. A literal ellipse width edit
12→16 leaves V=π·6·3·2 rather than π·8·3·2. Timeline feature and sketch-constraint targets work correctly.
Native sketch targets identify only entity-reference sets, so some x/y coordinate dimensions are ambiguous.

## Current implementation boundary

The task explicitly requires stopping impossible ADR points and reporting evidence/options while finishing
other work. Creation therefore retains fully driving ellipse axes/turn and polygon turn sketch formulas,
and exposes native polygon side-count formulas (which preserve existing point/first-side ids).

`dimension_set` refuses ambiguous sketch targets, new item names and item values discarded by driving
constraints. Sketch and timeline-feature driver naming/editing remain supported. Imported item names remain
visible, clearable and editable. Direct server item edits explicitly solve/dirty their owner, without claiming
to repair the pinned app's SetDriver scheduler. No new persisted dependency fields or upstream edits are added.

## Options awaiting a decision

1. Fix native item-aware constraint and owner scheduling behavior upstream, then adopt that app release.
2. Relax the fully defined creation requirement for item-driven ellipse/polygon geometry.
3. Approve synchronized item/dimension formulas and persisted owner scheduling dependencies; this changes
   the native recipe and requires its own save/open/app compatibility design.
4. Retain the current refusal boundary and use actual sketch dimensions/global parameter formulas.

No option is silently treated as accepted. Source locations and executable evidence are in F-081 and the
stage-1 report; the implementation leaves those parts explicitly stopped.

## Decision

Option 4. Sketch dimension formulas already give the same parametric control, and the app evaluates them, so
nothing a user can change in the app is lost. Options 1–3 either wait for an upstream release or change the
native recipe. The SetDriver scheduling defect (F-081) is a candidate upstream report, sent only with the
user's approval. Revisit when a QymCAD release fixes item-aware solving or owner scheduling.
