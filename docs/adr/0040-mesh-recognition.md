# ADR 0040: Recognise mesh bodies as B-reps

- Status: accepted (2026-11-01).
- Related: ADR 0039, ADR 0008, ADR 0020; F-008.

## Context

`import_mesh` stores display-only `MeshPiece` bodies. QymCAD provides the app-path `add_mesh_recognised` exact-surface operation and `add_mesh_solid` polyhedron operation, both consuming the mesh and creating an ordinary timeline body.

## Decisions

1. Expose `recognise` with `body`, `mode` (`exact` or `polyhedron`), `tolerance`, `sharp`, `simplify`, and optional `name`. Exact uses `add_mesh_recognised`; polyhedron uses `add_mesh_solid`. Tolerance and sharp are refused for polyhedron.
2. Store input formulas through native feature dimensions (`tol`, `sharp`, `simplify`) so `param_set` and `dimension_set` rebuild the existing node. Values are checked before native evaluation, including on later evaluated formula edits.
3. Refuse B-rep, consumed, unknown and non-finite/out-of-range inputs before native work. Exact is capped at 50,000 decoded triangles; polyhedron uses the import cap of 200,000. Refusals preserve the exact archive through the existing atomic/history boundary.
4. Success requires no rebuild errors and at least one solid. Shell-only or pass-through native results are refused with shell/solid counts and repair/polyhedron guidance. There is no silent mode fallback.
5. Results report feature/body/source ids, mode, evaluated settings and formulas, face/shell/solid counts, volume, bbox and warnings. Recognised bodies use all existing topology, selection, modifier, boolean and export paths; undo restores the source mesh.
6. `recognise` is a normal modelling tool, included in batch with one enclosing undo entry.

## Evidence

`crates/engine/tests/mesh_import.rs` asserts the cube `S^3` volume and six faces, cylinder exact `pi*r^2*h` and polyhedron `(n/2)*r^2*sin(2*pi/n)*h`, generated fixtures, persistence and existing import cap behavior. `tasks/research-medC-probes.md` section 2 records the pinned app-path native outputs and cost. `docs/TOOLS.md` is generated from the registry.
