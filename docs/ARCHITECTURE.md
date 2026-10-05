# Architecture

## Layers
```
MCP client (Claude, ...)
   │  JSON-RPC over stdio
crates/mcp  (qymcad-mcp)      transport, tool registry, argument schemas (schemars), result formatting
   │  typed Rust calls
crates/engine (qymcad-engine) Session: one open document; params, sketches, features, topology, export, render
   │  QymCAD API
qymcad-core / -io / -kernel / -testkit   (git dependency, pinned tag — ADR 0003)
   │
OpenCASCADE (dylibs, Homebrew)
```
Rules: only `engine` touches QymCAD (ADR 0001); the process is single-threaded and synchronous (ADR 0002).

## Engine modules (`crates/engine/src`)
| Module | Owns |
|---|---|
| `session.rs` | `Session` (one `Project` + live `Shape`s + path); `rebuild()`; `atomic()` / `transact()`; result bodies; name/id resolution |
| `params.rs` | parameters: lowercase names (F-001), evaluation, propagation to sketches and features (F-002), usage lookup |
| `sketch.rs` | sketches on base/datum planes or faces; fully dimensioned entities; contour/nesting/DOF report |
| `features.rs` | datum planes; extrude/cut/add/intersect |
| `export.rs` | `Session::export`: STEP via `write_step`, meshes via `tessellate_merged` + qymcad-io writers; flat, world transforms (F-3C-2) |
| `render.rs` | `Session::render`: CPU orthographic rasterizer over the body display meshes, face-id edges, 2× supersampling |
| `pngfile.rs` | minimal PNG writer (zlib via `flate2`, ADR 0004) |
| `info.rs` | `DocInfo` snapshot for the agent |
| `value.rs` | `Num` (number or expression) |
| `error.rs` | `Error` with agent-oriented messages |

## The regenerate pipeline (`Session::rebuild`)
1. `regenerate_dirty_with_shapes` with the live shapes (only dirty nodes rebuild, like the GUI).
2. If there were errors: mark everything dirty and run once more (F-005: datum planes resolve during
   regenerate; `retryable()` errors).
3. Copy `report.built` faces into the bodies (F-007), so saved files have faces like GUI-saved ones.
4. Report errors, warnings (`regen_warnings`) and the result bodies (unconsumed, with volume and bbox).

## Edits are atomic
- `atomic(edit)` — features: snapshot the `Project`, apply, rebuild; if a node created by the edit has an
  error, restore the snapshot, drop shapes of removed bodies and return `Error::Rebuild`. An agent never leaves
  a half-built feature behind.
- `transact(edit)` — sketch geometry (no rebuild needed): restore on error.
- `param_set` — the same, plus re-propagation after restoring.

## Sketch dimensions
Entities are added fully dimensioned so the GUI can edit them: rectangle = width + height (`Distance` along
x/y between corners) + a centre point (`Midpoint` of the diagonal) pinned from the origin; circle = `Diameter`
+ centre pinned. Pinning uses `Distance{axis}` from the origin (a magnitude `|Δ|`: the side comes from the
initial geometry, so a negative value stores `-(expr)`), or `PointOnLine` on an axis for a plain zero.

## MCP layer (`crates/mcp/src`)
| File | Owns |
|---|---|
| `transport.rs` | newline-delimited JSON-RPC: `initialize` (version negotiation, `instructions`), `ping`, `tools/list`, `tools/call`; notifications ignored |
| `tools/mod.rs` | `Registry`, `tool()` / `tool_content()` constructors (schema from the argument type), `State` (the open `Session`), `markdown()` for docs/TOOLS.md |
| `tools/common.rs` | `ObjRef` (id or name), `PlaneArg`, compact rebuild JSON |
| `tools/{doc,params,sketch,features,output}.rs` | one tool group each, `fn tools() -> Vec<Tool>`; `output` = `export`, `render` (image item, base64) |
| `lib.rs` | agent instructions; installed-app release check (F-018) |
| `main.rs` | moves fd 1 to stderr and serves the protocol on a duplicate of stdout (ADR 0005, F-3C-1) |

Error contract: an unknown tool or malformed request is a JSON-RPC error; a tool that runs and fails returns a
normal result with `isError: true` and the message (the model must see it). Argument structs use
`deny_unknown_fields` so typos fail loudly.

## Testing
- `tests/smoke.rs` — the kernel links and builds.
- `tests/golden_*.rs` — parts with hand-computed volume/bbox; parameter edits; save/open round trip; **the GUI
  rebuild path** (`common::gui_edit_param` reproduces QymCAD.app's open → edit parameter → rebuild sequence).
- `tests/golden_export.rs` — every export format read back (STEP volume via `read_exact`, STL/3MF mesh volume and
  bbox, GLB metres/+Y up, OBJ triangle count), quality presets, body selection; renders decoded with the `png`
  crate (size, coverage, plate aspect 1.5, holes show background).
- `tests/session_behaviour.rs` — API contract: atomicity, errors, parameter bookkeeping.
- `crates/mcp/tests/protocol.rs` — spawns the real binary: initialize/list/errors, builds, saves, reopens and
  edits the golden plate purely through tool calls.
- `crates/mcp/tests/docs_fresh.rs` — docs/TOOLS.md equals the generated registry docs.

## Roadmap
| Phase | Scope | Status |
|---|---|---|
| 0 | Repository skeleton, pinned build, docs, ADRs, smoke test | done |
| 1 | Engine core: session, regenerate pipeline, params, rect/circle sketches, extrude/cut, offset plane | done |
| 2 | MCP transport + phase-1 tools, protocol tests, registration | done |
| 3 | Sketch entities & constraints; topology + fillet/chamfer/hole/shell/arrays/mirror/revolve; export + render | — |
| 4 | Acceptance on real parts (collet test plate, hanging shelf) vs build123d references | — |
| 5 | Release 0.1.0 | — |
