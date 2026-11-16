# ADR 0060: Copy faces and Offset surface

- Status: accepted (2026-11-16).
- Related: ADR 0059 (sheet bodies), 0052 (picks addendum), 0041 (re-pick), 0056/0057 (one tool = one app action); `tasks/surfaces-research.md` sections 1-3, 10. Package T, step T1.

## What the tools are

`face_copy` is the app's `FaceCopy { src, faces, body }`: the chosen faces of a body (solid or sheet) are copied into ONE new sheet body (`sheet: true`, area = the sum of the face areas), lying on the source faces. `offset_surface` is `OffsetSurface { src, faces, dist, body }`: the same, moved along the face normals by `dist` (outward of a solid is positive; on a sheet the sheet's own normal). Faces meeting smoothly move as one piece, faces meeting at a sharp edge move apart, still in one body. The source is read, never consumed (`consumed()` of both kinds is empty). The node id is the sheet's id.

## Decisions

- Input is the usual face selection (ids or a description) on `body`. The app refuses only an empty set (`msg-faces-not-found`), so does the server; an unresolved id is refused by the selection layer. The faces are stored as `Ref::picks` of the resolved ids (the app's own edit mode stores the same, `apply/update.rs`), wrapped in `in_component(body_owner(src), ..)`.
- `body` defaults as for every tool: the part's last solid (ADR 0059), so a sheet's faces need `body: <sheet id>`. A sheet is not "consumed" by the copy or by an offset of its faces, so chains of sheets work.
- `distance` is a `Num`, stored as the feature dimension `dist` (`dimension_set` key `dist`, a named dimension once named). The app's field range is -10000..10000 (`start_face_copy_cmd`); the kernel has none. The server holds it on creation and on every evaluated edit (parameter, `dimension_set`, reopen: `check_offset_values`, next to the draft and thicken checks), and refuses a non-finite value. Zero is allowed: a copy in place.
- `feature` re-picks the faces (the app's `update.rs` allows it for both kinds), keeps the node id and the distance formula; `body`, `distance` and `name` are refused with it.
- Native failures come back as the app words them, nothing is worked around: a cylinder moved in by its radius or more is `OffsetPastRadius` ("a round face moved in by its radius 5.00 or more"; a hole's wall counts as moved in when the distance is positive); a base edit that loses a copied face is refused with "the faces are no longer in the source body" and rolled back.

## Probed, no refusal needed

On a block with a hole (hole wall r 5, through): offset +2 gives r 3, +4.9 gives r 0.1 (area 2 pi 0.1 12), +5 / +5.1 / +8 `OffsetPastRadius`, -3 gives r 8 and -50 gives r 55 (hand areas); the top with the hole lifted or sunk by -100..3000 is the same sheet (2400 minus the hole) at the right z. Nothing was wrong silently, so there is no FINDINGS entry and no server guard beyond the range.

## Tests

Engine `surface_makers.rs` (scenarios and numbers of the app's `face_copy.rs` and `an_offset_surface_moves_its_faces.rs`: 2400, follows 60 to 80 = 3200, lost face, two faces one body; top +5 = 1200 at z 15, cylinder +2 = r 12, -12 on r 10 refused, 0 in place, follows dz 10 to 20 = z 25, two faces 1600, offset of a sheet; plus the range, `dimension_set`, re-pick, save/open and the app's own edit path `gui_edit_param_model`), MCP `t1_surfaces.rs` (results, batch with rollback, undo, save/open, `feature`, Part ownership).

## Not covered

The app's "replace a face with a surface" round trip (SurfaceReplace, P21, T2), and a hand-written GUI sheet whose faces the server cannot name.
