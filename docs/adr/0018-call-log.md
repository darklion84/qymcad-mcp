# ADR 0018: Persistent local call log for each stdio server process

- Status: accepted (2026-10-11).

## Context
When a later session returns wrong geometry, reports an error, crashes or hangs, the client's transcript is
the only available record. It need not preserve what the server actually received or answered, and may omit
stderr. We need an ordered record on local disk, including a call that never returned, without changing tool
behavior or adding dependencies to the synchronous transport (ADR 0002).

## Decision
- Log every stdio server process by default. `QYMCAD_MCP_LOG` selects a directory; unset or empty uses
  `~/Library/Logs/qymcad-mcp/` on macOS, otherwise `$XDG_STATE_HOME/qymcad-mcp/` or
  `~/.local/state/qymcad-mcp/`. Case-insensitive `off`, `0`, `false` and `no` disable logging and create no
  directory. Create the selected directory with `create_dir_all`. `--version` and `--dump-tools` never open
  a log or run retention.
- Open one append-mode file named `qymcad-mcp-YYYYMMDDTHHMMSSZ-pid.jsonl`, using UTC and
  `std::process::id()`. Every record has `t` (UTC RFC 3339 with milliseconds) and `kind`. UTC formatting uses
  Howard Hinnant's civil-date conversion from days since the Unix epoch, tested against fixed instants;
  directory and time handling use the standard library, with no new crates.
- Record `start` with server/QymCAD versions, pid, cwd, installed-app note and log path. If cwd lookup fails
  (for example, the working directory was deleted), record `cwd: null` and keep logging. Record `initialize`
  with client info and requested/answered protocol versions. Assign sequence numbers only to recognized
  tool calls; write `call` with the received arguments **before** invoking the tool, then `result` with the
  same sequence/tool, elapsed milliseconds, error flag and result content. Keep text content; replace every
  image with its MIME type and `bytes`, meaning the original base64 string's UTF-8 byte length, never its data.
- Record `rpc_error` with code, message, request id and method when known, including parse failures of
  non-JSON input. A panic hook installed by `main` shares the logger with the transport and records message,
  source location and thread before chaining the previous hook. Handle a poisoned mutex without panicking.
  Record transport `stop` with `eof` or `io_error` and its error text. Ignore notifications, cancellations,
  `ping` and `tools/list` to keep the log useful.
- Serialize each entire JSON object plus newline, then make one `write_all` call on an unbuffered `File`.
  No `BufWriter` and no per-line fsync. Completed writes remain on disk if the server process dies; this is
  not a promise of durability through power loss. A `call` without its matching `result` is the signature of
  a call that crashed or hung, unless logging stopped due to an I/O failure or the file limit.
- Limit a serialized record, including newline, to less than 256 KiB. Truncate the largest string fields on
  UTF-8 boundaries with `…[truncated N bytes]` markers, preserving valid JSON. If numeric arrays, huge object
  keys or other structural data cannot be bounded by cutting string values, replace the largest field with
  a marked prefix of its serialized JSON text. The strict line-size bound takes priority over retaining that
  field's original type. Bound each file to 64 MiB,
  reserving room for one final timestamped `log_limit` record before disabling further writes. Report the
  limit once on stderr.
- Before opening the new file, remove matching regular `qymcad-mcp-*.jsonl` files whose mtime is older than
  30 days, then delete the oldest above a cap of 200 previous files. The new file is not counted. Never
  remove foreign files, directories or symlinks. Ignore deletion errors and report them at most once.
- Directory/file creation or write failure reports `qymcad-mcp: call log disabled: <reason>` once on stderr
  and disables logging for the process while serving continues. Print the active path (or `off`) on startup
  stderr. In `main`, use `instructions_with_log` with the opened log's path to append exactly
  `"\nCall log of this server process (JSONL, for diagnosing problems): <path>."` to
  `initialize.result.instructions`; append nothing when logging is off. This sentence is the only permitted
  protocol-output difference between logging on and off: all other initialize fields match, and every
  response after initialize is byte-identical (compare raw lines without normalization). The original
  instruction helper stays usable from tests; generated tool schemas and docs remain unaffected. See
  [the implementation review](../../tasks/review-calllog.md).
- Logs contain tool arguments/results (file paths, parameter values, error texts) and process/client metadata.
  They stay on local disk; nothing is sent anywhere. Document the default and opt-out in README and SECURITY.
  Tests spawning the server disable logging, except call-log tests with fresh temporary directories.

## Alternatives considered
- **stderr only:** the MCP client can drop it, and it does not reliably preserve requests/results after a
  failed session.
- **Environment opt-in:** logging would be off when an unexpected future failure makes it necessary.
- **Tracing/OTel crates:** dependency weight and runtime/configuration machinery are unnecessary for a
  single-user local tool with synchronous calls and a JSONL file.

## Consequences
- Failures can be diagnosed from ordered requests, answers and timings independently of the client transcript.
  Abrupt exits and hangs remain diagnosable without a successful shutdown record or a Rust panic hook firing.
- Image payloads and oversized text are intentionally omitted/truncated; this diagnostic log is not a complete
  replay archive. After a write failure or size limit, later calls have no log records.
- The logging module belongs to `crates/mcp`; QymCAD API calls remain entirely inside `crates/engine`.
  Timestamp, truncation, image-redaction and retention selection are pure functions with unit coverage; I/O is thin.
