# Findings about QymCAD

Facts about the QymCAD codebase that this server depends on, each with its evidence. This file is the single
source of truth: when QymCAD changes, re-verify the entries here (see [UPGRADING.md](UPGRADING.md)).

Conventions:
- **Version** is the QymCAD tag the entry was verified against.
- **Evidence** says how it was established: `test:` (a test in this repo), `observed:` (ran it and saw it),
  `source:` (read the code, path relative to the QymCAD repo root).
- An entry without evidence is a guess and must not be relied on.

---

## F-001 Uppercase parameter names never trigger a rebuild in the GUI

- **Version:** v0.1.0-dev.20261001
- **What:** `Project::param_map()` keys parameters by `name.to_lowercase()`, but `expr::mentions` /
  `expr::occurrences` match with a case-sensitive `str::find`. The GUI computes changed parameters from
  `param_map` (lowercase) and asks `mark_param_dependents_dirty_for(name)` to find features whose expressions
  mention it; an expression `T` never matches `t`. Result: editing parameter `T` moves sketches (the GUI
  re-solves every sketch that has any expression) but the solid does not rebuild.
- **Evidence:**
  - observed: user edited `T` in QymCAD.app, sketch moved, body did not.
  - observed: repro of the GUI path (load with B-rep, snapshot `param_map`, edit, `mark_param_dependents_dirty_for`,
    partial regenerate) gave an empty plan (0 of 7 nodes); with lowercase names the plan was 4 nodes and the
    volume correct.
  - source: `crates/qymcad-core/src/model.rs` `param_map` (~2123); `crates/qymcad-core/src/expr.rs`
    `occurrences` (~329); `crates/qymcad-ui-state/src/lib.rs` `mark_changed_params_dirty` (~7497).
- **How we handle it:** the engine lowercases parameter names and every expression it writes. Identifiers are
  case-insensitive in evaluation anyway (the tokenizer lowercases them, `expr.rs` ~97), so this is safe.
- **Also affected upstream:** `rename_driver` finds case-insensitively but rewrites case-sensitively
  (`drivers.rs` ~142-174).

## F-002 Parameter edits need `eval_parameters()` before regenerate

- **Version:** v0.1.0-dev.20261001
- **What:** regenerate reads cached `Param.value`s via `param_map()`; `settle_sketches` deliberately skips
  `eval_parameters`. After changing `Param.expr`: `eval_parameters()`, re-solve sketches that carry dimension
  expressions (`solve_sketch(si)` + `mark_sketch_dirty`), mark feature dependents dirty, then regenerate.
- **Evidence:** source: `crates/qymcad-core/src/model/regen.rs` `regenerate_watched` (~746-765);
  `crates/qymcad-core/src/model/sketch.rs` (~3947).

## F-003 A cut's direction is `Extent.reach`, not `down`

- **Version:** v0.1.0-dev.20261001
- **What:** for `add_combine_multi_op`, the tool spans `[0, h]` along the sketch normal N for `Reach::Forward`,
  `[-h, 0]` for `Backward`, `[-h/2, h/2]` for `BothWays`; `down` only extends the far side. A pocket sketched on a
  plane at the top face must use `Backward`. `through: true` ignores the height and spans the whole source body.
- **Evidence:** observed (prototype pocket cut nothing with `down`); source: `regen.rs` (~2951-2999),
  `crates/qymcad-core/src/feature.rs` (~1472-1481).

## F-004 Base-plane normals: XZ points to −Y

- **Version:** v0.1.0-dev.20261001
- **What:** `BasePlane::frame()`: XY x=+X y=+Y n=+Z; **XZ x=+X y=+Z n=−Y**; YZ x=+Y y=+Z n=+X.
- **Evidence:** source: `feature.rs` `BasePlane::frame` (~271-275).

## F-005 A datum plane created through the API is unresolved until the first regenerate

- **Version:** v0.1.0-dev.20261001
- **What:** a plane's origin/normal is resolved during regenerate (`resolve_plane_into`). A sketch on a fresh
  datum plane, consumed in the same regenerate, can fail (`CutRemovedNothing`); a second pass with the nodes
  dirty fixes it. More generally `CoreError::retryable()` errors need another pass.
