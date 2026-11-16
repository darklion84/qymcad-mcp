# ADR 0059: Sheet bodies

- Status: accepted (2026-11-16).
- Related: ADR 0052 (Draft and Thicken), 0056/0057 (style: one tool = one app action, refuse only silent wrong results), 0009 (advisory diagnostics), 0026 (`body_boolean`), 0027 (`sketch_project`); F-089 (loft sections), F-150, F-196, F-197.
- Research: `tasks/surfaces-research.md` (sections 0, 1, 8, 10). Package T, step T0: the groundwork before the surface tools (FaceCopy, SurfaceReplace, Patch, Stitch, Trim, OffsetSurface).
- Principle: the tool is the app's action with the app's limits. Where QymCAD silently gives a wrong result on a sheet, only that case is refused and recorded in FINDINGS; no workaround is written.

## What a sheet is

A body with faces and no solid: an open surface. QymCAD marks it `Body.sheet` (set by the kernel on every rebuild, stored in the file, so an opened document knows at once). A sheet has volume 0, no mass, no material and no toolpath, and the app's "one part is one body" rule does not count it. The server already made sheets before this ADR (`loft` with `surface: true`, `op: new_body`) and a GUI file may hold any of the six surface commands, so `doc_open` meets them too. ADR 0052's "the server creates no sheet bodies" was stale and is corrected.

## What the server reports

- `sheet: bool` and `area_mm2` on every body: `doc_info` bodies, the `bodies` of every tool's rebuild/result, the pieces of `split` and `body_boolean`, the `export` per-body entries and the assembly `local_metrics`. `area_mm2` is the sum of the face areas the kernel gave the body (`regen_faces`, or the faces stored with the body for a file opened without a rebuild); for a mesh body, the area of its triangles. Source of the flag: `Project.bodies[i].sheet`.
- `volume_mm3` of a sheet is 0 (the kernel's value: it weighs a stitched pair of faces as 0).
- `export` of a sheet writes it as any body (the app does the same: `file_doors.rs`, no sheet filter) but leaves out `mesh_volume_mm3`, the tetrahedron sum of an open mesh, and adds a per-body `note` saying it is an open surface that is not a printable solid. STEP writes the faces.
- The false warning "result body has N shells and 0 solids; may contain a sealed internal void" is our own check (rebuild and open). A void needs a solid: the check is skipped when the body has no solid. Real solids keep it.

## Defaults skip sheets

The app's `active_body_before` never picks a sheet as the body an operation lands on (a boss united with a sheet failed and ate the sheet). The server now agrees: `tip_body` (the active part's last current solid), `sketch_feature_body` (the same, in the sketch's own part) and the `sketch_project` default (the last result body that is not a sheet) skip sheets. When a document holds nothing but one sheet it is still the only body, so a tool without `body` finds it (thicken, topology). An `extrude op:add` without a target after a loft sheet used to fail with "Boolean" and roll back; it now lands on the solid. Primitives were already right (the core's `finish_base_body` skips sheets).

## Probe: what each tool does on a loft sheet

