# Working on qymcad-mcp (for AI agents and humans)

qymcad-mcp is an MCP stdio server that builds native, parametric [QymCAD](https://github.com/QymIs-Tech/QymCAD)
parts (`.qcad`) headlessly, so an agent can model and a person can then edit the parameters in the QymCAD app.

## Before you change anything
1. Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) (layers, data flow) and [docs/FINDINGS.md](docs/FINDINGS.md)
   (QymCAD behaviour we depend on — most bugs here are a forgotten finding).
2. Check the pinned QymCAD version: `[workspace.dependencies]` in `Cargo.toml` and `QYMCAD_VERSION` in
   `crates/engine/src/lib.rs`. They must agree, and the installed QymCAD.app must be the same release.

## Build and test
- Prerequisites (macOS): `brew install opencascade` (7.8+, 7.9.3 tested), Rust stable via rustup.
  OCCT paths are set in `.cargo/config.toml`; override `OCCT_INCLUDE_DIR` / `OCCT_LIB_DIR` for other layouts.
- `scripts/check.sh` — fmt, clippy (`-D warnings`), all tests. It must be green before every commit.
- The first build compiles the OCCT bridge (~1-2 min); later builds are incremental.

## Rules
- **All QymCAD API calls live in `crates/engine`.** The `mcp` crate never imports `qymcad_*` directly. An
  upgrade of QymCAD must touch only the engine (and tests/docs).
- **Every behaviour of QymCAD you rely on goes into docs/FINDINGS.md** with evidence (test, observation or
  source path). A guess is not a finding.
- **Every modelling feature has a golden test** (`crates/engine/tests/golden_*.rs`) that builds a part and
  checks volume/bbox against a hand-computed or reference value. These are what catch QymCAD regressions.
- **Parameter names and expressions are lowercased** by the engine (FINDINGS F-001). Do not bypass it.
- **Never trust "a shape exists"** — check `RegenReport.errors` (F-008).
- Significant decisions get an ADR in `docs/adr/` (copy the format of the existing ones).
- Tool schemas are generated from Rust types; after changing a tool run `cargo run -p qymcad-mcp -- --dump-tools
  > docs/TOOLS.md` (CI/check.sh verifies it is up to date once phase 2 lands).
- Keep CHANGELOG.md updated under `Unreleased`.

## Upgrading QymCAD
Follow [docs/UPGRADING.md](docs/UPGRADING.md). Never bump the tag without running the golden tests and the
manual GUI check listed there.
