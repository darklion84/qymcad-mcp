# ADR 0053: Part instances and copies

- Status: accepted (2026-11-09).
- Related: ADR 0048, 0051 (root assemblies; this ADR replaces its blanket instance refusal for single-body sources), 0052 (template); F-146, F-152, F-153.
- Revised 2026-11-09 after the independent review (`tasks/G-review.md`): rollback, bodiless sources, clone removal, grounded copies, copied names.
- Research: `tasks/research-G-instances.md` (native source reading and the executed harness `tasks/research-G`). Its §6 recommendation to unite plates before cloning is **not** followed: the owner's plan keeps the drawer's plates separate for the cut list and forbids workarounds.

## Context

QymCAD has two tree actions that give a second Part from an existing one. The server exposes both as they are and keeps their limits; it adds a refusal only where QymCAD would otherwise store a wrong result silently.

## `part_instance`: the app's «Make a clone»

### App behaviour (source, pin f25f27d)

- Menu: the Part row's context menu item `act-clone-part` (en "Make a clone", ru «Создать дубль»; `gui/panels_tree/build.rs:536`, action `:584-591`), one undo step through `edit_over`.
- Recipe: `Project::clone_part(src, parent)` (`model/assembly/tree.rs:283-298`) adds a Part to the source's parent assembly with **the source's own name** and one `PartInstance { src_comp, body }` node named `name-instance`. `src_comp` is `instance_origin(src)`: a clone of a clone repeats the original (`tree.rs:347-363`).
- Placement: `free_place_beside` (`tree.rs:305-345`): the source's placement shifted along X by `k·(len + max(len/5, 5 mm))`, the first `k` whose box meets no body.
- Regeneration copies `active_body_before(src_comp, at)`, the source's **last** unconsumed solid body (`regen/prep_bodies.rs:29-40`); `push_timeline` keeps every clone after the source's nodes pushed later (`timeline/order.rs:65`), so later source edits reach the clone. A clone made while the history is rolled back is inserted at the bar, above the source's nodes past it, and never sees them (F-152).
- Removal: the app's tree Delete on a clone Part removes the whole Part (`delete_component`, `model/components.rs:171`). Deleting only its PartInstance node would leave an empty Part, which is not what the app's Delete does.
- The GUI never models inside a clone: a double click on it enters the original.

### Tool contract

