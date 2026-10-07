# ADR 0004: PNG encoding for `render`

- Status: accepted (2026-10-05)

## Context
`render` returns a PNG as an MCP image item (base64 in the JSON-RPC reply), so every byte travels through the
transport and the client. A CAD render is mostly flat background and flat-shaded faces, which compresses very well;
an uncompressed image does not (512 × 384 RGB is 590 KB raw, ~790 KB in base64).

Options considered:
1. **The `png` crate.** Complete encoder/decoder. New crates: `png`, `fdeflate`, `simd-adler32`, a second
   `miniz_oxide` (0.8 next to the tree's 0.9).
2. **Hand-written PNG with stored (uncompressed) deflate blocks.** No dependency, ~60 lines, but the file is as big
   as the raw pixels: ~590 KB per 512 × 384 image.
3. **Hand-written PNG container + `flate2` for the zlib stream.** `flate2` (and `miniz_oxide`) are already compiled
   into the server through qymcad-io (`zip`, `usvg`), so this adds no crate to the build. The container is
   ~40 lines: signature, IHDR/IDAT/IEND chunks, a bitwise CRC-32.

## Decision
Option 3 for the encoder (`crates/engine/src/pngfile.rs`): 8-bit RGB, filter Up on every row, default zlib level.
The `png` crate is a **dev-dependency of the engine only**, used as an independent decoder in tests, so the encoder
is not checked only against itself. Base64 (for the MCP image item) is a ~20-line function in
`crates/mcp/src/tools/output.rs`, tested against the RFC 4648 vectors.

## Consequences
- Measured: the golden plate renders to 3.4 KB (top, 512 × 384), 1.6 KB (front), 4.7 KB (iso, 320 × 240) — about
  170× smaller than option 2.
- The shipped binary gains no new crate; tests gain `png` (+ `fdeflate`, `simd-adler32`, `miniz_oxide` 0.8).
- If QymCAD ever drops its `flate2` users, `flate2` becomes a real new dependency of ours; it is small and the
  standard choice, so that is acceptable.
