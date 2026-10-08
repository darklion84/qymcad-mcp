# ADR 0013: Kind previews and balanced intersections

- Status: accepted (2026-10-08); persistent face-kind queries deferred.

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

Stop persistent face-kind modifiers at the native capability boundary. Refuse kind-filtered `hole`, `shell`
and `push_face` selections with a preview-only explanation; retain explicit face ids and existing native
descriptions as alternatives. The options for future work are to freeze just the kind leaves to face ids
at creation (loses discovery of later faces), or extend upstream `Query` with a genuine kind filter. This
decision does not silently substitute fixed face picks for a descriptive persistent kind query.

## Consequences

MCP tests verify three- and 200-operand intersections, the retained size budget, exact taxonomy agreement,
incompatible/unknown kind refusal, and unchanged documents after refusing all three face modifiers.
`golden_selection_kinds::kind_selected_fillet_survives_server_and_gui_parameter_edits` checks a kind-selected
four-corner fillet, its eight quarter-circle cap edges, and parameter edits through both saved/opened server
and native GUI paths. The formula is `V=(20*16−4*(1−π/4)*r²)*h`, and each cap arc has length `πr/2`.
Changing the line-kind filter to match circles makes both the taxonomy test and golden fixture fail.

Corner previews and modifier empty-selection errors share one engine hint. It names the selected inward or
outward sign, counts the opposite sign across the body, and offers a `between` example only when the body's
consumed-source chain has more than one modelling feature. Topology and resolved selections sort ids for
stable output. Public body volume uses `volume_mm3` at native floating-point precision, while existing bounds
and topology display rounding remain independent presentation choices.
