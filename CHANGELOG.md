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
