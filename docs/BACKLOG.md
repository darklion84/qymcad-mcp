# Backlog

Known follow-ups that were consciously deferred. Each item says where it came from.

- **GLB export test checks accessor metadata only** (Codex review, phase 3C): decode the binary positions and
  indices and use an asymmetric fixture to prove +Y vs −Y and winding. The writer is QymCAD's (`qymcad_io::export_glb`).
- **OBJ export test counts faces only** (Codex review, phase 3C): parse vertices/faces, check indices, signed
  volume, bounds and multiple bodies.