- **Evidence:** observed in the prototype (`qymcad_demo/src/agent_plate.rs` in the 3d_modeling project needed
  two passes); source: `sketch.rs` `resolve_plane_into` (~3819), `crates/qymcad-core/src/errors.rs`
  `retryable` (~415).

## F-006 `contour_ids` order is not geometric; pick profiles by nesting

- **Version:** v0.1.0-dev.20261001
- **What:** circles without intersections are pushed before traced faces, so for a rectangle with four circles
  `contour_ids` starts with the circles. An empty profile list means "the first contour" (a hole disc here).
  Selecting a contour selects "itself minus its direct children": `[outer]` = plate with holes,
  `[circles]` = discs, `[outer, circles]` = solid plate. Use `project.contours.parent_of(cid)` (`Some(0)` = root).
- **Evidence:** source: `crates/qymcad-core/src/model/tess.rs` (~527), `sketch.rs` `profile_faces` (~3537-3556),
  `crates/qymcad-core/tests/fill_solid_profile.rs`.

## F-007 Headless regenerate does not fill `Body.faces`; save needs them

- **Version:** v0.1.0-dev.20261001
- **What:** regenerate writes meshes into bodies but faces only into `regen_faces` and `report.built`. The GUI
  copies them with `set_body_faces`. A file saved without faces opens with geometry, but bodies that have a
  B-rep blob get no faces (`detect_missing_faces` skips bodies with a live shape), so face picking in the GUI
  degrades. Before saving: `for (id, f) in &report.built { p.set_body_faces(*id, f.clone()) }`.
- **Evidence:** source: `regen.rs` (~3010-3044), `crates/qymcad/src/gui/io_jobs.rs` (~461-463, 951-985),
  `crates/qymcad-ui-state/src/lib.rs` (~13548-13560).

## F-008 Failed modifiers pass their source through

- **Version:** v0.1.0-dev.20261001
- **What:** a modifier that fails still has a shape (its unchanged input) and the error is in
  `RegenReport.errors`. A shape existing for a node is not proof of success. Partial fillets are warnings in
  `project.regen_warnings` (`EdgesDropped`), not errors.
- **Evidence:** source: `regen.rs` (~1191-1236, 1347, 1408).

## F-009 Feature ids: node id == body id; features consume their source

- **Version:** v0.1.0-dev.20261001
- **What:** every `add_*` modelling call allocates a new body whose id equals the timeline node id, consumes
  `src`, and returns the new id; pass it as the next `src`. Branching from a consumed body creates a ghost chain
  the GUI refuses. `feat_dims` are keyed by node id.
- **Evidence:** source: `crates/qymcad-core/src/model/timeline.rs` (feature adders), observed in the prototype.

## F-010 Edge and face ids are persistent names, not positions

- **Version:** v0.1.0-dev.20261001
- **What:** `MeshFace.id` / `MeshEdge.id` are recipe-based names (`project.names`) that survive regeneration and
  upstream edits; `add_fillet(src, r, ids)` matches them by name, and stale ids are repaired only on an
  unambiguous match (logged in `RegenReport.rebinds`). Descriptive selections (`refs::Ref` + `Query::{Adjacent,
  TangentChain, Oriented, Extreme, OfFeature, ...}`) via `add_fillet_ref` / `add_chamfer_ref` are re-evaluated on
  every rebuild, **but stored edge queries are not safe in QymCAD.app after reopening (F-3B-2)**: this server
  stores edges as pick lists and keeps queries only for faces. Topology must be re-read after every regenerate;
  ids live on the specific body.
- **Evidence:** source: `crates/qymcad-core/src/names.rs`, `crates/qymcad-core/src/refs.rs` (~69-154),
  `crates/qymcad-testkit/tests/topo_naming_survives_edit.rs`.

## F-011 `MeshFace.normal` is an area-weighted average

- **Version:** v0.1.0-dev.20261001
- **What:** on curved faces (a full cylinder) it is near zero. Classify faces with `Shape::face_cylinder`,
  `Shape::face_axis` (cylinder or cone) and mesh-normal flatness instead.
- **Evidence:** source: `crates/qymcad-core/src/geom.rs` `MeshFace` (~701), `crates/qymcad-kernel/src/lib.rs`
  (~1211, 1243).

