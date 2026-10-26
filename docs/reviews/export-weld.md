# Mesh export weld review log (F-093)

The fix was written by Claude (Opus 5.5) after the user's slicer (Bambu Studio) reported "680 non-manifold edges" on
a 3MF exported by the server. It was reviewed read-only on identical snapshots by OpenAI Codex (gpt-6.1-sol, high
reasoning) and Xiaomi MiMo (MiMo-V2.6-Pro). Every behavioural fix came with a test that failed on the code before it
and a mutation that turned it red again.

| Round | Codex | MiMo | Outcome |
|---|---|---|---|
| 1 | REJECT: welding across a whole body fused solids that only touch at an edge or corner; the test proved edge pairing, not a manifold; docs overclaimed for STL and bit identity | REJECT: the same major; a dropped sliver could open an edge (rejected: such an edge is already covered on that side only by zero area); STL wording; brittle 3MF parsing; normals turned for nothing | 8f5a9d9, 98364ef: pieces joined across two-triangle edges; a four-format closed-manifold test (3MF, OBJ, GLB, STL); weld before the transform |
| 2 | REJECT: four solids around a corner cut a base face off (all its edges carry four triangles); per-edge allocations | did not run (path errors) | 1ade325: triangles around an edge of more than two paired by the wedge of solid between them; one sorted edge vector (1e6 triangles in ~0.25 s) |
| 3 | REJECT: coincident faces of solids lying face to face were paired across a zero wedge; whole-surface pieces collapsed a ring touching itself along an edge | REJECT: the same coincident-face major; an edge of three untested; unnormalised angle basis | d91e7b7, cf06efd: points split per local fan (corner union-find); ties ordered solid-behind first from the widest gap; edge-of-three test |
| 4 (narrow) | REJECT: a solid pinched along an edge (both ends single fans) keeps four triangles there | ACCEPT; minor: a tie taking the whole edge could still pair across the seam | 7a3c6e7 for the minor. The pinched edge is a known limit (F-093, BACKLOG, ignored test): the user chose to ship after the research below |

Research (round 4): the 3MF Core Specification 4.1 requires every triangle edge to share its endpoints with exactly
one other triangle's edge, and 4.1.3 says a producer should not duplicate vertices unless joining them would create
non-manifold edges; CGAL's polygon soup repair likewise duplicates the points of edges shared by more than two
polygons. The weld follows that rule; the pinched solid is non-manifold in its own geometry.

MiMo's claim check (round 4, nit, correct): `welding_leaves_a_stray_sheet_on_an_edge_of_three_unpaired` passes on
the round 3 code too; it is a coverage test, not a regression.

Live check: all seven door hanger parts exported through the MCP binary to 3MF and OBJ are closed manifolds with
unchanged mesh volumes; the user's slicer no longer reports errors on `joint_key.3mf`.