`{source: PartRef, name, translation_mm?, rotation?}`. `name` is required and must differ from every component name (the app's same-name clone would make `PartRef` ambiguous); the tool renames the native clone to it. Placement follows `part_place`; without `translation_mm` the app's beside placement is kept; `rotation` requires `translation_mm`. The source is a root-child Part; the clone goes into the root assembly. The clone is built at once; one undo step; allowed in batch; the active Part does not change. The result is `part_result` plus `part.instance_of`.

### Protective refusals (server policy)

- **No clone during a history rollback (F-152).** Natively the clone would miss the source's later features, and a second source body past the bar would pass the single-body check. `part_instance` is refused until the history is rolled to the end.

- **Single-body source (F-146).** A source with more than one solid body is refused: a native clone copies only the last body and drops the others without an error. The message points to `part_copy`, which copies all bodies. A source with no solid body yet is refused with "model it first".
- **A cloned source stays single-body.** An edit that gives an existing cloned single-body source a second solid body (e.g. `extrude op:new_body` in it) is refused atomically: `edit makes part <src> multi-body while instances [<ids>] copy it (native clones copy only the last body, F-146); nothing was changed`. Sources that were already multi-body in an opened file keep ADR 0051's refusal and its contract test.
- **No stale clone (F-153).** An edit that leaves a cloned source with no solid body while a built clone keeps one — suppressing the source's base — is refused atomically, because QymCAD keeps the clone's old body with no error. A bodiless source is otherwise allowed: rolling the history back above the source leaves the clone past the bar and unbuilt, and rolling forward rebuilds it (the first version refused every bodiless source; that was stricter than the app and is withdrawn). Suppressing a later source feature reaches the clone.
- **Removing a clone.** Deleting or suppressing a clone's PartInstance node alone is refused with a message that says the app removes a clone as a whole Part, which the server has no tool for, and that undo removes it.
- **No modelling inside a clone.** Any node or sketch added to, changed in or removed from an existing clone Part — a feature whose target body is the instance body, a sketch on its face, a sketch made while the clone is active — is refused atomically: `part <clone> is an instance of <src>; edit the source part`. This mirrors the GUI's redirect into the original. The check runs in every acceptance path (`assembly_accept_edit`) and, for feature edits, before their rebuild.

### Assembly edits

ADR 0051 refused every assembly edit that touched a PartInstance. For a clone of a source with exactly one solid body, `part_place`, `ground`, `joint_create`, `joint_set`, `joint_delete` and `rename` now work as for any Part, and the source stays editable. Only the refusal for multi-body sources remains; ADR 0051's `component <id> is a part instance; instance editing is not supported in package A` is withdrawn. A Slider on a clone (as_built) moves only the clone (`golden_instances`).

## `part_copy`: the app's Copy / Paste of a Part

### App behaviour (source, pin f25f27d)

- Menu: the Part row's `act-copy-ctrl-c` puts `TreeClip::Component{id}` into the tree clipboard (`gui/panels_tree/copy.rs:8-56`); `act-paste-here` on a row (`build.rs:532`, `:578-581`) selects that row and pastes (`gui/panels_tree.rs:103-187`).
- **Target parent:** the selected component if it is an assembly; for a selected Part, its parent assembly; otherwise the active context. A paste into a Part is refused (`status-paste-needs-assembly`). So «paste here» on the source's own row puts the copy beside it in the same parent assembly; the server uses exactly that target.
- **Recipe:** `Project::clone_component(id, target)` (`model/components.rs:293-298`) → `clone_subtree_into` (`model/timeline/clone.rs:15-209`): every component, sketch, datum, node, body and `feat_dims` entry of the subtree gets a new id; face/edge names are remapped; references leading outside the subtree are kept as they are; the copied nodes are appended in the source's order and their bodies are rebuilt. Formula texts are copied verbatim, so document-global parameters drive both Parts, while later feature edits of either Part do not reach the other.
- **Name:** the source's name made unique by `name_apart` (`tree.rs:162-172`): `Name (2)`, `Name (3)`…; a copied clone keeps its name.
- **Placement:** the source's parent-local transform copied verbatim (`clone.rs:48`): the copy stands exactly on the source. `grounded` and `visible` are copied too.
- **Joints and connectors:** not copied. `clone_subtree_into` touches no `joints` or `connectors`; the source's joints stay on the source and the copy has none.
- The paste path marks the document dirty for the scheduler's rebuild; no `edit_over` appears in it (whether the app's undo records it as one step was not traced).

### Tool contract

`{source: PartRef, name, translation_mm?, rotation?}` with the same naming and placement rules as `part_instance`; without `translation_mm` the native placement (on the source) is kept. The copy keeps the source's grounding, as the app's paste does; like `part_place`, a grounded Part is not placed, so `translation_mm` is refused for a grounded source (copy, unground, then `part_place`). Sketch, feature and body names are copied verbatim, as natively (`clone.rs:200`), so later name references become ambiguous and need ids; the tool's own required unique name applies to the Part only. During a history rollback the copied nodes land past the bar (the copy is empty) and build in full when the history is rolled forward; nothing is silently wrong, so this is allowed (`golden_part_copy::copy_during_a_history_rollback_builds_the_whole_recipe_when_rolled_forward`). The result is `part_result` plus `copied_from` and `body_map` (`[[source body, copy body], …]` for current bodies). A copy of a clone is natively another clone and reports `part.instance_of`. One undo step; allowed in batch. No refusal is added: every body is copied, so the multi-body drawer needs no union (`golden_part_copy`: two drawers of five separate plates, each on its own as_built Slider, one `param_set` changes both).

## Reporting

`doc_info.components` and `assembly_info.components/parts` carry `instance_of` on a clone Part and `instances: [ids]` on a source; both fields are omitted otherwise, so documents without clones keep their shape.

## Consequences

Evidence: `crates/engine/tests/golden_instances.rs` (volume and local bbox equal, world bbox shifted by placement, beside placement 48 mm, clone of clone, source parameter edit followed, exact undo, save/reopen, the three refusals with an unchanged document, Slider on a clone), `crates/engine/tests/golden_part_copy.rs` (cabinet hand volumes 22625280 / 9224352 → 9505152 mm³, five plates per drawer, independent travel −300/−150 mm, healthy joints after the parameter edit, independent recipe, native placement/grounding, undo, save/reopen), `crates/mcp/tests/g34_instances.rs` (stdio registry, strict arguments, batch and batch rollback, undo, doc_info fields). Native evidence for F-152 and F-153: `golden_instances::native_clone_at_the_rollback_bar_misses_later_source_features_f152`, `native_suppressed_source_base_keeps_a_stale_clone_body_f153`. Not covered: a visible-app check.