## F-012 No format-version check on load

- **Version:** v0.1.0-dev.20261001
- **What:** `meta.ron` (`schema_version: 2`) is written but never read. A `.qcad` from another build loads unless
  serde fails (for example an unknown enum variant). Hence the server pins the same QymCAD release as the
  installed app.
- **Evidence:** source: `crates/qymcad-io/src/project_file.rs` (~14, 182-198).

## F-013 macOS packaging: `.qcad` is not a registered document type

- **Version:** v0.1.0-dev.20261001
- **What:** `open -a QymCAD file.qcad` and double-click fail with "cannot open files of this type"; files must be
  opened with File > Open.
- **Evidence:** observed on macOS 15 with the release zip.

## F-014 Build: git dependency works; OCCT dylibs use absolute install names

- **Version:** v0.1.0-dev.20261001
- **What:** the QymCAD crates resolve as git dependencies (workspace inheritance works; no out-of-crate
  includes). `qymcad-kernel/build.rs` reads `OCCT_INCLUDE_DIR` / `OCCT_LIB_DIR` and links OCCT as dylibs; brew's
  install names are absolute, so no `DYLD_LIBRARY_PATH` is needed at runtime. A brew OCCT upgrade (7.9 -> 7.10)
  requires a rebuild. Dependency workspaces' profiles are not inherited (we copy `opt-level = 3`).
- **Evidence:** observed: `cargo build` from this repo, smoke test `crates/engine/tests/smoke.rs`.

## F-015 GUI automation (for manual checks only)

- **Version:** v0.1.0-dev.20261001
- **What:** the egui window exposes an accessibility tree only while frontmost; element-index clicks work, but
  synthetic keyboard/coordinate input via Orca computer-use is refused (window never reports AX focus), and
  `set-value` on egui fields does nothing. Real CGEvents after `NSRunningApplication.activate()` work.
  Unfocused keystrokes act as hotkeys (Delete removed a sketch dimension once). Not used by the server.
- **Evidence:** observed 2026-10-04.

## F-016 `Shape::bbox()` is inflated by edge tolerances after booleans

- **Version:** v0.1.0-dev.20261001
- **What:** `bbox()` comes from `BRepBndLib::Add`, which includes tolerances. A 60 × 40 × 6 plate after hole/pocket
  cuts reports 60.0126 × 40.0126 × 6.0126 (+0.0063 mm per side). Compare sizes with a tolerance of ~0.05 mm;
  use volume for exact checks.
- **Evidence:** test: `crates/engine/tests/golden_plate.rs` `plate_volume_and_bbox`.

## F-017 After a parameter edit a cut can come out 0.001 mm deeper than on a fresh build

- **Version:** v0.1.0-dev.20261001
- **What:** a 30 × 16 pocket cut 3 mm down from a datum plane at the top face is exact when built fresh, but after
  changing the plate thickness (which moves the plane and the top face) the rebuilt part has 0.48 mm³ less
  volume, i.e. the pocket is 3.001 deep. Reproducible for any thickness. Probably the coincident-face nudge of
  one-sided cuts (F-003) applied against the moved plane. 0.004 % of volume: irrelevant for printing, but
  golden tests that edit parameters use a 1 mm³ tolerance. Not investigated further.
- **Evidence:** observed with a probe test (t = 6/8/10 → +4: built exact, edited −0.48 mm³ every time); also in
  the 2026-10-04 prototype (21923.347 vs 21923.83).

## F-018 The app bundle does not carry the release tag in Info.plist

- **Version:** v0.1.0-dev.20261001
- **What:** `CFBundleShortVersionString` and `CFBundleVersion` are both `0.1.0`. The release tag, commit and
  date are embedded in the executable as `v0.1.0-dev.20261001d2949f0a72026-10-01macosaarch64`. The server's
  startup check searches the executable for `v<x.y.z>-dev.<8 digits>`.
- **Evidence:** observed: `plutil -p ~/Applications/QymCAD.app/Contents/Info.plist`; `strings` on
  `Contents/MacOS/qymcad`.

## F-3C-1 OpenCASCADE prints to stdout on every STEP write

