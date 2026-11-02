# ADR 0043: Measure

- Status: accepted (2026-11-02).
- Related: ADR 0001, ADR 0008, ADR 0013, ADR 0020; F-069, F-073, F-131.

## Context

QymCAD's 3D ruler resolves picks to `qymcad_core::measure::MeasureItem` and calls `measure_one` or `measure_pair`. Its distances describe support geometry, full circles, circle centres or signed cylinder axis clearance. They are not a general minimum distance between trimmed B-rep elements. Its angles are acute and unsigned. Native fallback pairs can return an arbitrary anchor distance, and native zero-vector normalization can turn invalid geometry into a plausible zero (F-131).

The server replaces screen picks with existing body references and selection descriptions, preserves the native mathematics, and refuses misleading or invalid inputs before those calls.

## Decisions

1. Add a read-only `measure` tool with required `a` and optional `b`. Each operand is exactly one of `{"point":[x,y,z]}`, `{"body":ObjRef,"face":Sel}` or `{"body":ObjRef,"edge":Sel,"at"?:"start"|"end"|"mid"|"center"}`. All argument objects deny unknown fields. There is no body-only operand or quantity switch. Coordinates are finite millimetres. Body references use the common resolver; `Sel` uses the existing selection decoder. Resolve each selection to exactly one current element, never the first of several matches.
2. Validate current, built, unconsumed bodies, element kind, ownership, unambiguous ids, finite geometry and nonzero directions before any native measurement. Refuse missing/stale ids, ids of the wrong element kind, wrong-body ids, empty/multiple matches and degenerate geometry. Geometry must not reach native normalization with a zero direction.
3. Map a literal or selected edge point to `Point`, a straight edge to `Line`, a circle or arc to `Circle`, a planar face to `Plane`, and a cylindrical face to `Cylinder`. An arc represents its full underlying circle in pair measurements. `at` makes any supported edge location a point; `center` is restricted to circles/arcs. Endpoints follow the current native edge orientation and are not persistent vertex identities. Refuse spline/other edges and cone/sphere/other faces in primitive pairs; do not reproduce the GUI's spline chord or curved-face plane approximations.
4. Build these native items in world coordinates using `Project::body_world_transform`, as export/render do. Transform points and directions correctly, then call the native measure functions in the engine. The app displays active-context coordinates: signed deltas can therefore differ for a placed part, while distance and angle are invariant under a common rigid placement. No replacement distance calculation is introduced.
5. For a single straight edge return `length_mm`. For a circle/arc return `radius_mm`, `diameter_mm` and `length_mm`; arc length uses its real span through the existing `edge_kind_length`. For a cylinder face return `radius_mm` and `diameter_mm`. A single spline/other edge returns `length_mm` with `length_kind:"polyline_approximation"`. A point or planar face alone is refused with guidance to give a second operand.
6. Pair results preserve full native precision and omit unavailable quantities. Return `distance_mm` and `distance_kind` only when native returns a distance; `angle_deg` with `angle_kind:"acute_unsigned"` only when native returns an angle; `delta_mm=b-a` only for two points. Include the resolved body and face/edge ids per operand and `coordinate_frame:"world"` on every successful result.

| Pair | `distance_kind` and meaning |
|---|---|
| point / point | `point_point`: Euclidean distance |
| point / straight edge | `point_support_line`: infinite support line, ignoring the segment ends |
| straight edge / straight edge | `support_lines`: infinite support lines, including skew lines |
| point / plane | `point_support_plane`: absolute perpendicular distance to the infinite plane |
| parallel planes | `support_plane_gap`: absolute support-plane gap; opposite normals give angle 0° |
| straight edge parallel to plane | `support_plane_gap`: perpendicular gap between the support line and plane |
| point / cylinder | `point_cylinder_surface`: infinite cylindrical surface, ignoring axial bounds |
| point / circle or arc | `point_full_circle`: full underlying circle, ignoring arc bounds |
| parallel circles | `circle_centre_distance`: Euclidean centre distance; radii do not reduce it |
| parallel cylinders | `signed_axis_clearance`: axis gap minus both radii; may be negative |

Nonparallel planes return only their native angle. Line/plane angles follow native line-to-plane semantics. Refuse native fallback anchor-distance pairs, including circle/plane; advise selecting an explicit point with `at`. Never label signed axis clearance as a wall distance. Unsupported primitive combinations do not receive a substitute distance.

## Read-only and kernel gate contract

Like `select`, measure creates no modelling feature, dirty flag, saved annotation, dependency, undo entry or batch step. It leaves Project, diagnostics, archive contents and the previous undo step unchanged on both success and refusal. Existing topology restoration doors keep their established contract; any failed restoration must leave the original state intact.

Resolve references and selections outside any caller-held kernel gate. Use one short caller-held gate for raw `Shape` reads needed to obtain the primitives, and release it before native pure mathematics, result/error formatting or a self-gating Session call (`topology`, `select`, `info`, restore). Never acquire nested gates, including on error paths (F-069/F-073).

## Scope and evidence

T8 covers native primitive distances, acute unsigned angles, edge lengths and analytic radii/diameters. Exclusions are general trimmed B-rep minimum distance, area, volume, mass, signed angles, persistent vertex names, spline pairs and nonplanar/noncylindrical face pairs. Existing `doc_info` and `topology` remain the sources for volume and tessellated area.

Native evidence is retained in `tasks/research-medE/measure/main.rs` and `run.log` (`MEASURE PROBE PASSED`), with source citations and independent formulas in `tasks/research-medE.md`, T8 §1–3. Stage acceptance belongs to `crates/engine/tests/golden_measure.rs`, `crates/mcp/tests/e2_measure.rs` and `tasks/E2-report.md`; those tests and production mutations must establish the implemented contract separately from the earlier native probe.
