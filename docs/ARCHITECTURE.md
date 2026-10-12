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

`sketch_edit` and `param_set` share `with_rebuild_copies`: retain planned original live handles, regenerate
on independent B-rep copies, and restore Project/handles/diagnostics on failure. Callers keep their own retry
plan and error policy (F-034).

- `atomic(edit)` — features: rebuild pending dirty nodes first, refusing the new edit if that baseline fails;
  snapshot the clean `Project`, apply, rebuild (retrying only new nodes); if a node created by the edit has an
  error, restore the snapshot, drop shapes of removed bodies and return `Error::Rebuild`.
  If pending dirty nodes cannot rebuild, feature calls remain refused until parameter or sketch edits repair
  the document. Those repair paths bypass `atomic` and can edit a failing baseline.
- `sketch_edit` — retain the original live shapes of bodies in the sketch's dirty rebuild plan; rebuild on
  independent B-rep copies and retry only planned nodes. Failure restores the project and those original handles
  without another rebuild, so old bodies remain bit-identical (F-034).
- `transact(edit)` — sketch geometry (no rebuild needed): restore on error.
- `param_set` — solve only sketches reached by changed values, repeating for named reference measurements
  updated after solves (at most sketches+1 rounds). Rebuild dirtying retains native blanket feature-dimension
  marking; face-derived datum sketches are dirtied as placement barriers without extra solves (F-017).
  Reject scope evaluation errors and recheck array limits after reference propagation. Restore the original
  project directly when propagation fails or leaves a sketch unsolved (no shapes have changed). Before rebuilding, retain original live shapes of planned bodies and regenerate on B-rep
  copies, retrying only planned nodes; failure restores the project and original handles without regeneration,
  including shape absence for previously unbuilt planned bodies.

MCP modelling tool calls establish a session history boundary before invoking the typed handler. Independent
B-rep copies isolate live shapes; failed calls restore the original Project/handles/diagnostics, successful
calls retain up to 16 source-free recipe snapshots plus call metadata. Restoration moves embedded source bytes
by id and preserves the monotonic id allocator (F-056). Native sketch deletion's removed sources are retained
once in a session archive while undo references them. Deletion reuses an enclosing tool boundary; direct
engine deletion creates its own. Undo restores a snapshot without regeneration and reports its tool name, original arguments, human label
(localized name of the node returned by the tool, or action label when no named node was created), and
restored parameters/bodies/diagnostics; eviction is remembered for limit-exhaustion messages. Read/save/export calls do
not enter modelling history; doc_new/doc_open replace the Session and its history. Opening compares stored
and rebuilt metrics using separate bbox-padding and volume-roundoff thresholds, compares shell/solid/face
counts, and inspects stored bodies for sealed voids (ADR 0009). Export/render repeat current document warnings.
Empty-document saves inspect existing targets and require `allow_empty` to replace stored bodies (ADR 0012).
Pathless saves also require the first body-producing node id/kind from the last load/save to remain in the
recipe; an explicit path or `overwrite=true` permits another model (ADR 0014). `doc_new` needs a path. Save results report pre-save target existence as `replaced`.

`batch` extends this boundary to 1–200 modelling steps (ADR 0020). The registry first validates every
allowed tool name and deserializes its arguments through the registered tool's normal JSON-string decoder
and argument type, before opening a history boundary. It then invokes the existing handlers in order,
without recursively calling `Registry::call`: the enclosing batch supplies the only tool-edit snapshot
and undo entry. Every step retains its normal engine atomicity, rebuild and error checks, so later steps
can use earlier parameters and named/id-referenced sketches and features. A failure restores the enclosing
snapshot, adds no undo entry and reports the 1-based step index, tool and original error. Success records
tool `batch` and label `Batch (N steps)`; individual calls and batches interleave in the same 16-entry history.
Compact results report step/tool, created-node id/name where applicable and minimal operation details,
followed by current bodies/warnings/errors without an extra rebuild. `verbose=true` also includes every
step's normal result. Inspection, document replacement, disk output, undo and nested batches are excluded.

## Sketch dimensions
Entities are added fully dimensioned so the GUI can edit them: rectangle = width + height (`Distance` along
x/y between corners) + a centre point (`Midpoint` of the diagonal) pinned from the origin; circle = `Diameter`
+ centre pinned. Pinning uses `Distance{axis}` from the origin (a magnitude `|Δ|`: the side comes from the
initial geometry, so a negative value stores `-(expr)`), or `PointOnLine` on an axis for a plain zero.

