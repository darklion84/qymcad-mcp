# ADR 0037: DXF and SVG sketch import

- Status: accepted (2026-10-31).
- Related: ADR 0008, ADR 0020, ADR 0035, ADR 0036; F-024, F-074.

## Context

QymCAD's import door parses DXF and SVG into `ProfEdge` curves and creates an owned editable sketch through `Project::import_sketch`. DXF preserves native lines, arcs and circles where possible; SVG lowers paths and circles to spline/line geometry and flips SVG Y into CAD-up coordinates. Unsupported or invalid entities are explicitly reported by the native importer.

## Decisions

1. Expose `sketch_import` with `path`, `format` (`dxf`, `svg`, `auto`), `plane`, optional `name`, and optional `scale`. Auto follows the exact `.dxf`/`.svg` extension rule; unknown extensions refuse.
2. Parse the exact user path before mutation. Scale is a finite positive `Num`, evaluated once; it is a one-time literal transform, not a driver and has no `dimension_set` key.
3. Call `Project::import_sketch` inside one atomic engine edit. Empty imports refuse; valid curves with skipped/invalid/redrawn reports succeed and preserve every native report tuple.
4. Report native entity-kind counts, sketch identity, contours, reports, scale, rebuild bodies and unit notes. DXF drawing units are treated as mm and `$INSUNITS` is not applied. SVG lengths are px when unitless and use 25.4/96 mm per px; CAD-up Y is used. Files larger than 16 MiB are refused before parsing.
5. Imported curves remain literal editable geometry. Text is never promised; DXF text and SVG text are reported/ignored by the underlying importer.

## Consequences

Imported sketches use the normal sketch edit, pattern and extrude paths, persist through save/load, and participate in batch and undo. File access is limited to the exact path supplied by the caller; no globbing or directory scan is performed. Nonparallel DXF OCS and other unsupported geometry follow the native importer reports.

Evidence: pinned QymCAD source and probe in `tasks/research-medB.md` I3; implementation and acceptance evidence are recorded in `tasks/B3-report.md`.
