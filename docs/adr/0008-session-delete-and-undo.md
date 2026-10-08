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
Project recipe without embedded source bytes and original live shape handles; run the edit on independent
serialized/deserialized B-rep copies. Restore source bytes from the live project by id and preserve the higher
id allocator counter with `keep_ids_past` (F-056), so held references to removed nodes never alias new nodes.
Native sketch deletion also removes its source record. Move such originals to a session archive before
deletion, retaining each payload once while any history snapshot refers to it. Return archived originals
to the live source pool on restore and prune them when their final referencing snapshot is evicted.
A failed call restores the originals and adds no history. A successful call retains its pre-edit snapshot;
`undo` restores it directly without regeneration. Keep at most 16 entries per document, evicting the oldest.
Retain the tool name and arguments in each entry; undo returns them with restored bodies/errors/warnings
and the complete current parameter list (names, expressions and evaluated values). This shows restored
dependent values and parameter removal after undoing creation without inferring state from old arguments.
Remember eviction separately from snapshots so exhaustion reports the history limit instead of ordinary
empty history. Deletion inside an existing tool boundary shares that snapshot; direct engine deletion retains
its own rollback snapshot. Successful deletion returns all removed timeline node ids/names/kinds.

History is modelling history: reads, save and export do not add entries, and undo keeps the current file
association. `doc_new`/`doc_open` create a new session and empty history. Disk writes are outside undo; there
is no redo. Direct engine users can use `begin_tool_edit`/`finish_tool_edit` to establish the same boundary.

## Consequences
Undo preserves exact B-rep bytes and kernel bounds. Every modelling call copies the live shapes; up to 16
whole-document shape snapshots can consume substantial memory for large models. Immutable embedded source
payloads are retained once in the live project or the archive for removed originals. A bounded entry count keeps this
predictable for the small parametric parts this server targets, without introducing file-backed history.
Native cleanup preserves QymCAD's deleted-body/external-reference semantics. Tests cover dependent refusal,
source restoration, cascades, all three requested undo operations, exact geometry/recipe state and the history
limit. The id allocator intentionally remains monotonic rather than being restored byte-for-byte.
