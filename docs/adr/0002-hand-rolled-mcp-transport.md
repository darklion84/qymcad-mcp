# ADR 0002: Hand-rolled synchronous MCP transport, schemas from schemars

- Status: accepted (2026-10-04)

## Context
The server needs MCP over stdio: `initialize`, `notifications/initialized`, `tools/list`, `tools/call`,
`ping`, and JSON-RPC errors. The official Rust SDK (`rmcp`) is async (tokio) and brings a large dependency tree.
OCCT work must run on one thread anyway (`qymcad_kernel::kernel_gate()`; `Shape` is `Send` but not `Sync`).

## Decision
- Implement the stdio JSON-RPC loop by hand on `serde_json` (newline-delimited messages), single-threaded and
  synchronous. Requests are handled one at a time.
- Generate each tool's `inputSchema` from its argument struct with `schemars`, so schemas cannot drift from
  the code that parses them.

## Consequences
- Small, auditable transport (~200 lines) with protocol tests; no async runtime.
- New protocol features (resources, prompts, progress) must be added by hand. If we need much of that, revisit
  `rmcp`.
