# ADR 0014: Guard the implicit save path with model lineage

- Status: accepted (2026-10-08)

## Context

The session remembers its last loaded or saved path across modelling undo. Undoing/deleting a model's
first solid feature and constructing another solid can silently replace the earlier file with a different
model. The empty-document protection in ADR 0012 does not cover a replacement that has bodies.

## Decision

Associate the path with the id and native feature-kind discriminant of the document's first body-producing
timeline node when loading or successfully saving. A pathless save requires that node to remain in the
current timeline. Parameter/sketch edits and appended features preserve this lineage. Undo keeps the path
association outside its recipe snapshots, and native monotonic ids distinguish a replacement feature
(F-056). Reject a missing/replaced anchor before writing, with an explicit path or `overwrite=true` as
ways to deliberately replace the file. Every successful save establishes the current lineage. Saving an
empty recipe establishes no solid anchor, so its first model can be saved normally.

Retain ADR 0012's separate `allow_empty=true` permission for replacing stored bodies with an empty document;
that explicit empty replacement also resets lineage. `overwrite=true` alone does not authorize losing all
bodies. `doc_new` creates an unassociated session and always needs an explicit first path.

## Consequences

The guard compares model ancestry, not geometry: ordinary dimensional changes continue saving normally.
A removed first feature requires explicit intent even when remaining features still produce solids. It
does not compare an externally modified target file or promise semantic model identity when a first
feature is retained but the rest of the design changes. No QymCAD format or GUI behavior is altered.
Tests cover undo/delete replacement, unchanged file bytes on refusal, explicit override, normal edits,
reopened edits, and new-session path requirements.
