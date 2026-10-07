# ADR 0005: The protocol stream does not use file descriptor 1

- Status: accepted (2026-10-05)

## Context
MCP over stdio uses the process's stdout for JSON-RPC. OpenCASCADE prints to the same descriptor: every STEP
write emits a coloured "Statistics on Transfer (Write)" block through its default messenger (FINDINGS F-3C-1).
On stdio that text lands between JSON-RPC lines and breaks the client. The output comes from C++ (`std::cout`),
so nothing on the Rust side can intercept it, and the QymCAD kernel exposes no call to silence the messenger.

Options considered:
1. **Point fd 1 at stderr at startup and speak the protocol on a duplicate of the original stdout.** Covers every
   present and future chatter of the kernel (STEP read, meshing warnings, ...). Needs one `dup2`, which std does
   not offer safely.
   - with `libc` (already in the build through qymcad-io: no new crate) and one `#[allow(unsafe_code)]` call;
   - or with `rustix::stdio::dup2_stdout` (safe API, but a new crate tree for one call).
2. Write STEP in a child process with stdout to null: safe, but serialises shapes across processes and only
   fixes the one call site we know about.
3. Filter non-JSON lines in a relay process: fragile (a partial line without newline corrupts a reply).

## Decision
Option 1 with `libc`. `crates/mcp/src/main.rs::protocol_stdout` clones fd 1 (`try_clone_to_owned`, safe), then
`dup2(2, 1)`; the transport writes to the clone. The workspace lint `unsafe_code` goes from `forbid` to `deny`, so
the single audited `#[allow(unsafe_code)]` (with a SAFETY comment) is possible and any other unsafe still fails
the build.

## Consequences
- Anything printed to stdout by the kernel, or by Rust `println!` in the server process, goes to stderr (the MCP
  client's log). `--dump-tools` / `--version` are unaffected (they do not serve and print normally).
- A protocol test exports STEP twice and pings afterwards; it fails if chatter reaches the stream again.
- Unix only (the server targets macOS; Linux works the same way).
