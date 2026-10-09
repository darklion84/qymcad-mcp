# Backlog

Remaining evaluated follow-ups; implementation plan and settled evidence: tasks/review-j1.md.

## J5 — interop and robustness
- Preserve brace/bracket-leading literal names inside opaque selection/axis objects; schema-aware structured decoding.
- Cap nested encoded-string/structure decoding at 32 with clear error and test.
- Keep schemas unchanged; document object/array JSON-string interop in instructions and ARCHITECTURE.
- Reproduce repeated changed-document renders through protocol and compare image bytes/caption; close client observations with evidence.

## J6 — engine internals and performance
- Solve only parameter-reached sketches, including transitive/lowercase dependencies; solve-count tests and shelf timings.
- Measure rebuild guard Project clone on shelf/scaled case; close if negligible or optimize if needed.
- Verify no Patch creation; close and comment future unrestorable-edge guard requirement.
- Document or align shared rollback helper semantic differences and test distinguishing case.
- Classify empty-corner hints once.
- Add kernel-gate precondition comments to export/save/rebuild/undo/open public callers.
- List all upstream-defect pinning tests in UPGRADING for review/removal upon fixes.

## J7 — closed by decision
- Close native 0.001 mm internal cut clearance: no geometry compensation; exact-floor recipe is F-061.
