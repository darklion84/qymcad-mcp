# ADR 0020: Atomic modelling batches with one history boundary

- Status: accepted (2026-10-12).

## Context
Predictable parts can require dozens of tool round trips, often dominated by parameter creation before
sketches and features. The existing modelling handlers already define argument compatibility, reference
resolution, rebuild checks and error messages. Session history retains original live B-reps and recipe
snapshots for exact rollback and bounded undo (ADR 0008). Batching should reduce round trips while keeping
those established semantics and leaving all QymCAD calls inside the engine.

## Decision
- Add `batch` with `{ "steps": [{ "tool": "<name>", "args": { ... } }], "verbose": false }`.
  `verbose` is optional and defaults to false. Refuse fewer than 1 or more than 200 steps.
- Share the registry's modelling mutation allowlist: `param_set`, `param_delete`, `sketch_create`,
  `sketch_add`, `sketch_constrain`, `sketch_remove`, `plane_offset`, `extrude`, `revolve`, `fillet`,
  `chamfer`, `hole`, `shell`, `push_face`, `linear_array`, `circular_array`, `mirror`, `feature_delete`.
  Refuse every other tool, including document tools, undo, inspection, export, render and nested batches.
- Validate every step before starting the history boundary or invoking any handler. Reuse each registered
  tool's typed deserialization and schema-aware JSON-string decoder, including unknown-field refusal and
  literal strings inside opaque selection/axis objects. A validation error reports the 1-based step index,
  tool and reason; no mutation or history entry occurs. This checks argument types, not references to
  objects that may be created by earlier steps.
- Open one existing `begin_tool_edit` / `finish_tool_edit` boundary for the whole batch. Invoke registered
  handlers directly in order, preserving each handler's engine transaction, rebuild and checks. Do not
  recurse through `Registry::call`, which would add per-step history entries and nested tool boundaries.
  Native deletion shares the enclosing boundary as it does for an individual modelling call.
- On a handler failure, restore the enclosing snapshot without regeneration. Restore parameters,
  sketches, timeline, names, original live B-reps and diagnostics, and retain the existing history. Report
  the 1-based step index, tool, original error and that nothing was changed. Preserve the monotonic id
  allocator required by ADR 0008/F-056; rollback never permits a handed-out id to alias a future node.
- On success, retain one undo entry with tool `batch`, the batch arguments and label `Batch (N steps)`.
  Batch and individual calls share the same 16-entry retention limit and undo in reverse call order.
- Return `{ "ok": true, "steps": [...], "bodies": [...], "warnings": [...], "errors": [...] }`.
  Each step reports its 1-based `step`, `tool`, optional `created: {id, name}` for a created node, and compact
  operation details. `verbose=true` additionally includes its full normal result. Bodies use the normal
  result id/name, `volume_mm3` and bbox presentation. Collect current final bodies and diagnostics without
  another rebuild. Images are impossible because render is excluded.
- Transport sees one batch call and result, so ADR 0018's logging needs no per-step records or new limits.
  Its bounded record handling also applies to 200-step arguments and verbose results. No new crates or
  QymCAD API calls in the MCP layer are needed.

## Alternatives considered
- **Defer all rebuilds to one final rebuild:** this changes when references and selections resolve, obscures
  the failing step, and bypasses handlers' existing rebuild and error policies. Retaining every step's
  rebuild provides equivalence with individual calls; batching targets round-trip cost here.
- **Keep successful steps after a failure:** partial success leaves an unexpected intermediate part and
  forces the caller to reason about retries and history. Whole-batch rollback gives one predictable
  document transition and one undo entry.
- **Call `Registry::call` recursively for each step:** this would create per-step snapshots/history entries
  inside an enclosing boundary. Direct registered-handler dispatch reuses modelling logic while keeping
  history ownership at the batch boundary.

## Consequences
Batches reduce protocol round trips without changing individual tools or modelling behavior. Steps can
refer to earlier objects by name or known id, but cannot pause for topology, sketch inspection or a render;
use individual calls or multiple batches around those decisions. Rebuild costs remain per step. A batch
retains one document snapshot rather than one per step; verbose responses can still be large and call-log
truncation intentionally prevents logs from becoming complete replay archives.
