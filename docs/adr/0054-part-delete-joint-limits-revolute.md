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

## 3. Revolute hinges on straight edges

### App behaviour (source, pin f25f27d)

- `JointKind::Revolute` frees slot 0 only, the rotation about the connector frame's Z (`feature/mates.rs:380-384`,
  `asm/joint.rs:75-77`). The axis is the anchor's frame Z (`model/assembly/anchors.rs:265-380`): an `EdgeMid` on a
  straight edge gives the edge tangent through its midpoint; a planar `FaceCenter` gives the face **normal**.
- The app's Joint tool infers the anchor type under the cursor (a face → FaceCenter, an edge → EdgeMid;
  `qymcad-assembly/src/picks.rs:200-245`); the app's own by-hand test makes Revolute on edges
  (`gui/every_kind_is_made_by_hand.rs:70-72`). Two clicks → two connectors + `add_joint`, optional «as built»
  captured before the first solve, one undo step.
- Drive `drive[0]` in degrees through `set_joint_drive(j, 0, …)`; a formula at `feat_dims[j]["angle"]` reasserts on
  rebuild (F-144). Limits on slot 0 as in §2. The solver moves B's Part.

### Tool contract

`joint_create {kind:"revolute", a:{body, edge}, b:{body, edge}, mode:"as_built", angle?, limits?}`: exactly one
current straight edge per anchor (id or edge description; `JointAnchor::edge`), `EdgeMid` connectors, `add_joint`,
`set_joint_as_built`, limits, then the angle drive. `joint_set {joint, angle}` / `{joint, release:"angle"}` (engine
`joint_set_slot`); `joint_limits` on the angle slot. Results report `angle`, `read`, `unit:"deg"`, the angle
`expression`, the slot-0 `signed_world_axis` and anchors with `edge`. The opening sign follows the edge frames (−90
opens the golden door outward); it is reported, not normalised.

### Server rules (each is native semantics made explicit, or a protective refusal)

- **Edge anchors only for Revolute**, face anchors only for Rigid/Slider: a planar face would turn the door about the
  face normal, like a wheel (research R9).
- **as_built only and collinear axes** (distance ≤ 1e-3 mm, angle ≤ 0.01°): natively an as-built hinge pivots about
  B's edge and ignores A's (R10), and align folds the door into the carcass at 0° (R10). Place the Part on the hinge
  first.
- **B ungrounded (F-159, protective)**, at creation and in `ground`: a grounded B does not turn and nothing is
  reported.
- **Angles within (−180, 180], angle bounds within [−180, 180] (F-158, protective)**: QymCAD reads the angle through
  `atan2` with no unwrapping, so a drive outside the range never matches its reading and a released door snaps to
  the other bound. Checked on creation, `joint_set`, `joint_limits` and every evaluated edit (an angle formula is
  judged after the limits clamp it).
- Edge witness: `inspect` now checks `EdgeMid` anchors like faces (exactly one current edge with the persistent id on
  a live body). As for faces (ADR 0051), a fillet that consumes the anchored body keeps the native witness; deleting
  or suppressing the body is refused.
- Chained mechanisms stay refused (an anchor A moved by a Slider or a Revolute). ADR 0051's unsupported-kind refusal
  now lists Rigid, Slider and Revolute.
- `ground` checks for unsupported mechanism records before it solves. The ADR 0051 contract test used a Revolute as
  its unsupported stand-in; with Revolute supported it uses a Cylindrical joint, whose solve failed first and hid the
  unsupported-record refusal, so the check now runs before the solve.

### Tests

`golden_revolute` (cabinet door: −90 opens outward with the pivot edge fixed, world bbox by hand; limits [−110, 0]
clamp −150 to −110 and report it; formula `-swing` and a parameter edit held at the stop; release; undo; save/reopen;
creation refusals: non-collinear axes, align, planar faces, grounded door, bounds outside ±180, min > max, angle 200,
edge anchors on a Slider; edit refusals: grounding the door, limits, angle domain, offset on a Revolute, a formula
pushed out of the domain; the hinge-edge witness; native evidence for F-158 and F-159) and `mcp/tests/h3_revolute`.
