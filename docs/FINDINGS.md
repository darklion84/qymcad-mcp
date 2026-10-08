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
  `crates/qymcad-core/src/feature.rs` (~1472-1481). Formula-derived tests in `golden_extrude.rs`:
  `intersect_keeps_only_the_overlap_and_follows_height`,
  `through_cut_spans_both_sides_of_an_interior_plane_and_follows_stock_height`,
  `symmetric_extrude_uses_half_the_height_on_each_side` check volumes/bboxes on creation, server and GUI edits;
  deliberate union/finite-cut/forward-reach mutations each fail.

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
  every rebuild, **but stored edge queries are not safe in QymCAD.app after reopening (F-024)**: this server
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
  use volume for exact size checks and face positions/topology for placement (F-017).
- **Evidence:** test: `crates/engine/tests/golden_plate.rs` `plate_volume_and_bbox`.
- **Reopened bounds:** B-rep deserialization can give tighter bounds than live in-session boolean shapes.
  `doc_info` documents this difference (U6f); exact volume and topology positions remain the size/placement
  checks. Evidence: `usability_warnings::opening_unchanged_dirty_boolean_geometry_does_not_warn` and native
  `Shape::to_brep_bytes` / `from_brep_bytes` round trips used by the open and undo paths.


## F-017 Parallel preparation can capture a datum sketch's old placement

- **Version:** v0.1.0-dev.20261001
- **What:** the former “0.001 mm deeper” diagnosis was wrong. For a literal 30 × 16 pocket sketched on an XY
  datum at `t`, changing stock thickness 6 → 10 leaves the pocket floor at z=3 instead of z=7, creating a
  sealed internal void. The datum's origin itself does update; parallel preparation captured its old sketch
  frame before the timeline walk resolved the datum. The cut's 0.001 mm entry clearance now lies inside stock:
  extra removed volume = 30 × 16 × 0.001 = 0.48 mm³. Stock bbox and regeneration errors cannot expose this.
  Sketches with unrelated dimension expressions mask the fault by being marked dirty before preparation.
- **Root cause:** `FeatureKind::inputs` omits sketch→datum and child-datum→parent-datum references;
  `node_reads_any` adds only face-body placement dependencies. `ready_from` therefore allows a dependent solid
  to prepare before datum resolution. A dirty sketch declares its sketch id as pending, blocking its consumers
  until the walk reaches that sketch. Native GUI datum creation uses exactly the same offset definition and
  `set_feat_dim(id, "dist", expr)` as the engine; moving the expression elsewhere on the plane cannot fix it.
- **Evidence:**
  - source: `crates/qymcad-part/src/lib.rs:641-659` (offset creation), `:344-352` (`store_cmd_exprs`);
    `crates/qymcad-core/src/model/sketch.rs:3819-3850` (distance evaluation), `:3356-3373` (sketch frame).
  - source: `crates/qymcad-core/src/model/regen.rs:984-1069` (parallel preparation), `:1109-1114` (datum
    resolution), `:2313-2340` (`ready_from`), `:2587-2610` (combine placement capture), `:2990-3002` (clearance);
    `crates/qymcad-core/src/feature.rs:2567-2632` (inputs), `model/regen.rs:622-629` (`node_reads_any`).
  - source: `crates/qymcad-core/src/model.rs:2328-2347` scans every feature-dimension expression for changed
    parameters; `model/regen.rs:3078-3079` validates every stored expression, including sketch-node entries.
  - test: `golden_plate.rs` direct/chained literal-pocket edits through server, saved/opened server, and native
    `common::gui_edit_param_faces`; before fix: “pocket floor z = thickness - depth: got 3, expected 7 ± 0.000001”.
    Tests check floor position, upward normal, opening area, 15 faces (no ceiling), and formula volume at 1e-3 mm³.
  - test: `golden_datum.rs` checks both caps of a standalone literal extrusion when only its datum parameter
    changes (volume stays W×L×H). Disabling datum tracking fails the literal-pocket placement regressions and
    datum-only cap-position regressions on server, GUI, and reopened paths; expression-driven pocket dimensions
    in `golden_plate::build()` mask this defect by dirtying the sketch.
- **How we handle it:** retain the GUI's native datum definitions and persist each parameterized ancestor's
  distance expression in the dependent sketch node's `feat_dims`, under reserved `datum_dist_<plane id>` keys.
  These expressions act only as scheduling dependencies, without altering sketch dimensions or geometry.
  Native GUI parameter marking dirties the sketch, supplying the missing barrier. Creation and `Session::open`
  reconcile these keys; opening legacy files adds them and rebuilds affected sketches, and saving persists them
  for GUI edits. Obsolete keys are removed after datum definition changes/reattachment. See ADR [0007](adr/0007-datum-sketch-dependencies.md).
  GUI structural edits (new/reattached sketches or replacement distance expressions) cannot refresh the guards
  in the running app: reopen/save through the server before further GUI parameter edits. This is a persisted
  engine workaround, not an upstream scheduler fix. The old 1 mm³ cut-volume tolerance is removed.

## F-018 The app bundle does not carry the release tag in Info.plist

- **Version:** v0.1.0-dev.20261001
- **What:** `CFBundleShortVersionString` and `CFBundleVersion` are both `0.1.0`. The release tag, commit and
  date are embedded in the executable as `v0.1.0-dev.20261001d2949f0a72026-10-01macosaarch64`. The server's
  startup check searches the executable for `v<x.y.z>-dev.<8 digits>`.
- **Evidence:** observed: `plutil -p ~/Applications/QymCAD.app/Contents/Info.plist`; `strings` on
  `Contents/MacOS/qymcad`.

## F-019 OpenCASCADE prints to stdout on every STEP write

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

## F-020 The app exports STEP/GLB/3MF as a component tree; `write_step` and the flat mesh writers do not

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

## F-021 Mesh quality presets do not change small holes: the kernel's angular deflection (0.3 rad) decides

- **Version:** v0.1.0-dev.20261001
- **What:** `qym_shape_tessellate` meshes with `BRepMesh_IncrementalMesh(shape, defl, false, 0.3, true)`. On the
  golden plate (Ø4.5 holes) draft, standard and high give the identical mesh (716 triangles, volume error
  1.42 mm³ = 0.011 %); only max (0.005 mm) refines it (1116 triangles, 0.56 mm³). The linear deflection matters only
  for larger radii. For printing this is harmless; do not expect `quality` to change a small part's file.
- **Evidence:** observed with `finer_quality_never_loses_accuracy` instrumented (counts and errors above);
  test: `crates/engine/tests/golden_export.rs` `finer_quality_never_loses_accuracy`; source:
  `crates/qymcad-kernel/src/occt_bridge.cpp` `doc_from_shape` (~143-152).

## F-022 The app's isometric view is `Cam3::default()`: yaw −0.7, pitch 0.6

- **Version:** v0.1.0-dev.20261001
- **What:** the default 3D camera (and the component thumbnails) look from yaw −0.7 rad, pitch 0.6 rad; `Cam3::basis`
  builds the forward vector `(−cos p·cos y, −cos p·sin y, −sin p)` with Z up. `render` copies this so that its iso view
  matches what the user sees on opening the file. If the app changes its default view, update `View::Iso` in
  `crates/engine/src/render.rs`.
