# ADR 0012: Protect existing bodies from empty-document saves

- Status: accepted (2026-10-07)

## Context
Modelling undo retains the latest save association. Undo or deletion can leave no result bodies, and a
pathless save could silently overwrite the associated solid model with an empty document.

## Decision
Before saving a document with no result bodies to an existing target, inspect the target's stored Project
body pool without regeneration (F-057). Refuse replacement when that pool contains bodies unless the caller
sets `allow_empty=true`. Apply the guard to explicit and implicit paths in the engine; the MCP tool documents
and forwards this option. Inspection failures are errors before any file write. New targets and already
empty documents remain saveable without an override.

## Consequences
Ordinary saves incur no extra load. Empty replacement requires deliberate intent; refusal preserves the
file and its save association. The inspection checks stored bodies rather than rebuilding the target or
silently accepting a potentially failing recipe. Tests exercise undo/delete, both path forms, unchanged
file bytes, override, and creation/replacement of empty files.