## Topology and selections
Face and edge ids are QymCAD's persistent names (F-010), valid for one body after a rebuild; every feature makes a
new body, so the agent re-reads `topology` after each one. Selections are explicit ids or descriptions (`Sel`,
mapped onto `refs::Query`, with engine-only local-corner and kind filters lowered to ids). Topology lists,
adjacency ids and selection results are sorted by persistent id. `and` supports two or more operands;
kind previews use topology's classification, with `curve` matching arc/other edges (ADR 0013).
Corner signs use outward adjacent triangles plus analytic cylinder/cone normals at five native-polyline
arc-length fractions. Cone meridian vertices determine dr/dz; normals are perpendicular to that generator
and oriented by winding. Native circular geometry supplies circle/arc tangents. All signs must exceed the
normal/tangent uncertainty and agree (ADR 0010). Native aggregate surface kinds prove a plane only when no
unidentified curved type remains; otherwise mesh-planar normals retain a triangle-count-dependent allowance.
Fitted spheres and axis-bearing faces never receive a plane normal. Cone normals retain a one-degree allowance;
3° and steeper circular countersinks are tested, while native smoothness below ~1.5° can omit shallow rims.
For bare filters and positive intersections, corner previews count uncertain candidates matching the other
selection conditions; intersections requiring both corner signs report no omission because no edge is both.
Other compositions report the uncertain body total and corner-filter treatment without claiming omission or membership. Sampled G1
junctions and seams are excluded from that count.
Other faces retain facet normals and other curved edges chord tangents.
The angular-deflection allowances and finite samples are engineering
checks, not a guarantee for arbitrary unsampled surface behavior or variation along one facet side.
Edge selections are resolved when the feature is created and stored as pick lists:
stored edge queries break after the document is reopened in the app (F-024). Face selections (hole, shell, push
face) normally store dynamic native queries. If a selection contains a kind leaf, the entire selection is
resolved to a fixed persistent pick list at creation: QymCAD has no native kind query, and shell's
asked-versus-opened guard requires pick-list storage. No part of that frozen selection discovers faces
created later (ADR 0013). A selection that matches nothing is refused (an
empty edge list would mean "every edge", F-025).
Fillet/chamfer drop seam edges from the resolved picks and report their count in transient `rebuild.notes`;
seam-only selections are refused. Topology/select retain seam previews. The pinned bridge exposes face
names rather than native identity; bodies with duplicate face names do not infer or drop seams. Duplicate face-name warnings also
appear in document open/info and feature results; all face operands, including descriptive leaves nested
inside edge selections, are checked before native resolution.
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
| `transport.rs` | newline-delimited JSON-RPC: `initialize` (version negotiation, `instructions`), `ping`, `tools/list`, `tools/call`; notifications ignored; optional shared call log records requests, results, errors and shutdown |
| `calllog.rs` | per-process local JSONL log; environment/default-directory selection, UTC timestamps, image redaction, record/file limits, retention and thin unbuffered file I/O (ADR 0018) |
| `tools/mod.rs` | `Registry`, `tool()` / `tool_content()` constructors (schema and validation from the argument type), batch dispatch/history boundary, `State` (the open `Session`), `markdown()` for docs/TOOLS.md |
| `tools/common.rs` | `ObjRef` (id or name), `PlaneArg`, compact rebuild JSON |
| `tools/{doc,params,sketch,features,history,output}.rs` | one tool group each, `fn tools() -> Vec<Tool>`; `output` = `export`, `render` (image item, base64) |
| `tools/topology.rs` | `topology`, `select`; JSON forms of selections, axes and directions (hand-parsed for precise errors), shared with `tools/features.rs` |
| `lib.rs` | agent instructions and optional call-log-path helper; installed-app release check (F-018) |
| `main.rs` | moves fd 1 to stderr and serves the protocol on a duplicate of stdout (ADR 0005, F-019); opens the call log and installs its chained panic hook only when serving |

`main` shares the optional logger with `Server` and its panic hook through `Arc<Mutex<…>>`; lock poisoning is
handled without panicking. `Server::new` remains usable without logging. A `call` is written before the
registry invokes a tool, then `result` records the same sequence number, elapsed milliseconds and redacted
content. Image summaries retain MIME type and `base64_bytes`, the original base64 string's UTF-8 byte length.
Arguments are logged before registry decoding, preserving what the server received. JSON-RPC errors
and initialization are logged separately; notifications, `ping` and `tools/list` add no records or sequence
numbers. The transport records EOF or I/O shutdown; the panic hook records Rust panic details and chains the
previous hook so normal stderr diagnostics remain. Abrupt termination may leave only the unmatched `call`.
Batch steps run below transport dispatch, so a batch produces one call/result pair; the existing record
and file limits also bound 200-step arguments and verbose results.