- **Evidence:** source: `crates/qymcad-ui-state/src/lib.rs` `Cam3::default` (~2384) and `Cam3::basis` (~2389); test:
  `crates/engine/src/render.rs` `view_bases_are_orthonormal` (iso camera at +X −Y +Z).
## F-023 A reopened document has no edges (and, without the app's restore, no faces) until bodies rebuild

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
  a failed fillet was silently passed through and the body changed). That test also compares timeline ids/kinds.
  If `Shape::edges_info()` is nonempty but `Kernel::edges` yields no usable named edges, restoration refuses
  before falling back to a full rebuild (F-024). Evidence: source `qymcad-kernel/src/kernel.rs:386-399` filters
  zero ids or polylines with fewer than two points; test `session::tests::live_unnamed_edges_refuse_topology_restoration`
  renames a cylinder's edges to zero, proves refusal and retained shape/volume; bypassing the guard fails it.
  Opening with failed edge restoration is refused only if missing shapes or dirty nodes require a rebuild;
  the error identifies the body and the file stays byte-identical. A clean document still opens, with topology
  reporting the problem later. Evidence: `open_edges.rs`
  `open_refuses_unnamed_edges_before_a_dirty_stored_query_rebuild` (box, stored Along(z) query; dirty fillet or
  missing fillet B-rep) and `open_keeps_a_clean_document_with_unnamed_edges_usable`; bypassing the open guard fails.
  Restoration attempts all bodies even if one fails, so unrelated named queries still receive their edge pools.
  Clean-open usability does not authorize later query rebuilds: the shared `rebuild_retrying` preflight refuses
  planned fillet/chamfer queries depending directly or transitively on a live body with an empty edge pool and
  no usable named edges. Errors identify its id and name; parameter/sketch/feature rollback preserves the Project
  and original B-reps. Pick lists and unrelated features are not blocked by this preflight (F-024).
  The safety plan includes dirty sketch dependents: `regen_plan` omits sketch outputs from its dirty set
  (`qymcad-core/src/model/regen.rs:699-734`), but regenerate inserts them (`regen.rs:1159`).
  Evidence: `open_edges.rs` `clean_open_refuses_radius_edit_over_unnamed_edges_without_changing_state`,
  `clean_open_refuses_sketch_edit_over_unnamed_edges_without_changing_state`, and
  `clean_open_refuses_atomic_dirty_baseline_over_unnamed_edges_without_changing_state` compare exact serialized
  Project and every live B-rep. `clean_open_refuses_query_with_transitive_unnamed_ancestor` uses an intervening
  array with named edges. Disabling the shared preflight fails all four regressions.

## F-024 A stored edge *query* rounds every edge after the document is reopened

- **Version:** v0.1.0-dev.20261001
- **What:** a fillet/chamfer whose edges are a descriptive `refs::Ref` (`add_fillet_ref`) resolves it against
  `regen_edges` of its source. When the source was not rebuilt earlier (a reopened document, F-023, then an edit
  of the fillet's own parameter), the pool is empty, the query yields nothing, and for a query without explicit
  descriptors (`Adjacent(OfFeature ..)`, `Oriented`, `Extreme` ...) the empty list reaches the kernel as "every
  edge" (F-025). The node stays green. A full rebuild right after opening does the same, because edges are only
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
  (made in the app) are safe to edit in this server when their edges restore successfully: the edges restored on
  open (F-023) give the query its real pool, so
  inspecting them changes nothing and a radius edit rounds the intended edges (test
  `stored_edge_query_is_safe_to_inspect_and_edit_after_open`; before the fix, `topology` alone removed 105.5 mm³).
  When live edges cannot be restored, a clean document can still open, but every rebuild path refuses planned
  stored edge queries (`!edges.query.is_pick_list()`, source `qymcad-core/src/refs.rs:202-208`) that depend on that
  body. Transitive ancestry uses `Project::dependents` (`model/timeline.rs:20-51`). This also refuses a query when
  its unnamed ancestor would rebuild earlier in the same pass; obtaining names by regeneration does not bypass
  the safety check. An unrelated successful edit remains allowed. A full retry can introduce additional query
  nodes, so it is checked before dirtying them. If that retry could be unsafe, the first pass runs on independent
  B-rep copies and its original Project/handles are restored when the retry is refused.
  Evidence: `open_edges.rs` `clean_open_radius_edit_with_named_edges_rounds_only_vertical_corners` derives
  V = 20³ − 4(1 − π/4)r²·20 at r = 3; `clean_open_with_unnamed_edges_allows_an_unrelated_feature` preserves all
  B-rep bytes while adding a datum; `unsafe_full_retry_restores_the_first_pass_project_and_shapes` compares exact
  Project/B-rep bytes after a failed independent contour expands the retry. Disabling retry restoration fails
  its Project byte assertion. The app itself still shows the upstream bug; this refusal is engine-only.

## F-025 An empty edge list means "every edge"

- **Version:** v0.1.0-dev.20261001
- **What:** fillet/chamfer with an empty edge list round/bevel every SHARP edge of the body (smooth, tangent edges
  are skipped). A pick list that lost all its edges is refused (`EdgesNotFound`), but a descriptive query that
  resolves to nothing is passed on as empty.
- **Evidence:** source: `regen.rs` `prep_fillet` / `prep_chamfer` (`asked_edges` is computed from
  `picked_descs()`, empty for descriptive queries); `crates/qymcad-kernel/src/kernel.rs` fillet (~657-660: an empty
  list becomes `sharp_edge_ids()`, ~678 `fillet_all`) and chamfer (~800-806 `chamfer_all`); consequence observed
  in F-024.
- **How we handle it:** a feature's final selection that resolves to no edge is refused at creation (test
  `stale_and_foreign_ids_are_clear_errors`); edges are stored as pick lists (F-024).
  Preview selections and intermediate empty sets remain valid: `Query::Ids([])` yields no candidates and
  `Ref::many` permits zero matches. Evidence: `qymcad-core/src/refs.rs:135,453-499`; MCP `edge_corners` cube,
  composition, cylinder, and empty-final-modifier tests. Restoring the early empty-ids guard fails these tests.

## F-026 Revolve: axis and direction

- **Version:** v0.1.0-dev.20261001
- **What:** `RevolveAxis { axis: 0 = sketch x, 1 = sketch y; datum; line }`; a sketch line wins over a datum axis,
  which wins over x/y. `Reach::Forward` sweeps `[0, angle]` by the right-hand rule about the axis direction (a
  profile at +X on XY revolved 180° about sketch y lands at z <= 0); `Backward` starts at `-angle`; `BothWays` at
  `-angle/2`. With a body `src`, `add_revolve_multi_op` joins (1), cuts (0) or intersects (2) in one node.
- **Evidence:** test: `golden_features.rs` `revolve_direction` (bbox z per direction), `revolve_tube_angles_and_axes`,
  `revolve_cut_groove`; source: `regen.rs` `prep_revolve` (~2505-2560), `revolve_axis_local` (~2463).
- **How we handle it:** world axes and `{origin, dir}` become manual datum axes; world Z needs one too for a
  revolve (datum 0 means "none").

