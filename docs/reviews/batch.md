# Batch tool review log

The `batch` tool (ADR 0020) was implemented by OpenAI Codex (gpt-6.1-sol, high reasoning) from a specification
by Claude (Opus 5.5), reviewed by Xiaomi MiMo (MiMo-V2.6-Pro, read-only, on a snapshot) and Claude, and
live-tested through a real MCP client (Codex as tester).

| Round | Scope | MiMo | Claude | Live test | Outcome |
|---|---|---|---|---|---|
| 0 | implementation | — | the implementer stopped on one conflict: an existing protocol test pins exactly 28 tools; Claude added `batch` to its list (the only change to an existing test). check.sh 432 passed | PASS: the left half of the door hanger built with single calls (46 calls) and with batches (10 calls) gives identical doc_info; reply text halved; a failing step 4 and a misspelt argument in step 5 leave the document unchanged; one undo per batch, mixed undo in reverse order; refusals | — |
| 1 | full change | ACCEPT; one latent minor (finish_tool_edit reached through `state.doc()?`, unreachable with the allowed steps) and nits | agreed they are not defects today | — | deferred to docs/BACKLOG.md; merged |

MiMo's first two attempts aborted within 30 seconds after asking to read paths outside its snapshot; the third,
told to use only relative paths, completed.
