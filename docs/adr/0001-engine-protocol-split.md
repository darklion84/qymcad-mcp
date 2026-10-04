# ADR 0001: Engine and protocol are separate crates

- Status: accepted (2026-10-04)

## Context
QymCAD changes daily and its API is not stable. The MCP protocol is a thin, stable layer. Mixing them would
spread QymCAD calls over tool handlers, so every upgrade would touch everything.

## Decision
- `crates/engine` (`qymcad-engine`) owns every call into QymCAD (`qymcad-core`, `-io`, `-kernel`, `-testkit`)
  and exposes a small typed API: `Session` and operations returning plain Rust results.
- `crates/mcp` (`qymcad-mcp`) owns JSON-RPC, tool schemas and argument parsing, and calls only the engine.

## Consequences
- An upgrade changes the engine; the protocol layer and its tests stay put.
- The engine is tested directly (golden parts) without spawning a server.
- A second front end (CLI, Python bindings) could reuse the engine.