## F-027 Arrays copy the whole body; circular step is 360/count or angle/count

- **Version:** v0.1.0-dev.20261001
- **What:** linear/circular arrays and mirror take a source *body* and produce one body holding all copies (a
  mirror with `keep` fuses both halves). Circular step: `360/count` when `|angle| >= 359.9`, otherwise
  `angle/count` (3 copies over 90° stand at 0/30/60°). Counts are feature dimensions (`count`, `count2`), so a
  parameter can drive them; steps are `dx dy dz dx2 dy2 dz2`.
- **Evidence:** test: `golden_features.rs` `circular_arrays_of_a_boss`, `linear_arrays_of_a_boss` (count from a
  parameter), `mirror_keeps_or_replaces`; source: `regen.rs` `prep_circulararray` (~2266-2290),
  `prep_lineararray` (~2229).
- **Feature-pattern capability:** there is no native seed-feature or cut-tool array. `FeatureKind` stores a
  body source (`feature.rs:1187-1230`); timeline adders (`model/timeline.rs:1271-1291`) and preparers
  (`model/regen.rs:2230-2290`) pattern that whole body. `qymcad-kernel/src/kernel.rs:1006-1041` clones/unites
  complete Shapes. Native `add_hole_from_sketch` (`timeline.rs:1345-1359`, `regen.rs:2081-2093`, kernel
  `kernel.rs:997-1005`) drills isolated points but has no seed/count/spacing expressions.
  Supported workaround: expression-positioned sketch circles plus one through cut; circle cardinality is
  not parameter-driven. General feature-pattern implementation stopped; options in `tasks/review-u7.md`.


## F-028 Hole tool details

- **Version:** v0.1.0-dev.20261001
- **What:** a flat-bottomed cylinder along −normal from `at` projected onto the face; `depth` includes the
  counterbore/countersink; a countersink is a cone from `dia2` at the face to `diameter` over `depth2`. If
  `dia2 <= diameter` or `depth2 <= 0` the step is silently omitted (a plain hole). The position is stored as
  numbers (no feature dimension); there is no "through" flag.
- **Evidence:** test: `golden_features.rs` `holes_*`; source: `crates/qymcad-kernel/src/occt_io.cpp`
  `make_hole_tool` (~128-142), `regen.rs` `prep_hole` (~2064-2110).
- **How we handle it:** the engine refuses a step that would be omitted; `through` stores a fixed 10000 mm
  depth (F-037). Creation is refused when the source body's bbox diagonal exceeds 10000 mm, a safe bound on
  its extent along any hole axis (`through_hole_refuses_stock_beyond_10000_mm`). An explicit depth remains
  available; later stock growth beyond the stored depth can make a hole blind.

## F-029 Seam edges and planar normals

- **Version:** v0.1.0-dev.20261001
- **What:** a cylindrical face's closing line is an edge whose `edge_face_pairs` entry names the same face twice;
  descriptions such as "along z" include it. Seams are not blendable: the kernel drops smooth edges from a
  fillet/chamfer and moves a seam off an edge that lies along one before blending, refusing the edge if it cannot
  (`occt_io.cpp` `bladeable` / `along_a_seam` / `off_seams_first`, ~317-348). `MeshFace.normal` of a planar face
  is outward. A fillet larger than the geometry fails with a node error ("fillet R15.00 only works edge by edge",
  pinned in the test).
- **Evidence:** test: `golden_features.rs` `holes_plain_blind_and_through` (seams, exact corner fillet next to
  them), `topology_of_a_block` (normals), `too_big_fillet_is_rolled_back_with_the_reason`.

## F-030 `Largest` ranks edges by chord, not length

- **Version:** v0.1.0-dev.20261001
- **What:** `Project::edge_pool` scores an edge's "area" as |b − a|, so for `Query::Largest` a full circle scores
  0 and an arc scores its chord: on a Ø20 × 10 cylinder the 10 mm seam is the "largest" edge, not the 62.8 mm
  rims. `Largest` is evaluated against the whole pool wherever it is nested.
- **Evidence:** test: `golden_features.rs` `largest_edge_is_the_longest_by_true_length` (returned only the seam
  before the fix); source: `crates/qymcad-core/src/model.rs` `edge_pool` (~2896), `refs.rs` `Query::Largest`.
- **How we handle it:** for edge selections the engine replaces every `largest` with the ids of the longest edges
  by true length (as `topology` reports it) before resolving; edges are stored as pick lists anyway (F-024).

## F-031 The expression evaluator has no recursion limit

- **Version:** v0.1.0-dev.20261001
- **What:** `qymcad_core::expr::eval` is recursive descent (expr → term → power → unary → atom): each `(` costs
  five frames, each unary sign and each `^` (right-recursive) one or two, with no depth or length cap. 100 000
  `(` abort the process with a stack overflow. Parameters are evaluated by the same parser
  (`eval_parameters`), so a file carrying such an expression would crash on open too.
- **Evidence:** test: `crates/mcp/tests/protocol.rs` `a_pathological_expression_does_not_kill_the_server` (the
  server died, EOF on stdout, before the gate); source: `crates/qymcad-core/src/expr.rs` (~131-234).
- **How we handle it:** `value::check_expr` (≤ 1000 characters; ≤ 64 nested parentheses, `^`, consecutive signs)
  runs in `Num::eval`, through which every feature and sketch dimension passes before it is stored, and in
  `param_set`. Generated formulas also pass `check_expr` before storage: negative-coordinate magnitude,
  slot half-width, and directed ArcLength with turn subtraction. Valid input can exceed the budget after
  composition; `expression_limits.rs` covers length and depth at each site, with unchanged sketches on refusal.
  Removing the generated checks fails all six regressions. Files are not checked on open
  (docs/SECURITY.md: open trusted files only).

## F-032 A deep query ladder makes the document unsaveable

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

## F-033 Arrays have no size limit upstream

- **Version:** v0.1.0-dev.20261001
- **What:** `prep_lineararray` multiplies the three counts and allocates a transform per copy; nothing caps it, so
  10000 × 10000 would try 10⁸ copies. Counts are feature dimensions, so a parameter edit can grow an array that
  was small when created. Cost measured here: 1000 copies of a Ø2 × 2 cylinder took 1.2 s (debug and release),
  linear in the count.
- **Evidence:** source: `regen.rs` `prep_lineararray` (~2229-2260); observed with a probe 2026-10-05; test:
  `golden_features.rs` `array_copies_are_bounded` (1089 and 1001 copies were built before the bound).
- **How we handle it:** at most 1000 copies per array in total (`patterns::MAX_ARRAY_COPIES`), checked at creation
  and in `param_set` before rebuilding (the edit is refused and rolled back). Each edited count, like creation,
  must be a whole number in 1..=1000; rounding/flooring invalid expressions is refused. Evidence:
  `golden_features.rs` `edited_array_counts_refuse_negative_numbers` and `edited_array_counts_refuse_fractional_numbers`
  cover linear/circular edits to -5 and 2.5, exact document rollback and V=2*pi*1²*2. Restoring the old coercion
  fails both tests. The QymCAD GUI is not limited and retains its upstream rounding behavior.

