# qymcad-mcp

An [MCP](https://modelcontextprotocol.io) server that lets an AI agent build **native, parametric
[QymCAD](https://github.com/QymIs-Tech/QymCAD) parts** — sketches, extrusions, cuts, fillets, holes, driven by
named parameters — and save them as `.qcad` files you open and edit in the QymCAD app.

> Status: **pre-release (0.1.0 in progress).** 26 tools: documents and parameters; sketches (rectangle, circle,
> line, polyline, arc, polygon, slot) fully dimensioned with expressions, plus constraints; datum planes;
> extrude / cut / revolve; fillet, chamfer, hole, shell, push face; linear / circular arrays and mirror;
> topology and descriptive selections; export to STEP / STL / 3MF / GLB / OBJ and a rendered PNG preview.
> See [docs/TOOLS.md](docs/TOOLS.md) and the [roadmap](docs/ARCHITECTURE.md#roadmap). Known limits and QymCAD
> quirks are in [docs/FINDINGS.md](docs/FINDINGS.md).

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

## Use with Claude Code
```sh
cargo build --release
claude mcp add qymcad --scope project -- "$PWD/target/release/qymcad-mcp"   # run inside your CAD project
```
or put this in the project's `.mcp.json`:
```json
{ "mcpServers": { "qymcad": { "command": "/absolute/path/to/qymcad-mcp/target/release/qymcad-mcp" } } }
```
Any MCP client that speaks stdio works the same way. The server prints its QymCAD release and whether the
installed QymCAD.app matches to stderr at startup.

Paths: the tools read and write only `.qcad` (and export formats); set `QYMCAD_MCP_ROOT` to confine them to one
directory. See [docs/SECURITY.md](docs/SECURITY.md).

## Call log
Every stdio server process writes a local JSONL call log by default: `~/Library/Logs/qymcad-mcp/` on macOS,
or `$XDG_STATE_HOME/qymcad-mcp/` elsewhere (fallback: `~/.local/state/qymcad-mcp/`). Set `QYMCAD_MCP_LOG`
to another directory, or to `off` to disable it (`0`, `false`, and `no` also work, case-insensitively).
An unset or empty value uses the default. Startup stderr shows this process's file, named
`qymcad-mcp-YYYYMMDDTHHMMSSZ-pid.jsonl`; `--version` and `--dump-tools` create no log. When logging is on,
`initialize.result.instructions` appends exactly
`"\nCall log of this server process (JSONL, for diagnosing problems): <path>."` with that file's path.
Nothing is added when off. This sentence is the only protocol-output difference: all other initialize fields
match, and every response after initialize is byte-identical with logging on or off.

Read the records in order: `call` saves received arguments before a tool runs, and `result` saves its answer
and elapsed milliseconds with the same `seq`. The last `call` without a matching `result` identifies the call
that crashed or hung, provided logging had not stopped because of an I/O failure or the size limit. Records also
include startup, initialization, JSON-RPC errors, Rust panics and transport shutdown. Image results contain
MIME type and `bytes` (the original base64 string's UTF-8 byte length), with image data omitted.

Logs contain tool arguments and results, including file paths, parameter values and error texts. They stay on
local disk; nothing is sent anywhere. Each record is limited to 256 KiB and each file to 64 MiB. Oversized
strings are cut with a truncation marker; oversized structured fields can become marked JSON-text prefixes.
Startup removes matching logs older than 30 days, then keeps at most 200 previous logs.
See [ADR 0018](docs/adr/0018-call-log.md).

## Development
See [AGENTS.md](AGENTS.md) (rules, build, test), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md),
[docs/FINDINGS.md](docs/FINDINGS.md) (QymCAD behaviour we rely on), [docs/adr/](docs/adr/) (decisions),
[docs/UPGRADING.md](docs/UPGRADING.md) and [docs/BACKLOG.md](docs/BACKLOG.md). How changes are reviewed:
[docs/reviews/](docs/reviews/).

## License
AGPL-3.0-or-later, the same as QymCAD, whose code this server links.
