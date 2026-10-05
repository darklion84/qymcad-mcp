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
- Topology and finishing features (phase 3B). Engine: `topology` (face/edge kinds, geometry, adjacency, seams),
  selections (ids or descriptions mapped onto QymCAD's `refs::Query`) with `select`, `revolve` (sketch/world/datum/
  face axes, add/cut/intersect/new body), `fillet`, `chamfer` (symmetric or two distances), `hole` (plain/
  counterbore/countersink, blind or through), `shell`, `push_face`, `linear_array`, `circular_array`, `mirror`; all
  atomic, every dimension may be an expression. `Session::open` restores faces like the app. MCP tools of the same
  names. Golden tests with hand-computed volumes, GUI-path tests, protocol tests. FINDINGS F-3B-1..F-3B-7
  (notably: stored edge queries break after reopening, so edges are stored as pick lists).
### Security
- File paths accepted by tools are restricted to their file type (`.qcad`) and optionally confined to
  `QYMCAD_MCP_ROOT` (docs/SECURITY.md).
