# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## Unreleased

### Added
- Repository skeleton: Cargo workspace (`qymcad-engine`, `qymcad-mcp`), QymCAD pinned at `v0.1.0-dev.20261001`,
  kernel smoke test, `scripts/check.sh`, AGENTS.md, ADR 0001-0003, FINDINGS F-001..F-015, UPGRADING draft.
- Engine core: `Session` (new/open/save/info, rebuild pipeline, atomic edits, name resolution), parameters
  (lowercase, propagation, usage-checked delete), fully dimensioned rectangle/circle sketches on base/datum
  planes or faces, offset datum planes, extrude with add/cut/intersect/new body, direction and through.
- Golden plate tests incl. the QymCAD GUI parameter-edit path; engine contract tests. FINDINGS F-016, F-017.
- MCP server: stdio JSON-RPC transport (protocol 2024-11-05 .. 2025-06-18), tools `doc_new`, `doc_open`,
  `doc_save`, `doc_info`, `param_set`, `param_delete`, `sketch_create`, `sketch_add` (rect, circle),
  `sketch_info`, `plane_offset`, `extrude`; agent instructions; installed QymCAD.app release check (F-018);
  `--dump-tools` generating docs/TOOLS.md; protocol tests.
- `export` tool / `Session::export`: STEP (exact), STL, 3MF, GLB, OBJ of the result bodies (or selected ones), with
  the app's mesh quality presets; per-body mesh volume in the result. Golden read-back tests for every format.
- `render` tool / `Session::render`: shaded orthographic PNG (iso/top/bottom/front/back/left/right) with feature
  edges, returned as an MCP image item. CPU rasterizer adapted from QymCAD's thumbnail renderer; PNG via `flate2`
  (ADR 0004). FINDINGS F-3C-1..F-3C-3.
- Topology and finishing features (phase 3B). Engine: `topology` (face/edge kinds, geometry, adjacency, seams),
  selections (ids or descriptions mapped onto QymCAD's `refs::Query`) with `select`, `revolve` (sketch/world/datum/
  face axes, add/cut/intersect/new body), `fillet`, `chamfer` (symmetric or two distances), `hole` (plain/
  counterbore/countersink, blind or through), `shell`, `push_face`, `linear_array`, `circular_array`, `mirror`; all
  atomic, every dimension may be an expression. `Session::open` restores faces like the app. MCP tools of the same
  names. Golden tests with hand-computed volumes, GUI-path tests, protocol tests. FINDINGS F-3B-1..F-3B-7
  (notably: stored edge queries break after reopening, so edges are stored as pick lists).
- Sketch entities `line`, `polyline`, `arc` (radius + angles or start/end points), `polygon` (regular; radius +
  angle or a vertex), `slot`, each fully dimensioned from numbers/expressions (`dimensioned: false` leaves them
  free); tools `sketch_constrain` (coincident, horizontal, vertical, parallel, perpendicular, collinear, equal,
  tangent, concentric, midpoint, point_on_line, symmetric, fix; distance aligned/x/y, angle, diameter, radius;
  reference dimensions; refuses over-constraining) and `sketch_remove`; `sketch_info` lists entities, points and
  constraints. Sketch edits rebuild the features built from the sketch. Golden sketch tests incl. the GUI path,
  sketch contract tests, protocol test. FINDINGS F-3A-1..F-3A-6.

### Fixed
- Line-line distances refuse contradictory rank-dependent parallelism and restore the whole sketch (A1).
- Removing unrelated sketch geometry preserves free direction helpers and their parametric ArcLength dimensions;
  helpers are pruned when their owner is removed (A2, F-3A-8).
- Large finite numbers stay finite in rounded tool output, including nested topology JSON (A3).
- Through-hole creation refuses stock whose bbox diagonal exceeds the fixed 10000 mm depth; tool descriptions
  and findings document the limit and possible blindness after later stock growth (B1/B3, F-3B-6/F-3B-15).
- Feature edits rebuild pending dirty nodes before snapshotting; failed sketch edits restore original live shapes
  without rebuilding old bodies, preserving volume bits and project/shape consistency (B2, F-3B-12).
- Tool argument fields are fully documented; fillet seam handling and asymmetric chamfer side selection now
  match QymCAD's limitations (B4/B5, F-3B-7/F-3B-14).
- Outward-shell golden coverage checks all four spherical corner radii against wall thickness (B6, F-3B-16).
- OpenCASCADE's STEP-writer statistics no longer corrupt the stdio protocol: the server speaks JSON-RPC on a
  duplicate of stdout and redirects fd 1 to stderr (ADR 0005).
- Phase 3B review round 1: non-finite dimensions ("nan", "1e400") refused; extrude/revolve `target` must be a
  current body; edge `largest` ranks by true length; ids validated anywhere in a selection; unions balanced and
  selections budgeted so documents stay saveable; arrays capped at 1000 copies (also on parameter edits);
  a face axis without a body uses the active part's current body; strict `[x, y, z]` and tolerance parsing;
  `ensure_topology` reports a failed rebuild and undoes it; a failed or retried feature no longer rebuilds old
  bodies; fixed datum axes reused; `shell` requires open faces (QymCAD has no closed shell). FINDINGS F-3B-8..14.
- `param_set` refuses the names `pi`, `tau`, `e`: QymCAD reads them as constants (F-3A-1).
- Sketches are solved until they settle (one QymCAD solve can stop short, F-3A-2).
- A parameter edit that leaves a sketch unsolved is refused and rolled back (was committed silently).
- Parametric arc ends and polygon rotations are arc-length dimensions from a reference point, so they follow a
  parameter across 90°/180° (QymCAD dimensions keep their side, F-3A-7) and settle in one GUI solve; this also
  covers a parametric radius with a literal angle.
- Removing a sketch entity keeps spline control points (F-3A-8).
- A distance between two lines adds parallelism; reference radius/diameter on arcs is refused (F-3A-9).
- Rect and circle add only independent dimensions (a circle centred on a dimensioned vertex was over-constrained).
- `param_delete` works for an old parameter named `pi`/`tau`/`e`; `sketch_add` refuses an empty list.

### Security
- Exports are written via a temporary file and an atomic rename (no write-through of hard links); export and
  render refuse documents with features that did not build (review findings, phase 3C).
- Expressions are bounded (1000 characters, 64 levels of nesting) before QymCAD's recursive evaluator: a deep
  expression from an agent crashed the server (F-3B-9).
- File paths accepted by tools are restricted to their file type (`.qcad`) and optionally confined to
  `QYMCAD_MCP_ROOT` (docs/SECURITY.md). `export` accepts only its format's extensions (.step/.stp, .stl, .3mf,
  .glb, .obj) under the same rules.
