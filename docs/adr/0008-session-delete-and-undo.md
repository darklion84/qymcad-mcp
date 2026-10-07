# ADR 0008: Guarded timeline deletion and bounded modelling undo

- Status: accepted (2026-10-07)

## Context
Agents previously rolled back unwanted features by saving/opening checkpoints. Native QymCAD deletion can
relink consumers to the deleted operation's source, or leave unsupported consumers red. Its input graph
omits some datum/sketch dependencies (F-017). Project snapshots alone cannot restore live B-reps exactly.

## Decision
Expose `feature_delete` with default dependent refusal and an explicit `cascade` option. Extend the native
transitive graph with hosted sketches, datum ancestry, associative points/axes, revolve datum axes and
sketch-driven holes. Delete the closure in reverse timeline order through native operation/sketch cleanup,
remove datum pools/dimensions, prune live shapes, and rebuild. Preserve the rollback bar's surviving prefix.
Restore a Project/shape/diagnostic snapshot if the rebuild fails. Deleting a final modifier makes its
unconsumed source the result again; no recipe reconstruction is needed.

Wrap every MCP modelling mutation in one history boundary, including a batch `sketch_add`. Retain the
original Project and live shape handles and run the edit on independent serialized/deserialized B-rep copies.
A failed call restores the originals and adds no history. A successful call retains its pre-edit snapshot;
`undo` restores it directly without regeneration. Keep at most 16 entries per document, evicting the oldest.

History is modelling history: reads, save and export do not add entries, and undo keeps the current file
association. `doc_new`/`doc_open` create a new session and empty history. Disk writes are outside undo; there
is no redo. Direct engine users can use `begin_tool_edit`/`finish_tool_edit` to establish the same boundary.

## Consequences
Undo preserves exact B-rep bytes and kernel bounds. Every modelling call copies the live shapes; up to 16
whole-document snapshots can consume substantial memory for large models. A bounded entry count keeps this
predictable for the small parametric parts this server targets, without introducing file-backed history.
Native cleanup preserves QymCAD's deleted-body/external-reference semantics. Tests cover dependent refusal,
source restoration, cascades, all three requested undo operations, exact state and the history limit.
