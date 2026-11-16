# ADR 0063: Patch

- Status: accepted (2026-11-16).
- Related: ADR 0059 (sheet bodies), 0061 (Replace face and Stitch), 0062 (Trim), 0041 (re-pick), 0056/0057 (one tool = one app action); FINDINGS F-024, F-199; `tasks/surfaces-research.md` sections 1, 4, 10. Package T, step T4.

## What the tool is

`patch` is the app's `Patch { src, edges, tangent, body }`: a surface is spanned over boundary edges of the body `src` (a solid or a sheet) by the kernel's `BRepOffsetAPI_MakeFilling`. The node's body is a NEW sheet of one face (the node id is its id); `src` is NOT consumed (the lid lies beside the box). `mode` is `"flat"` (default) or `"tangent"` (the surface meets the neighbouring faces smoothly; over the same boundary it is larger than the flat one); no numbers. A patch over the outer rim of a shelled opening is the lid, and `thicken` of that sheet closes the loop (the app's `thicken_sheet.rs`: the box and the sheet are consumed, one solid is left, volume box + 40 x 30 x 2). The kernel's own refusals come back natively and the edit is rolled back.

## The app's boundary pre-check, reimplemented

The GUI refuses before it lays a node when `patch_boundary` (qymcad-part `apply/faces.rs:67-90`, `pub(crate)`, not callable from the engine) is false, with `msg-patch-needs-edges` ("The edges do not close: pick edges that run all the way round the opening, or one ring"). The engine has its own copy, rule for rule (`patch_boundary` in `surfaces.rs`, lines cited in its comment):

- every picked id must be an edge of the body (the app looks it up in `regen_edges`); none picked refuses;
- an edge whose two ends are less than 1e-6 apart is a ring (a circle, an ellipse) and counts no ends;
- every end of the other edges, on a 1e-4 mm grid (`round(c * 1e4)`), must be met by exactly one other end.

So accepted: one ring (the rim of a hole), several rings, a closed chain of edges meeting end to end. Refused: one straight edge, an open chain, two opposite edges, an edge picked past the loop. The refusal carries the app's words (`localization::text("msg-patch-needs-edges")`, the Fluent key of the pinned catalogue) and the picked ids, and the document is untouched. It is the app's limit, not the kernel's (the kernel also fills open chains). The selection is a set: an id named twice counts once (the app's selection holds each edge once). Unit tests pin the rules, the 1e-4 grid and the 1e-6 ring threshold on synthetic edges.

A re-pick (`feature`) has NO pre-check in the app: the edit mode's `cmd_ready` is true for any edit (`params.rs`), so the engine does not ask either. What the kernel does with such a boundary is tested: one straight edge and three of four rim edges are refused natively ("operation failed: Patch", console: "one edge does not bound a patch") and the re-pick is rolled back, document unchanged.

## Decisions

- Edges are stored as `Ref::picks` of the resolved ids, never as a stored query (F-024: a stored edge query re-evaluates against an empty pool after the file is reopened). So the `edge_query_rebuild_errors` guard in `session.rs`, which the research said must learn Patch "if queries are stored", needs no change; its comment is updated. A Patch that a GUI file wrote with a recorded query is not judged (not probed).
- `body` defaults as for every tool (the part's last solid, never a sheet); a sheet is a valid source and is not consumed (the four boundary edges of a copied top face take a patch of the same 1200). A consumed body, a mesh body, an id that is no edge of the body are refused as for the other tools. The node is wrapped in `in_component(body_owner(src))` (tested with another Part active).
- `feature` re-picks as the app's edit mode does (`update.rs:404-409`): `edges` replace the stored ones, `mode` is set; either may be given, an omitted `mode` keeps the old one (the app sets it from its option, a tool cannot read that option). `body` and `name` are refused with it.
- Result: `feature` (= the sheet's id), `selected_edges`, `sheet`, `rebuild`.

## The outer and the inner rim

A shelled opening is outlined twice: the OUTER outline (40 x 30 for the app's test box) and the INNER one, a wall thickness in (36 x 26). The app's tests use the outer outline for the lid (`thicken_sheet.rs:33`). The server does not pick for the user: the tool text says to pick the outer outline for the lid and that the inner one gives a smaller patch (936 for the test box); both are tested. The server does not guess which rim is meant.

## F-199: edges of more than one boundary, one of them is silently dropped

The outer and the inner outline picked TOGETHER pass the app's pre-check (every end meets one other end), the node is green and the surface is the 40 x 30 = 1200 lid with four edges: the kernel dropped the inner four (console: "WireFromList: can't find the next edge. The wire is not complete, some edges are lost."). The same for the four outline edges of a block's top plus the ring of a hole in it (the app's click on a face takes ALL its edges, which is exactly this selection): a full 1200 face over the hole, the ring dropped. Two rings (the top and the bottom rim of a hole) fail natively ("empty body"). The server refuses all three at creation and on re-pick (`patch_loops`: every ring is one boundary, other edges join where their ends meet on the same 1e-4 grid; more than one boundary is refused), naming the number of boundaries. Probe `t4_probes.rs` pins the app's behaviour (core API only); when QymCAD refuses or builds all the edges the probe goes red and the guard can be dropped. The check is on the picked edges, not on a rebuilt result, so an edit that changes the structure of a stored boundary is not re-judged (the ids stay edges of the body; not probed).

## Tests

Engine `patch.rs` (numbers of the app's `patch.rs`, `a_patch_over_one_closed_edge.rs`, `thicken_sheet.rs` and the acceptance `PATCH`: lid 1200 with the source not consumed and picks stored, follows the opening 40 to 60 = 1800, tangent larger and stored, foreign ids and no edge refused, hole rim r5 = pi 25, disc over a hole 20 across: two bodies, volume 8858.407, patch + thicken, the pre-check refusals one by one, outer and inner rim, F-199, re-pick of edges and mode, re-pick refused by the rebuild, a sheet as the source, the node's Part, save/open and the app's own edit path), unit tests of `patch_boundary` and `patch_loops`, `t4_probes.rs` (F-199 in the app), MCP `t4_patch.rs` (results, hole rim + thicken, refusals, `feature`, batch with rollback, undo, save/open).

## Budget

`tools/list` was 3 bytes under its budget when this step began, and `patch` weighs about 1960 bytes. `BUDGET_BYTES` is unchanged: the `Num` schema description (repeated in 33 schemas), the "name for the new feature" docs (12 schemas) and the "ids are stale" suffix of 15 feature tools were shortened with the same meaning.

## Not covered

A Patch with a recorded edge description written by the GUI (a stored query); the structure of a stored boundary after an upstream edit; curvature continuity and constraint curves (the app has none); the closed-loop pre-check for re-picks (the app has none, the kernel judges).
