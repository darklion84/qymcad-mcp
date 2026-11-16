# ADR 0061: Replace face and Stitch

- Status: accepted (2026-11-16).
- Related: ADR 0059 (sheet bodies), 0060 (Copy faces, Offset surface), 0041 (re-pick), 0056/0057 (one tool = one app action); FINDINGS F-198; `tasks/surfaces-research.md` sections 1, 5, 7, 10. Package T, step T2.

## What the tools are

`surface_replace` is the app's `SurfaceReplace { src, faces, surface, body }`: the chosen faces of a solid are dropped and a sheet is put in their place; `src` and `surface` are consumed and the node's body is a NEW solid (the node id is its id) that takes later features (a fillet of its rim is tested). The kernel has two modes: it sews the solid minus the faces plus the sheet (tolerance 1e-4), and for exactly one face and a one-face sheet it reaches the sheet by extension (the top replaced by a copy lifted 5 makes the block 5 taller, sunk 3 cuts it 3 lower; a tilted or shifted sheet is extended as a plane).

`stitch` is the app's `Stitch { parts, tol, body }`: two or more sheets become ONE body and every part is consumed. When the sewing leaves no free edge the result is a SOLID (six face copies of a 60 x 40 x 12 plate give the plate, volume 28800), otherwise a sheet. Sheets that share no edge within the tolerance fail natively ("nothing to stitch"). The first part is the lineage (its edge names survive; edge names stay unique, tested).

## Decisions

- Input kinds are the GUI's: a click on a sheet picks the surface, a click on a body toggles faces. So `body` must be a solid and `surface` and every stitch part a sheet; the wrong kind is refused with a message naming the body and the role (the app's `msg-stitch-only-sheets` for a solid in a stitch, `msg-stitch-needs-two` for fewer than two). A body consumed by a later feature, a mesh body, a part listed twice and an id that does not exist are refused. `body` defaults as for every tool (the part's last solid); `surface` and `parts` are always given.
- The app's pre-check `qymcad_kernel::ask::sheet_closes(base, picks, sheet)` is called as the GUI does (`apply/faces.rs:188-208`) on the live shapes, BEFORE a node is laid: a sheet that leaves free edges is refused with the app's `SurfaceDoesNotClose` ("the sheet does not close what it replaces: surface leaves N free edges") and the document is untouched. Top and front replaced by a copy of the top is the tested case. A re-pick has no pre-check in the app's edit path (`update.rs`); the rebuild fails natively and the edit is rolled back.
- Faces are stored as `Ref::picks` of the resolved ids, the node is wrapped in `in_component(body_owner(src))` (stitch: the first sheet's Part), as in T1. An empty or unresolved face set is refused (`msg-faces-not-found`).
- `tolerance` is a `Num` stored as the feature dimension `tol` (`dimension_set` key `tol`, a named dimension once named). The app's field is 0.01 by default with range 1e-6..10 (`start_stitch_cmd`); the kernel has none. The server holds the range on creation and on every evaluated edit (`check_stitch_values`, next to `check_offset_values`).
- `feature` re-picks as the app's edit mode does (`update.rs`): `surface_replace` takes `faces` and/or `surface`, `stitch` takes `parts`; the node id, the other inputs and the tolerance formula stay; `body`, `name` (and `tolerance`) are refused with it. A sheet that this node consumes now is free to it again; one built after the node, a solid, or a body consumed by another node is refused, and a re-pick that fails to rebuild leaves the document as it was.
- The tool results say what the app builds: `stitch` returns `sheet` (true when the sheets did not close).

## F-198: a tolerance wider than the gap builds a solid that encloses nothing

Two parallel sheets 5 mm apart stitched with tol 4.9 fail natively ("nothing to stitch"); with tol 5.1, 6 or 10 the node is green and a SOLID of kernel volume 4000 whose mesh encloses 0 (gap 1, tol 2: 800 against 0; gap 0.5, tol 1: 400). The sewing glued the rims of sheets that do not meet into a closed shell of two faces. The server refuses exactly that (`stitch_refusals`, after the rebuild, so it also holds on later edits): a stitched solid whose kernel volume and mesh volume differ by more than 5 %. Real stitched solids agree (the plate 28800, a cylinder from its side and two caps pi r^2 h). Probe `t2_probes.rs` pins the app's behaviour (core API only); when QymCAD refuses or builds it right it goes red and the guard can be dropped. A sheet result of a wide tolerance (top copy and front offset by 1 at tol 2: area 3150.62 against 3120) is sewing within the tolerance and is not refused.

## Probed, no refusal needed

Top and front replaced by a copy of both (identity, 28800); top and front moved apart by 5 (refused, 14 free edges); a sheet lifted 5 or sunk 3 and tilted 20 or 40 degrees about its centre (volumes 18000, 8400, 9143.87 by hand: the same); a sheet shifted 10 sideways and a 20 x 20 sheet over a 40 x 30 face (extended as a plane, 18000); two copies of one face stitched (native "empty body"); five of six plate faces (a sheet, 4800); gap 1 tol 2 sheet (above).

## Tests

Engine `surface_replace_stitch.rs` (scenarios and numbers of the app's `face_copy.rs`, `an_offset_surface_moves_its_faces.rs`, `stitch.rs` and the matrix: round trip 28800 with both consumed, fillet after, 18000 and 8400, follows 60 to 90, top+front by a copy of the top refused, lost face, wrong kinds, re-pick, tol range, two touching sheets 3120, six faces 28800 solid, top+bottom refused, follows 4680, unique edge names, F-198, save/open and the app's own edit path `gui_edit_param_model`), `t2_probes.rs`, MCP `t2_surfaces.rs` (results, refusals, batch with rollback, undo, save/open, `feature`, Part ownership, `dimension_set`).

## Not covered

Patch and Trim (P22, P24); a stitch whose parts lie in different Parts (the node goes to the first part's Part, as the app's parent rule is the active Part; not probed); the replacement's `replace_by_reaching` mode with curved sheets (not probed).
