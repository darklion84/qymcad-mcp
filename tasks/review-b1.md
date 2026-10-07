# B1–B6 review

Worktree: phase4b/backlog. No commits, per user instruction; `.git` is outside writable roots.
Pinned dependencies and engine version: v0.1.0-dev.20261001. Installed `~/Applications/QymCAD.app` executable contains `v0.1.0-dev.20261001d2949f0a7`.

## B1 — curved corner classification

Files: `crates/engine/src/topology.rs`, `crates/engine/tests/golden_curved_corners.rs`, `crates/mcp/tests/edge_corners.rs`, selection/fillet descriptions, ADR 0010 and F-053.

First failure (`cargo test -p qymcad-engine --test golden_curved_corners`, before implementation, exit 101):

```text
assertion `left == right` failed: boss/plate circle is the sole concave corner
  left: []
 right: [1610612748]
called `Result::unwrap()` on an `Err` value: Invalid("the edge selection matched no edge of body 28")
both hole rims are convex: [1610612748, 1610612749], got [1610612737, 1610612736, 1610612738, 1610612739, 1610612743, 1610612746, 1610612747, 1610612740, 1610612741, 1610612742, 1610612744, 1610612745]
assertion `left == right` failed: recessed floor-to-counterbore-wall circle is concave
  left: []
 right: [1610612754]
```

MCP first failure (`cargo test -p qymcad-mcp --test edge_corners`):

```text
assertion `left == right` failed: a cylinder has two convex rims
  left: Number(0)
 right: 2
```

Formula/test details are in `tasks/review-b1-tests.md`. Base fillet uses Pappus with cross-section area r²(1−π/4), radial first moment r³(5/6−π/4), V_added=2π[R·area+first moment]. Server and native GUI paths edit r,d,h,t independently after save. Counterbore floor/wall at the larger radius is the concave step corner; the smaller bore/floor shoulder is convex (270° versus 90° material sectors).

Mutation and final verification: pending.

## B2 — empty corner hint

Files: `crates/mcp/src/tools/topology.rs`, `crates/mcp/src/tools/features.rs`, `crates/mcp/tests/edge_corners.rs`.
The hint appears for empty direct/nested corner selections, with a named-feature between example. Noncorner empty selections have no hint. The fillet description includes the same example.

First failure, same baseline MCP command:

```text
assertion `left == right` failed
  left: Null
 right: "0 corner edges; for a junction between named features use {\"between\": [{\"of_feature\": \"X\", \"role\": \"wall\"}, {\"of_feature\": \"Y\", \"role\": \"cap_end\"}]}"
```

Mutation and final verification: pending.

## B3 — stored topology and void diagnostics

Pending integration; detailed evidence in `tasks/review-b3.md`.

## B4 — output warnings

Files: `crates/mcp/src/tools/output.rs`, `crates/mcp/tests/backlog_output_history.rs`. Two tests compare export warning strings with doc_info and retain render's image while checking its warning text. Formula fixture: 20³−4×6×3.001 mm³ interior pocket.
First failure: export `left: Null` versus `Array [String("pocket (35): result body has 2 shells and 1 solids; may contain a sealed internal void")]`; render `render must repeat current document warning: pocket (35): result body has 2 shells and 1 solids; may contain a sealed internal void`.
Mutations: return empty export warnings and skip render warnings (`take(0)`); both regressions fail, restored by editing. Exact evidence in `tasks/review-b4b5.md`.

## B5 — restored undo values

Files: `crates/mcp/src/tools/history.rs`, `crates/mcp/tests/backlog_output_history.rs`. Undo includes the complete restored parameter list, expressions/evaluated values, alongside original call metadata and current bodies. Test covers t=5, h=t+1, height volume 20×30×6, uppercase normalized edit, parameter creation/deletion.
First failure: `undo must report restored parameter expressions and values`, `left: Null`, expected parameters t expression 5 value 5, h expression t+1 value 6.
Mutation: return an empty params array; regression fails, restored by editing. Exact evidence in `tasks/review-b4b5.md`.

## B6 — tessellated area descriptions

Files: `crates/mcp/src/tools/topology.rs`, `crates/mcp/src/tools/sketch.rs`, generated `docs/TOOLS.md`. Docs only. Descriptions identify tessellation-based areas, typically 0.1–0.2% below analytic for curved geometry. Registry freshness verification pending.

## Refusals and limits

- No commits or `.git` writes attempted, per explicit request.
- QymCAD dependency sources remain read-only and pinned.
- Process inspection via `ps` was refused by sandbox (`operation not permitted`); build status obtained from Cargo output and `lsof` instead. No implementation item blocked by this.
- Seams/G1 junctions excluded; unavailable/degenerate shared tessellation omitted instead of guessing.

## Final verification

Pending scripts/check.sh, generated TOOLS freshness, final test count.
