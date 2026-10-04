# Architecture

## Layers
```
MCP client (Claude, ...)
   │  JSON-RPC over stdio
crates/mcp  (qymcad-mcp)      transport, tool registry, argument schemas (schemars), result formatting
   │  typed Rust calls
crates/engine (qymcad-engine) Session: one open document; params, sketches, features, topology, export, render
   │  QymCAD API
qymcad-core / -io / -kernel / -testkit   (git dependency, pinned tag — ADR 0003)
   │
OpenCASCADE (dylibs, Homebrew)
```
Rules: only `engine` touches QymCAD (ADR 0001); the process is single-threaded and synchronous (ADR 0002).

## Session and the regenerate pipeline
(Filled in during phase 1.)

## Roadmap
| Phase | Scope | Status |
|---|---|---|
| 0 | Repository skeleton, pinned build, docs, ADRs, smoke test | in progress |
| 1 | Engine core: session, regenerate pipeline, params, rect/circle sketches, extrude/cut, offset plane | — |
| 2 | MCP transport + phase-1 tools, protocol tests, registration | — |
| 3 | Sketch entities & constraints; topology + fillet/chamfer/hole/shell/arrays/mirror/revolve; export + render | — |
| 4 | Acceptance on real parts (collet test plate, hanging shelf) vs build123d references | — |
| 5 | Release 0.1.0 | — |
