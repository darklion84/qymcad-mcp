# Call log review log

The persistent JSONL call log (ADR 0018) was implemented by OpenAI Codex (gpt-6.1-sol, high reasoning) from a
specification by Claude (Opus 5.5), and reviewed by Xiaomi MiMo (MiMo-V2.6-Pro, read-only, on a snapshot) and
Claude. Two live tests ran the server through a real MCP client (Codex as tester) and compared the log with what
the client sent and received.

| Round | Scope | MiMo | Claude | Live test | Outcome |
|---|---|---|---|---|---|
| 0 | implementation | — | read the code; check.sh 402 passed; own mutations (retention cap, an unlogged protocol error, an off alias) each red | — | the specification contradicted itself (byte-identical stdout vs the log path in `initialize`); resolved: only the instructions sentence differs, every later response is byte-identical |
| 1 | full change | ACCEPT; minors M1 (a failed start record still advertised the log), M2 (truncation could take seconds), nits N1–N5 | accepted M1, M2, N1–N3 | PASS a–g; the image field `bytes` was read as the PNG size (it is the base64 length) | fixed by Codex: R1–R5, field renamed `base64_bytes` |
| 2 | fixes | ACCEPT; one missed rename in SECURITY.md (fixed) | check.sh 406 passed | PASS: a 428 KB call is logged truncated with its seq/tool, reply not delayed | merged |

M2 was real: a crafted record with 2,048 nested fields took 5.2 s to bound; after the 32-pass cap and cached field
sizes it takes about 5 ms.

Not changed, with reasons:
- N4: file names assume a four-digit year; only extreme `SystemTime` values are affected.
- N5: a planted future-dated log file counts as recent for the 200-file cap; the specification is silent and the
  directory is the user's own.
- A record needing more than 32 single-field cuts becomes the short truncated envelope; this is the intended
  trade-off for bounded time.