Fixture: a ruled `loft surface:true op:new_body` between a 20 x 16 rectangle on XY and the same rectangle at z = 10: a tube of four planar faces, 720 mm2, volume 0, open at both ends. Tests: `crates/engine/tests/it/sheet_bodies.rs` (`probe_*`, `shell_of_a_sheet_...`, `chamfer_on_a_sheet_...`), `t0_probes.rs` (the app's own result, no server guard).

| Tool | What QymCAD does | Verdict |
|---|---|---|
| `body_boolean` sheet cut solid | The sheet minus the part inside the solid: 576 mm2 = 720 - 2 x 12 x 6 for a block piercing two walls; the tool is consumed | Right, kept |
| `body_boolean` union (either order), solid cut sheet, intersect (either order) | Refused: "came out empty" / "cut left nothing" / "empty body"; rolled back | Native refusal, kept |
| `move` (translate, rotate) | Moves the surface, still a sheet, area kept | Right, kept |
| `mirror` (keep or not) about an offset plane | Image at x 50..70 (area 720), with keep both halves in one sheet body (1440) | Right, kept |
| `linear_array` / `circular_array` | Copies of the surface in one sheet body, area 3 x 720; overlapping copies share face names and give the "ambiguous native face ids" warning | Right, kept |
| `hole` | The cylinder cuts a round opening in each wall it crosses (blind 6: one wall, 720 - 4 pi; through all: both walls) | Right, kept |
| `shell` | Green node, a smaller sheet with the open face's neighbours cut back (area 480 / 520, no wall) | Wrong silently: refused, F-196 |
| `draft` | The wall tilts about the neutral face's plane and its neighbours follow: area 160/cos 5 + 160 + 200 + (20 + 16 tan 5) x 10 = 734.609 | Right, kept |
| `remove_face` | Refused ("neighbouring surfaces do not extend") | Native refusal, kept |
| `chamfer` | A shared edge is bevelled right (720 - 10 x (2 - sqrt 2)); boundary edges picked with them are skipped silently; alone they fail as "too big"; asymmetric on a boundary edge was already refused | Wrong silently in the mixed case: refused, F-197 |
| `fillet` | Refused by the core ("solid tool on a sheet") | Native refusal, kept |
| `push_face` | Refused by the core ("push face on a sheet") | Native refusal, kept |
| `thicken` | The whole sheet is offset from one face into a solid; either sign (next section) | Right, explicit and tested |
| `split` (body) | The kernel counts no two pieces; refused as "split plane cuts nothing" (the reason is not literally true for a plane across the tube, the refusal is) | Native refusal, kept |
| `split_face` | Refused by the server since ADR 0057 | Kept |

Other readings of a sheet: `topology` lists faces and edges normally; the one-sided boundary edges have no `faces` pair (the kernel's pair API omits them; an `edges[].faces` of null is normal on a sheet), `FaceInfo.normal` is the orientation. `render` draws with back-face culling, as the app does (`raster.rs`): a sheet seen from behind disappears (a closed tube of four faces shows two). That is parity and stays; it is a fact of the picture, not of the model. `part_instance` already ignores sheets.

A Shell or Chamfer node already on a sheet in a GUI file is not judged (the refusals are at creation); the node builds as the app builds it.

## Thicken of a sheet

The app's Thicken takes one face and a thickness in -100000..100000 and refuses only |t| < 1e-9 (`start_thicken_cmd`, `apply_thicken_cmd`); the plate is the whole sheet offset (a planar face is not extruded alone), and `add_thicken` joins it to the part the sheet came from when there is one. So on a sheet either sign is the app's own behaviour: positive grows along the face normal, negative against it. F-150 (a negative value on a solid builds material on the far side) does not apply to a sheet, which has no far side. The server takes either sign for a sheet source at creation, in `dimension_set` and in every evaluated edit (`sheet_thickness`: 1e-9 <= |t| <= 100000), and keeps positive-only for a solid. Hand values on the tube: +1 gives (22 x 18 - 20 x 16) x 10 - 10 x (4 - pi) = 751.416 mm3 (grows outward, rounded corners), -1 gives (20 x 16 - 18 x 14) x 10 = 680 mm3 (inside the sheet).

## Consequences

- Tools: `doc_info`, `export`, `thicken`, `shell`, `chamfer`, `sketch_project` and the server instructions say what changed; `docs/TOOLS.md` is regenerated.
- Tests: `sheet_bodies.rs` (engine; each behavioural change fails without its fix: the report fields, the sealed-void warning, the default body, the shell and chamfer refusals, the negative thicken), `t0_sheets.rs` (the tools), `t0_probes.rs` (the app's result, to be dropped with the refusals when QymCAD fixes F-196/F-197).
- Not covered: the surface commands (P21-P24; P20 and P27 are ADR 0060), the "sheet closes what it replaces" and boundary pre-checks they need, and a rebuild-time judgement of Shell/Chamfer nodes the app made on a sheet.