- **Version:** v0.1.0-dev.20261001
- **What:** `qymcad_kernel::write_step` (`qym_step_write`, `STEPControl_Writer`) prints a coloured block to fd 1:
  `Statistics on Transfer (Write)`, `Transfer Mode = 0 I.E. As Is`, `Step File Name : <path>(350 ents) Write Done`
  (ANSI escapes included), on every call, not only the first. On an MCP stdio server this corrupts the JSON-RPC
  stream (the client reads `\u001b[32;1m` as a response).
- **Evidence:** observed: piping requests into `qymcad-mcp` before the fix, stdout contained the block before
  each `export` reply; test: `crates/mcp/tests/protocol.rs` `exports_through_mcp` failed with
  `bad response "\u{1b}[32;1m\n"`. source: `crates/qymcad-kernel/src/occt_io.cpp` `qym_step_write` (~2510), no
  messenger configuration anywhere in the kernel (`grep Messenger` finds nothing).
- **How we handle it:** the server moves fd 1 to stderr at startup and speaks the protocol on a duplicate of the
  original stdout (ADR 0005).

## F-3C-2 The app exports STEP/GLB/3MF as a component tree; `write_step` and the flat mesh writers do not

- **Version:** v0.1.0-dev.20261001
- **What:** File > Export in QymCAD.app writes STEP via `write_step_tree` and GLB/3MF via `export_glb_tree` /
  `export_3mf_tree` with `project.export_tree(root, ..)`: component names, colours, placements, and **one body per
  part** (`active_body`). STL/OBJ and IGES go out flat. We write every format flat: `write_step(&[(shape,
  body_world_transform)])`, `export_{stl,3mf,glb,obj}(meshes)` with `mesh.transform(body_world_transform)` — one
  solid/object per result body (named `body_<n>` in 3MF/GLB), no colours, no tree. Flat keeps every result body
  of a multi-body part, which the tree would drop. STEP written this way reads back (`read_exact`) as one shape per
  body with the same volume (within 1e-3 mm³) and `write.step.unit = MM`. Mesh deflection presets are the app's:
  draft 0.2, standard 0.05, high 0.02, max 0.005 mm; meshes are re-tessellated from the live B-rep with
  `Shape::tessellate_merged(deflection)`. GLB positions are metres with +Y up: (x, y, z) mm → (x, z, −y)/1000.
- **Evidence:** source: `crates/qymcad/src/gui/io_jobs.rs` `write_exact_to` (~888-934), `write_mesh_to` (~727-800),
  `mesh_job` / `tree_to_write` (~681, ~876); `crates/qymcad-core/src/model/assembly.rs` `export_node` (~344);
  `crates/qymcad/src/gui/panels_windows.rs` `mesh_quality_dialog` (~1721-1727); `crates/qymcad-io/src/gltf.rs`
  (~24). test: `crates/engine/tests/golden_export.rs` (`step_reads_back_with_the_same_volume`,
  `glb_is_in_metres_with_y_up`, `threemf_is_in_millimetres`).

## F-3C-3 Mesh quality presets do not change small holes: the kernel's angular deflection (0.3 rad) decides

- **Version:** v0.1.0-dev.20261001
- **What:** `qym_shape_tessellate` meshes with `BRepMesh_IncrementalMesh(shape, defl, false, 0.3, true)`. On the
  golden plate (Ø4.5 holes) draft, standard and high give the identical mesh (716 triangles, volume error
  1.42 mm³ = 0.011 %); only max (0.005 mm) refines it (1116 triangles, 0.56 mm³). The linear deflection matters only
  for larger radii. For printing this is harmless; do not expect `quality` to change a small part's file.
- **Evidence:** observed with `finer_quality_never_loses_accuracy` instrumented (counts and errors above);
  test: `crates/engine/tests/golden_export.rs` `finer_quality_never_loses_accuracy`; source:
  `crates/qymcad-kernel/src/occt_bridge.cpp` `doc_from_shape` (~143-152).

## F-3C-4 The app's isometric view is `Cam3::default()`: yaw −0.7, pitch 0.6

- **Version:** v0.1.0-dev.20261001
- **What:** the default 3D camera (and the component thumbnails) look from yaw −0.7 rad, pitch 0.6 rad; `Cam3::basis`
  builds the forward vector `(−cos p·cos y, −cos p·sin y, −sin p)` with Z up. `render` copies this so that its iso view
  matches what the user sees on opening the file. If the app changes its default view, update `View::Iso` in
  `crates/engine/src/render.rs`.