## F-034 Rollback must preserve the original project and B-rep representation

- **Version:** v0.1.0-dev.20261001
- **What:** the engine once retried a failed pass with the whole timeline dirty (F-005), replacing old shapes;
  `atomic` then restored the document but kept the retried shapes. The historical 22559.52 → 22560 mm³ change
  repaired a wrongly placed sealed pocket (F-017), rather than demonstrating normal cut-depth drift. Datum
  dependency tracking now fixes that geometry, and the rollback fixtures assert the correct open pocket first.
  A refused edit must still preserve exact committed project/B-rep state, including pending dirty flags.
- **Evidence:** test: `golden_features.rs` `a_rolled_back_feature_leaves_old_bodies_bit_identical` (failed with
  exactly those volumes before the fix; now also compares serialized B-rep bytes of the old result bodies,
  with no digest dependency); `rollback.rs` `a_failed_sketch_edit_restores_old_shapes_bit_identically`
  reproduced 22559.519999999993 → 22560 during sketch rollback. Pending engine-level sketch edits can also make
  an atomic feature rebuild old shapes before restoring an older project mesh: test
  `a_failed_feature_uses_a_clean_baseline_for_pending_sketch_edits` (20³ block minus Ø4 through circle: 8000−80π).
  `a_feature_refuses_a_failing_dirty_baseline_before_editing` checks refusal before adding a datum.
  `an_unsolved_parameter_edit_restores_project_and_breps_exactly` and
  `a_failed_parameter_rebuild_restores_project_and_breps_exactly` compare exact project serialization and every
  live body B-rep on the pocket fixture after t 6 → 10; the unsolved path also preserves a pending dirty sketch.
  Source: `crates/qymcad-core/src/model/regen.rs:699-734`, `regen_plan` follows dirty body/datum inputs through
  the timeline, but omits dirty sketch outputs; the edge-query safety preflight adds their dependents (F-023).
- **How we handle it:** `atomic` first rebuilds pending dirty nodes and refuses the new edit if that baseline
  has errors; only then is the project snapshotted. Feature calls remain refused while the dirty baseline cannot
  rebuild. Parameter and sketch edits bypass `atomic`, so they can repair that baseline. Its retry marks only
  newly created nodes dirty. `sketch_edit`
  marks the sketch dirty in a project copy to plan affected bodies, including already dirty nodes, before the
  edit. It retains those original live shape handles and rebuilds on independent B-rep copies; its retry is
  limited to the planned nodes. Failure restores the project and the original shapes without another rebuild,
  preserving exact volume bits. `param_set` restores the original project directly if a sketch fails to solve,
  since propagation has not rebuilt shapes. Otherwise it snapshots the planned bodies before regeneration,
  rebuilds on independent B-rep copies, retries only planned nodes, and restores the project and original handles
  on failure. Untouched shapes remain live. `open` and `ensure_topology` keep the full retry.

## F-035 There is no closed (hollow, unopened) shell

- **Version:** v0.1.0-dev.20261001
- **What:** a shell needs at least one face to remove: with none, the kernel refuses with `FacesNotFound` before
  doing anything (inward/outward), and nothing offers a closed hollow body.
- **Evidence:** observed: `shell` without open faces failed with `error-faces-not-found` (review round 1);
  source: `crates/qymcad-kernel/src/kernel.rs` `shell_named` (~844-849).
- **How we handle it:** `open_faces` is required (engine and tool); omitting it is a clear argument error.

## F-036 A two-distance chamfer puts `dist` on a face QymCAD chooses

- **Version:** v0.1.0-dev.20261001
- **What:** with `ChamferMode::TwoDist` and `ref_face: 0`, which of the edge's two faces takes the first distance
  is the kernel's choice per edge: on a 40 × 30 block, the vertical edge at (+x, +y) put `dist` on the +x face;
  the other three vertical edges put it on the y face. The engine does not expose `flip` / `ref_face`, so an
  agent cannot choose the side.
- **Evidence:** observed with a probe (face areas after a 2/4 chamfer at each corner), 2026-10-05; test:
  `golden_features.rs` `chamfer_vertical_edges` pins the (+x, +y) case.

## F-037 A hole has no through-all; the app's dialog caps the depth at 10000 mm

- **Version:** v0.1.0-dev.20261001
- **What:** `HoleTool` is `{kind, diameter, depth, dia2, depth2}`; there is no extent or through flag, and `prep_hole`
  reuses the stored depth (or its `depth` expression) on every rebuild. The app's hole command asks for a depth in
  0.1–10000 mm. A "through" depth computed from the stock at creation becomes a blind hole after the stock grows.
- **Evidence:** source: `crates/qymcad-core/src/model/regen.rs:205-214` `HoleTool`, `regen.rs:2068-2081` `prep_hole`;
  `crates/qymcad-part/src/lib.rs:3448` hole command depth cap. observed before the fix: 40 × 30 block, h 10 → 100,
  through Ø6 hole stopped at ≈ 52 mm (volume 118530.01 instead of 117172.57). test: `golden_features.rs`
  `through_hole_stays_through_when_the_stock_grows` (server and GUI path);
  `through_hole_refuses_stock_beyond_10000_mm` (40 × 30 × 20000 stock previously accepted a blind “through” hole).
- **How we handle it:** `depth` omitted = 10000 mm (`modifiers::THROUGH_DEPTH`), the dialog's own maximum. Creation
  refuses a source bbox diagonal > 10000 mm, which bounds the extent along any hole axis. The GUI shows an ordinary
  fixed-depth hole. Later growth beyond that depth can make it blind; this is not an unbounded through-all feature.

## F-038 An outward shell rounds bottom corners with the offset thickness as radius

- **Version:** v0.1.0-dev.20261001
- **What:** an outward shell of a rectangular block with its top open creates four spherical bottom corner
  octants of radius equal to wall thickness t: every point of the offset sphere is distance t from its original
  vertex. Its eight cylindrical edge rounds also have radius t.
- **Evidence:** test: `golden_features.rs` `shell_open_top` checks all four sphere radii against t (1e-6 mm
  tolerance); observed mutation: reporting sphere radii as 2r fails the new assertion.

## F-039 The constants `pi`, `tau`, `e` shadow parameters of the same name

- **Version:** v0.1.0-dev.20261001
- **What:** the expression evaluator resolves a bare name as a constant first and only then as a variable, so a
  parameter named `e` (or `pi`, `tau`) is never read: a dimension `e` evaluates to 2.718..., silently.
- **Evidence:**
  - observed: a hexagon dimensioned by a vertex at `["e", 0]` with parameter `e = 8` came out with circumradius
    2.718 (area 19.2 instead of 166.3).
  - source: `crates/qymcad-core/src/expr.rs` (~224: `constant(&name).or_else(|| self.vars.get(&name))`, ~256).
  - test: `crates/engine/tests/sketch_behaviour.rs` `constant_names_are_not_parameters`.
- **How we handle it:** `param_set` refuses these names (case-insensitive).

## F-040 One sketch solve stops short when an angle dimension's arm must change length

