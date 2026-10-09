# ADR 0013: Kind previews and balanced intersections

- Status: accepted (2026-10-08); face-kind persistence decision updated 2026-10-09.

## Context

Agents can filter topology by geometric kind, but could not use the same filter in a selection. An
intersection accepted exactly two operands, unlike unions. QymCAD's pinned `refs::Query` exposes binary
`Filter` and `Union` but no geometric-kind query (`qymcad-core/src/refs.rs:69-115`). Edge modifiers already
store resolved persistent picks (F-024), while face modifiers store native queries.

## Decision

Accept `and` with at least two operands and lower its JSON operands to a balanced tree of binary
intersections. Retain the existing 512-part and 48-level query limits. This preserves the native file format
and avoids RON depth growth with a wide intersection (F-032).

Add a `kind` selection using the same classification as `topology`: edges line/circle/arc/other, faces
plane/cylinder/cone/sphere/other. The edge alias `curve` selects arc/other, excluding full circles. Native
cylinder/cone/sphere detection precedes mesh planarity. Preview kind filters by lowering their leaves to
persistent ids; edge modifiers use their usual pick-list storage. Kind filters can compose with other
descriptions, including inside `edges_of` and `between` for edge modifiers.

For `hole`, `shell` and `push_face`, lower each face-kind leaf to fixed persistent face ids at feature
creation, just as edge modifiers already resolve their picks (F-024). Retain surrounding native descriptive
queries, such as `facing` and `of_feature`, in the stored composition. The frozen kind leaves do not rediscover
faces created by later edits. This explicit limitation avoids an upstream file-format/API change while making
kind-selected modifiers usable; truly dynamic kind discovery still requires upstream native query support.

## Consequences

MCP tests verify three- and 200-operand intersections, the retained size budget, exact taxonomy agreement,
incompatible/unknown kind refusal, and successful calls to all three face modifiers.
The three face golden tests inspect fixed kind ids plus a native oriented leaf, then check formula-derived
volumes on creation, parameter edits, reopening and native GUI edits: hole `20*16*h−3π`, inward shell
`20*16*h−18*14*(h−1)`, and pushed top `20*16*(h+2)`.
`golden_selection_kinds::kind_selected_fillet_survives_server_and_gui_parameter_edits` checks a kind-selected
four-corner fillet, its eight quarter-circle cap edges, and parameter edits through both saved/opened server
and native GUI paths. The formula is `V=(20*16−4*(1−π/4)*r²)*h`, and each cap arc has length `πr/2`.
Changing the line-kind filter to match circles makes both the taxonomy test and golden fixture fail.

Corner previews and modifier empty-selection errors share one engine hint. It names the selected inward or
outward sign, counts the opposite sign across the body, and offers a `between` example only when the body's
consumed-source chain has at least two solid-creating extrude/revolve features. Topology and resolved selections sort ids for
stable output. Public body volume uses `volume_mm3` at native floating-point precision, while existing bounds
and topology display rounding remain independent presentation choices.
