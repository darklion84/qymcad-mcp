# ADR 0022: The modelling capabilities new in QymCAD 0.1.1

- Status: accepted (2026-10-25). Research: stage 0, retained in the project task folder (research-features.md).

## Context
ADR 0021 ported the server to QymCAD v0.1.1-dev.20261026 without adding capabilities. The user's goal is that every modelling capability new in 0.1.1 is reachable through MCP: parametric, saved the way the app writes it, opened and rebuilt by the app's own parameter edit. 0.1.1 adds no new feature kind. What it adds lies inside existing records:
- `EntityKind::Spline`;
- ellipse arcs (`EllipseSpan`);
- `Constraint::{Orientation, PointOnEllipse, PointOnCurve, TangentCurves, CurveDistance}`;
- `Sweep.along` (body-edge paths);
- `Hole.shift`;
- formula-carrying named dimensions and item values;
- generic curve editing: trim, extend, break, offset, corner blends, point mirror and tangent circles.

Sweep and loft predate 0.1.1, but the server never exposed them; they are in scope because the release changes both.

## Decision

### Scope
In scope:
1. Splines (open or closed, through points).
2. Ellipses and ellipse arcs.
3. Rectangles as native figures, with a rotation and a centre or corner anchor.
4. Constraints and dimensions on any curve.
5. Trim, extend, break and offset.
6. Sketch corner fillets and chamfers in every native size mode.
7. Sweep along a sketch path and along body edges.
8. Loft.
9. Profile choice by a point in the region.
10. Every result body in the export, kept by regression tests.

Added from stage 0:
- editing named driving dimensions and their formulas (`dimension_set`);
- item values as formulas: polygon sides and turn, ellipse axes and turn;
- mirroring sketch curves about a point;
- a circle tangent to two curves;
- formula-driven hole shifts (`off_u`, `off_v`).

Out of scope:
- **Assemblies.** The server models parts; mates, joints and connectors are a separate project. Their new formula fields are not reachable until it exists.
- **Creating sketch text and patterns, and generic rotate/scale/copy.** These predate 0.1.1.
- **App-only features:** languages, icons, layout, graphics recovery, sketch-local undo (the server has its own undo per call), and box selection. Box selection is a gesture; a point selects the same regions.

### App equivalence
Every capability uses the public core call the app uses, and writes the records the app writes. Every size is a `Num`; a formula is stored where the app evaluates it.

A sketch made by a creation tool is fully defined: DOF (0,0), residual ≤ 1e-6. An edit tool reports the resulting DOF and the free points, and does not refuse because of them. The user may dimension the rest with `sketch_constrain`.

### Offsets of circles and arcs
For these the app writes `EdgeDistance` with an expression that `eval_parameters` never evaluates, so the offset does not follow a parameter, even in the app (verified in stage 0). The server writes the native `CurveDistance` instead: QymCAD 0.1.1 evaluates it, and the app rebuilds it. This departs from the app's recipe on purpose. It is recorded as FINDINGS F-076 and is a candidate for an upstream report (sent only with the user's yes).

### Trim, extend and break
These are one-time edits, as in the app. Where a new end point lies on the cutting or boundary curve, the server ties it to that curve with the native `PointOnCurve`. The end then follows the curve when parameters change, and the result is reported with its DOF. Formulas of dimensions that the native edit drops are reported in the result, never silently lost.

### Arcs of ellipses
A directed span uses the native span fields. Turns and sweeps whose quadrant cannot be held by native constraints are refused with the allowed range in the message, rather than solved onto a wrong branch.

### Loft
Closed sections only, as the public encoder requires. The default `op` is `new_body`; `add`, `cut` and `intersect` are available against a target where the app allows them.

### Profile choice
`extrude` and `revolve` accept `profiles_at: [[x, y], …]`, sketch coordinates, as an alternative to contour ids. Each point picks the innermost region containing it, as a click in the app does. Ids and points are mutually exclusive.

### Names and atomicity
Individual sketch entities get no names: the native records have no field for them. Sketches and features keep their names, as today. Every new tool is atomic, recorded in undo history, allowed in `batch`, and its created ids are reported.

## Consequences
- The tool count grows. TOOLS.md, the initialize instructions and README describe the new surface.
- The F-074 fillet check also applies to bodies built from the new curves. Its known gaps stay listed in F-074.
- Files keep opening and rebuilding in QymCAD 0.1.1. Circle offsets differ from the app's own recipe by design (F-076).

## Amendments

- **Stage 2 (2026-10-25).** Trim, extend and break tie a new end with the membership constraint the app itself
  uses for that curve kind (`point_on`: PointOnLine, PointOnCircle, PointOnEllipse or PointOnCurve), not always
  `PointOnCurve`: it is what the app writes for lines and round curves, and it solves the same way (F-083).
  The point mirror ties its copy to the source through the centre, where the app leaves it free (F-086), so a
  mirrored profile stays fully defined and follows its source. All six edits are one tool, `sketch_edit`, with
  an `op` discriminator: they share the sketch, the curve selectors and the result report.

