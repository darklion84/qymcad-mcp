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
- Sketch entities `line`, `polyline`, `arc` (radius + angles or start/end points), `polygon` (regular; radius +
  angle or a vertex), `slot`, each fully dimensioned from numbers/expressions (`dimensioned: false` leaves them
  free); tools `sketch_constrain` (coincident, horizontal, vertical, parallel, perpendicular, collinear, equal,
  tangent, concentric, midpoint, point_on_line, symmetric, fix; distance aligned/x/y, angle, diameter, radius;
  reference dimensions; refuses over-constraining) and `sketch_remove`; `sketch_info` lists entities, points and
  constraints. Sketch edits rebuild the features built from the sketch. Golden sketch tests incl. the GUI path,
  sketch contract tests, protocol test. FINDINGS F-3A-1..F-3A-6.

### Fixed
- `param_set` refuses the names `pi`, `tau`, `e`: QymCAD reads them as constants (F-3A-1).
- Sketches are solved until they settle (one QymCAD solve can stop short, F-3A-2).

### Security
- File paths accepted by tools are restricted to their file type (`.qcad`) and optionally confined to
  `QYMCAD_MCP_ROOT` (docs/SECURITY.md).
