# ADR 0051: Native root-level assemblies and draggable joints

- Status: accepted (2026-11-08).
- Related: ADR 0008, 0020, 0022, 0036, 0042, 0045, 0048, 0050; F-069/F-073, F-143–F-147.
- Contract: `tasks/research-medA.md` §§4–7, with the package A implementation request's overrides.

## Scope and native records

Expose one root Assembly containing root-child, multi-body Parts. A Part's placement moves all its bodies;
it creates no Move feature and does not change body ids or local geometry. Geometry creation keeps the existing
owner-routing rules: an explicit body's Part or a sketch's Part supplies its modelling context, with the previous
active component restored afterwards. `doc_new` still creates its initial empty Part; rename that Part to
`carcass` rather than introducing an unused extra Part.

Persist native components, grounding, FaceCenter connectors, joints, as-built frames, drive slots and native
`feat_dims` formulas. No separate relationship file or file-format extension is introduced. Rigid and Slider
are the only new joint kinds. Excluded: Revolute and other joint creation, limit editing, cloning/instances,
component patterns, Piece/MirrorPart, nested assembly creation/editing, Group/Width/Tangent, relations, library
files, collision solving and migration of an existing single-Part model into Parts. Existing imported trees,
unsupported records and limits remain readable, reported and preserved; affected unsupported mechanisms refuse
edits rather than being flattened. An independent healthy mechanism does not prevent an unrelated edit.

## Tools and references

All argument objects reject unknown fields. `PartRef` is a numeric id or unique raw/localised Part name;
`{root:true}` is additionally available for root navigation. `JointRef` is a numeric id or unique native joint
name. These resolvers remain separate from body/feature resolution. Duplicate labels remain legal; ambiguity
requires an id. Names use the existing trimmed, case-preserving object-name policy.

| Tool | Arguments | Behaviour and result |
|---|---|---|
| `part_new` | `{name}` | Create and activate a new root-child Part, regardless of navigation; return `part:{id,name,parent}`, `active_component`, `changed:true`. |
| `part_select` | `{part}` | Select the Part or `{root:true}`; return previous/active components, `context_kind`, `changed`. A changed selection is persisted and undoable; no-op standalone selection adds no history. Root navigation retains ADR 0048's modelling ambiguity refusal. |
| `part_place` | `{part,translation_mm:[x,y,z],rotation?:{axis:[x,y,z],angle_deg}}` | Replace a free, ungrounded, unconstrained root-child Part's parent-local placement. Rotation is literal finite axis-angle; omitted rotation is identity. Return local/world matrices and affected current body ids. |
| `ground` | `{part,grounded:bool}` | Set native grounding, solve and validate the complete mechanism; refuse ungrounding a joint's anchor A Part or grounding a Slider's B Part; return Part/grounding, placements, diagnostics and `changed`. Joint creation never grounds implicitly. |
| `joint_create` | `{name?,kind:"rigid"\|"slider",a:{body,face},b:{body,face},mode:"align"\|"as_built",axis?:{base_plane:"XY"\|"XZ"\|"YZ"},offset?:Num}` | Make native connectors and one joint; return joint id/name/kind, connector ids, resolved anchors/local frames, as-built/side state, drive/read/formula/limits, actual signed world axis, placements and diagnostics. |
| `joint_set` | `{joint,offset:Num}` or `{joint,release:"offset"}` | Exactly one Slider action. Return prior/requested/evaluated travel, current `offset` readout, drive/formula state, pose/axis, limits, `changed` and diagnostics. |
| `joint_delete` | `{joint}` | Remove that joint and its unshared, non-standalone auto-connectors, retain current placements and solve the remainder. Return removed ids, placements and diagnostics. |
| `assembly_info` | `{}` | Read-only root/active component, components/Parts with parent/local/world frames/grounding/current bodies, connectors, ordered joints and native slot values/units/formulas/limits, actual axes and separate assembly diagnostics. |

`rename.target` additionally accepts `{part:PartRef}` and `{joint:JointRef}`. There is no `part_delete` tool.
Creation tools and all changed assembly mutations join the existing registry and batch allowlists. Compact
batch results expose created Part/joint ids and names, alongside existing created-node fields. A successful
batch is one global undo entry. A failed later step restores active context, placements, grounding, connectors,
joints, formulas, original live geometry and diagnostics; existing allocator rules remain unchanged. History
rollback remains one global timeline prefix, and feature reorder remains same-parent.

## Anchors, motion and formulas

