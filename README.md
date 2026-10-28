# qymcad-mcp

An [MCP](https://modelcontextprotocol.io) server that lets an AI agent build **native, parametric
[QymCAD](https://github.com/QymIs-Tech/QymCAD) parts** — sketches, extrusions, cuts, fillets, holes, driven by
named parameters — and save them as `.qcad` files you open and edit in the QymCAD app.

> Status: **pre-release.** 33 tools for QymCAD 0.1.1: documents, parameters and named driving dimensions;
> sketches (rotated rectangle, circle, tangent circle, line, polyline, spline, arc, ellipse, ellipse arc,
> polygon, slot) fully dimensioned with expressions, plus curve constraints and curve editing (trim, extend,
> break, offset, corner fillet/chamfer, point mirror); datum planes;
> extrude / cut / revolve / sweep / loft; fillet, chamfer, hole, shell, push face; linear / circular arrays and mirror;
> topology and descriptive selections; atomic modelling batches, feature deletion and undo;
> export to STEP / STL / 3MF / GLB / OBJ and a rendered PNG preview.
> See [docs/TOOLS.md](docs/TOOLS.md) and the [roadmap](docs/ARCHITECTURE.md#roadmap). Known limits and QymCAD
> quirks are in [docs/FINDINGS.md](docs/FINDINGS.md).

## How it works
The server links the QymCAD geometry crates (OpenCASCADE kernel) directly and runs headless: no QymCAD window
is driven. The agent calls tools (`doc_new`, `param_set`, `sketch_create`, `extrude`, `fillet`, `doc_save`, ...);
each call rebuilds the model and reports errors, volume and bounding box. You open the saved file in QymCAD
(File > Open) and change parameters there; the part rebuilds.

Use `batch` for predictable sequences of 1–200 modelling operations on the open document:

```json
{
  "steps": [
    { "tool": "param_set", "args": { "name": "w", "value": 30 } },
    { "tool": "param_set", "args": { "name": "t", "value": 4 } },
    { "tool": "sketch_create", "args": { "plane": "XY", "name": "outline" } },
    { "tool": "sketch_add", "args": { "sketch": "outline", "entities": [{ "type": "rect", "w": "w", "h": 20 }] } },
    { "tool": "extrude", "args": { "sketch": "outline", "height": "t", "name": "plate" } }
  ]
}
```

Every step's tool name and argument type are checked before anything runs. Steps use the same arguments,
JSON-string compatibility and handlers as individual calls, and can refer to earlier objects by name or id.
Each step retains its normal rebuild and checks. A failed step rolls back the whole batch; a successful
batch is one undo entry, labelled `Batch (N steps)`, among the 16 retained calls. Results are compact by
default, with created-node ids/names and final bodies/warnings/errors; `"verbose": true` adds each tool's
full result. Use individual calls when you need to inspect `topology`, `sketch_info`, `select` or a `render`
between operations. Document tools, undo, output, inspection and nested batches are refused inside `batch`.

## Versions and compatibility

**The server and the QymCAD app must be the same release.** Each server version is built against exactly
one QymCAD release, and it carries that release's number:

- server `0.1.1-dev.20261026` works with QymCAD `v0.1.1-dev.20261026`;
- a fix to the server for the same QymCAD release adds a counter: `0.1.1-dev.20261026.1`, `.2`, …;
- tags in this repository follow the same pattern: `v0.1.1-dev.20261026`, `v0.1.1-dev.20261026.1`.

So the QymCAD part of the server's version is the app you need. `qymcad-mcp --version` prints both, and at
startup the server warns when the installed QymCAD.app is a different release.

| qymcad-mcp | QymCAD release | OpenCASCADE |
|---|---|---|
| 0.1.1-dev.20261026.3 | v0.1.1-dev.20261026 | 7.9.3 (Homebrew) |
| 0.1.1-dev.20261026.2 | v0.1.1-dev.20261026 | 7.9.3 (Homebrew) |
| 0.1.1-dev.20261026.1 | v0.1.1-dev.20261026 | 7.9.3 (Homebrew) |
| 0.1.1-dev.20261026 | v0.1.1-dev.20261026 | 7.9.3 (Homebrew) |
| 0.1.0 (numbered before this scheme) | v0.1.0-dev.20261001 | 7.9.3 (Homebrew) |

Why it matters: QymCAD changes its file format between releases without saying so (the `schema_version` stays
the same). Files saved by the server for QymCAD 0.1.1 do not open in QymCAD 0.1.0, because they use a new sketch
constraint (`Orientation`) and a new B-rep format (QYMB v3). Files from 0.1.0 do open in QymCAD 0.1.1. The app
warns that rebuilt fillets, chamfers and planes may lie differently, and that old picks may be "found by place,
not by name". Since 0.1.1-dev.20261026, parameter names are case-sensitive, and a through cut goes only to the
side it is directed to (ADR 0021).

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
MIME type and `base64_bytes` (the original base64 string's UTF-8 byte length), with image data omitted.

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
