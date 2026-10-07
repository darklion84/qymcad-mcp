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
| `sketch.rs` | sketches on base/datum planes or faces; persisted datum-distance dependencies (F-017, ADR 0007); rect/circle; pinning; `sketch_edit` (rebuild dependents); settled solve |
| `sketch/entities.rs` | line, polyline, arc, polygon, slot: QymCAD adders + independent driving dimensions |
| `sketch/constrain.rs` | `sketch_constrain` (constraints, dimensions, over-constraint refusal), entity/constraint removal |
| `sketch/detail.rs` | `SketchDetail`: entities, points (frame roles), constraints for the agent |
| `features.rs` | datum planes; extrude/cut/add/intersect |
| `export.rs` | `Session::export`: STEP via `write_step`, meshes via `tessellate_merged` + qymcad-io writers; flat, world transforms (F-020) |
| `render.rs` | `Session::render`: CPU orthographic rasterizer over the body display meshes, face-id edges, 2× supersampling |
| `pngfile.rs` | minimal PNG writer (zlib via `flate2`, ADR 0004) |
| `topology.rs` | faces/edges of a body (kinds, geometry, adjacency, seams); `Sel` selections → `refs::Query` (ids checked recursively, edge `largest` by true length F-030, balanced unions and a size/depth budget F-032); `select`; `ensure_topology` after open (F-023; a failed rebuild is reported and undone); `source_body` (only unconsumed bodies, F-009) |
| `modifiers.rs` | fillet, chamfer (edges stored as pick lists, F-024), shell, push face, hole |
| `revolve.rs` | revolve about a sketch axis/line, world/datum/face axis; add/cut/intersect/new body |
| `patterns.rs` | linear/circular arrays and mirror of the whole body (F-027), at most 1000 copies also on parameter edits (F-033); `AxisRef` → datum axes (identical fixed axes reused) |
| `history.rs` | guarded native deletion with extended dependency closure; Project/original-shape snapshots, 16-entry modelling undo (ADR 0008) |
| `info.rs` | `DocInfo` snapshot for the agent |
| `value.rs` | `Num` (number or expression); finite values only; `check_expr` bounds length and nesting before QymCAD's recursive parser (F-031) |
| `error.rs` | `Error` with agent-oriented messages |

## The regenerate pipeline (`Session::rebuild`)
1. `regenerate_dirty_with_shapes` with the live shapes (only dirty nodes rebuild, like the GUI).
2. If there were errors: mark everything dirty and run once more (F-005: datum planes resolve during
   regenerate; `retryable()` errors).
3. Copy `report.built` faces into the bodies (F-007), so saved files have faces like GUI-saved ones.
4. Compare rebuilt shell counts with previous/source bodies, retaining session advisory diagnostics (ADR 0009).
5. Report errors, warnings (`regen_warnings` plus session advisories) and the result bodies (unconsumed, with volume and bbox).

## Edits are atomic
- `atomic(edit)` — features: rebuild pending dirty nodes first, refusing the new edit if that baseline fails;
  snapshot the clean `Project`, apply, rebuild (retrying only new nodes); if a node created by the edit has an
  error, restore the snapshot, drop shapes of removed bodies and return `Error::Rebuild`.
  If pending dirty nodes cannot rebuild, feature calls remain refused until parameter or sketch edits repair
  the document. Those repair paths bypass `atomic` and can edit a failing baseline.
- `sketch_edit` — retain the original live shapes of bodies in the sketch's dirty rebuild plan; rebuild on
  independent B-rep copies and retry only planned nodes. Failure restores the project and those original handles
  without another rebuild, so old bodies remain bit-identical (F-034).
- `transact(edit)` — sketch geometry (no rebuild needed): restore on error.
- `param_set` — restore the original project directly when propagation leaves a sketch unsolved (no shapes
  have changed). Before rebuilding, retain original live shapes of planned bodies and regenerate on B-rep
  copies, retrying only planned nodes; failure restores the project and original handles without regeneration.

MCP modelling tool calls establish a session history boundary before invoking the typed handler. Independent
B-rep copies isolate live shapes; failed calls restore the original Project/handles/diagnostics, successful
calls retain up to 16 snapshots. Undo restores a snapshot without regeneration. Read/save/export calls do
not enter modelling history; doc_new/doc_open replace the Session and its history. Opening compares stored
and rebuilt metrics using separate bbox-padding and volume-roundoff thresholds (ADR 0009).

## Sketch dimensions
Entities are added fully dimensioned so the GUI can edit them: rectangle = width + height (`Distance` along
x/y between corners) + a centre point (`Midpoint` of the diagonal) pinned from the origin; circle = `Diameter`
+ centre pinned. Pinning uses `Distance{axis}` from the origin (a magnitude `|Δ|`: the side comes from the
initial geometry, so a negative value stores `-(expr)`), or `PointOnLine` on an axis for a plain zero.