`body` is an existing body reference. `face` is its persistent face id or an existing selection description;
the existing exactly-one-face rule resolves it before mutation. Descriptions let a batch create joints without
a topology read. Each creation anchor must resolve to one current, unambiguous planar face with a fully oriented
native frame. The stored native FaceCenter is Part-local; no global face-id or world-coordinate anchor is stored.
The owners must be different root-child Parts. A must be grounded. Slider B must be ungrounded. `ground` keeps this
invariant for existing joints: it refuses a change that ungrounds an anchor A Part or grounds a Slider's B Part
(violations already present in an opened file do not block unrelated grounding).

Slider requires `axis.base_plane`, stored as an explicit native `axis_ref` on both connectors. XZ selects native
−Y travel; do not infer travel from the selected face normal. Rigid refuses axis and offset. `as_built` captures
the existing relative pose **before the first solve**; `align` intentionally aligns the selected connector frames.

Offset is an **absolute** native travel coordinate in millimetres. `Some(0)` is a real zero request. Numeric
offset clears the existing offset formula and writes native `drive[1]`. A string validates names, cycles and
finite evaluation before native evaluation, persists under native `feat_dims[joint]["offset"]` and regenerates.
Readouts are not commands. Native numeric drives follow body dragging; a saved formula reasserts its target on
regeneration. `release:"offset"` clears **both** formula and drive, keeps the reached pose and makes a later drag
persistent. Global parameters remain document-global. Existing native limits are preserved/reported; evaluated
requests outside them refuse rather than accepting native clamping. Limit editing is deferred.

## Acceptance and atomic restoration

One engine assembly acceptance routine checks feature errors, native joint faults, actual anchor witnesses,
returned solve errors, violated ids, conflict, finite rigid transforms and requested-versus-read drive values.
Inspection solves an independent Project for diagnostics without changing the live recipe or placements.
Open/rebuild report an existing unhealthy assembly explicitly; absence of feature errors is not healthy-joint
evidence. Missing or ambiguous actual face witnesses cannot be replaced by FaceKey fingerprint fallback.

Every mutation that can affect placements or anchors must use the same acceptance policy. Geometry changes,
parameter/dimension edits, deletion, suppression, rollback and reorder that invalidate an existing joint refuse
atomically. A consumed source is not automatically a lost anchor: a retained actual native face witness can
remain meaningful. Positive unrelated edits remain supported. Snapshots retain the whole Project, original live
B-rep handles/absence and diagnostics; failure restores them without a repair regeneration or solve. All new
mutations use the existing history boundary, including persisted `part_select`; no-op standalone calls skip it.

No self-gating public function is called under an outer kernel gate. Shape reads/serialization use short gates;
restore, regenerate, reporting, `export_mesh` and history-copy operations run after releasing them (F-069/F-073).

## Sketch host ownership

ADR 0048 routes a new sketch to the Part that owns its datum plane or face. With `part_select` that routing
became a silent trap: a datum shared from the carcass put drawer bodies into the carcass (live run, two
Sliders then conflicted). While a Part is active, `sketch_create` on a datum owned by another Part is refused,
naming both Parts and the `part_select` remedy; base planes always use the active Part. A face is an explicit
pick of a body and keeps routing to that body's Part (review R1, imported bodies); the result reports
`owner_part` in documents with assembly structure so the routing is visible. With the root Assembly active,
ADR 0048 routing is unchanged.

## Coordinate/reporting compatibility

`doc_info` gains components, active component, assembly health and `owner_part` on bodies, sketches and timeline
features. Bodies expose `world_transform`, `bbox_local` and `bbox_world`. Body records carry `owner_part`,
`world_transform`, `bbox_frame`, `bbox_local` and `bbox_world` only when the document has assembly structure
(more than one Part or any non-identity placement); a plain single-Part document keeps the pre-assembly body
shape byte-for-byte. Rebuild reports carry `assembly` (health diagnostics) only when the document has joints,
mates or relations; `doc_info.assembly` and `assembly_info` are always present. Preserve legacy `bbox` deliberately:
B-reps stay Part-local (`bbox_frame:"part_local"`); mesh bodies stay world-space (`bbox_frame:"world"`). New
consumers use the explicitly named bounds. For B-reps, world bounds transform the eight corners of the local
reporting bbox; this is conservative for arbitrary rotations and exact for the orthogonal box test fixtures.
Mesh world bounds transform actual mesh vertices. Existing presentation rounding remains unchanged.

