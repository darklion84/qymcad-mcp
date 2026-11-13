# Package J review log (0.1.1-dev.20261026.15 + primitives)

Subject: `primitive` (ADR 0056), branch medium-j on top of 0.1.1-dev.20261026.15. Principle: every tool is one existing QymCAD app action; only a silently wrong native result is refused, nothing is worked around, and a refusal comes with a FINDINGS entry.

| Round | Subject | Verdict |
|---|---|---|
| Phase 0 | Native probes `j0_probes` (core API, pin f25f27d), results in ADR 0056 "Probes" | done, one reconnaissance expectation disproved (probe 8) |
| Implementation | `1bece3e` (engine `primitive.rs`, MCP tool, tests, `docs/TOOLS.md`) and the docs commit | built; no independent review round has been run |

## What the probes changed against the reconnaissance

| Expectation | Observed | Done |
|---|---|---|
| A formula that stopped evaluating is taken silently as the stored number (`eval_dim`) | The rebuild reports `UnknownName` / `UnexpectedEnd`; the server's `param_delete` already refuses a used parameter | No code, tests only (probe 8). |
| The core's missing minimum / maximum gives wrong bodies | Wrong only BELOW about 1e-5 mm (a third of the volume, 2 faces, `errors: []`); exact up to 1e12 mm | The fields' minimums are the protection (F-180); the upper 1e5 and the torus gap are kept as the app's field policy, said so in the refusal text and ADR. |
| `n` out of range is a silent coercion | Confirmed: rounds half away, clamps to 3, no upper limit (F-181) | Whole 3..=64 on every edit. |
| Cone `r1 == r2` is an unknown | Visible `OpFailed(Cone)` down to 1e-7 apart (F-182) | Clear refusal at 1e-7 or less. |
| A disjoint union is a candidate for refusal | One body, two solids, correct volumes | Allowed and described. |
| New: suppressing a united primitive | Unbuilds the union; the Part's body disappears, reported in `unbuilt` (F-183) | Accepted, tested, described. |
| New: several bodies in a Part | The primitive is united with the last body only (F-184) | Accepted, tested, described. |

## Decisions that a reviewer should look at first

- The fields' limits are enforced on every edit, not only at creation, through `check_evaluated_features` (the global guard). A file with a primitive already outside the fields opens with a warning and the node is exempt (the shared `open_node_faults` set, renamed from `open_datum_faults`) until it is back inside; while exempt, every size of that node is unguarded (listed under Known limits in the ADR).
- The cone's `r1` lower bound is 0.01 at creation and 0 on edit, copied from the app's two fields (`start.rs`, `reopen.rs`); not run in the GUI.
- `place` takes the point's or the vertex's coordinates as a pure translation; the primitive's reference point is its base centre (box, cylinder, cone, prism) or its centre (sphere, torus). A placement on a plane or a face is not offered.
- The undo label and the batch `created` entry follow `feature`, not `body` (`body` is the union node for the second primitive).

## Evidence

- Targeted (observed): `j0_probes` (8), `golden_primitives` (12), the unit test in `primitive.rs`, `mcp/tests/j1_primitive` (5), `protocol`, `batch`, `docs_fresh` and `coverage_matrix` pass; `cargo fmt` and `cargo clippy --workspace --all-targets -D warnings` clean.
- `scripts/check.sh` on `007fc88`: OK, 1494 passed, 0 failed, 3 ignored (fmt, clippy `-D warnings`, `cargo test --workspace`; the sum of the `test result` lines of the run).
- Not run: QymCAD.app itself (the GUI); every app-side claim is a native core-API probe or read in source, as marked in FINDINGS. No independent review round of this package has happened yet.
