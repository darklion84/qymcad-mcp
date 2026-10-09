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

For `hole`, `shell` and `push_face`, resolve the complete face selection to a fixed persistent pick list
at feature creation whenever it contains a kind leaf, just as edge modifiers resolve their picks (F-024).
This freezes surrounding descriptions too: no part of that selection rediscovers faces after an edit.
Selections without kinds retain native dynamic queries.

Freezing only the kind leaves inside `Filter` is insufficient: QymCAD recognizes only `Id`, `Ids`, and
pick-only unions as pick lists (`refs.rs:202-208`). Its shell asked-versus-opened guard runs only for a pick
list (`model/regen.rs:1442-1449`). A mixed frozen/dynamic filter can silently lose openings; storing the
whole resolved list preserves the guard and refuses missing faces. Dynamic kind discovery still needs
upstream native query support.

## Consequences

MCP tests verify three- and 200-operand intersections, the retained size budget, exact taxonomy agreement,
incompatible/unknown kind refusal, and successful calls to all three face modifiers.
The three face golden tests inspect whole-selection picks, then check formula-derived
volumes on creation, parameter edits, reopening and native GUI edits: hole `20*16*h−3π`, inward shell
`20*16*h−18*14*(h−1)`, and pushed top `20*16*(h+2)`.
`modifiers::hint_tests::kind_shell_refuses_partial_loss_of_frozen_faces` replaces a box source with a
cylinder after selecting its top and two sides: only the top survives, and shell regeneration refuses
rather than opening just one face. Removing whole-selection pick storage makes the test fail.
`golden_selection_kinds::kind_selected_fillet_survives_server_and_gui_parameter_edits` checks a kind-selected
four-corner fillet, its eight quarter-circle cap edges, and parameter edits through both saved/opened server
and native GUI paths. The formula is `V=(20*16−4*(1−π/4)*r²)*h`, and each cap arc has length `πr/2`.
Changing the line-kind filter to match circles makes both the taxonomy test and golden fixture fail.

Corner previews and modifier empty-selection errors share one engine hint. It names the selected inward or
outward sign, counts the opposite sign across the body, and offers a `between` example only when the body's
consumed-source chain has at least two solid-creating extrude/revolve features. Topology and resolved selections sort ids for
stable output. Public body volume uses `volume_mm3` at native floating-point precision, while existing bounds
and topology display rounding remain independent presentation choices.
