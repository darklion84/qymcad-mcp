# Backlog

Known follow-ups that were consciously deferred. Each item says where it came from.

- **GLB export test checks accessor metadata only** (Codex review, phase 3C): decode the binary positions and
  indices and use an asymmetric fixture to prove +Y vs −Y and winding. The writer is QymCAD's (`qymcad_io::export_glb`).
- **OBJ export with several bodies** (Codex and MiMo reviews, phase 3C): the single-body read-back (bbox, volume)
  is in `obj_is_the_same_solid_in_millimetres`; the per-body `o body_k` offsets across bodies are not tested.