- **Version:** v0.1.0-dev.20261001
- **What:** the solver holds the arms of every `Angle`/`AngleLines` softly at their pre-solve lengths
  (`w_len = 0.1`, arms already set by a `Distance` excepted). An edit that needs an arm to change length ends one
  `solve_sketch` (120 iterations) at a compromise; each further call gains only a fraction. Arcs whose ends are held
  by angle dimensions therefore lag a radius change, and a triangle given a new angle converges at ~0.85× per call.
  The GUI solves once per parameter edit, so the part comes out slightly wrong there.
- **Evidence:**
  - observed: sector arc (radius + two `AngleLines`), radius 20 → 25: residuals 0.119, 3.5e-3, 2.9e-6, 2.5e-9 on four
    successive calls; the same edit through the GUI path gave r = 24.996 (−0.5 mm³ on 1472.6). A circle (radius
    only) converges in one call. Replacing the angles by coordinate dimensions: residual 8e-16 after one call.
  - source: `crates/qymcad-core/src/solver.rs` (~231-275, `angle_arms`).
  - test: `crates/engine/tests/golden_sketch.rs` `sector_gui_radius_edit_rebuilds`,
    `hexagon_gui_radius_edit_with_rotation`, `hexagon_parametric_radius_literal_angle_gui`; `sketch_behaviour.rs`
    `a_dimension_moves_free_geometry`.
- **How we handle it:** the engine re-solves until the residual stops dropping (`solve_settled`, ≤ 200 calls).
  When the radius or the angle of an arc end or a polygon rotation is parametric, the direction is an `ArcLength`
  from a reference point instead of an angle dimension (F-045): no arms, so a GUI edit settles in one solve.
  Plain-number arcs/rotations keep radius + angle dimensions (friendlier to edit by hand; only a hand edit in the
  app can change them). Angle dimensions added with `sketch_constrain` and driven by a parameter may still land
  slightly off after a GUI edit — not measured in the app.

## F-041 A tangency at its own contact point is invisible to the rank analysis (QymCAD's slot reports 4 + 4)

- **Version:** v0.1.0-dev.20261001
- **What:** `Tangent{a, b, c}` where `a` or `b` already lies on the circle (an arc endpoint) is a second-order
  condition: its Jacobian row is parallel to the arc's intrinsic `PointOnCircle`. `add_slot_entity` uses four such
  tangencies, so a slot with its centres and radius dimensioned still reports `sketch_dof` = (4, 4). The GUI hides
  the redundancy markers for sketches with tangencies but shows the dof count from `sketch_dof`. Adding such a
  tangent by rank (as `sketch_constrain` does) would be refused as redundant.
- **Evidence:**
  - observed: fully dimensioned slot → (4, 4).
  - source: `crates/qymcad/src/gui/redundant_flag.rs` (module doc and tests), `crates/qymcad-ui-state/src/lib.rs`
    `flagged_redundant` (~8972).
  - test: `golden_sketch.rs` `slot_is_fully_defined_and_extrudes`; `sketch_behaviour.rs`
    `a_line_ending_on_an_arc_can_be_made_tangent`.
- **How we handle it:** the slot's tangencies are rewritten as `Perpendicular` between the side and the radius to
  its contact point (same geometry, first order); `sketch_constrain` tangent on a line that ends on the arc does the
  same. The GUI then shows perpendicular glyphs instead of tangent ones.

## F-042 Entity adders and the points they share

- **Version:** v0.1.0-dev.20261001
- **What:** `add_line_entity` (and rect/arc/slot/polygon vertices) reuse any existing non-system point within
  1e-6 (`sketch_point_at`), so a polyline end and an arc end at the same coordinates become one point; the frame
  (origin, anchor, axis guides) is never adopted. A circle/arc/polygon/slot centre (`radius_center_at`) also reuses
  an ordinary point there — a circle centred on a polyline vertex takes the vertex as its centre — and gets a node
  of its own only when the point found is already some curve's centre (one radius variable per centre). An entity
  may therefore share points that earlier geometry already dimensions. `add_arc_entity` and `add_slot_entity` return `()` (new entities are found by diffing
  `entities`; the slot pushes side, arc around c2, side, arc around c1). `add_polygon_param` adds a construction
  circle, `PointOnCircle` per vertex, n−1 `Equal` and a radius `Diameter` without expression: 3 dof left (centre,
  rotation).
- **Evidence:** source: `crates/qymcad-core/src/model/sketch.rs` (~3043-3105, `radius_center_at` ~3095, 3141-3265);
  test: `golden_sketch.rs` `three_lines_close_a_triangle`, `arc_joins_a_polyline_into_a_tombstone`;
  `sketch_behaviour.rs` `circles_on_a_vertex_share_or_get_their_centre`; `golden_shared_rect.rs`
  `rectangle_on_previously_pinned_corners_follows_its_parameters` and `_gui_parameters` share two earlier pinned
  corners, verify DOF (0,0), and check V=w*h*t and the bbox after server/GUI width edits. Bypassing independent
  rectangle size dimensions fails with DOF (0,2).
- **How we handle it:** every entity, rect and circle included, adds its dimensions only when independent
  (`add_constraint_if_independent`, F-049), so a shared point is not dimensioned twice.

## F-043 Signs and units of dimension values

- **Version:** v0.1.0-dev.20261001
- **What:** `Angle`/`AngleLines` are unsigned, 0..180 (`atan2(|cross|, dot)`): the side comes from the geometry.
  `DistancePL.d` is signed (the side); `eval_parameters` writes `±|expr|` keeping the stored sign. A `Diameter`'s
  expression value goes to `d` unchanged — a diameter when `diam`, a radius otherwise.
- **Evidence:** source: `crates/qymcad-core/src/solver.rs` (~1093-1128, 1166-1180), `crates/qymcad-core/src/model.rs`
  `eval_parameters` (~2412-2431). test: `golden_sketch.rs` `hexagon_rotation_follows_an_expression`,
  `sketch_entities_and_constraints_end_to_end` (protocol, point-to-axis distance).
- **How we handle it:** angle dimensions are used only for plain numbers (folded into 0..180, the side from the
  geometry; 0/180 become horizontal and ±90 vertical constraints); parametric directions use `ArcLength` (F-045).
  Point-line distances take their sign from the current geometry. (An earlier `angle_expr` that rewrote angle
  expressions was never reached and has been removed.)

## F-044 A sketch edit does not rebuild the features built from it

- **Version:** v0.1.0-dev.20261001
- **What:** `mark_sketch_dirty` marks only the sketch node; the bodies change at the next regenerate.
  `dependents_of(sketch id)` lists the features that read the sketch.
- **Evidence:** source: `crates/qymcad-core/src/model/timeline.rs` (~1384, 1514); test: `sketch_behaviour.rs`
  `editing_a_sketch_rebuilds_its_features`.
- **How we handle it:** `Session::sketch_edit` rebuilds when the sketch has dependents and rolls the edit back if a
  feature that built before now fails; the sketch tools use it. Rollback restores original live shapes rather
  than rebuilding the old recipe (F-034; test: `a_failed_sketch_edit_restores_old_shapes_bit_identically`).

## F-045 Dimensions keep their side: a coordinate expression cannot change sign; arc length is directed

