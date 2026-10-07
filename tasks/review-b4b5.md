# B4/B5 implementation review

- [x] Read AGENTS.md, architecture, findings, backlog; confirm matching dependency/engine QymCAD release.
- [x] Define reporting contract: export `warnings` matches doc_info; render text repeats warning strings; undo `params` gives complete restored expressions/values beside existing bodies.
- [x] Add formula-derived regression fixtures before implementation.
- [x] Record first failing tests and exact messages.
- [x] Implement output and undo reporting.
- [x] Verify focused tests.
- [x] Mutate each reporting behavior, record failures, restore by editing.
- [x] Run formatting and compiler/linter checks; root runs complete check.sh and generates tool docs.

Tests: `crates/mcp/tests/backlog_output_history.rs`.

B4 fixture: sealed pocket in a 20×20×20 prism. Expected volume `20³ − 4×6×(3+0.001)` mm³, because the interior cut includes native 0.001 mm entry clearance. Compare export warning list exactly to doc_info; render retains image and repeats each warning verbatim in its text.

B5 fixture: 20×30 prism, height `h=t+1`, t=5. After t=9 and undo, expected volume `20×30×(5+1)` mm³. Compare complete restored parameter names, expressions and evaluated values. Check normalized uppercase call name and exact original arguments. Also cover creation undo (parameter absent) and deletion undo (value restored).

No engine calls are added to the MCP crate; only existing typed Session info/params APIs are used. No commits. No uncertain design decisions or refused implementation items.

## B4 files, first failure and mutation

Files: `crates/mcp/src/tools/output.rs`, `crates/mcp/tests/backlog_output_history.rs`.

Tests: `export_repeats_current_document_warnings`, `render_repeats_current_document_warnings`.

First failures, command `cargo test -q -p qymcad-mcp --test backlog_output_history -- --nocapture` (exit 101, 0 passed/3 failed):

```text
assertion `left == right` failed: export must repeat current document warnings
  left: Null
 right: Array [String("pocket (35): result body has 2 shells and 1 solids; may contain a sealed internal void")]

render must repeat current document warning: pocket (35): result body has 2 shells and 1 solids; may contain a sealed internal void; got: iso view, isometric from front-right-top: +X to the lower right, +Y to the upper right, +Z up: bodies [35], bbox x -10..10 y -10..10 z -0..20 mm
```

Fixed output uses the complete current `Session::info().warnings`, so warnings belong to the document even if bodies are filtered. Export always returns the list; render keeps its image/summary and adds `Warning: ...` lines.

Mutation: replace export warning list with `json!([])` and skip render warnings with `.into_iter().take(0)`. Both tests turn red (export left `Array []` versus the expected warning; render identical missing-warning message above). Restored both lines through `apply_patch`, then reran focused tests successfully.

## B5 files, first failure and mutation

Files: `crates/mcp/src/tools/history.rs`, `crates/mcp/tests/backlog_output_history.rs`.

Test: `undo_reports_restored_parameter_expressions_values_and_bodies`.

Exact first failure:

```text
assertion `left == right` failed: undo must report restored parameter expressions and values
  left: Null
 right: Array [Object {"expr": String("5"), "name": String("t"), "value": Number(5.0)}, Object {"expr": String("t+1"), "name": String("h"), "value": Number(6.0)}]
```

Undo now adds top-level `params` from the restored `Session::params()`. This gives evaluated dependent parameters and preserved expressions consistently for any undone call, including creation/deletion, alongside existing current bodies and original call metadata.

Mutation: replace returned parameters with `json!([])`. The test turns red with the same assertion; left `Array []` versus the expected two parameters. Restored through `apply_patch`.

## Focused verification

After implementation: new test file 3 passed. After mutations restored:
`cargo test -q -p qymcad-mcp --test backlog_output_history --test usability_history --test usability_polish -- --nocapture`
passed 10 tests (3 new, 7 existing). `cargo fmt --all` applied.
`cargo clippy -p qymcad-mcp --all-targets -- -D warnings` passed (exit 0).

Shared docs/TOOLS, FINDINGS, CHANGELOG, ADRs and BACKLOG are owned by the root implementer. Suggested updates:

- ADR 0008: undo returns the complete restored parameter list (names/expressions/evaluated values), retaining original call metadata and restored bodies/diagnostics.
- ADR 0009/F-051: export returns document warning strings matching doc_info; render repeats those warnings in its text with the image. Evidence: both B4 regressions, formula fixture above.
- CHANGELOG: export/render repeat current document warnings; undo reports current parameter expressions/values.
- Remove completed export/undo backlog entries.