Each complete JSON line, including its newline, uses one `write_all` on an unbuffered append-mode `File`.
There is no per-line fsync: completed writes survive a server-process crash, without promising durability
through power loss. Logging I/O failures or the file limit disable further records and report once on stderr;
serving continues. An unavailable working directory is recorded as `cwd: null` without disabling logging.
Startup exposes a path only after writing the `start` record; a failed startup write returns no logger and
reports only the disable note. `main` also uses `instructions_with_log` with the active log's path
to append exactly `"\nCall log of this server process (JSONL, for diagnosing problems): <path>."` to
`initialize.result.instructions`, with nothing added when off. This sentence is the only protocol-output
difference: all other initialize fields match, and every response after initialize is byte-identical between
logging on and off (raw lines, no normalization). Oversized string values are
marked and truncated; if structural data alone exceeds the record budget, the largest field becomes a marked
JSON-text prefix. Truncation caches unchanged field sizes and stops after 32 iterations with a marked
envelope when the record still cannot fit. See [ADR 0018](adr/0018-call-log.md) and
[call-log privacy](SECURITY.md#call-log-privacy).

Output policy: every body-volume field in rebuild results, `doc_info` and undo is `volume_mm3`, at native
floating-point precision without decimal rounding. Volume is a B-rep integral in mm³. Display clients may
round it for presentation. Mesh export reports per-body `mesh_volume_mm3`, also in mm³ (including GLB),
and omits it for exact STEP exports. Bboxes measure a fresh independent B-rep copy tessellated at nominal deflection
max(0.005 mm, 1e-5 of body diagonal), plus f32 coordinate rounding, consistently after open and rebuild (F-066). Curve extrema may be slightly
under-bounded; incomplete/failed tessellation falls back to conservative native padded bounds. Use volume and topology positions for accurate size/placement checks.
JSON bbox, sketch point x/y and world_frame coordinates/directions serialize to four decimal places
for presentation, rounded half away from zero, with
negative zero normalized to zero. Internal measurements retain full precision; decimal rounding can slightly
over-bound or under-bound the actual extent. Render caption bounds are rounded to .01 mm for display.
Topology coordinates keep their existing presentation rounding; sorting happens before formatting.
Sketch radii, constraint dimension values, parameter values and undo arguments retain full precision for reuse
as inputs. Both doc_open body lists report post-open geometry; stored metrics appear in mismatch warnings.

Error contract: an unknown tool or malformed request is a JSON-RPC error; a tool that runs and fails returns a
normal result with `isError: true` and the message (the model must see it). Argument structs use
`deny_unknown_fields` so typos fail loudly.

Before typed deserialization, `tools/mod.rs` accepts JSON object/array values encoded inside strings by MCP
clients. Objects and arrays may also be sent as JSON strings where the argument schema expects structured
values; advertised schemas retain their native object/array types. The generated schema guides decoding,
including references, alternatives, nested properties and array items: a string whose first non-space
character is `{` or `[` is parsed only when an explicit object/array schema applies. String-only names,
paths and expressions retain their meaning; plain forms such as `largest`, `+z`, `XY`, `sketch_y` and `t-2`
remain strings. Whole opaque selection/axis arguments may be encoded, but their children have no structural
schema and retain literal strings (including feature names such as `{x}` or `[1]`). Send nested opaque
selection/axis values as native JSON. Malformed structured JSON and nesting beyond 32 levels report the
argument path; the normal deserializer still rejects unknown fields and invalid values.

## Testing
- `tests/smoke.rs` — the kernel links and builds.
- `tests/golden_*.rs` (`golden_plate`, `golden_features`: every 3B operation) — parts with hand-computed volume/bbox; parameter edits; save/open round trip; **the GUI
  rebuild path** (`common::gui_edit_param` reproduces QymCAD.app's open → edit parameter → rebuild sequence).
- `tests/golden_export.rs` — every export format read back (STEP volume via `read_exact`, STL/3MF mesh volume and
  bbox, GLB binary positions/indices with asymmetric metres/+Y-up and outward-winding checks, OBJ per-object
  global offsets and geometric read-back), `finer_quality_never_loses_accuracy`, body selection;
  the placed-part test verifies world-space render/export geometry, and staging-directory tests verify safe
  output creation and hard-link replacement. Renders are decoded with the `png`
  crate (size, coverage, plate aspect 1.5, holes show background).
- `tests/session_behaviour.rs` — API contract: atomicity, errors, parameter bookkeeping.
- `crates/mcp/tests/protocol.rs` — spawns the real binary: initialize/list/errors, builds, saves, reopens and
  edits the golden plate purely through tool calls.
- `crates/mcp/tests/docs_fresh.rs` — docs/TOOLS.md equals the generated registry docs.
- Call-log unit tests cover UTC conversion, truncation, image redaction, retention and write/file-limit behavior;
  real-binary protocol tests use isolated temporary log directories. Other binary-spawning tests set
  `QYMCAD_MCP_LOG=off` so tests do not write to the developer's default log directory.

## Roadmap
| Phase | Scope | Status |
|---|---|---|
| 0 | Repository skeleton, pinned build, docs, ADRs, smoke test | done |
| 1 | Engine core: session, regenerate pipeline, params, rect/circle sketches, extrude/cut, offset plane | done |
| 2 | MCP transport + phase-1 tools, protocol tests, registration | done |
| 3 | Sketch entities & constraints (3A); topology + fillet/chamfer/hole/shell/arrays/mirror/revolve (3B); export + render (3C) | implemented and integrated; review fixes in progress on `phase3/codex-r2` |
| 4 | Acceptance on real parts (collet test plate, hanging shelf) vs build123d references | — |
| 5 | Release 0.1.0 | — |