Topology and selection geometry/directions remain Part-local, explicitly reporting `coordinate_frame:"part_local"`;
measurement remains world-space. Export/render apply each body's complete world placement once. The reproduced
mesh render-bbox double transform is corrected by using the named world bounds. Existing export placement
handling remains unchanged: world-coordinate mesh/STEP geometry with identity flat export-node placement
(ADR 0045). Exchange formats contain placed geometry, not editable joints; `.qcad` retains the mechanism.

## Refusal strings

Deterministic ids and sorted lists replace the placeholders. Existing selection cardinality/stale-id messages
remain unchanged. `Error::Assembly` preserves these bare messages through engine/MCP reporting.

```text
no part with id <id>
no part named '<name>'
several parts are named '<name>' (<ids>); use the part id
no joint with id <id>
no joint named '<name>'
several joints are named '<name>' (<ids>); use the joint id
component <id> is a nested assembly; nested assembly editing is not supported in package A
part_place requires a free, ungrounded part; part <id> is grounded or constrained; use joint_set
part_place rotation axis must be finite and nonzero
joint_create requires anchors in two different parts; both bodies belong to part <id>
joint_create requires root-child parts; part <id> is nested
joint_create requires grounded anchor A; ground part <id> first
joint_create requires grounded anchor A; part <id> is moved by slider joint <id>, so chained mechanisms are not supported in package A
joint_create supports planar face anchors in package A; body <id> face <face> is <kind>
joint_create requires a current body; body <id> is consumed, suppressed or rolled back
joint_create cannot resolve a fully oriented frame for body <id> face <face>; rebuild and choose a usable planar face
slider joint_create requires axis.base_plane (XY, XZ or YZ)
rigid joint_create does not accept axis
rigid joint_create does not accept offset
slider target part <id> is grounded; unground it first
joint_set requires exactly one of offset or release
joint <id> has no editable offset slot for kind <kind>
joint <id> offset <value> mm is outside [<min>, <max>] mm; nothing was changed
assembly solve failed: <present causes: faulty joints, violated joints, solver errors, requested joint values not reached, non-finite joint values, non-rigid part transforms, formula/constraint/relation/feature errors, conflicting joints or drives>; nothing was changed
edit leaves joints <ids> unsatisfied (conflicting joints, drives or grounding); nothing was changed
joint <id> needs its anchor part <id> grounded; nothing was changed
slider joint <id> moves part <id>, which must stay ungrounded; nothing was changed
edit would invalidate anchors of joints <ids>; delete those joints first; nothing was changed
assembly edit touches unsupported constraints or relations <ids>; package A supports root-level Rigid and Slider joints
part instances are not supported for multi-body source parts in package A
component <id> is a part instance; instance editing is not supported in package A
assembly feature <id> uses derived components; derived component editing is not supported in package A
```

`no joint named '<name>'` fills an omission in the research's list. Rigid's explicit offset refusal makes its
non-adjustable contract unambiguous. The explicit instance/derived-component refusals cover affected imported
PartInstance, MirrorPart, ComponentPattern and Piece records, including copies whose owner differs from the
recipe node parent. The before-edit recipe is checked too, so removing the last such record cannot bypass
the guard. Geometry creation in affected nested mechanisms uses the same acceptance boundary; selecting navigation remains readable.
These additions do not alter measured native behaviour; they are server policy.

## Verification boundary

`golden_assembly_frames` covers translated/rotated B-rep and mesh bounds, rendering, STEP and 3MF readback and
ownership/frame reporting. `golden_assembly_app` exercises native cached and forced saved-file regeneration,
formula parameter-window scheduling with no B-rep rebuild, released/numeric dragging and saved persistence.
Its cabinet test builds three Parts and twenty plates, sums 60251472 mm³ and moves exactly drawer1's six bodies
by −300 mm in world Y. The cabinet plan is copied byte-for-byte from the research into `crates/engine/tests/data/assembly-cabinet-plan.json`
so the acceptance suite builds without the ignored tasks directory. The common app helper uses native
Project/kernel calls, with no Session repairs.
Engine and real-stdio tests cover acceptance/history/schema/batch boundaries. Exact executed results, red and
mutation evidence and any unfinished verification belong to `tasks/A-report.md`.

The app-data path is headless native evidence. The full GUI wrapper and visible-app open/drag/save check are
unavailable and are not claimed. Native PartInstance's multi-body limitation (F-146) and the excluded mechanism
types remain excluded despite readable native records.