- **Version:** v0.1.0-dev.20261001
- **What:** an axis `Distance` measures `|Δ|` (residual `|Δ| − d`), and `eval_parameters` writes `d = expr` as is, so
  an expression that turns negative can never be met. `DistancePL` is re-signed as `±|expr|` with the stored sign,
  so it keeps its side whatever the expression does. The solver's side flip (mirror a point and re-solve) fires only
  for a violated axis dimension, never for a negative `d`. So a point dimensioned `t-20` cannot cross the axis when
  `t` goes 10 → 30, and `r*cos(rot)` breaks when `rot` crosses 90°, in the app as well. The exception is
  `ArcLength`: `R·θ` with θ the counter-clockwise sweep from `a` to `b` in [0, 2π), so it gives a direction over a
  whole turn.
- **Evidence:**
  - source: `crates/qymcad-core/src/solver.rs` (~988-1013 axis residual, 41-114 side flip, 1156-1165 arc length),
    `crates/qymcad-core/src/model.rs` `eval_parameters` (~2416 Distance, ~2425 DistancePL, ~2436 ArcLength).
  - observed before the fix: hexagon rot 30° → 120°: first vertex stayed at x = 4.99 (expected 0.0), GUI volume
    506.8 instead of 779.4; sector a1 120° → 240°: 300π instead of 700π on both paths; square at x = `t-20`,
    t 10 → 30: committed as a least-squares compromise centred at 0.
  - test: `golden_sketch.rs` `hexagon_rotation_crosses_90_degrees(_gui)`, `sector_end_angle_crosses_180_degrees(_gui)`,
    `a_coordinate_that_would_cross_zero_is_refused`.
- **How we handle it:** parametric directions (arc ends, polygon rotation) are `ArcLength` dimensions from a
  construction point on the +x side of the centre (`Horizontal` + `PointOnCircle`, role `angle_reference` in
  `sketch_info`), `len = (r)*(a)*pi/180`, valid while the angle stays in the turn it was created in ([0°, 360°)
  for 0..359°). Negative turns explicitly group the subtraction operand, e.g. `(rot)-(-360)`;
  `golden_negative_direction.rs` tests -30° -> -120° using C+R(cos(theta),sin(theta)) on server and GUI paths.
  Restoring the old `(rot)--360` expression fails the formatting regression (the pinned parser accepts it today).
  Horizontal + PointOnCircle also admit a -x reference. No natural flip was observed for r 10 -> 100 -> 5,
  centre (20,30) -> (200,300) -> (5,300), and rotations 30 -> 120 -> 210 -> 300 -> 30 on both paths
  (`golden_reference.rs`). Rotating a triangle and its reference by 180° does preserve every raw constraint;
  `sketch::tests::a_flipped_angle_reference_is_refused_and_rolled_back` proves this alternate branch and full
  rollback. The engine checks free ArcLength references after each settled solve and refuses a -x reference
  through the residual/rollback path. This guard is engine-only; the pinned GUI does not gain it.
  Linear coordinates still cannot cross zero: `param_set` refuses such an edit and rolls back
  (F-040 settled solve + residual check); in the app the sketch would not solve. Decision (2026-10-05): keep the
  refusal, and the server instructions tell the agent to place the origin so parametric coordinates keep their
  sign. Splitting sums into positive terms (works for some expressions only) and far anchors (zoom the GUI view
  out) were rejected. See ADR [0006](adr/0006-sign-crossing-coordinates.md).

## F-046 Deleting an entity drops spline control points and free dimension helpers

- **Version:** v0.1.0-dev.20261001
- **What:** `delete_entities` keeps only points used by entities, the frame and midpoints. Spline control points
  and angle-reference points are none of these, so removing unrelated geometry drops them and their constraints,
  including ArcLength driving a parametric arc end or polygon rotation (F-045).
- **Evidence:** source: `crates/qymcad-core/src/model/sketch.rs:685-705` `delete_entities`; engine helper creation:
  `crates/engine/src/sketch/entities.rs:493-497`. Observed: "spline control point 4 was dropped"; test:
  `sketch_behaviour.rs` `removing_an_entity_keeps_splines`, `golden_sketch.rs`
  `unrelated_removal_keeps_parametric_polygon_direction` and `_gui` (rotation 30° → 120° stayed at 30° before
  the fix; x extent √3 r instead of 2r).
- **How we handle it:** `sketch_remove_entity` protects spline controls and free helpers tied to surviving
  entity/spline points with temporary self-Midpoint markers. Points owned exclusively by the removed entity
  are not protected, so their dimensions disappear too. Existing Midpoints/frame points need no markers;
  cleanup removes only the marker ids inserted by the call. Evidence: `sketch_removal.rs`
  `removing_a_line_drops_its_endpoint_dimension_to_surviving_geometry` and
  `removing_an_entity_keeps_a_preexisting_self_midpoint`; targeted over-protection/unscoped-marker mutations fail.
  The polygon regression also verifies helpers disappear when their owner is removed.
  Removing the last direction dimension prunes its reference and leftover Horizontal/PointOnCircle supports;
  removing a support first keeps the point while ArcLength still uses it. Evidence:
  `removing_the_last_direction_dimension_prunes_its_angle_reference` checks all three deletion orders and the
  regular hexagon's one newly free rotation (DOF (1,0)); disabling cleanup fails the regression.

## F-047 Reference radius/diameter dimensions are refreshed from circles only

