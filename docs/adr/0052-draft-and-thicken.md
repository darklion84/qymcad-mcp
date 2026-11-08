# ADR 0052: Draft and Thicken

- Status: accepted (2026-11-08).
- Related: ADR 0013 (frozen kind leaves), 0041 (re-pick), 0048/0051 (Part routing); F-011, F-034, F-148, F-149, F-150, F-151.
- Research: `tasks/research-G-draft-thicken.md` (native source reading and the measured experiment `tasks/exp-draft-thicken`).

## Context

QymCAD has native Draft (`FeatureKind::Draft`) and Thicken (`FeatureKind::Thicken`) with app commands (COVERAGE P9, P15). The server exposes existing QymCAD behaviour; it is not a CAD of its own. So both tools write the app's recipe through the native adders and keep the app's limits. The server adds a refusal only where QymCAD would otherwise store a wrong result silently; each such case is a FINDINGS entry and an upstream issue candidate.

## Draft

### App behaviour (reproduced)

- Recipe: `Project::add_draft(src, faces, neutral, angle, flip)`; the angle formula is feature dimension `angle` (`store_cmd_exprs` in the app), so `param_set`/`dimension_set` and the app's parameter table drive it.
- Range: the app's command bar allows `0 < |angle| <= 60` (start.rs `CmdParam(... -60, 60).nonzero()`). The server enforces the same range at creation and on every evaluated edit (`check_evaluated_features` → `check_draft_thicken_values`), because the core enforces only the zero test (F-148).
- Sign: the pull direction is the neutral face's outward normal, reversed by `flip`; a positive angle widens the solid moving against it; negative ≡ positive with `flip` (measured, research §1.4). A drafted planar wall's normal satisfies `asin(n · pull) = angle`.
- The neutral is a face of the same body; the app removes it from the drafted set. The server refuses the overlap instead of editing the caller's selection silently.
- Faces: ids are stored as native picks, descriptions as their creation query with kind leaves frozen (as `shell`). The neutral is stored like PushFace's face: id pick or creation query, with the centroid/normal hint the app's adder leaves at default.
- Re-pick (`feature`): replaces the complete face set, and the neutral when given (ADR 0041 contract: same node id, formula, flip and name; independent B-rep copies; atomic rollback; batch; one undo step).

### Protective refusals (ours; upstream issue candidates)

- **Face loss (F-149):** after every rebuild that built a Draft node, its result must have at least as many faces as its source; otherwise the node gets a rebuild error and the edit rolls back. QymCAD builds −45° on a 20×10 block as a pyramid without an error.
- **Curved neutral (F-151):** the neutral must be planar; QymCAD tilts about an averaged plane silently.
- Clearer up-front words for cases QymCAD fails anyway: a drafted planar face parallel to the neutral (`DraftFailed`).

## Thicken

### App behaviour (reproduced)

- Recipe: `Project::add_thicken(src, face, thickness)`, with the app's face witness; the formula is feature dimension `thickness`. The plate is always united with the solid (no new-body mode); a planar face becomes a prism along its normal, a curved one an offset.
- Native Thicken stores a bare face id, not a query: a description must resolve to exactly one face and is frozen to its id.
- Re-pick (`feature`) replaces the face id and refreshes the witness (`face_refs[node]`), which the app's edit forgets (research §2.2); otherwise as ADR 0041.
- A plate into a cavity is not clipped (native behaviour, documented in the tool description).

### Protective refusal (ours; upstream issue candidate)

- **Thickness > 0 (F-150):** the app allows negative thickness; on a solid a negative value either adds nothing (native refusal) or, deeper than the part, builds material on its far side without an error. The server accepts positive thickness only, at creation and on every evaluated edit. `dimension_set` already refused non-positive `thickness`.
- Sheet sources (the app's whole-sheet offset path) are out of scope: the server creates no sheet bodies.

## Consequences

Both tools are modelling tools: batch-allowed, one undo entry, created in the source body's Part (`in_component(body_owner)`), named through `set_node_name`. Evidence: `crates/engine/tests/golden_draft.rs`, `crates/engine/tests/golden_thicken.rs` (hand volumes 3660.25/4360.16/7446.72 and 5200/9047.79, tilt by topology normals, formulas, refusals with byte-identical state, save/reopen and the app's parameter-edit path), `crates/mcp/tests/g12_draft_thicken.rs` (stdio registry, batch, undo). Removing `check_draft_thicken_values` or the face-loss check turns named tests red (G1/G2 mutation runs).
