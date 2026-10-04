# qymcad-mcp

An [MCP](https://modelcontextprotocol.io) server that lets an AI agent build **native, parametric
[QymCAD](https://github.com/QymIs-Tech/QymCAD) parts** — sketches, extrusions, cuts, fillets, holes, driven by
named parameters — and save them as `.qcad` files you open and edit in the QymCAD app.

> Status: **early development** (phase 0 of [the plan](docs/ARCHITECTURE.md#roadmap)). Not usable yet.

## How it works
The server links the QymCAD geometry crates (OpenCASCADE kernel) directly and runs headless: no QymCAD window
is driven. The agent calls tools (`doc_new`, `param_set`, `sketch_create`, `extrude`, `fillet`, `doc_save`, ...);
each call rebuilds the model and reports errors, volume and bounding box. You open the saved file in QymCAD
(File > Open) and change parameters there; the part rebuilds.

## Compatibility
| qymcad-mcp | QymCAD release | OpenCASCADE |
|---|---|---|
| 0.1.x | v0.1.0-dev.20261001 | 7.9.3 (Homebrew) |

The `.qcad` format changes between QymCAD releases: **use the QymCAD app release listed above.**

## Build (macOS)
```sh
brew install opencascade      # 7.8+ required
cargo build --release         # first build compiles the OCCT bridge, ~2 min
```
Other OCCT locations: set `OCCT_INCLUDE_DIR` and `OCCT_LIB_DIR`.

## Development
See [AGENTS.md](AGENTS.md) (rules, build, test), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md),
[docs/FINDINGS.md](docs/FINDINGS.md) (QymCAD behaviour we rely on) and [docs/UPGRADING.md](docs/UPGRADING.md).

## License
AGPL-3.0-or-later, the same as QymCAD, whose code this server links.