- **Evidence:** source: `crates/qymcad-ui-state/src/lib.rs` `Cam3::default` (~2384) and `Cam3::basis` (~2389); test:
  `crates/engine/src/render.rs` `view_bases_are_orthonormal` (iso camera at +X −Y +Z).
## F-3B-1 A reopened document has no edges (and, without the app's restore, no faces) until bodies rebuild

- **Version:** v0.1.0-dev.20261001
- **What:** `regen_faces` / `regen_edges` are derived and not saved. QymCAD.app's `finish_project_load` puts the
  faces stored in `Body.faces` back into `regen_faces`, but nothing restores `regen_edges`: they are copied only
  in the post pass of a regenerate, for the bodies rebuilt in that pass. A headless `load_project_with_brep` has
  neither. Everything that reads topology (face/edge selections, sketches on faces) sees an empty body.
- **Evidence:**
  - observed: `topology` right after `Session::open` found no faces for the current body (probe, 2026-10-04).
  - source: `crates/qymcad/src/gui/io_jobs.rs` `finish_project_load` (~131-137, faces restored, edges not);
    `crates/qymcad-core/src/model/regen.rs` (~1265-1276, edges copied for `report.built` only).
  - test: `crates/engine/tests/golden_features.rs` `topology_is_available_after_open`,
    `hole_diameter_follows_its_parameter` (reopen, then edit).
- **How we handle it:** `Session::open` restores `regen_faces` from `Body.faces` exactly like the app
  (`session::restore_faces`; `tests/common::gui_edit_param` does the same so the GUI-path tests stay faithful).
  It also restores `regen_edges` from the live B-reps through `Kernel::edges`, the call the regenerate post pass
  makes (`Session::restore_edges`), so nothing is rebuilt to get edges. `topology`, `select` and every 3B feature
  call `Session::ensure_topology`, which does the same and rebuilds everything once only when a current body has
  no faces (a file saved without them). If that rebuild fails, the document and the shapes (kept as B-rep bytes)
  are put back and the errors returned (test `a_failing_topology_rebuild_is_reported_and_changes_nothing`; before,
  a failed fillet was silently passed through and the body changed).

## F-3B-2 A stored edge *query* rounds every edge after the document is reopened

