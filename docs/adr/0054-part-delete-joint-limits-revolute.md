# ADR 0054: Part deletion, joint limits and Revolute hinges

- Status: accepted (2026-11-11).
- Related: ADR 0051 (root assemblies; this ADR extends its joint kinds and lifts its limit-editing exclusion), 0053
  (clones; `part_delete` is the clone removal it lacked); F-154–F-159.
- Plan: `tasks/plan-H.md` (binding decisions). Research: `tasks/research-H.md`, harness `tasks/research-H/` and its
  `run.log` (native pin f25f27d).
- Principle: each tool is one existing QymCAD app action with the GUI's limits. Where the native result is silently
  wrong, only that case is refused and recorded in FINDINGS; no workaround is written.

## 1. `part_delete`: the app's tree Delete of a Part

### App behaviour (source, pin f25f27d)

- Del / tree «Delete part» → one yes/no question → `execute_delete` `Sel::Component` (`qymcad/src/gui/commands.rs:215-244`):
  one undo step; if the Part is on the active path, `set_context_to(root)` first; then
  `Project::delete_component(id)` (`qymcad-core/src/model/components.rs:171-252`); then a dirty-only resync.
- `delete_component` removes the subtree, its timeline nodes, meshes, body `feat_dims`, sketches, datums, every
  connector owned by the subtree or anchored on its bodies, and every joint using such a connector. Joints are not
  removed through `delete_joint`, so the other side's connector and the joint's own `feat_dims` stay (F-155).
- The root is a no-op. The app asks no question about clones, joints, grounding or an active rollback.

### Tool contract

`part_delete {part: PartRef}` on a root-child Part. Runs exactly the native sequence above (root becomes active
when the deleted Part was active, otherwise the active component stays), then the dirty rebuild and the ADR 0051
acceptance. One undo step; allowed in batch. Result: `removed:{part,bodies,joints,connectors}`,
`native_leftovers:{connectors,formulas}` (what the app leaves, F-155), `active_component`, `placements`,
`diagnostics`, `changed:true`. A Part moved by a deleted joint keeps its reached placement and becomes free.
Deleting a clone Part keeps its source; the ADR 0053 instance-node refusal now names `part_delete`.

### Refusals

- The root assembly (`{root:true}` or its id); a nested Part; a nested assembly (existing).
- **A source with clones (F-154, protective).** Natively the clone keeps its old body with no error in the GUI's
  dirty-only resync, and only a forced rebuild reports `SourcePartHasNoBody` while still showing the stale body.
  Delete the clones first.
- **An active history rollback (F-156, protective).** `delete_component` keeps the `rollback` index, so nodes past the
  bar are built at the next rebuild.
- Parts in component patterns, mirrors, pieces, constraints or relations (existing ADR 0051 policy).

### Leftovers (F-155): mirrored, not cleaned

The plan's decision: the server does not clean what the app leaves (that would be behaviour of our own). It reports
the leftovers, and its own checks ignore them: `param_users`/`param_delete` no longer count a `feat_dims` entry
whose owner is no live object and whose keys are all joint/connector keys (`part_delete::orphan_joint_formulas`).
Native regeneration never reads such entries (`item_value` answers only live joints).

### Tests

`golden_part_delete` (cabinet with a formula Slider and a copy; anchor-Part deletion; clone deletion; undo;
save/reopen; refusals with snapshot/unchanged; native evidence for F-154/F-155/F-156) and `mcp/tests/h1_part_delete`
(stdio registry, strict arguments, batch, batch rollback, undo).

## 2. Joint limits: the app's «Limits» section

### App behaviour (source, pin f25f27d)

- `Joint.limit_min/limit_max: [Option<f64>; 3]` per slot (`feature/mates.rs:516-521`). The joint box's folded
  «Limits» section offers one row per **free** slot: a tick switches a bound on at 0.0 or off to `None`
  (`qymcad-assembly/src/popup.rs:398-405`), a field in −100000..100000 (`popup.rs:113-118`) takes a number or a
  formula kept at `feat_dims[joint]["min<slot>"|"max<slot>"]` (`popup.rs:128,229-240`) and imposed on every rebuild
  (`model/item_values.rs:144-176`). No min ≤ max check, no check against the current value.
- A limit **clamps**: `set_joint_drive` clamps a written drive (`model/assembly/joints.rs:382-389`), `solve_joints` →
  `clamp_free` clamps every stored drive, including formula drives (`model/assembly/drive.rs:353-356`), and free slots
  are pinned at a violated bound (`asm/bridge.rs:439-480`). The only signal is `joints_at_limit()`, the app's at-limit
  mark (`gui/panels_props.rs:322`).

### Tool contract

`joint_limits {joint, slot?, min?, max?}` (H2: Slider `offset`; Revolute `angle` in §3). Omitted keeps a bound;
`null` ticks it off and drops its formula; a number ticks it on with that value and drops a formula; a string is a
formula stored where the app stores it, validated as joint formulas are, then rebuilt. `slot` defaults to the
joint's only free slot; naming another is refused; Rigid has no free slot. `joint_create` takes `limits:{min?,max?}`,
applied before the first drive. One undo step; batchable. `assembly_info` and joint results report `limit_formulas`
and `at_limit` (slot names); diagnostics report native `at_limit` pairs and `clamped` formula drives.

### Clamping is the app's meaning of a limit: reported, not refused

Plan-H overrides ADR 0051's "requests outside limits refuse rather than accept native clamping". `joint_set`, a
drive formula and a new bound past the current value are clamped as natively. The result is never silent:
`joint_set`/`joint_create`/`joint_limits` return `clamped:{joint,slot,requested,applied}` and rebuild reports
(`param_set`, `dimension_set`, open) list formula drives a limit holds in `assembly.clamped`. The request-versus-read
check now compares the reading with the **applied** (clamped) drive; any other mismatch is still refused.
The two ADR 0051 tests that pinned the old refusal were rewritten to the clamp (`assembly_tests`,
`assembly_contract_tests`).

### Refusals

- **min > max (F-157, protective)**: natively every value goes to max. Refused on `joint_limits`, `joint_create`, and on
  every evaluated edit (a `param_set` that pushes a limit formula past the other bound), through
  `validate_joint_formulas`, which now evaluates limit formulas too.
- A bound outside the app's field range ±100000.
- `joint_limits` with neither `min` nor `max`; a named slot that is not the joint's free slot; a Rigid joint.

### Tests

`golden_joint_limits` (numeric clamp of `joint_set`, new bound past a driven and a free slot, formula limits and a
formula drive held by a limit, clear, creation limits, undo, save/reopen, refusals with snapshot/unchanged, native
F-157 evidence) and `mcp/tests/h2_joint_limits` (null vs omitted, clamps, batch, batch rollback, undo).
