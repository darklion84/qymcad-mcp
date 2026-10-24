# Exact native fixtures for the release API port

These 20 unchanged QYMB v2 assets were copied byte-for-byte from
`tasks/probe/probe-fixtures/`. They replace test setup APIs made private in QymCAD
v0.1.1-dev.20261026. Test assertions are unchanged. Runtime code never reads them.
No upstream `specimens` feature or raw FFI is used.

The probe generated the primitive/Bezier assets in an isolated program using the
old release v0.1.0-dev.20261001 (d2949f0) and OCCT 7.9.3. Its exact source and pinned
manifest/lock remain in `tasks/probe/probe-fixtures/generator/`. The generator's
original output paths refer to `tasks/probe-fixtures/`; do not run it over the
accepted legacy baseline. `cone-base` is the exact ZIP entry `brep/37.brep` from
`crates/engine/tests/fixtures/cone_negative_chamfer.qcad` (11122 bytes, SHA-256
`b78e3a6fcca0ba8a980bce9c22d668c4c91c06e86df80a221c83802fd04af8c3`); the compound
`cone-and-sphere` adds the disjoint sphere described by the generator.

| Asset | Exact geometry / purpose |
|---|---|
| `cube-10`, `cube-20` | Positive-octant square extrusions, side/height 10 or 20 mm; imports and bounds |
| `hollow` | 20 mm cube minus enclosed 4×6×3 mm cavity, translated to (8,7,7); void diagnostics |
| `sphere-1`, `sphere-2`, `sphere-1000` | Radius 1, 2, 1000 mm spheres; native names initially 1/2/3 |
| `cylinder-1-2`, `cylinder-2-3`, `cylinder-5-10` | Radius/height in mm; native names initially 1/2/3 |
| `cone-<height>` (six files) | r1=10, r2=5; height=5·tan(angle), angles .9°, 3°, 5°, 10°, 20°, 45°; corner-normal tests |
| `freeform-sheet` | Flat unit-square bicubic Bezier sheet; freeform aggregate classification |
| `changing-crease` | Nine sewn Bezier patches, zero free edges; crease changes sign along its length |
| `sphere-and-cylinder` | R2 sphere and disjoint r1/h2 cylinder translated +10 x; mixed native types |
| `cone-base` | Repaired conical bevel with duplicate persistent face names; false sphere-fit support regression |
| `cone-and-sphere` | `cone-base` plus disjoint R1 sphere translated +100 x, names 2000000000–2000000002 |

`src/test_support.rs` uses public `imports::restore` without an outer gate.
For tests needing deliberate names or unnamed edges it edits only documented
QYMB v2/v3 header name tables, preserving the exact OCCT payload. This replaces
the removed native test renamers. Format source for the new release:
`crates/qymcad-kernel/src/occt_io.cpp` (see the release gate audit for exact paths).