- **Version:** v0.1.0-dev.20261001
- **What:** a fillet/chamfer whose edges are a descriptive `refs::Ref` (`add_fillet_ref`) resolves it against
  `regen_edges` of its source. When the source was not rebuilt earlier (a reopened document, F-3B-1, then an edit
  of the fillet's own parameter), the pool is empty, the query yields nothing, and for a query without explicit
  descriptors (`Adjacent(OfFeature ..)`, `Oriented`, `Extreme` ...) the empty list reaches the kernel as "every
  edge" (F-3B-3). The node stays green. A full rebuild right after opening does the same, because edges are only
  copied in the post pass. Pick lists (`add_fillet(ids)`) are resolved against the kernel's live edges instead
  (`live_edge_refs`) and are unaffected.
- **Evidence:**
  - test: `golden_features.rs` `stored_edge_query_rounds_everything_after_reopen_upstream_bug`: a cylinder with a
    rim fillet stored as `Adjacent(OfFeature(cap end))`, reopened through the GUI path, radius 2 -> 4: both rims
    rounded (removed 2 x 412.2 mm³). The test asserts the bug; when it fails, QymCAD fixed it.
  - observed: a block with an `Oriented`-query fillet on the 4 vertical edges, reopened and fully rebuilt, had 26
    faces (all 12 edges rounded) instead of 10.
  - control: `golden_features.rs` `stored_edge_query_is_safe_to_inspect_and_edit_after_open` — the same stored
    query reopened in this server (edges restored from the B-reps, `Session::restore_edges`) rounds one rim only,
    on inspection and after editing the radius.
  - source: `regen.rs` `prep_fillet` (~1317-1380), `live_fillet_edges` (~2614-2632).
- **How we handle it:** `fillet` and `chamfer` resolve the agent's selection (ids or description) at creation
  and store a pick list of persistent edge names. Those survive upstream edits that keep the faces' recipes
  (test `descriptive_fillet_survives_an_upstream_edit`, through the server and the GUI path) and QymCAD warns
  `EdgesDropped` when some vanish. Face selections (hole, shell, push face) are stored as queries: the app restores
  faces on open, so they keep working (test `hole_diameter_follows_its_parameter`). Edge queries that *grow* with
  the topology are therefore not available until this is fixed upstream. Documents that already store edge queries
  (made in the app) are safe in this server: the edges restored on open (F-3B-1) give the query its real pool, so
  inspecting them changes nothing and a radius edit rounds the intended edges (test
  `stored_edge_query_is_safe_to_inspect_and_edit_after_open`; before the fix, `topology` alone removed 105.5 mm³).
  The app itself still shows the bug.

## F-3B-3 An empty edge list means "every edge"

- **Version:** v0.1.0-dev.20261001
- **What:** fillet/chamfer with an empty edge list round/bevel every SHARP edge of the body (smooth, tangent edges
  are skipped). A pick list that lost all its edges is refused (`EdgesNotFound`), but a descriptive query that
  resolves to nothing is passed on as empty.
- **Evidence:** source: `regen.rs` `prep_fillet` / `prep_chamfer` (`asked_edges` is computed from
  `picked_descs()`, empty for descriptive queries); `crates/qymcad-kernel/src/kernel.rs` fillet (~657-660: an empty
  list becomes `sharp_edge_ids()`, ~678 `fillet_all`) and chamfer (~800-806 `chamfer_all`); consequence observed
  in F-3B-2.
- **How we handle it:** a selection that resolves to no edge is refused at creation (test
  `stale_and_foreign_ids_are_clear_errors`); edges are stored as pick lists (F-3B-2).

## F-3B-4 Revolve: axis and direction

- **Version:** v0.1.0-dev.20261001
- **What:** `RevolveAxis { axis: 0 = sketch x, 1 = sketch y; datum; line }`; a sketch line wins over a datum axis,
  which wins over x/y. `Reach::Forward` sweeps `[0, angle]` by the right-hand rule about the axis direction (a
  profile at +X on XY revolved 180° about sketch y lands at z <= 0); `Backward` starts at `-angle`; `BothWays` at
  `-angle/2`. With a body `src`, `add_revolve_multi_op` joins (1), cuts (0) or intersects (2) in one node.
- **Evidence:** test: `golden_features.rs` `revolve_direction` (bbox z per direction), `revolve_tube_angles_and_axes`,
  `revolve_cut_groove`; source: `regen.rs` `prep_revolve` (~2505-2560), `revolve_axis_local` (~2463).
- **How we handle it:** world axes and `{origin, dir}` become manual datum axes; world Z needs one too for a
  revolve (datum 0 means "none").

## F-3B-5 Arrays copy the whole body; circular step is 360/count or angle/count

- **Version:** v0.1.0-dev.20261001
- **What:** linear/circular arrays and mirror take a source *body* and produce one body holding all copies (a
  mirror with `keep` fuses both halves). Circular step: `360/count` when `|angle| >= 359.9`, otherwise
  `angle/count` (3 copies over 90° stand at 0/30/60°). Counts are feature dimensions (`count`, `count2`), so a
  parameter can drive them; steps are `dx dy dz dx2 dy2 dz2`.
- **Evidence:** test: `golden_features.rs` `circular_arrays_of_a_boss`, `linear_arrays_of_a_boss` (count from a
  parameter), `mirror_keeps_or_replaces`; source: `regen.rs` `prep_circulararray` (~2266-2290),
  `prep_lineararray` (~2229).

## F-3B-6 Hole tool details

- **Version:** v0.1.0-dev.20261001
- **What:** a flat-bottomed cylinder along −normal from `at` projected onto the face; `depth` includes the
  counterbore/countersink; a countersink is a cone from `dia2` at the face to `diameter` over `depth2`. If
  `dia2 <= diameter` or `depth2 <= 0` the step is silently omitted (a plain hole). The position is stored as
  numbers (no feature dimension); there is no "through" flag.
- **Evidence:** test: `golden_features.rs` `holes_*`; source: `crates/qymcad-kernel/src/occt_io.cpp`
  `make_hole_tool` (~128-142), `regen.rs` `prep_hole` (~2064-2110).
- **How we handle it:** the engine refuses a step that would be omitted; `through` is a depth longer than the
  body's bbox diagonal at creation.

## F-3B-7 Seam edges and planar normals

- **Version:** v0.1.0-dev.20261001
- **What:** a cylindrical face's closing line is an edge whose `edge_face_pairs` entry names the same face twice;
  descriptions such as "along z" include it. Seams are not blendable: the kernel drops smooth edges from a
  fillet/chamfer and moves a seam off an edge that lies along one before blending, refusing the edge if it cannot
  (`occt_io.cpp` `bladeable` / `along_a_seam` / `off_seams_first`, ~317-348). `MeshFace.normal` of a planar face
  is outward. A fillet larger than the geometry fails with a node error ("fillet R15.00 only works edge by edge",
  pinned in the test).
- **Evidence:** test: `golden_features.rs` `holes_plain_blind_and_through` (seams, exact corner fillet next to
  them), `topology_of_a_block` (normals), `too_big_fillet_is_rolled_back_with_the_reason`.

## F-3B-8 `Largest` ranks edges by chord, not length

- **Version:** v0.1.0-dev.20261001
- **What:** `Project::edge_pool` scores an edge's "area" as |b − a|, so for `Query::Largest` a full circle scores
  0 and an arc scores its chord: on a Ø20 × 10 cylinder the 10 mm seam is the "largest" edge, not the 62.8 mm
  rims. `Largest` is evaluated against the whole pool wherever it is nested.
- **Evidence:** test: `golden_features.rs` `largest_edge_is_the_longest_by_true_length` (returned only the seam
  before the fix); source: `crates/qymcad-core/src/model.rs` `edge_pool` (~2896), `refs.rs` `Query::Largest`.
- **How we handle it:** for edge selections the engine replaces every `largest` with the ids of the longest edges
  by true length (as `topology` reports it) before resolving; edges are stored as pick lists anyway (F-3B-2).

## F-3B-9 The expression evaluator has no recursion limit

- **Version:** v0.1.0-dev.20261001
- **What:** `qymcad_core::expr::eval` is recursive descent (expr → term → power → unary → atom): each `(` costs
  five frames, each unary sign and each `^` (right-recursive) one or two, with no depth or length cap. 100 000
  `(` abort the process with a stack overflow. Parameters are evaluated by the same parser
  (`eval_parameters`), so a file carrying such an expression would crash on open too.
- **Evidence:** test: `crates/mcp/tests/protocol.rs` `a_pathological_expression_does_not_kill_the_server` (the
  server died, EOF on stdout, before the gate); source: `crates/qymcad-core/src/expr.rs` (~131-234).
- **How we handle it:** `value::check_expr` (≤ 1000 characters; ≤ 64 nested parentheses, `^`, consecutive signs)
  runs in `Num::eval`, through which every feature and sketch dimension passes before it is stored, and in
  `param_set`. Files are not checked on open (docs/SECURITY.md: open trusted files only).

## F-3B-10 A deep query ladder makes the document unsaveable

- **Version:** v0.1.0-dev.20261001
- **What:** a stored `refs::Query` is written as nested RON; past the format's recursion limit `save` fails with
  "Exceeded recursion limit, try increasing `ron::Options::recursion_limit`" (and such a file would not load).
  A left-deep `Union(Union(..))` of 150 face descriptions still saved; 300 did not. Upstream `refs.rs` warns
  about the same ladder for pick lists (`Ref::picks` uses a flat `Ids`).
- **Evidence:** observed with a probe (shell open faces = union of n `facing` descriptions: n = 150 saved and
  reopened; 300, 600, 1200 failed to save), 2026-10-05; test: `golden_features.rs`
  `a_wide_union_saves_and_reopens` (failed at save before the fix), `selection_budget`.
- **How we handle it:** unions of id lists become one flat `Ids`; other unions become balanced trees (depth
  log2 n); every stored or resolved selection is limited to 512 parts and a query depth of 48
  (`Sel::to_query`).

## F-3B-11 Arrays have no size limit upstream

- **Version:** v0.1.0-dev.20261001
- **What:** `prep_lineararray` multiplies the three counts and allocates a transform per copy; nothing caps it, so
  10000 × 10000 would try 10⁸ copies. Counts are feature dimensions, so a parameter edit can grow an array that
  was small when created. Cost measured here: 1000 copies of a Ø2 × 2 cylinder took 1.2 s (debug and release),
  linear in the count.
- **Evidence:** source: `regen.rs` `prep_lineararray` (~2229-2260); observed with a probe 2026-10-05; test:
  `golden_features.rs` `array_copies_are_bounded` (1089 and 1001 copies were built before the bound).
- **How we handle it:** at most 1000 copies per array in total (`patterns::MAX_ARRAY_COPIES`), checked at creation
  and in `param_set` before rebuilding (the edit is refused and rolled back). The QymCAD GUI is not limited.

## F-3B-12 A full rebuild does not reproduce parameter-edited bodies bit for bit

- **Version:** v0.1.0-dev.20261001
- **What:** after a parameter edit (F-017) the bodies differ slightly from a fresh build of the same recipe. The
  engine's rebuild retried a failed pass with the whole timeline dirty (F-005), which replaced every old body's
  shape with a fresh one; `atomic` then restored the document but kept those shapes. A rolled-back feature moved
  the plate of F-017 from 22559.52 to 22560 mm³.
- **Evidence:** test: `golden_features.rs` `a_rolled_back_feature_leaves_old_bodies_bit_identical` (failed with
  exactly those volumes before the fix).
- **How we handle it:** an edit's retry pass marks dirty only the nodes the edit created (`rebuild_retrying`);
  old bodies are never rebuilt by a feature call, whether it succeeds or fails. `open`, `param_set` and
  `ensure_topology` keep the full retry. Note: no current test needs the retry pass at all (all engine tests pass
  with it disabled), so F-005 should be re-verified.

## F-3B-13 There is no closed (hollow, unopened) shell

- **Version:** v0.1.0-dev.20261001
- **What:** a shell needs at least one face to remove: with none, the kernel refuses with `FacesNotFound` before
  doing anything (inward/outward), and nothing offers a closed hollow body.
- **Evidence:** observed: `shell` without open faces failed with `error-faces-not-found` (review round 1);
  source: `crates/qymcad-kernel/src/kernel.rs` `shell_named` (~844-849).
- **How we handle it:** `open_faces` is required (engine and tool); omitting it is a clear argument error.

## F-3B-14 A two-distance chamfer puts `dist` on a face QymCAD chooses

- **Version:** v0.1.0-dev.20261001
- **What:** with `ChamferMode::TwoDist` and `ref_face: 0`, which of the edge's two faces takes the first distance
  is the kernel's choice per edge: on a 40 × 30 block, the vertical edge at (+x, +y) put `dist` on the +x face;
  the other three vertical edges put it on the y face. The engine does not expose `flip` / `ref_face`, so an
  agent cannot choose the side.
- **Evidence:** observed with a probe (face areas after a 2/4 chamfer at each corner), 2026-10-05; test:
  `golden_features.rs` `chamfer_vertical_edges` pins the (+x, +y) case.

## F-3B-15 A hole has no through-all; the app's dialog caps the depth at 10000 mm

- **Version:** v0.1.0-dev.20261001
- **What:** `HoleTool` is `{kind, diameter, depth, dia2, depth2}`; there is no extent or through flag, and `prep_hole`
  reuses the stored depth (or its `depth` expression) on every rebuild. The app's hole command asks for a depth in
  0.1–10000 mm. A "through" depth computed from the stock at creation becomes a blind hole after the stock grows.
- **Evidence:** source: `crates/qymcad-core/src/model/regen.rs` `HoleTool` (~205-214), `prep_hole` (~2075);
  `crates/qymcad-part/src/lib.rs` hole command params (~3448). observed before the fix: 40 × 30 block, h 10 → 100,
  through Ø6 hole stopped at ≈ 52 mm (volume 118530.01 instead of 117172.57). test: `golden_features.rs`
  `through_hole_stays_through_when_the_stock_grows` (server and GUI path).
- **How we handle it:** `depth` omitted = 10000 mm (`modifiers::THROUGH_DEPTH`), the dialog's own maximum: through
  for anything that fits a printer, and the GUI shows an ordinary hole with that depth.