- **Version:** v0.1.0-dev.20261001
- **What:** `update_driven_dims` takes radii from `Circle` entities; a reference radius on an arc keeps its creation
  value (20 after the arc's radius became 25).
- **Evidence:** source: `crates/qymcad-core/src/model/sketch.rs` `update_driven_dims` (~2641-2650, 2681-2685);
  test: `sketch_behaviour.rs` `a_reference_radius_on_an_arc_does_not_go_stale`.
- **How we handle it:** `sketch_constrain` refuses reference radius/diameter dimensions on arcs.

## F-048 Reference (driven) dimensions take no part in solving or counting

- **Version:** v0.1.0-dev.20261001
- **What:** `is_driven()` constraints are filtered out before `solver::dof` (`sketch_dof`), before the solve
  (`solve_sketch_inner`) and in `add_constraint_if_independent`; their values are rewritten from the geometry after
  each solve (`update_driven_dims`). Adding one never changes dof or redundancy.
- **Evidence:** source: `crates/qymcad-core/src/model.rs` `is_driven` (~1113), `crates/qymcad-core/src/model/sketch.rs`
  `sketch_dof` (~292-298), `add_constraint_if_independent` (~355-371), `solve_sketch_inner` (~2506-2515); test:
  `sketch_behaviour.rs` `a_conflicting_dimension_is_refused_and_rolled_back` (reference part: dof stays (0, 0)).
- **How we rely on it:** `sketch_constrain` skips the over-constraint check for reference dimensions.

## F-049 Independence is the rank of a numeric Jacobian at the current geometry

- **Version:** v0.1.0-dev.20261001
- **What:** `add_constraint_if_independent` (and `sketch_dof`) compute the rank of a forward-difference Jacobian
  (`h = 1e-6`) of all non-driven constraints plus the entity intrinsics, at the current coordinates. A constraint is
  added only if the dof drops. It is local: a constraint that holds only after a large move, or a second-order one
  (F-041), is judged by the current configuration; satisfaction is not checked.
- **Evidence:** source: `crates/qymcad-core/src/model/sketch.rs` `add_constraint_if_independent` (~355-371),
  `crates/qymcad-core/src/solver.rs` `dof` (~1217-1257); test: `golden_sketch.rs` `three_lines_close_a_triangle`,
  `sketch_behaviour.rs` `circles_on_a_vertex_share_or_get_their_centre`, `an_implied_constraint_is_not_added`.
  `a_line_distance_refuses_a_contradictory_rank_dependent_parallel` demonstrates a vertical length-10 line
  against the x axis: adding Parallel is rank-dependent although unsatisfied.
- **How we rely on it:** entity dimensions are filtered through it; `sketch_constrain` compares `sketch_dof` before
  and after (more redundancy = refused; a fully implied geometric constraint is checked for satisfaction by its own
  residual before being skipped) and still requires the solve to reach residual ≤ 1e-6. For a line-line distance,
  omit the required Parallel only when it is both rank-dependent and satisfied; otherwise contradictory
  parallelism refuses the whole constrain call and restores the sketch. Parallel's raw residual is an
  unnormalized cross product (mm², source `qymcad-core/src/solver.rs:1024-1030`); the engine compares the absolute
  cross product of unit directions (sin(theta) < 1e-9). Evidence: `sketch_parallel.rs`
  `implied_parallelism_uses_angle_tolerance_at_any_line_length` accepts a 5e-10-radian deviation for both
  1 mm and 1e6 mm lines; restoring the raw residual refuses the long line. The contradictory A1 regression
  still passes.

## F-050 Native deletion relinks consumers; its dependency graph omits some structural references

- **Version:** v0.1.0-dev.20261001
- **What:** the app deletes body-producing nodes with `delete_feature_op` / `delete_feature_with_dependents`
  and removes returned live-shape ids. A noncascade operation relinks consumers to its consumed source and
  translates persistent names; a source-less operation can leave consumers red. Sketch deletion has separate
  pool cleanup. Native `dependents` includes body/sketch inputs and face placement, but not datum ancestry,
  sketch datum hosts, revolve axes, sketch-driven holes, associative datum points/two-point axes, thicken
  join targets, or some component-copy inputs. `copy_source` identifies a component whose active body is read.
- **Evidence:** source: `qymcad-part/src/lib.rs:4099-4120`; `qymcad-core/src/model/timeline.rs:20-155,275-413`,
  `feature.rs:2547-2553,2567-2632`, `model/timeline.rs:1210-1233`, `model.rs` `PointDef`, `AxisDef`, `PlaneDef`. Tests: `usability_undo` source/cascade/axis
  cases and `history_tests` native point, sketch-hole, and rollback-bar cases; omitted references failed first.
- **How we handle it:** extend the closure, refuse dependent deletion by default, delete explicit cascades in
  reverse order with native operation/sketch cleanup and datum pool removal, preserve the surviving rollback
  prefix, and rollback failed regeneration. Original live handles plus independent B-rep copies give exact
  modelling undo with 16 snapshots (ADR [0008](adr/0008-session-delete-and-undo.md), F-034).

## F-051 Native shell counts expose sealed cavities and disconnected solids

- **Version:** v0.1.0-dev.20261001
- **What:** `Shape::shell_count` counts OCCT shells with `TopExp_Explorer(TopAbs_SHELL)`. A normal solid has
  one shell; an interior cut yields outer and inner shells, while an open pocket still has one. Disconnected
  copied solids can also produce multiple shells. `Shape::solid_count` counts OCCT solids, allowing an excess
  shell count to distinguish internal shells from disconnected solids with one shell each.
- **Evidence:** source: `qymcad-kernel/src/lib.rs:862-870`, `occt_helical.cpp:1336-1350`. Tests:
  `usability_warnings::{a_pocket_inside_stock_warns_and_names_the_feature,a_normal_open_pocket_does_not_warn}`
  derive interior V=20³−4*6*3.001 and open V=20³−4*6*3 (entry clearance F-017).
  `disjoint_array_copies_do_not_warn_about_a_void` derives V=3*2*3*4=72 with three shells/three solids;
  restoring shell-count-only detection makes it fail.
- **How we handle it:** feature-named warning when excess shells increase relative to the old/source body;
  disconnected solids stay silent. Also inspect each stored B-rep on open, including clean opens without
  regeneration (`opening_unchanged_sealed_geometry_checks_the_stored_body_for_voids` derives the same interior
  volume). Export/render repeat the current warning strings (`backlog_output_history` compares against
  `doc_info` and retains render's image). Session diagnostics survive failed edits/undo, and are not stored in `.qcad`
  (ADR [0009](adr/0009-advisory-geometry-diagnostics.md)).

## F-052 Stored B-reps can disagree with the recipe without a rebuild error

- **Version:** v0.1.0-dev.20261001
- **What:** the IO loader restores stored shapes independently of dirty feature expressions. Regeneration can
  replace them with different geometry while reporting no error. Bbox padding tolerance (F-016) must not be
  converted into a large volume allowance: a deeper pocket changes volume without changing the stock bounds.
- **Evidence:** `usability_warnings::opening_stale_stored_geometry_warns_about_the_rebuilt_body` stores a
  20*30*6 B-rep alongside dirty height 10, then opens with V=20*30*10; `opening_stale_pocket_warns_even_when_the_bbox_is_unchanged`
  changes depth 3→4 with delta V=4*6*1=24; unchanged dirty boolean rebuild stays silent.
  `opening_legacy_sealed_plate_reports_the_stored_shell_difference` reconstructs F-017's legacy preparation:
  stored V=60*40*10−30*16*3.001 versus rebuilt V=60*40*10−30*16*3, a 0.48 mm³ difference, with shells 2→1
  and faces 12→11. `opening_detects_a_face_count_difference_with_equal_volume_and_bounds` splits four cube
  walls: V=20³ and bounds unchanged, faces 4*2+2→6. `opening_stale_disjoint_array_reports_solid_counts_without_a_void_warning`
  changes three disjoint 2*3*4 prisms to two, reporting solids 3→2 and faces 18→12 without a void warning.
  Native face count is the sum of `Shape::face_kinds` (source: `qymcad-kernel/src/lib.rs:890-892`).
- **How we handle it:** compare stored and rebuilt shell/solid counts exactly and face counts when available,
  in addition to volume/bbox on open: 0.05 mm per bound from F-016 and
  `max(1e-6,1e-9*max(|Vold|,|Vnew|))` mm³ numerical volume tolerance. Warn with body/name and before/after
  metrics with four decimals, bbox only beyond its tolerance, and guidance that rebuilt geometry is now used
  and save updates the file. Include shell/solid/face differences and flag excess stored shells as a sealed
  void even when the rebuild repaired it. Unconditional bbox formatting fails the same-bbox pocket regression;
  equal metrics and counts still cannot prove identical geometry (ADR 0009).

## F-053 Signed corners, including curved junctions, require local face geometry

- **Version:** v0.1.0-dev.20261001
- **What:** `Shape::edge_face_pairs` returns the two adjacent faces; seams can repeat a face. `refs::Query`
  has no concave/convex filter. Tessellated triangle winding follows outward OCCT face orientation, swapping
  vertices for reversed faces. An incident triangle supplies the local inward face tangent.
- **Evidence:** source: `qymcad-kernel/src/lib.rs:1314-1332`, `occt_bridge.cpp:60-68,3091-3115`,
  `qymcad-core/src/refs.rs` `Query`; MCP `edge_corners::concave_and_convex_select_l_profile_corners_along_y`.
  `golden_curved_corners` checks boss/plate and both bore rims, the concave counterbore floor/wall circle
  (larger radius; the smaller-radius shoulder is convex), two-cylinder wall intersections, an elliptic oblique
  rim and cone/plane rims. Volumes derive from prisms, cylinders, disc overlap and the frustum formula.
  Its boss-base fillet adds `2π[R*r²(1−π/4)+r³(5/6−π/4)]` by Pappus; independent r/d/h/t edits follow through
  the reopened server and native GUI paths. Source: `face_cylinder` in `qymcad-kernel/src/lib.rs:1243`,
  edge analytic centre/axis/radius in `qymcad-core/src/geom.rs` `MeshEdge`, native smooth flags in
  `qymcad-kernel/src/lib.rs:953-994`. Removing curved classification fails all seven curved regressions.
- **How we handle it:** for two distinct adjacent faces, dot face A's local inward tangent with face B's
  outward normal: positive = concave, negative = convex. Local triangles handle concave caps unlike a global
  face centroid. Match shared triangle sides nearest the edge midpoint by coordinates; winding orients the
  tangent. Analytic radial cylinder normals keep the outward orientation (reversed on bores); analytic circle
  tangents refine chords. Other surfaces/edges use local tessellation. Exclude seams, native smooth/G1
  junctions, near-parallel normals and unavailable/degenerate shared triangles; lower composable filters to persistent ids
  (ADR [0010](adr/0010-planar-edge-corners.md), F-024).
  Empty corner previews carry a named-feature `between` hint; direct/nested hint regressions are in MCP
  `edge_corners`. Modifiers continue refusing empty final selections (F-025).

## F-054 Native face sketch origins are projected coordinate origins

- **Version:** v0.1.0-dev.20261001
- **What:** `PlaneFrame::world_aligned` sets origin=n*dot(point,n). Positive coordinate axes satisfy x×y=n:
  +Z X/Y, −Z Y/X, +X Y/Z, −X Z/Y, +Y Z/X, −Y X/Z. Tilted faces use normalized Z×n as x and n×x as y.
  `Project::sketch_frame` resolves native hosts and GUI origin shifts in the sketch owner's local space.
- **Evidence:** source: `qymcad-core/src/feature.rs:144-193`, `model/sketch.rs:3356-3400`,
  `model/assembly.rs:1566`; `usability_info` face-frame, server/GUI height-following and component-placement tests.
- **How we handle it:** multiply the native frame by its owner's world transform for `sketch_info`/`doc_info`;
  unresolved frames report null. Describe the origin/axes in tool docs (ADR [0011](adr/0011-report-native-sketch-frames.md)).

## F-055 A cylinder's underlying axis origin is not centred on its trimmed face

- **Version:** v0.1.0-dev.20261001
- **What:** `Shape::face_cylinder` returns an underlying surface's axis origin, unit direction and radius. A
  cylinder starting at z=25, height 10, returns origin z=25; the trimmed face's axial centroid is z=30.
- **Evidence:** source: `qymcad-kernel/src/lib.rs:1243` `face_cylinder`; test:
  `usability_info::cylinder_axis_point_is_at_the_faces_axial_centroid` failed first: got 25, expected 30 ± 1e-6.
- **How we handle it:** report axis point o+dot(c−o,d)*d at the face's axial centroid; this lies on the axis,
  not on the curved surface. Radius/direction stay native (ADR 0011).

## F-056 Native undo restoration preserves handed-out ids and immutable source bytes

- **Version:** v0.1.0-dev.20261001
- **What:** `Project::keep_ids_past` preserves max(snapshot.next_id, live.next_id). Native documentation describes
  held body picks aliasing a new node if undo restores the earlier allocator counter. `clone_without_source_data`
  retains source records with empty payloads; `take_source_data_from` moves bytes from live sources matched by id.
  Native `remove_sketch` removes the embedded source too, so restoring a deleted sketch also requires retaining
  its removed original outside the snapshots.
- **Evidence:** source: `qymcad-core/src/model.rs:4253-4285`; tests: `usability_undo::undo_never_reuses_a_removed_body_id_or_held_pick`
  and `history::snapshot_tests::snapshots_omit_source_bytes_and_restore_them_on_undo_and_failure`. Removing
  id preservation reproduces id 19 reuse; full clone retains payloads; omitting byte transfer loses the source.
  Source: `qymcad-core/src/model.rs:2520-2533`; test:
  `history::snapshot_tests::undo_restores_source_bytes_removed_by_native_sketch_deletion` first restored an
  empty source instead of 2048 bytes; it also verifies failed-call rollback, original allocation reuse,
  preservation of the enclosing boundary, and release after 1 deletion + 16 edits evicts its snapshot.
- **How we handle it:** use all three APIs in session history; preserve exact recipe/B-reps while intentionally
  keeping the id allocator monotonic (ADR 0008). Share deletion's enclosing MCP snapshot. Move removed originals
  into a session archive while history needs them, then recover/prune them without copying their byte buffers.

## F-057 Stored Project body pools can be inspected without rebuilding geometry

- **Version:** v0.1.0-dev.20261001
- **What:** `qymcad_io::load_project` returns the stored Project, including its body pool, without regenerating
  features. Native save can replace this file even if the new Project is empty.
- **Evidence:** source: `qymcad-io/src/project_file.rs:162-199`; MCP
  `usability_polish::empty_save_requires_override_to_replace_a_body_containing_file` first allowed overwriting
  after undo. It now verifies unchanged file bytes on refusal, delete/undo, explicit/default paths, override,
  new empty files, and replacement of already empty files. Disabling the guard fails the regression.
- **How we handle it:** empty result documents inspect existing targets and refuse to replace stored bodies
  unless `allow_empty=true` (ADR 0012).

## F-058 Reported face and sketch contour areas come from tessellation

- **Version:** v0.1.0-dev.20261001
- **What:** face areas sum mesh triangle areas; sketch contour areas use the shoelace sum of contour points.
  These are approximate for curved geometry, commonly about 0.1–0.2% below analytic areas, rather than native
  B-rep surface integrals. The approximation varies with tessellation and shape.
- **Evidence:** source: `qymcad-core/src/geom.rs:675-695` (`meshface_from_triangles`), `:242-252`
  (`Contour::signed_area`); engine `topology.rs` copies `MeshFace.area`, and `sketch.rs` uses `signed_area().abs()`.
  Observed: live final-test report (`tasks/qymcad-tests/final-test-report.md` in the 3d_modeling project)
  reported roughly 0.1–0.2% discrepancies on curved areas.
- **How we handle it:** `topology` and `sketch_info` descriptions state the approximation and direct agents
  to analytic dimensions for exact areas. B-rep volume remains the exact geometry check.
