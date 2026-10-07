# U3 review

- [x] Read project instructions, architecture, findings, backlog and pinned kernel source.
- [x] Review supported scope with coordinator: straight edges between two planar faces; curved junctions, seams and tangent junctions remain unmatched.
- [x] Add failing MCP-only L-profile selection regression (`crates/mcp/tests/edge_corners.rs`).
- [ ] Record first failure.
- [ ] Implement classifier and composable concave/convex parser selections.
- [ ] Run regression and engine tests.
- [ ] Mutate classifier sign, record failure, restore by editing.
- [ ] Report files, source evidence, formula and deferred scope options.

Formula: L area = 20×15 − 16×11 = 124 mm²; volume = 124×8 = 992 mm³.
Six profile vertices yield six y-directed edges: one reflex inner corner at (4,−4,4), five convex corners.
Two six-edge caps add twelve more straight planar junctions, for eighteen classified edges total.

Design: use both adjacent planar outward normals and local incident triangle interior direction; a global
face centroid is unsuitable on nonconvex faces. Pinned kernel has adjacency but no signed dihedral query.
