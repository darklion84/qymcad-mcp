# ADR 0058: A panic rolls back and puts the server in recovery mode

- Status: accepted (2026-11-15).
- Related: ADR 0002 (one request at a time, no cancellation or timeouts), 0047 (rollback); external review 2026-11-15, item M7.

## Context

A tool handler runs inside the native core and OCCT. A panic there (a Rust `unwrap`, an index out of range, a kernel
assertion surfaced as a panic) used to unwind out of `Registry::call` and end the process: the model lost its
connection and the user lost every unsaved change. The tool-edit snapshot was dropped with the stack, so nothing was
rolled back either.

How the big packages behave: SolidWorks, Fusion 360 and Inventor catch an internal fault, keep the session alive and
offer to save the recovered document under a new name, telling the user to restart before modelling on; FreeCAD's
recovery likewise writes to a separate file and never over the original. The process after the fault is suspect, so
editing on is not offered.

## Decision

1. `Registry::call` runs every handler under `catch_unwind`. On a panic the tool-edit snapshot of a modelling call is
   restored (`finish_tool_edit(.., false)`, the same rollback as an error), no undo entry is made, and the call returns
   an `isError` result: `internal error in `<tool>`: <panic message>. The document is as it was before this call.` plus
   the recovery rule.
2. `batch` owns its own snapshot, so it catches the panic itself, rolls the whole batch back, and re-raises it for
   `Registry::call` to report.
3. The server is then **tainted** until restart. Allowed: `doc_info`, `export`, `render`, and `doc_save` with an
   explicit `path` that is not the document's own file. Everything else is refused with the cause and the rule.
   `doc_new`/`doc_open` are refused too: the fault may sit in process-wide state (the kernel), not in the document.
4. No timeouts or cancellation are added (ADR 0002). The kernel gate is poison-tolerant upstream, so a panic while
   holding it does not deadlock the recovery reads.

## Consequences

- A panic costs the call, not the session: the model can save the work elsewhere and ask for a restart.
- Read tools that panic taint too; they changed nothing, but the process is equally suspect.
- Only Rust panics are caught: an abort or a segfault inside the native kernel still ends the process.
- Tools other than modelling calls and `batch` have no snapshot (`topology`/`select`/`measure` can rebuild topology in place, `undo`, `doc_save`); a panic there says the document may be partly changed instead of claiming it is intact.
- The protected file is the one the document had when the panic happened; a recovery save elsewhere does not make it writable again.
- `panic = "abort"` must never be set for this binary; it would bypass the recovery.
