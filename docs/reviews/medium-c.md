# Review log: remove face, mesh import, mesh recognition (ADR 0038–0040, 0.1.1-dev.20261026.7)

Authors: sol61 (gpt-6.1-sol through OmniRoute/OpenCode) for stages C0–C3b; Claude (Opus 5.5) accepted every stage
(report against the task items, tests read one by one against the items, its own production mutations, full check.sh)
and wrote the import_mesh security hardening (F-126), F-127 and the fixes below. Reviews follow the incremental
policy: each round reviews only its own diff and the contracts it touches.

| Round | Scope | Verdict | Findings | Fixed in |
|---|---|---|---|---|
| acceptance of C1 (Claude) | remove_face | fixed | localisation showed a raw key; items 5 and 9 missing (C1b); first own mutation survived, test strengthened | 032722a |
| acceptance of C2/C2b (Claude) | import_mesh | fixed | report claimed complete with 9 of 10 items missing (C2b); two own mutations survived (16 MiB cap, decoded-triangle cap); B-rep tools gave misleading errors on mesh bodies — one refusal in `source_body`/`topo_body` | 1c3b6a7 |
| automated security notes 1–4 + Claude probes | import_mesh | fixed | glTF buffer `uri` / OBJ `mtllib` read any file (`../`, absolute, `/dev/zero` hang) — confirmed by probe; 3MF/zipped AMF inflated past declared size; OBJ continuation-line parser differential; GLB last-JSON-chunk differential; check/read race; a buffer naming the mesh itself overwrote the staged copy | 2d3aee6, 0c67ed3, 0d4a13e, d51387e |
| acceptance of C3/C3b (Claude) | recognise | fixed | Missing=None with boolean, cap acceptance, rollback and B-rep refusal not really tested (a setup call failed before the code under test; a snapshot taken after the refusal); m1–m6 re-run by Claude; native call recognises a B-rep (F-127) | f077ee2, 5ca3dbe |
| live MCP test (sol61, binary 0d4a13e) | remove_face, import units/move/refusal, recognise + fillet + STEP, refusals, param edit, save/open | checks 1–6 PASS | reported FAIL was tester misuse (recognise on a body already consumed); cylinder save/open rechecked by Claude through MCP; `recognise` result bbox was the source mesh's | 4435028 |
| review C (sol61) | f38cc1a..0d4a13e | REJECT | 1 "major": the public engine API follows a symlinked main mesh path. MCP `checked_path` refuses it, so not reachable through MCP; the window after that check was real and is closed | a94e6ba |

Checks: check.sh 1009 passed, 3 ignored on a94e6ba. Live MCP test: remove_face heals a blind hole (31371.68 → 32000,
undo back), STL mm/cm import and move, mesh-body refusal text, recognise polyhedron cube + fillet 997.854 + STEP,
exact cylinder 12566.37 (3 faces), refusals unchanged, `t` 1→2 on an exact recognition — PASS. Manual QymCAD.app
check: four files in `tmp/gui-check-medC` (remove_face h 20→30, mesh cube + app Recognise, recognised fillet r 1→2,
exact cylinder t 1→2) — pending at the time of this log.