## Topology and selections
Face and edge ids are QymCAD's persistent names (F-010), valid for one body after a rebuild; every feature makes a
new body, so the agent re-reads `topology` after each one. Selections are explicit ids or descriptions (`Sel`,
mapped onto `refs::Query`, with engine-only concave/convex planar-corner filters lowered to ids). Edge selections are resolved when the feature is created and stored as pick lists:
stored edge queries break after the document is reopened in the app (F-024). Face selections (hole, shell, push
face) are stored as queries and keep following the geometry. A selection that matches nothing is refused (an
empty edge list would mean "every edge", F-025).
New entities (line, polyline, arc, polygon, slot) collect candidate dimensions — vertex pins from the origin,
radius, angles — and add each only if it removes a degree of freedom (`add_constraint_if_independent`): a point
shared with earlier geometry is not dimensioned twice, so the result is (0, 0) without redundancy (rect and circle
too). Exceptions, because QymCAD's solver/rank analysis needs them (FINDINGS F-040, F-041, F-045): a parametric
direction (arc end, polygon rotation) is an `ArcLength` from an `angle_reference` construction point on the +x side
of the centre — QymCAD's only directed dimension, and one without the slow angle arms — and the slot's tangencies
are first-order perpendiculars. Every sketch edit is solved until it settles and must reach residual ≤ 1e-6;
`param_set` rolls back an edit that leaves any sketch unsolved. Linear coordinate expressions cannot change sign
(QymCAD dimensions keep their side); such an edit is refused.

`sketch_constrain` resolves ids to points/lines/circles (plus `origin`, `x_axis`, `y_axis`), builds the QymCAD
constraint, and compares `sketch_dof` before/after: more redundancy → refused (a satisfied, fully implied geometric
constraint is just not added); then it must solve. A distance between two lines requires `Parallel`: rank
dependence permits omitting it only when its residual is satisfied; a contradiction refuses the call.
`sketch_edit` wraps sketch tools: on success, features that
read the sketch are rebuilt; a newly failing feature rolls the edit back.

## MCP layer (`crates/mcp/src`)
| File | Owns |
|---|---|
| `transport.rs` | newline-delimited JSON-RPC: `initialize` (version negotiation, `instructions`), `ping`, `tools/list`, `tools/call`; notifications ignored |
| `tools/mod.rs` | `Registry`, `tool()` / `tool_content()` constructors (schema from the argument type), `State` (the open `Session`), `markdown()` for docs/TOOLS.md |
| `tools/common.rs` | `ObjRef` (id or name), `PlaneArg`, compact rebuild JSON |
| `tools/{doc,params,sketch,features,history,output}.rs` | one tool group each, `fn tools() -> Vec<Tool>`; `output` = `export`, `render` (image item, base64) |
| `tools/topology.rs` | `topology`, `select`; JSON forms of selections, axes and directions (hand-parsed for precise errors), shared with `tools/features.rs` |
| `lib.rs` | agent instructions; installed-app release check (F-018) |
| `main.rs` | moves fd 1 to stderr and serves the protocol on a duplicate of stdout (ADR 0005, F-019) |

Error contract: an unknown tool or malformed request is a JSON-RPC error; a tool that runs and fails returns a
normal result with `isError: true` and the message (the model must see it). Argument structs use
`deny_unknown_fields` so typos fail loudly.

## Testing
- `tests/smoke.rs` — the kernel links and builds.
- `tests/golden_*.rs` (`golden_plate`, `golden_features`: every 3B operation) — parts with hand-computed volume/bbox; parameter edits; save/open round trip; **the GUI
  rebuild path** (`common::gui_edit_param` reproduces QymCAD.app's open → edit parameter → rebuild sequence).
- `tests/golden_export.rs` — every export format read back (STEP volume via `read_exact`, STL/3MF mesh volume and
  bbox, GLB metres/+Y up, OBJ geometric read-back), `finer_quality_never_loses_accuracy`, body selection;
  the placed-part test verifies world-space render/export geometry, and staging-directory tests verify safe
  output creation and hard-link replacement. Renders are decoded with the `png`
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
| 3 | Sketch entities & constraints (3A); topology + fillet/chamfer/hole/shell/arrays/mirror/revolve (3B); export + render (3C) | implemented and integrated; review fixes in progress on `phase3/codex-r2` |
| 4 | Acceptance on real parts (collet test plate, hanging shelf) vs build123d references | — |
| 5 | Release 0.1.0 | — |
