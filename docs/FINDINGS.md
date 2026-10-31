# Findings about QymCAD

Facts about the QymCAD codebase that this server depends on, each with its evidence. This file is the single
source of truth: when QymCAD changes, re-verify the entries here (see [UPGRADING.md](UPGRADING.md)).

Conventions:
- **Version** is the QymCAD tag the entry was verified against.
- **Evidence** says how it was established: `test:` (a test in this repo), `observed:` (ran it and saw it),
  `source:` (read the code, path relative to the QymCAD repo root).
- An entry without evidence is a guess and must not be relied on.
- **Upgrade to v0.1.1-dev.20261026 (2026-10-24):** entries with an "On v0.1.1" line were re-checked one by one
  (ADR 0021). The others were not re-read individually; every test they name passes on v0.1.1-dev.20261026.

---

## F-001 Uppercase parameter names never trigger a rebuild in the GUI

- **Version:** v0.1.0-dev.20261001
- **On v0.1.1-dev.20261026 (2026-10-24):** **Fixed upstream** (5dcaa65): names and expressions match exactly; an uppercase parameter edited in the app rebuilds (`golden_plate::gui_param_edit_rebuilds_from_an_uppercase_name`). The server no longer lowercases: names are case-sensitive and a new case twin is refused (ADR 0021). The text below is history.
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
- **Reached-sketch policy (J3 correction to J6; since 0.1.1 names are exact, not lowercased, ADR 0021):** stabilize
  named-driver scope, then compare old/new `param_map` values, including transitive parameters and named driving dimensions. Native
  evaluation seeds named dimensions once before applying sketch expressions (`model.rs:2374,2409`,
  `model/sketch.rs:2414`); repeat at most `named_dims.len()+1` evaluations, stopping when scope is unchanged.
  Solve only sketches whose dimensional expressions mention changed values. After solving, recompute the scope: `update_driven_dims` rewrites named reference measurements
  from geometry (`model/sketch.rs:2542,2641-2673`). Newly changed names reach further sketches in the next
  round. Bound rounds to `sketches.len()+1`; refuse and restore a non-stabilizing reference cycle.
  Re-evaluation errors abort with recipe restoration; recheck array limits after reference propagation
  before rebuilding (F-033). Tests `updated_reference_revalidates_global_expressions_before_rebuild`
  and `updated_reference_revalidates_array_total_before_rebuild` cover sqrt(11-ra) becoming invalid and
  round(ra)*100 growing from 1000 to 1200 copies, both with exact rollback.
  Rebuild scheduling keeps native blanket `mark_param_dependents_dirty()` (`model.rs:2353-2357`), independent
  of solve reachability. Face-derived datum sketch barriers supply the missing placement dependency (F-017).
- **Tests:** `params::tests::parameter_edit_reaches_a_chain_of_imported_named_sketch_dimensions` checks
  w→a→b→c; the transitive/uppercase and unused-edit tests retain targeted solve counts. The named-reference
  golden uses A's diagonal sqrt(w²+3²) to size B, V=2*sqrt(w²+3²)*4*2. It is red on J6 HEAD, green on the
  pre-J6 propagation body and the native GUI path. `named_reference_chain_solves_in_rounds_independent_of_sketch_storage_order`
  checks three reference hops with reverse pool order; `cyclic_named_references_are_bounded_and_restore_the_project`
  checks exact recipe/B-rep rollback. Removing follow-up rounds fails both acyclic regressions.
- **Timing:** same shelf blend edits 3.5→4→3.5: J6 HEAD median 221.5 ms, pre-J6-style propagation 253.4 ms,
  corrected implementation 248.8 ms initially, 256.4 ms after final validation checks. Four→zero explicit
  sketch solves remains; these small samples establish no reliable total speedup vs pre-J6. Blanket
  rebuild safety sacrifices J6's earlier speedup. Ignored test: `parameter_and_project_clone_shelf_timings`; rerun it to reproduce the samples.

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
- **On v0.1.1-dev.20261026 (2026-10-24):** Still open upstream (#170, taken by a contributor 2026-10-12).
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

## F-016 Native `Shape::bbox()` is inflated by tolerances and geometric enclosures

- **Version:** v0.1.0-dev.20261001
- **What:** `bbox()` comes from `BRepBndLib::Add`, which includes tolerances. A 60 × 40 × 6 plate after hole/pocket
  cuts reports 60.0126 × 40.0126 × 6.0126 (+0.0063 mm per side). A ~0.05 mm size tolerance fits this plate;
  curved countersinks can carry about 0.26 mm padding (live k5-test-report, 2026-10-08), so the former reporting descriptions
  allowed up to ~0.3 mm rather than a universal 0.05 mm bound;
  use volume for exact size checks and face positions/topology for placement (F-017).
- **Server reporting:** superseded by F-066: fresh mesh extrema replace native padded reporting bounds.
  The observations above describe the native getter and the server before H1.
- **Evidence:** test: `crates/engine/tests/golden_plate.rs` `plate_volume_and_bbox`.
- **Reopened bounds:** B-rep deserialization can give tighter bounds than live in-session boolean shapes.
  `doc_info` documents this difference (U6f); exact volume and topology positions remain the size/placement
  checks. Evidence: `usability_warnings::opening_unchanged_dirty_boolean_geometry_does_not_warn` and native
  `Shape::to_brep_bytes` / `from_brep_bytes` round trips used by the open and undo paths.


## F-017 Parallel preparation can capture a datum sketch's old placement

- **Version:** v0.1.0-dev.20261001
- **On v0.1.1-dev.20261026 (2026-10-24):** **Still holds** for the server path: with the datum dependency and barriers disabled, 11 datum tests fail (`datum_only_param_moves_a_literal_extrusion`, `chained_datum_param_edit_moves_the_pocket`, ...); #165 (fc1dedc) fixed only the sealed-pocket case. In the 0.1.1 app a constant sketch on a face-derived datum keeps its old placement on a parameter edit, now also when an unrelated sketch reads the name (`native_gui_face_datum_parameter_edit_characterization`; the app no longer dirties every expression sketch, `qymcad/src/gui/panels_windows/params.rs:375-398`). Workaround kept.
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

### Face-derived datums (J3)

`OffsetFace` and `OffsetPlane` chains ending at `OffsetFace` also omit scheduler inputs. When F's height
changes, a constant-distance datum resolves to its moved top face, but its constant/unrelated-dimension
sketch can be skipped. Blanket dirtying rebuilds a parametric consumer, yet parallel preparation can still
capture the old frame unless the sketch is dirty. On engine parameter edits, retain blanket dirtying and
mark all face-derived datum sketches dirty as placement barriers, without explicitly solving them.
Evidence: `golden_param_propagation::parameter_edit_rebuilds_extrusion_on_face_derived_datums` constructs
native datums as in GUI-authored files, then edits through Session after reopen. F height=w, datum dist=2,
optional child dist=5, E2 height=h2=3: caps must be at w+2(+5) and w+2(+5)+3, V=4*2*3. HEAD is red;
removing blanket dirtying or the added barrier independently is red. The native GUI characterization
moves unrelated-expression sketches (it solves them all), but leaves constant sketches stale on both datum
forms. Pre-J6 propagation has the same constant-sketch limitation. This is an engine-only edit barrier;
no persisted guard or upstream GUI fix for OffsetFace dependencies is claimed.

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
  `glb_is_in_metres_with_y_up`, `threemf_is_in_millimetres`). GLB now decodes binary positions/indices from
  an asymmetric 6*10*8 box: expected bounds [0.008,0,-0.022,0.014,0.008,-0.012] m, signed V=480/1000³ m³
  and outward triangle winding. `obj_multiple_bodies_use_global_vertex_offsets_and_preserve_each_solid` checks
  each object's exclusive global vertex range, formula bounds and signed volume; reflection/winding/offset
  mutations turn these regressions red (tasks/review-c1.md).

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
- **On v0.1.1-dev.20261026 (2026-10-24):** **Fixed upstream** (d2f2184): a stored edge query reopened in the app rounds only the queried rim, analytic volume to 1e-6 (`stored_edge_query_rounds_one_rim_after_reopen_in_the_app`). The server keeps storing pick lists and keeps the empty-pool refusal for legacy documents; storing descriptions is in docs/BACKLOG.md.
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
  body. Transitive ancestry extends `Project::dependents` with sketch projections (F-107). This also refuses a query when
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
- **On v0.1.1-dev.20261026 (2026-10-24):** Seam policy unchanged; see the F-068 note on 0.1.1.
- **What:** a cylindrical face's closing line is an edge whose `edge_face_pairs` entry names the same face twice;
  descriptions such as "along z" include it. Seams are not blendable: the kernel drops smooth edges from a
  fillet/chamfer and moves a seam off an edge that lies along one before blending, refusing the edge if it cannot
  (`occt_io.cpp` `bladeable` / `along_a_seam` / `off_seams_first`, ~317-348). `MeshFace.normal` of a planar face
  is outward. A fillet larger than the geometry fails with a node error ("fillet R15.00 only works edge by edge",
  pinned in the test).
- **Evidence:** test: `golden_features.rs` `holes_plain_blind_and_through` (seams, exact corner fillet next to
  them), `topology_of_a_block` (normals), `too_big_fillet_is_rolled_back_with_the_reason`.
- **Server selection policy (J2/A1):** infer seams from equal adjacent names only on bodies without
  duplicate face names. The bridge returns names, not native `IsSame` identity (`qymcad-kernel/src/lib.rs:
  1314-1332`, `occt_bridge.cpp:3091-3115`). On F-068's repaired cone, equal adjacent names falsely
  classified five edges as seams; keep every blend pick and omit seam flags on ambiguous bodies.
  Tests: `golden_backlog_j2::duplicate_face_names_do_not_identify_seams` and
  `modifiers::hint_tests::ambiguous_body_blend_picks_are_not_dropped_as_seams`; both guard mutations fail.
  Otherwise drop seam ids before creating fillet/chamfer pick lists, emit transient
  `rebuild.notes` with the number dropped, and refuse a seam-only selection. `select` and `topology` retain
  seam edges, marked `seam: true`. Evidence: `golden_backlog_j2::{blend_selections_drop_seams_and_report_note,
  seam_only_blend_selection_is_refused_clearly}` and MCP `backlog_j2::blend_reports_seam_note_and_keeps_preview_seam`.

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
- **On v0.1.1-dev.20261026 (2026-10-24):** **Native crash fixed** (244bb1b, our #171; nesting cap 256). The server limits (64 levels, 1000 characters) stay as policy.
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
- **On v0.1.1-dev.20261026 (2026-10-24):** **Native cap added** (2f2e03c: 10 000 copies before allocation). The server cap of 1000 and strict whole counts stay as policy.
- **What:** `prep_lineararray` multiplies the three counts and allocates a transform per copy; nothing caps it, so
  10000 × 10000 would try 10⁸ copies. Counts are feature dimensions, so a parameter edit can grow an array that
  was small when created. Cost measured here: 1000 copies of a Ø2 × 2 cylinder took 1.2 s (debug and release),
  linear in the count.
- **Evidence:** source: `regen.rs` `prep_lineararray` (~2229-2260); observed with a probe 2026-10-05; test:
  `golden_features.rs` `array_copies_are_bounded` (1089 and 1001 copies were built before the bound).
- **How we handle it:** at most 1000 copies per array in total (`patterns::MAX_ARRAY_COPIES`), checked at creation
  and before rebuilding when values are known. The same centralized evaluated guard rechecks the settled native reference scope on parameter/dimension edits, standalone rebuild and open; late violations restore the exact pre-edit archive without another rebuild. Each edited count, like creation,
  must be a whole number in 1..=1000; rounding/flooring invalid expressions is refused. Evidence:
  `golden_features.rs` `edited_array_counts_refuse_negative_numbers` and `edited_array_counts_refuse_fractional_numbers`
  cover linear/circular edits to -5 and 2.5, exact document rollback and V=2*pi*1²*2. Restoring the old coercion
  fails both tests. R4 projected-reference probes initially built 1001 copies or rounded 4.5 to5 after a reference repeat. `golden_medium_r4` and `session::r4_tests` now assert refusal and immediate archive equality across all evaluated paths (`tasks/R4-report.md`). The QymCAD GUI retains its upstream 10000 cap and rounding behavior.

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
  preserving exact volume bits. Both paths now use `Session::with_rebuild_copies`; skipping its original-handle
  restore makes both exact-B-rep rollback regressions red (tasks/review-c1.md). `param_set` restores the original
  project directly if a sketch fails to solve,
  since propagation has not rebuilt shapes. Otherwise it snapshots the planned bodies before regeneration,
  rebuilds on independent B-rep copies, retries only planned nodes, and restores the project and original handles
  on failure. Untouched shapes remain live. `open` and `ensure_topology` keep the full retry.
  Shared-helper extraction deliberately strengthens the former early sketch failure by restoring advisory
  diagnostics, and the former parameter/early-sketch paths by dropping shapes absent from the restored
  recipe. The helper comment records this distinction; test:
  `session::tests::failed_rebuild_closure_restores_diagnostics_and_drops_orphan_shapes` injects both before
  a pre-regeneration closure error. Separate warning/shape retention mutations fail the regression.

- **Previously unbuilt planned bodies (J3):** rollback restores absence as well as original handles.
  Keeping only restored-recipe IDs is insufficient when a planned recipe already existed without a live
  shape: a partial rebuild can create its shape before a later node fails. Capture pre-edit shape IDs
  and remove newly present shapes on failure. Test:
  `session::tests::failed_partial_rebuild_drops_shapes_of_previously_unbuilt_planned_bodies` verifies the
  partial extrusion V=4*6*5=120, then missing-sketch failure, exact recipe restoration and absent shapes.
  Restoring the former cleanup by mutation fails the absent-shape assertion.

- **R3 snapshot ownership (observed on the pinned f25f27d):** even a plain `copy_shapes` serialize/restore round-trip can change a pocket B-rep’s bytes before any reporting or rebuild. `tasks/R3-logs/copy-roundtrip-characterization.log` records direct before/after comparisons. Atomic feature refusal retains pre-edit original handles across its entire edit/rebuild/post path. Standalone/open guarded rebuilds retain originals and return actual built-body IDs: on success, originals replace copies for every body never rebuilt, so a clean rebuild preserves all B-rep bytes. Scoped edits retain their enclosing original-handle snapshots. The existing `golden_features::a_rolled_back_feature_leaves_old_bodies_bit_identical` and `rollback::a_failed_parameter_rebuild_restores_project_and_breps_exactly` exposed these two R3 regressions; production mutations and restoration are in `tasks/R3-report.md`.

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
  `golden_features.rs` `chamfer_vertical_edges` pins the (+x, +y) case. Source:
  `crates/qymcad-kernel/src/occt_io.cpp:1680-1684,1706-1727` chooses the first native adjacent-face traversal
  entry when flip=false/ref_face=0; the API returns only a Shape, with no chosen-reference-face report.
- **Inspection:** compare adjacent face setbacks before/after chamfer; for straight rectangular faces the
  removed face area divided by the selected edge length gives its setback, as that golden test proves.
  Sorted topology adjacency ids do not expose the kernel traversal order.

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
- **On v0.1.1-dev.20261026 (2026-10-24):** **Fixed upstream**: `drivers::check_ident` rejects `pi`, `tau`, `e` (`IdentError::Constant`). Constants are exactly these lowercase names; `PI` is an ordinary parameter (`sketch_behaviour::constant_names_are_not_parameters`). The server keeps its clearer refusal message.
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
- **On v0.1.1-dev.20261026 (2026-10-24):** **Fixed upstream** (c899ea7): one solve settles angle arms. The server solves once, and once more only when the first stops between 1e-6 and QymCAD's own 1e-4 bar; with the old loop removed every test passes, and a parameter-driven angle is exact to 1e-6 in the server and in the app (`an_angle_driven_by_a_parameter_settles_in_one_solve_in_the_app`). The text below is history.
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
- **On v0.1.1-dev.20261026 (2026-10-24):** **Fixed upstream** (4767faf, our #168): slots keep QymCAD's four `Tangent` constraints and report (0, 0); a tangent on a line ending on an arc is stored as `Tangent` and removes one freedom. Both rewrites are removed; their tests assert the native kinds and were red with the old rewrites restored. The app's own rank-gated add paths (`independent_of`, `add_constraint_if_independent`) judge a new contact tangency before it is stored, so the app may still drop it (reasoned from source by the review, not run). The text below is history.
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
- **On v0.1.1-dev.20261026 (2026-10-24):** **Fixed upstream** (our #166): `delete_entities` keeps spline controls and helpers of surviving geometry. The temporary markers are removed; with them disabled all 440 tests passed before removal. That shows the markers are no longer needed; no test can be made red by restoring them. Splines in files saved by 0.1.0 are a separate loss on load (F-075). The text below is history.
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
- **On v0.1.1-dev.20261026 (2026-10-24):** **Fixed upstream** (our #167): reference radii on arcs follow the arc. The refusal is removed; `a_reference_radius_on_an_arc_does_not_go_stale` was red with the old refusal restored. The text below is history.
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
  `qymcad-kernel/src/lib.rs:953-994`. The kernel's `face_axis` (`lib.rs:1211`, `occt_io.cpp:680`) returns a
  cone's reference-axis origin and direction, not apex/angle data; there is no general surface-normal-at-point
  getter. `topology::normal_tests::cylinder_normal_corrects_a_facet_that_reverses_a_shallow_sign` supplies an
  8° facet error reversing a 4° corner sign, recovered by the exact radial normal. Its alignment-fallback test
  covers zero and near-zero radial/facet dot products. The shallow conical spotface golden checks omission
  with a formula-derived removed volume. `topology::corner_tests::native_spline_crease_with_changing_sign_is_omitted`
  builds a valid closed native Bezier solid of volume `240-10-4/15 mm³`; its curved continuous crease changes
  sign away from the midpoint. All five facet samples clear their margins and include both corner signs.
  A straight-seam prototype instead had one shared tessellated side despite changing surface normals;
  every fraction read the same facet normal, so boundary curvature is required for this fixture's tessellation.
  `golden_planar_corners` checks two convex 135° boundaries of a 45° chamfer (V=3160 mm³), a concave
  90° V-groove bottom (V=3136 mm³), and a convex 30° wedge (V=600√3 mm³), including formula bounds,
  edge positions/lengths and planar adjacency. Before correction the chamfer and wedge were omitted.
  Reverting only planar normals to mesh uncertainty fails the wedge; reverting plane and line allowances
  fails the chamfer too. `topology::corner_tests::straight_polyline_detection_uses_relative_collinearity`
  checks two-point/subdivided lines, three length scales, curved and degenerate polylines.
  `topology::normal_tests::narrow_native_cylinders_and_cones_keep_their_curved_normal_path` checks
  native 0.1° cylinder/cone sectors against the frustum-volume formula times `0.1/360`; both pass
  mesh planarity but must keep the curved path. Removing the analytic-type guard fails the test.
- **How we handle it:** for distinct adjacent faces, dot face A's local inward tangent with face B's
  outward normal: positive = concave, negative = convex. Coordinate-matched shared sides at five arc-length
  fractions (0.1,0.3,0.5,0.7,0.9) orient the tangent by outward winding. Cylinder normals are radial;
  cone normals use their axis plus the meridian slope from that face's vertices (F-059). Native circle/arc
  tangents and collinear native line directions are oriented by shared-side winding with `1e-6` allowance.
  Cylinder normals and proven native planes also receive `1e-6`; cone normals retain delta(1°), and other
  facet normals/tangents receive delta(0.3 rad), where delta(theta)=2*sin(theta/2). Mesh-only plane normals
  receive the conservative allowance in F-060. All sample signs must agree and clear the sum of allowances.
  Seams, native smooth edges, degenerate samples and ambiguous signs are not selected. Native smoothness
  includes shallow sharp edges below ~1.5° (occt_io.cpp:778-804); the omission count includes those unless
  every sample's normals agree within `1e-6`. Unknown curved G1 facets can therefore count as uncertain.
  Five samples and mesh-error allowances remain engineering checks, not a formal bound on arbitrary unsampled
  surface behavior. Empty previews state body-level requested/opposite corner counts, distinguishing corners eliminated by
  other composition conditions (MCP `backlog_review` box and cone/plane tests), adding a `between` example
  only for source ancestry with at least two solid-creating extrude/revolve features (modifiers do not count); fillet/chamfer repeat the hint on empty matches. Corner previews
  also report "omitted N uncertain edges" (MCP `backlog_selection`, `edge_corners`; ADR 0010).

## F-054 Native face sketch origins are projected coordinate origins

- **Version:** v0.1.0-dev.20261001
- **What:** `PlaneFrame::world_aligned` sets origin=n*dot(point,n). Positive coordinate axes satisfy x×y=n:
  +Z X/Y, −Z Y/X, +X Y/Z, −X Z/Y, +Y Z/X, −Y X/Z. Tilted faces use normalized Z×n as x and n×x as y.
  `Project::sketch_frame` resolves native hosts and GUI origin shifts in the sketch owner's local space.
- **Evidence:** source: `qymcad-core/src/feature.rs:144-193`, `model/sketch.rs:3356-3400`,
  `model/assembly.rs:1566`; `usability_info` face-frame, server/GUI height-following and component-placement tests.
- **How we handle it:** multiply the native frame by its owner's world transform for `sketch_info`/`doc_info`;
  unresolved frames report null. Describe the origin/axes in tool docs (ADR [0011](adr/0011-report-native-sketch-frames.md)).
- **Sketch point materialization:** the GUI creates the origin when entering sketch edit (`qymcad/src/gui.rs:
  1442-1450,1389-1401`), but frame and axis guides remain lazy (`qymcad-core/src/model/sketch.rs:2875-2892,
  2954-2993`). The server keeps its existing lazy origin/frame/guide policy: special points appear when geometry
  or constraints need them, usually after the first sketch_add. The named origin/x_axis/y_axis constraint
  aliases materialize their needed reference points (`engine/sketch/constrain.rs::geo`). Tool descriptions
  document this; no speculative eager creation of all points or change to existing id allocation.

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
  features. Native save can replace this file even if the new Project is empty. Explicit-path saves may also
  replace a different nonempty model; MCP reports pre-save target existence as `replaced`.
- **Evidence:** source: `qymcad-io/src/project_file.rs:162-199`; MCP
  `usability_polish::empty_save_requires_override_to_replace_a_body_containing_file` first allowed overwriting
  after undo. It now verifies unchanged file bytes on refusal, delete/undo, explicit/default paths, override,
  new empty files, and replacement of already empty files. Disabling the guard fails the regression.
- **How we handle it:** empty result documents inspect existing targets and refuse to replace stored bodies
  unless `allow_empty=true` (ADR 0012). Pathless saves additionally require the saved/loaded first body-producing
  node id/kind to remain in the timeline; parameter edits preserve it, undo/delete replacement can remove it.
  `save_lineage` verifies unchanged file bytes on refusal, normal/reopened edits, explicit path/overwrite override,
  initially empty files and `doc_new` requiring a path; MCP
  `backlog_review::save_reports_fresh_and_replaced_targets` checks fresh/repeated/associated/different-model
  saves, reopening the replacement against prism V=10*12*2; disabling the guard fails (ADR 0014).

## F-058 Reported face and sketch contour areas come from tessellation

- **Version:** v0.1.0-dev.20261001
- **What:** face areas sum mesh triangle areas; sketch contour areas use the shoelace sum of contour points.
  These are approximate for curved geometry, rather than native B-rep surface integrals. Curved areas can
  lie below analytic by up to ~2% on very small curved regions, including planar faces with curved
  boundaries, in measured examples; this is an observation,
  not a guaranteed bound. The approximation varies with tessellation, angular deflection and shape.
- **Evidence:** source: `qymcad-core/src/geom.rs:675-695` (`meshface_from_triangles`), `:242-252`
  (`Contour::signed_area`); engine `topology.rs` copies `MeshFace.area`, and `sketch.rs` uses `signed_area().abs()`.
  Observed: live final-test report (`tasks/qymcad-tests/final-test-report.md` in the 3d_modeling project)
  reported roughly 0.1–0.2% discrepancies on curved areas. Additional accepted H1-H4 live measurements
  (2026-10-08): Ø8 floor −0.37%, full sphere −0.38%, small sphere patches −0.72..−0.79%.
  Additional round-I live observation (2026-10-09): R=.2 sketch circle area was ~1.8% low.
  For an n-sided inscribed circle mesh, shoelace area / analytic area is `n*sin(2*pi/n)/(2*pi)`;
  face triangle areas have additional surface sampling effects, so angular deflection alone is not a
  universal relative area bound.
- **How we handle it:** `topology` and `sketch_info` descriptions state the approximation and direct agents
  to analytic dimensions for exact areas. B-rep volume remains the exact geometry check.


## F-059 A native cone's generator can be recovered from its own vertices

- **Version:** v0.1.0-dev.20261001
- **What:** `face_axis` exposes origin/direction, and vertices from the same native cone satisfy r(z)=r0+s*z
  in its meridian plane. Their widest axial span determines s=dr/dz; every vertex is checked against that line.
  The generator axis+s*radial is perpendicular to the normal radial-s*axis. Triangle winding chooses its
  outward sense, including inward bore faces. Degenerate axial spans or failed fits retain facet uncertainty.
- **Evidence:** `topology::normal_tests::native_cone_meridian_normals_match_formula_and_outward_winding`
  uses native R=10,r=5,h=5*tan(alpha) cones at 0.9/3/5/10/20/45°; V=pi*h*(R²+Rr+r²)/3 and normal
  (sin(alpha)*cos(phi),sin(alpha)*sin(phi),cos(alpha)) agree within 1e-7, including reversed winding/axis.
  Mesh coordinate rounding limits tighter slope comparisons. The analytic circle-tangent test proves
  (-sin(phi),cos(phi),0) even at chord-midpoint samples; reversing its cross product makes it fail.
  `golden_curved_corners::shallow_countersink_rims_are_convex_and_follow_server_and_gui_edits` checks
  20/10/5/3/30/45° circular rims and server/native GUI depth edits against bore+frustum overlap volume.
  Before fix: "20 degree countersink rim must be convex"; restoring full cone uncertainty fails at 10°.
  The 0.9° spotface remains omitted and reports one uncertain edge, excluding its seams.
- **How we handle it:** analytic cone normals retain a one-degree engineering allowance; native circle/arc
  tangents use a numerical allowance. Slopes ≥3° are verified; below ~1.5° native smoothness can omit rims.
  Unrefined facets retain the full 0.3-radian allowance and may need ~20–30° or more, depending on both faces
  and tangent. Expose degree thresholds and omission counts in selection/fillet descriptions (ADR 0010).

## F-060 Few curved triangles do not establish native planarity

- **Version:** v0.1.0-dev.20261001
- **What:** one triangle always passes `planar_normal`. `Project::face_sphere` is a vertex fit, not a kernel
  type getter, and requires at least 12 vertices. `Shape::face_kinds` exposes aggregate native surface counts,
  not a per-id plane getter; its seven slots are plane/cylinder/cone/sphere/torus/freeform/other.
- **Evidence:** source: `qymcad-core/src/model/assembly.rs:2330-2375`, `qymcad-kernel/src/lib.rs:888-893`,
  `occt_helical.cpp:1354-1376`. `single_triangle_native_sphere_must_not_receive_a_plane_normal` reduces
  the face mesh of a native R=2 sphere to one triangle (V=4*pi*2³/3); before fix and after guard-removal
  mutation: "native sphere must not receive a plane normal even with one triangle".
  `sparse_mesh_only_planes_keep_uncertainty_that_prevents_shallow_wrong_signs` checks decreasing allowances
  for 1/2/100 triangles and omission of a wrong 9° facet sign; forcing numerical allowance makes it red.
- **How we handle it:** reject axis-bearing faces, fitted spheres and mesh planes on bodies with no native
  planar surfaces. An axis-free face on a body with only plane/cylinder/cone types is a proven native plane.
  In mixed bodies, mesh-planar faces use delta(max(0.3/sqrt(n),acos(0.99999))), n=triangle count, instead of
  exact allowance; even dense patches retain nonzero uncertainty. Native torus/freeform identity cannot be
  assigned per face from aggregate counts, so uncertain mixed patches remain conservative rather than guessed.
  The pinned bridge has no per-face torus-type getter: `face_kinds` counts tori only in slot 4
  (`lib.rs:888-894`, `occt_helical.cpp:1350-1376`). Torus faces therefore remain `other` in topology and kind
  selections; aggregate counts cannot justify assigning an individual face kind.

## F-061 One-sided native cuts include a 0.001 mm entry seam

- **Version:** v0.1.0-dev.20261001
- **What:** native `tool_extent` extends the entry end 0.001 mm behind the sketch plane for one-sided cuts
  with zero `down`, regardless of whether the plane coincides with a stock face. Internal planes remove extra
  material; at a stock boundary the extension lies outside stock. Through-all and symmetric reach bypass it.
  The far stock boundary has separate outward clearance for breaking through.
- **Evidence:** source: `qymcad-core/src/model/regen.rs:2587-2610,2949-3006`; test:
  `golden_cut_clearance::internal_cut_entry_seam_and_top_face_cut_agree_on_server_and_gui_paths` derives
  internal V=20*30*20-4*6*(depth+.001), floor z=3-.001, and top-entry V=20*30*20-4*6*depth, floor z=20-depth;
  depth 4 then 5 agrees through server and native GUI parameter edits.
- **How we handle it:** extrude and plane_offset explain the exact-floor pattern: sketch at the stock top
  and cut in reverse, so entry clearance is outside material; preserve native recipes without compensation.
  No native disable-seam extent flag exists. F-017's dependency workaround remains independent.


## F-062 Native selections have binary intersections and no kind query

- **Version:** v0.1.0-dev.20261001
- **What:** `refs::Query::Filter` is a binary intersection; no native geometric-kind variant exists.
  Existing edge modifiers store resolved persistent ids (F-024), while face modifiers persist native queries.
- **Evidence:** source: `qymcad-core/src/refs.rs:69-115`; MCP `backlog_selection` checks 3/200-operand
  intersections, 0/1 refusal, the existing 512-part budget, taxonomy equality and acceptance of all three face
  modifiers. `golden_selection_kinds::kind_selected_fillet_survives_server_and_gui_parameter_edits` checks
  V=(20*16-4*(1-pi/4)*r²)*h and eight cap arcs of length pi*r/2 on server/native GUI edits; mutating line
  filters to select circles makes both taxonomy and geometry regressions red.
- **How we handle it:** balance N-operand MCP intersections and retain F-032's size/depth limits. Lower
  kind previews using topology's shared classifier; `curve` matches arc/other edges, excluding full circles.
  Edge modifiers keep normal pick storage. Face modifiers containing a kind leaf freeze the complete
  resolved selection as picks at creation; selections without kinds retain dynamic native queries.
  Frozen selections do not rediscover new faces; dynamic kind discovery needs an upstream Query change.
  Native `Query::is_pick_list` (`refs.rs:202-208`) excludes `Filter`, so freezing leaves alone would bypass
  shell's asked-versus-opened check (`model/regen.rs:1442-1449`). ADR 0013 records the full-selection policy.
  The three face golden tests inspect whole-selection picks and derive prism/hole/shell/push volumes on
  creation/server/reopened/native-GUI edits. The partial-loss shell unit regression replaces a box source
  with a cylinder after selecting its top and two sides, proving refusal when two openings vanish.
  Restoring leaf-only storage makes this regression silently shell only one face.


## F-063 Native mesh rounding depends on world coordinates, including small chamfer cones

- **Version:** v0.1.0-dev.20261001
- **What:** native tessellation exports f32 vertices, promoted to f64. A small cone near x/y=±20 retains
  that coordinate rounding after subtracting its axis origin. Local meridian scale alone can reject a valid
  r(z) fit, losing analytic normals and falsely counting plane/cone tangent boundaries as uncertain.
- **Evidence:** source: `qymcad-kernel/src/lib.rs:2039-2057` (`doc_to_bodies`, f32 `vbuf` and f64 promotion).
  Test: MCP `backlog_review::chamfer_chain_tangent_junctions_are_not_uncertain_corners` reconstructs a
  40×40×10 box, two top x-edge R1 fillets, Ø8 through hole, and 0.5 chamfer on cylinder boundaries.
  Four conical end patches each have two nonseam straight plane/cone boundaries, length 0.5√2, at
  midpoint z=10−1=9 or z=10−0.5/2=9.75. Exact OCCT probe (`BRepAdaptor_Surface`, `BRepLProp_SLProps`,
  edge parameters at fractions 0.1/0.3/0.5/0.7/0.9) gives matching outward normals at all eight edges:
  angle 0°, internal dihedral 180°. Plane normals are (±1,±1,0)/√2 or (±1,0,1)/√2.
  Meridian fit residuals are 1.55–2.03e-6 mm versus the former local tolerance 1e-6 mm;
  f32 spacing near 20 is 2^(4−23)=1.9073486328125e-6 mm. Accepted fitted cone normals differ from
  their adjacent plane normals by at most 0.714e-6, below the unchanged 1e-6 sampled G1 threshold.
  Before fix the regression reports eight uncertain edges. Restoring local-only fit tolerance reproduces it.
  `topology::normal_tests::cone_meridian_fit_at_large_coordinates_accepts_f32_rounding_but_rejects_noncone_deviation`
  checks r(z)=1+0.37z translated to world coordinates near 1000 mm after f32 promotion, rejecting a
  0.01 mm meridian deviation above the coordinate-scaled tolerance. Local-only tolerance fails acceptance.
- **How we handle it:** include absolute mesh coordinate scale in the cone-fit residual tolerance while
  retaining the existing axial-span check, one-degree cone allowance and sampled G1 threshold. Seams and
  these verified tangent junctions contribute no uncertainty count. Truly shallow rims remain uncertain.
  Bare corner and positive `and` previews replace corner leaves with uncertain ids before evaluating other
  conditions, except that a positive intersection requiring both signs is impossible and reports no omission.
  MCP `backlog_review::contradictory_corner_intersection_has_no_omission_note` and
  `nested_contradictory_corner_intersection_has_no_omission_note` check flat/nested intersections on a
  formula-verified 0.9° countersink with one uncertain rim.
  Union/subtraction/tangent-chain compositions report only the uncertain body total and explain that
  corner filters treat those edges as neither concave nor convex, without any causal or result-membership
  claim. `backlog_review::uncertain_note_in_nonmonotone_compositions_is_truthful` checks minus/union;
  `subtracted_corner_filter_note_makes_no_membership_claim` covers a corner filter on the subtracted side,
  including a circular uncertain rim retained in the result. `uncertain_union_note_reports_body_total_without_membership_claims`
  checks two shallow rims with one included through a union pick branch; the note still reports the body total.


## F-064 Fillet and chamfer contours propagate beyond the selected edge ids

- **Version:** v0.1.0-dev.20261001
- **What:** `BRepFilletAPI_MakeFillet` and `BRepFilletAPI_MakeChamfer` construct contours of tangent-continuous
  edges when adding a seed. Thus kernel blending can modify edges absent from the resolved pick list, without
  a warning. Corner filtering excludes G1 face junctions; contour propagation concerns tangent continuity
  along consecutive edges, which is a separate property. `select` previews only the selected edges.
- **Evidence:** source: `qymcad-kernel/src/occt_io.cpp:887-908` (`mk.Add` for selected edges),
  `:997-1028` (explicitly handles generated faces from unselected edges), `:1534-1536,:1659-1661`
  (native fillet/chamfer builders). Installed OCCT `BRepFilletAPI_MakeFillet.hxx:85-88` documents
  tangent-edge contour construction; `BRepFilletAPI_MakeChamfer.hxx:62-67` documents propagation through
  mutually tangent edges delimiting tangent support-face series. Test: MCP
  `backlog_review::kernel_chamfer_extends_beyond_cylinder_selection` uses F-063's model. The preview has
  2*4+3=11 cylinder boundaries: two four-boundary fillet cylinders plus two bore rims and its seam. Its
  four end arcs seed contours that add four vertical stock corners and two top end edges absent from preview.
  Equal setbacks bisect orthogonal support planes, creating four normals (±1,±1,0)/√2 and two
  (±1,0,1)/√2; none exists before chamfer. The result has all six without a warning. Exact removed volume:
  `[4(h-r)+2(L-2r)]c²/2 + π(rc²-c³/3) + 2π(Rc²+c³/3)`, h=10,L=40,r=1,R=4,c=.5 mm.
  These are straight triangular prisms, four quarter-cylinder end bevels and two bore-rim bevels,
  derived by integrating the difference of radius-squared cross sections; test tolerance 1e-6 mm³.
  The preview also selects the four long G1 boundary lines of the two fillet cylinders. At those
  plane/cylinder junctions the outward radial normal equals the adjacent plane normal, (0,±1,0) or
  (0,0,1), so the internal dihedral is 180°. They remove no volume: the formula excludes any term for
  them and agrees with the native volume difference within 1e-6 mm³. F-063's exact outward-normal
  probe gives corresponding 180° evidence for the generated plane/cone G1 junctions.
- **How we handle it:** describe propagation explicitly in fillet/chamfer tool documentation; preserve native
  behavior and selected-edge preview. No inferred face-count warning: ordinary corner patches, splitting
  and merging make that criterion unsound. A future positive-evidence warning could inspect actual output
  Blend face names with source edge ids outside the picks (source: `model/regen.rs:2763-2789` prepares all
  edge blend names). This would be incomplete for unnamed/duplicate edges; native contour enumeration would
  require an upstream FFI addition.


## F-065 Upstream sphere fits can accept a flat rounded pocket floor

- **Version:** v0.1.0-dev.20261001
- **On v0.1.1-dev.20261026 (2026-10-24):** **Planar case fixed upstream** (0646b3c, our #172): `face_sphere` rejects the flat floor (`native_sphere_fit_rejects_a_single_coplanar_floor_mesh`). The 2% radius tolerance is unchanged for curved faces, so the server residual check stays.
- **What:** `Project::face_sphere` is a least-squares fit to this face's mesh vertices, not a native
  surface query. Unscaled normal equations with an absolute pivot cutoff do not reliably reject a
  numerically singular coplanar point set. Its 2% radius residual and 0.9 normal cosine checks can then
  accept a large spurious sphere over a wide plane. This is an upstream bug candidate.
- **Evidence:** source: `crates/qymcad-core/src/model/assembly.rs:2330-2345,2367-2437,2443-2465`.
  Repro: fixture `crates/engine/tests/fixtures/hanging_shelf.qcad`, pocket floor face 1073741842;
  268×170 R15 stock, 260×162 R11 pocket cut from z=10, R4 bottom loop fillet. The floor's
  support plane is z=10−0.001 (F-061); all its edges lie on that plane, yet upstream returns a sphere.
  `golden_acceptance::shelf_floor_is_planar_after_open_and_rebuild` failed before the server fix
  with `left: Sphere, right: Plane`. Minimal fit-only repro:
  `upstream_sphere_fit_accepts_a_single_coplanar_floor_mesh` loads stored mesh data and places only that
  floor in regen_faces, then calls `Project::face_sphere` without a native Shape or recipe rebuild. Every
  referenced vertex has z=10−.001, and the accepted sphere contradicts their radial distances beyond
  coordinate precision. Native `face_cylinder`/`face_axis` are kernel queries
  (`crates/qymcad-kernel/src/lib.rs:1211-1225,1243-1266`); no per-face native plane/sphere getter exists.
- **How we handle it:** validate the fitted radial residual of every vertex on the same face against
  16·f32::EPSILON·max(1,radius,absolute coordinates,absolute center coordinates), allowing mesh
  coordinate rounding (F-063) and fit arithmetic, rather than a percentage of radius. Rejecting the
  spurious fit lets the existing planar-normal path report the floor, and avoids excluding it from
  corner normals. Sphere precedence remains for actual sphere fits, protecting sparse curved patches.
  The original residual-only fix failed when restoring the native 2% allowance; F-068 subsequently adds
  native zero-sphere and degenerate-support checks, so that mutation alone no longer bypasses validation.
  Outward shell corner spheres
  (`golden_features::shell_open_top`) and the single-triangle native sphere guard remain green.


## F-066 Unmeshed torus bounds explain the shelf's open-time inflation

- **Version:** v0.1.0-dev.20261001 (OCCT 7.9.3)
- **What:** `Shape::bbox` calls `BRepBndLib::Add`, using triangulations when present, otherwise geometric
  enclosures. Saved binary B-reps exclude triangulations; clean open deserializes those shapes and its no-op
  dirty regenerate retains them. Dirty feature rebuilds tessellate the new shapes, so bounds differ.
  The four outer top-rim corner faces are native tori, not B-splines: unmeshed OCCT torus bounds use
  enclosing points at radius `(major+minor)*sec(pi/8)`. Here major+minor=15, giving excess
  `15*(sec(pi/8)-1)=1.2358830044 mm`, plus native tolerance, exactly explaining the open bbox.
- **Evidence:** source: `crates/qymcad-kernel/src/lib.rs:1508-1510`, `occt_helical.cpp:764-770`,
  `occt_io.cpp:23-27,76-84`, `kernel.rs:170-171`; engine `session.rs` open/deserialization and
  `regenerate_dirty_with_shapes`. OCCT V7_9_3 sources: `src/BRepBndLib/BRepBndLib.cxx:91-106`,
  `src/BndLib/BndLib_AddSurface.cxx:277-279`, `src/BndLib/BndLib.cxx:96-110,1540-1548`.
  A direct OCCT probe on fixture body 123 identifies its four torus enclosures; `AddOptimal(shape,false,false)`
  agrees with formula extents ±134×±85, but no optimal getter is exposed by the pinned Rust kernel.
- **How we handle it:** reporting (`result_bodies`, open stored/rebuilt metrics and thus doc_info/render/undo)
  measures vertices of `tessellate_merged(max(.005,diagonal*1e-5))` on an independent B-rep copy.
  Bodies up to 500 mm diagonal use .005 mm; larger imports use size-adaptive accuracy to avoid
  runaway mesh growth (native size-scaling rationale: `lib.rs:1568-1574`). The native tessellator
  cleans/replaces triangulations even through `&Shape` (`occt_bridge.cpp:133-153`); copying avoids observer
  mutation. Merged meshes include all solids (`lib.rs:1542-1566`). Incomplete face coverage, nonfinite
  vertices or failed copies/meshes fall back to native conservative bounds. Deflection is nominal, with
  f32 coordinate rounding (`occt_bridge.cpp:52-58`, `lib.rs:2039-2057`); curve extrema can be slightly
  under-bounded. The native rescue mesher disables surface-deflection control (`occt_bridge.cpp:167-176`),
  so this is an engineering approximation, not a universal certified containment or error bound.
- **Tests:** `golden_acceptance::shelf_bounds_are_tight_on_open_rebuild_and_save_round_trip` checks
  ±268/2×±170/2×[0,17] with tolerance `.005+f32::EPSILON*134`, derived from requested chord deflection
  and coordinate precision, including doc_info, render, save/reopen and parameter rebuilds. Reporting preserves
  B-rep bytes and the live native bbox/triangulation state. Sphere bounds are tested at r=2 and
  r=1000 against ±r and the size-derived deflection tolerance. Before fix: `got -135.23588310438592, expected -134 ± 0.005015974044799805`.
  Switching back to native-first bounds makes the same regression red; restored from /tmp backup.
  MCP `backlog_selection::opened_shelf_reports_tight_bounds_in_both_fields_and_render_caption` checks both
  doc_open bbox fields, doc_info and the caption through actual tool handlers.


## F-067 Revolved cone parameterization can defeat a later mouth chamfer (upstream candidate)

- **Version:** v0.1.0-dev.20261001 (OCCT 7.9.3)
- **What:** revolving equivalent cone profiles on opposite sides of a construction axis can create different
  cone surface-frame handedness. Pinned OCCT can refuse the cone/plane mouth chamfer on the indirect frame,
  even though the solid is valid and the same distances fit the equivalent direct-frame geometry. Axis sign
  alone is not the cause: reversing the requested revolve axis changes handedness/success while the cone's
  axial sign can stay unchanged. This is an upstream parameterization bug candidate; no exact failing
  OCCT internal branch is claimed. QymCAD's seam rescue repairs some simple cases, so negative-side
  profiles do not universally fail.
- **Evidence:** source: `crates/qymcad-core/src/model/sketch.rs:3765-3776` resolves a line axis from its
  endpoint difference; `model/regen.rs:2511,2553-2558` forwards it; `crates/qymcad-kernel/src/kernel.rs:495`,
  `occt_bridge.cpp:2793-2800` call `BRepPrimAPI_MakeRevol`. Chamfer seeds use `BRepFilletAPI_MakeChamfer`
  (`occt_io.cpp:908`), retrying after seam rescue (`:1658-1665`). The source itself warns a reversed
  circular frame makes cone/plane chamfers fail (`:1467-1469`). Any native failure maps to
  `ChamferTooBig` (`kernel.rs:817`), so the distance diagnosis is not proof of an oversized chamfer.
- **Minimal native OCCT repro:** box [-15,15]×[-15,15]×[0,10]; revolve the XZ face with points
  (0,-1),(3.375,-1),(1.875,11),(0,11) about +Z for 2*pi and subtract it; chamfer the two mouth circles
  .5 mm with `BRepFilletAPI_MakeChamfer::Add(distance,edge)`. Reflect the profile in x=0, reversing point
  order to retain face winding. Direct OCCT (no QymCAD rescue) succeeds for the original/direct cone frame,
  fails for reflected/indirect; reversing the axis swaps success. `BRepCheck_Analyzer` accepts the cut.
  The cone radius is r(z)=3.25−.125*z: intended bottom Ø6.5/top Ø4 with 1 mm same-taper overrun.
- **Server repro/controls:** `golden_revolve_chamfer::centered_conical_seat_chamfers_from_both_profile_sides`
  proves both centered profiles work through QymCAD rescue with formula frustum and bevel volumes.
  `upstream_negative_side_shelf_refuses_each_mouth_but_axis_reversal_recovers` reflects fixture profiles
  around their own x=±119 axes, preserving r(z), all other geometry and pre-chamfer volume. Every one
  of the eight native mouth-circle chamfers fails, and reversing the construction-axis endpoints recovers.
  `negative_side_revolve_chamfer_error_suggests_profile_or_axis_workaround` initially failed because
  `chamfer 0.50 too big` omitted advice; disabling the new advice reproduces that failure.
- **How we handle it (option b):** only after a native ChamferTooBig failure whose complete consumed-body ancestry contains a revolve, retain the
  native reason explicitly as the kernel's reason (which does not prove an excessive distance), and add
  conditional advice: first try a smaller distance; for a full turn draw the axis line toward the sketch's +y with the profile at larger x than the
  line, or reverse its endpoints. Orientation alone does not predict failure; overlapping cones can matter,
  while clean-block controls pass both ways. User-facing text omits internal finding references. Partial-turn
  endpoint reversal changes the sweep. Preserve recipes/axes; a cone axis sign cannot prove frame handedness,
  and replacing moving line axes by static datums loses their dependency. A future full-turn-only hidden
  reversed line sharing original endpoints could preserve motion, but requires a sketch/recipe policy and
  does not cover opposite-side multi-profiles or generic datum axes. Automatic normalization is deferred.


## F-068 Seam repair can duplicate blend face names and hide conical surface identity

- **Version:** v0.1.0-dev.20261001 (OCCT 7.9.3)
- **On v0.1.1-dev.20261026 (2026-10-24):** **Fixed upstream for rebuilt geometry** (5e334fc, 81c0dda, 0eb0c3b; our #173): rebuilt cone chamfers have no duplicate face names (`rebuilt_cone_chamfers_have_unique_face_and_edge_names`). Files saved by 0.1.0 still carry duplicates, so the guards stay.
- **Upstream bug candidate / minimal repro:** block 15×15×10; XZ closed profile
  (0,-1),(-3.375,-1),(-1.875,11),(0,11); construction axis (0,-2)→(0,12);
  revolve cut 360°, chamfer both mouth circles .5 mm. Stored fixture
  `tests/fixtures/cone_negative_chamfer.qcad` has 11 faces, ids 1073741836 and 1073741837 twice.
  Reverse only the axis endpoints (`cone_negative_axis_reversed.qcad`): 9 unique faces.
- **Naming evidence:** `crates/qymcad-kernel/src/occt_io.cpp:1331-1347` divides closed surfaces for seam
  repair; `:1459-1460` copies an original edge name to all matching new pieces. `name_blend_faces`
  (`:922-949`) restarts `piece=0` for every source-edge occurrence and gives its first generated face
  `names[k]`. Distinct seam pieces can therefore receive the same blend name. Chamfer seam retry and
  subsequent unification use this path (`:1658-1671,:1508-1516`). Candidate causal mechanism, not a
  claim that every split surface or every negative-side revolve fails.
- **Classification evidence:** native OCCT probe on stored QYMB B-reps (format `occt_io.cpp:19,64-79`,
  `BRepAdaptor_Surface::GetType`) finds six planes, three cones and two B-splines, zero spheres.
  836 appears Cone then BSpline; 837 appears BSpline then Cone; split metadata is zero on both pairs.
  Control has six planes/three cones. Native `face_axis` (`occt_io.cpp:680-696`) stops at the first
  matching persistent id and accepts only Cylinder/Cone. Thus 837's spline hides its later cone;
  836 incorrectly supplies the first cone's axis to both rows. `Shape::face_kinds` (`lib.rs:888-894`,
  `occt_helical.cpp:1354-1376`) exposes only aggregate counts; there is no per-face surface-type getter.
  `Project::face_sphere` remains the mesh fit in F-065. Two coaxial circles fit a common sphere:
  for radii r1,r2 at heights z1,z2, its center height is
  `(r1²-r2²+z1²-z2²)/(2*(z1-z2))`; radial residual cannot distinguish a two-ring band.
- **Server policy:** ADR 0016; native zero-sphere counts disprove fits only when freeform/other counts are
  also zero: those surface representations can describe a sphere. Coplanar points and points on
  at most two axial levels about a boundary-plane normal also disprove sufficient sphere support.
  Circle metadata is absent on the rescued spline rings; native `edges_info` exposes 25 f32 curve samples
  (`occt_io.cpp:752-762`, `lib.rs:985-1003`) but populates circle metadata only for GeomAbs_Circle
  (`occt_io.cpp:764-777`). Noncollinear boundary samples supply a candidate unit axis; requiring all
  sphere-fit vertices on at most two perpendicular planes is the rejection proof, even for spline rings. Keep the
  f32 radial tolerance. Flag ambiguous topology ids, warn, and refuse selections resolving to them
  with a full-turn axis/profile workaround, including explicit ids and descriptive face operands nested
  in edges_of/between. Document open/info and feature results repeat the naming warning for result bodies.
  The model remains inspectable/renderable/exportable.
- **Tests:** `golden_round_i` verifies both unchanged fixtures, formula frustum/bevel volume, no false
  spheres, warning/flags, ambiguous selection refusal and all control face selections. Before fix:
  `ambiguous face selection must be refused, got Ok((37, [1073741836, 1073741836]))` and
  `a straight conical bevel cannot be spherical`. Removing sphere guards / selection refusal separately
  makes their respective regressions red; restored by editing /tmp backups. The mixed-native-sphere
  regression in sphere_fit_tests defeats the aggregate shortcut and fails without spline-boundary candidate
  normals, proving the two-ring rejection itself. Rotated-plane and two-/three-ring geometric tests cover
  support degeneracy independently. Nested explicit face-reference regression fails without name validation.
- **J2 guard evidence:** `sphere_fit_tests::freeform_native_faces_do_not_disprove_a_geometrically_valid_sphere_fit`
  injects exact R2 spherical vertices over a native Bezier/freeform B-rep to unit-test the aggregate decision;
  it does not claim that the B-rep itself is spherical. The pinned Rust API exposes Bezier patches but no
  sphere-to-NURBS conversion. Native aggregate slots are documented in `qymcad-kernel/src/lib.rs:888-894`;
  `occt_helical.cpp:1354-1376` classifies Bezier/B-spline as freeform. Geometric support/residual guards still
  reject the repaired cone fixture. `corner_planar_normal_itself_rejects_a_validated_sphere_fit` supplies
  four exact sphere latitude triangles whose normals all appear planar, proving the direct fitted-sphere
  guard. Removing either guard separately is red. Both regressions follow initial behavioral failures.
  A3's `face_is_planar_itself_rejects_a_validated_sphere_fit` reuses the formula R2 latitude mesh, proving
  the same fitted-sphere guard inside `face_is_planar`, rather than relying on callers; removal is red.
- **Edge-name investigation (J2, closed without a new policy):** the unchanged repaired fixture has 25
  distinct edge names. Rebuilding its chamfer at .25/.5/.75 mm also yields 25 distinct edge names in every
  case, with four ambiguous face rows confirming seam rescue. Test:
  `golden_backlog_j2::seam_repaired_chamfers_keep_unique_edge_names_in_investigated_cases`, including
  formula-derived volume checks. No duplicate edge name reproduced; inferred edge ambiguity alone does not
  justify an untested policy. A reproduced fixture would unblock adding mirrored edge checks.


## F-069 The kernel gate is a non-reentrant mutex with no ownership query

- **Version:** v0.1.0-dev.20261001
- **Evidence:** `crates/qymcad-kernel/src/kernel.rs:68-71` declares a function-local static
  `std::sync::Mutex<()>` and returns `MutexGuard<'static, ()>` from kernel_gate(). Recursive acquisition
  cannot be used; the pinned public API exposes neither the mutex nor current-thread ownership.
- **Server policy:** reporting_bbox always acquires the gate; its callers result_bodies/info/render document
  that the current thread must not already hold it. Public open/save/rebuild/export/undo callers now
  state the same precondition. No reporting-only debug assertion: a thread-local flag
  would miss raw/native acquisitions, and a contention probe cannot distinguish other-thread locking from
  recursion. Centralizing every native acquisition would be a separate cross-module change. Existing
  reporting isolation/sphere/shelf/render tests exercise calls after the caller's gate has been released.

## F-070 Stored names and error texts are Fluent keys; the English words come from `qymcad-i18n`

- **Observation:** QymCAD stores default object names as Fluent keys ("feat-name-extrude", "name-plane") and
  reports errors such as `CutRemovedNothing` by key ("error-cut-removed-nothing"); the GUI translates them at
  display time. Agents saw the raw keys.
- **Source:** `qymcad-i18n/src/lib.rs` (`set_language`, thread-local, ~213; `name` with substitutions, ~315),
  `qymcad-i18n/src/error_words.rs` (`error_text`, ~11-115), catalogue `i18n/en/main.ftl` and `errors.ftl`
  (Codex round I report, tasks/qymcad-phase4/codex-evidence-i1).
- **Server policy:** `crates/engine/src/localization.rs` translates at presentation time only (info, results,
  errors, undo labels); `Project` keeps the stored keys so files round-trip unchanged. Language selection is
  thread-local, so every call sets "en".
- **Evidence:** `crates/engine/tests/localization.rs` (unnamed features/datum planes read "Extrusion"/"Plane";
  the missing cut reads in English with the direction hint), `localization_tests.rs`. `usability_polish::undo_keeps_tool_and_arguments_with_a_localized_feature_label`
  verifies undo `undone.label` for default Fillet and custom names while retaining the tool and exact arguments;
  calls without a created node use a human action label. Mutation (returning the
  stored key) turns both integration tests red (verified 2026-10-09).
- **Cost:** one more pinned QymCAD crate (bump with the other four, docs/UPGRADING.md) and its Fluent
  dependencies (about 20 crates in Cargo.lock).
- **Version:** v0.1.0-dev.20261001.

### J1 diagnostics follow-up

FilletRadiusTooBig's native Display only gives radius/edge count (`qymcad-core/src/errors.rs:551`). The
server adds conditional geometric fit advice (thin wall, short edge, small adjacent fillet), without claiming
an exact kernel cause. Tests: localization_tests::failed_fillet_explains_fit_and_suggests_smaller_radius;
modifiers::hint_tests::chamfer_advice_only_wraps_too_big_native_message; topology::ancestry_tests;
MCP backlog_review modifier-only/junction/singular/direction regressions. Full ancestry uses
FeatureKind::consumed (`feature.rs:2961`), not primary lineage alone. Kernel reason annotation wraps
only the native message after the localized name/id. Eight separate mutation regressions verified red.

### J3 presentation and open-result clarification

Sketch world_frame origins/directions and point x/y serialize with the bbox four-decimal half-away policy;
internal values remain unrounded. Radius/constraint values and parameter/undo argument values retain full
precision because clients can reuse them as inputs. This replaces blanket six-decimal sketch formatting.
Evidence: MCP backlog_j3 tests for frame/point presentation, a 45° frame, input-ready values, and curved
feature bounds after height edits 10→12→10. Cylinder bounds use r=3.6 and V=pi*r²*h; the returned feature
bbox equals later doc_info after rebuilding, confirming H1/F-066 fixed the old padding discrepancy.

Both doc_open.doc.bodies and doc_open.rebuild.bodies are post-open geometry, not separate stored/rebuilt
snapshots: `crates/mcp/src/tools/doc.rs` calls info on the Session returned by Session::open, after its
rebuild. Stored metrics are compared internally and appear only in mismatch warnings (F-052).
True scaled binary ties: ±33/32 and ±35/32, multiplied by 10^4, give ±10312.5 and ±10937.5 exactly.
`golden_round_i::bbox_serialization_rounds_half_away_normalizes_zero_and_keeps_large_finite_values`
asserts those exact fractional halves before checking away-from-zero output.

## F-071 Named feature dimensions evaluate against globals only

- **Version:** v0.1.0-dev.20261001.
- **Evidence:** source: `qymcad-core/src/model/sketch.rs:2434-2443`, `Project::dim_target_value` for
  `DimTarget::Feature` evaluates its `feat_dims` expression against `self.parameters` only. `param_map`
  (`model.rs:2123-2133`) inserts a named dimension only when `named_dim_value` returns Some.
- **What:** a named feature dimension whose expression references another named dimension is omitted from
  `param_map`, even if that expression can evaluate in the full scope used during regeneration. Global-only
  formulas and literal feature dimensions remain available. This is a pre-existing native limitation;
  the engine does not change feature-dimension evaluation or recursively synthesize these missing names.

## F-072 Missing import B-reps need source restoration; unavailable embedded sources must warn

- **Version:** v0.1.0-dev.20261001, pinned checkout `d2949f0`.
- **What:** an imported STEP/IGES body has no modelling recipe. Native regeneration only re-tessellates
  its existing kernel shape; without one it keeps the loaded display mesh and returns success. Thus an
  error-free rebuild does not establish that an import has live exact geometry. Our previous open path
  supplied only stored B-reps, so an import whose B-rep was omitted opened without its live shape and
  without an error. Documents containing stored import B-reps already survived open, parameter edits,
  save and modelling undo. An unavailable embedded original also needs a warning when the stored B-rep
  remains usable; retessellating that live shape does not repair the original.
- **Upstream evidence:** `qymcad-core/src/model/regen.rs:1805-1816` implements the mesh-preserving,
  success-returning import branch; `qymcad-kernel/src/kernel.rs:1111-1113` tessellates the cached shape
  without replacing it. `qymcad-testkit/src/lib.rs:54-84,89-91` restores embedded sources before an
  app-style rebuild. The actual GUI restores the source format, solid index and import scale in
  `qymcad/src/gui.rs:374-409`; the testkit helper ignores scale and uses shared source-id temporary
  paths (`lib.rs:58,69`). GUI save includes all live B-reps (`qymcad/src/gui/io_jobs.rs:963-980`);
  the bundle embeds B-reps and originals (`qymcad-io/src/project_file.rs:75-106`), and loading tolerates
  absent embedded-source entries (`:206-211`). All upstream paths refer to checkout `d2949f0`.
- **Engine policy:** `crates/engine/src/session.rs:166-178` restores missing imports before computing
  `missing`, while retaining valid stored B-reps as authoritative. `imports.rs:13-63` groups import
  bodies by source and parses each source once to validate availability. Only absent shapes require a
  matching source solid index; readable sources never cause index warnings on stored live shapes. It restores
  only absent shapes, applies the GUI's uniform import scale, and reports failures with body name/id
  and source id, distinguishing retained stored geometry from unavailable exact geometry.
  `imports.rs:66-95` stages embedded bytes in exclusively created process/counter temporary files,
  reads the exact format under the native gate,
  and removes the staged file. We use this equivalent helper although testkit is a runtime dependency:
  it avoids the testkit helper's shared temporary names, supplies the gate, preserves scale and reports
  errors. Source warnings, structurally marked by the private `AdvisoryWarning::ImportSource` variant,
  survive retessellation while their nodes remain (`Session::rebuild_retrying`),
  because a live shape cannot repair an unavailable original. Existing regeneration already seeds all
  live shapes (`session.rs:366-368,382-384`), so regeneration scheduling, dirty marking, save and history
  are unchanged (ADR 0019).
- **Tests / reproduced before fix:** `crates/engine/tests/imports.rs:141-147` separately verifies STEP
  and IGES imports saved without their B-reps, failing before the fix with
  `import body 4 has no live B-rep`. `:262-272` independently verifies missing, empty and unreadable
  embedded sources, failing before the fix with
  `{missing/empty/unreadable} source silently lost import body 4: []`; each now warns and retains the
  import timeline/display body and available source metadata/bytes through save and reopen.
  `damaged_sources_warn_even_when_the_import_has_a_stored_brep` (`:275`) reproduced
  `missing source has no warning for import body 4: []` with a stored live shape. It also reproduced
  that failure after adding source validation, because a dirty import's retessellation cleared its
  new advisory; retaining import advisories fixes that lifecycle failure.
  S1 (`:118`) checks the 1000 mm³ cube, doc_info bounds and exact STEP export; S3 (`:150`) changes a
  separate parametric prism beside the import from 500 to 800 mm³ while retaining the import B-rep
  bytes, with stored STEP and omitted STEP/IGES B-reps; S4 (`:168`) checks server save followed by both
  app-testkit and server reopen with stored and omitted STEP/IGES B-reps; S5 (`:192`) checks deletion
  and undo of a later datum through the MCP's modelling boundaries. Stored-B-rep variants of these
  controls pass with the unchanged engine. Fixtures use fresh temporary directories, remove the
  original exchange file before opening, and spawn no binary. The scaled-import regression (`:282`)
  checks a 2× source import against 8000 mm³ and 20 mm bounds. All 11 tests pass. Independently
  disabling restoration, diagnostics, stored-source validation, warning retention and scale handling
  makes the corresponding tests red (restored by editing). The scale mutation gives
  `scaled source cube volume: got 999.9999999999998, expected 8000 ± 1`. See
  `tasks/review-imports.md` for exact mutation failures and the full-check result.
- **Round 2 evidence:** `session::tests::import_retessellation_replaces_void_advisories_but_retains_source_warning`
  builds a fully enclosed rectangular cavity (20³ − 4*6*3 mm³, two shells/one solid). Two dirty import
  rebuilds yield one void advisory and one persistent source warning; repairing the geometry clears the
  void advisory, and removing the node clears its source warning. Retaining all import advisories reproduces
  the duplicate on rebuild two. `imports::stored_import_sharing_an_already_restored_solid_has_no_source_warning`
  pins the shared-index case, which already passed: `Vec<Option<Shape>>::get` still returns `Some(None)`
  after `take`. `stored_import_with_readable_source_does_not_require_a_matching_solid_index` reproduces
  the actual stored-body index warning with an out-of-range index, fixed by skipping readable-source stored
  bodies before indexing. The pinned `ExactFormat::of_extension` maps every extension except `igs`/`iges`
  to STEP, including an empty extension (`qymcad-kernel/src/lib.rs:1762-1767`), matching the GUI's empty
  extension default (`qymcad/src/gui.rs:396`). `empty_source_extension_is_step_and_restores_an_import_without_a_brep`
  pins that dispatch and restores legacy embedded STEP bytes; no production format change was needed.
- **Scope:** S3 covers an independent parametric body in another part, rather than a modifier consuming
  the import. The pinned generic fillet constructor accepts imported body ids without a kind restriction
  (`qymcad-core/src/model/timeline.rs:1007-1013`), but that is not a tested modifier-on-import result.
  App reopen evidence uses the specified testkit path; no installed QymCAD.app GUI check was available.

## F-073 Release public doors own different kernel-gate scopes

- **Version:** v0.1.1-dev.20261026, f25f27d409418448a4968cfd5698759ee6633f2f.
- **Evidence:** release `crates/qymcad-kernel/src/imports.rs:21-31` self-gates
  `restore` and `scaled`; `ask.rs:46-49` self-gates `export_mesh` and returns `Built`.
  `crates/qymcad-testkit/src/lib.rs:19-39,53-62` gates regeneration and transitively
  gates import/app-open helpers. `kernel.rs:83-145` now returns `KernelHeld` and
  offers diagnostic `gate_now`, but the static mutex remains non-reentrant; thread
  names in its holder record are not reliable ownership tokens. F-069's old return
  type is historical; the no-nested-lock precondition still applies.
- **Port:** serialize under a short caller gate, release it, restore through the
  public door, and verify every requested snapshot id. Reporting tessellates an
  independent copy without holding an outer gate, retaining separate guards for
  diagonal/native-face/fallback reads. STEP/native exchange retain caller gates.
- **Adjacency:** `Shape::edge_face_pairs` is private (`shape/ops.rs:301`); the public
  `Kernel::edge_face_pairs` needs an owning kernel (`kernel.rs:366-368`). A checked
  named-B-rep copy is the smallest compatible adapter. QYMB v3 preserves names,
  split maps and sheet neighbors (`occt_io.cpp:19-49,66-115`); serialization omits
  triangulations, preserving reporting/rollback isolation. `bbox_exact` is still
  private (`shape/asks.rs:184`). Copying large bodies has a cost; no caching or
  Session ownership redesign is introduced in stage A.
- **Test setup:** the old public primitive/Bezier/renaming APIs are private in normal
  release builds. Exact old-release B-reps in `tests/fixtures/brep` preserve setup
  geometry, with provenance in their README; test-only header edits preserve chosen
  persistent names. No specimens or raw FFI is used.
- **Sketch API:** splines now live in `EntityKind`, which is no longer Copy;
  ellipses can carry arc spans (`qymcad-core/src/model/curve/kinds.rs:5-39`).
  Public `entity_points` (`:470-479`) supplies ownership ids, including spline nodes
  and ellipse endpoints. The port initially retained spline-detail omission; stage 1
  exposes native spline rows and nodes (F-077). `tasks/upgrade-evidence/release-sketch-port.md` records the port.
- **Identifier API:** `drivers::check_ident` now rejects `IdentError::Constant`
  (`qymcad-core/src/drivers.rs:80,86-99`). Normalization accepts that specific code
  so assignment retains the existing built-in-constant explanation and deletion
  still removes constant-named legacy parameters (F-039). Stage B then dropped
  lowercasing (F-001, ADR 0021). Existing `sketch_behaviour` constant
  tests caught the initial port gap and verify this adaptation.
- **Audit:** `tasks/release-api-audit.md` records the full release call inventory,
  caller scopes and source evidence. Historical findings above are deliberately
  retained for stage B policy review; they are not blanket revalidated by this port.

## F-074 A fillet too big for neighbouring edges is built from overlapping blends, without an error

- **Version:** v0.1.1-dev.20261026
- **What:** when a group fillet fails, the kernel tries an exact limiting construction (`blend_limit`, prisms
  taken off per edge). For a radius beyond the limit it returns overlapping blend cylinders that meet each other
  and the caps at sharp edges, and the node is green. R15 on the four vertical edges of a 20 mm cube gives
  V = 4363.25 mm³, bbox ±9.142 (joins at acos(7/9) = 38.9°). 0.1.0 refused it: "fillet R15.00 only works edge
  by edge". A chamfer past its faces uses the same path; it gives the intersection of the chamfer planes
  (15 mm on the cube: |x| + |y| ≤ 5, V = 1000), which is predictable, so it is not guarded.
- **Evidence:** source: `crates/qymcad-kernel/src/ops/blend.rs:142-176,245-300`, `door/mod.rs:205-235`,
  `occt_limits.cpp:160-265`; test: `golden_features.rs` `too_big_fillet_is_rolled_back_with_the_reason`,
  `growing_a_fillet_past_its_limit_by_a_parameter_is_rolled_back`, `fillets_that_roll_off_a_neighbour_are_accepted`,
  `chamfer_wider_than_its_faces_keeps_the_intersection_of_its_planes`.
- **How we handle it:** `modifiers::fillet_acceptance` looks at named non-seam joins between two blend faces of
  the fillet (`Role::Blend`, named after the edge they round: a 1-based place in the flattened pick list of any
  pick form, `Id`/`Ids`/`Union`, or the edge's own name). If the two source edges share no vertex (compared by
  native vertex names, `MeshEdge::ends`; by position only where a name is missing), their blends overlap: the
  join is refused when the kernel flags it sharp, or, for two cylindrical blends, when their normals at the join
  differ by more than 0.1° (the kernel's own flag lets about 1.5° pass; R10.13 past an R10 limit meets at 1.47°).
  Blends of edges that share a vertex may meet at a sharp mitre (a rim corner), and blends of separate edges meet
  tangentially at an exact limit, so neither is refused. It runs after every rebuild of a fillet and, in
  `Session::open`, on fillets kept from the file and not rebuilt. A refusal is a rebuild error: a new fillet and a
  parameter edit roll back; found outside an edit it is kept in `regen_errors` as `CoreError::Kernel` with the same
  words, so doc_info shows it, export refuses, and unrelated parameter edits still work.
  Accepted with exact volumes: R9.9 and R10 on the cube, R9 and the R10 limit at the end of a 100 x 20 x 20 block,
  R10 as high as a 10 mm block, two R5 meeting across a 10 mm top, a quarter round (R10 on a 10 x 10 bar).
  Accepted: the top rim at R2 and at the limiting R10 (sides eaten, corner mitres), rim with vertical edges, every
  edge. Refused: R15 on the cube (also saved clean, also with a `Union` pick list rebuilt natively), R15 at the
  block end, R10.13 at the block end, R6 across the 10 mm top, R0.6 on a 0.8 mm end at y = 10⁷.
  Accepted too: a convex and a concave blend meeting tangentially in an S at their limit (radial directions are
  compared as lines, since their axes lie on opposite sides). Not judged (known gaps): overlaps with blends of an
  earlier fillet node, unnamed join edges, non-cylindrical blends within the kernel's 1.5° flag, and picks renamed
  since they were stored (their source edge is not found). Far from the origin (Codex: about 10⁷ mm) f32 display
  coordinates collapse: vertex names are attached through them upstream (`name_edge_ends`), so tiny parts there can
  be refused wrongly (an R0.2 rim on a 0.8 x 0.2 block), and the f32 sample point of the angle check can let a shallow
  overlap pass. Parts for a printer are far inside that range.
  Two earlier rules failed review: "no tangent edge" refused the quarter round, and "fewer than two tangent rails at
  a sharp join" refused the limiting rim. Each check copies the source and result B-reps once; not measured on large
  imports. Reported upstream as #322.
## F-075 QymCAD 0.1.1 drops the splines of a 0.1.0 document on load

- **Version:** v0.1.1-dev.20261026
- **What:** 0.1.0 kept splines in `Sketch.splines` records; 0.1.1 keeps them as `EntityKind::Spline` entities and its
  `Sketch` has no `splines` field, no alias and no migration (`qymcad-core/src/model/sketch_types.rs:13-108`,
  `doc_file.rs` `into_model`, `model.rs:823-836` `from_ron`). serde ignores the unknown field: the document opens
  without an error, the spline's points stay as free points and the curve is gone. Saving writes the loss into
  the file. The server never created splines; GUI-drawn ones are affected.
- **Evidence:** observed: a document written by the 0.1.0 release (`add_spline` + `save_project`, one open spline
  of four points) opens on 0.1.1 with 0 entities and 4 points, no warning (2026-10-24). Test:
  `usability_warnings.rs` `opening_a_0_1_0_spline_says_it_is_lost` (fixture `tests/fixtures/legacy_spline_0.1.0.qcad`).
- **How we handle it:** `Session::open` reads the old records from `document.ron` and warns once per sketch that its
  splines are lost and that saving to a new path keeps the original. The warning lasts for the session and is in
  doc_info. Reported upstream as #323.

## F-076 Circular offsets need CurveDistance for an evaluated formula

- **Version:** v0.1.1-dev.20261026.
- **What:** native circle offsets write `EdgeDistance.expr`, which native `eval_parameters` does not evaluate.
  `CurveDistance.expr` is evaluated. ADR 0022 reserves the latter recipe for the later offset stage.
- **Evidence:** source: `model/sketch/offset.rs:98–106`, `model/params.rs:381–409` in the pinned QymCAD source;
  stage-0 executable probes and exact outputs in `tasks/research-features.md`, section 5.
  Stage 2 `sketch_edit` offset replaces the circle's `EdgeDistance` and an arc's radial endpoint `Distance` with
  `CurveDistance` between the two round curves (`golden_offset_2`; mutation: keeping them fails the app edit).
- **Upstream:** reported as #341.

## F-077 Native splines retain interpolation nodes and automatic handles

- **Version:** v0.1.1-dev.20261026.
- **What:** `add_spline` stores an `EntityKind::Spline` with ordered sketch-point node ids, default automatic
  handles and `Ends`. Nodes weld to existing points within 1e-6 mm. Node pins can carry independent x/y
  formulas and give DOF (0,0). Splines belong in entity inspection, unlike the old omission after the port.
- **Evidence:** source: `model/sketch/entities.rs:669–695`, `model/curve/kinds.rs:470–479`,
  `geom/bspline.rs:426–490`. Tests: `golden_splines_1` checks open/closed interpolation, node formulas,
  persistence and exact app parameter edits. Open-arch area is independently integrated as 5LH/8;
  the four-node periodic fixture uses independent Hermite polynomial quadrature with converged area.
- **Numerical scope:** the open fixture native volume differs by 0.00416 mm³ initially and 0.00665 after
  H=16 (0.02 mm³ local allowance). Closed L20/H10 and L20/H12 fixtures differ by +0.02904 and -0.19759 mm³
  (0.25 mm³ local allowance, below 0.022%). The independent closed integral gives exactly 344 mm² initially.
  Default kernel volume integration is the suspected cause, not an isolated proof: `shape/asks.rs:14`
  delegates to `BRepGProp::VolumeProperties` (`occt_helical.cpp:725–739`). These are fixture tolerances,
  not a universal spline accuracy guarantee.

## F-078 Ellipse arcs and rotated rectangle figures need explicit branch dimensions

- **Version:** v0.1.1-dev.20261026.
- **What:** ellipse constructors return a centre handle for whole ellipses, entity id for arcs, and clamp
  initial semi-axes to 0.01 mm. Arc endpoint angles are eccentric, stored as owned endpoint points plus
  `EllipseSpan` winding. Rectangle `add_rect`/`add_rect_from_centre` store native figure metadata;
  `give_rect_turn_to` replaces its axis-alignment ties with ties relative to the turn dimension.
- **Evidence:** source: `model/sketch/entities.rs:67–101`, `model/sketch/entities.rs:1028–1058` constructors and
  `model/sketch/edit.rs:149–176` (`give_rect_turn_to`); tests `golden_figures_1` compare πab, wh, rotated corner/centre bounds and
  ab(Δt−sinΔt)/2 segment area, before/after global edits, save/open and native app edits/SetDriver.
- **Policy:** unsigned AngleLines cannot hold both mirrored half-planes. Formula turns and eccentric arc
  endpoint angles are restricted to the open range 0..180 degrees; literal turn 0/180 also work. Saved
  formula guards reject unsupported edits in the app as well. Semi-axes below the constructor minimum
  are refused initially. Clockwise and counter-clockwise arcs are tested.
- **Deletion:** breaking a native rectangle removes its auxiliary diagonals and centre along with figure
  metadata (`model/sketch/entities.rs:843–863`). Saved inspection entity lists can contain those diagonals,
  so clients refresh ids after deletion. The existing rollback baseline test now does so.
- **Override semantics:** replacing an arc axis dimension directly leaves its endpoint angle formulas
  reading the original global axis parameter; native SetDriver edits that one driver, not other formulas.
  The arc golden independently converts retained polar directions to the new eccentric angles for its
  expected segment volume. Editing the original global axis parameter preserves the requested eccentric span.

## F-079 Generic curve relations must retain drawn spans and every native tangency row

- **Version:** v0.1.1-dev.20261026.
- **What:** `point_on` chooses PointOnEllipse for ellipses and PointOnCurve for splines; ordinary round
  membership uses its whole circle. `tangency` can append multiple rows, including endpoint membership.
  `curve_ref_drawn` preserves arc spans; `curve_ref_of` denotes whole parent circles/ellipses.
  Saved splines must use native node references, never the serde-skipped solver-only SplineNow variant.
- **Evidence:** source: `model/curve/kinds.rs:522–529`, `model/curve/touch.rs:131–202`,
  `model/curve/refs.rs:12–50,239–262`, `model/sketch/touch.rs:41–48`.
  `golden_curve_constraints_1` verifies ellipse/natural-cubic point ordinates, tangent support lines and
  gap geometry with formulas/persistence/app edits, and strict selector ambiguity/refusal/rollback.
  Its short/long circular and elliptical arc distances differ from whole-parent distances, proving
  drawn-span retention for distance and nearest selection. New generic rows compare rank before and
  after solving and refuse redundant/conflicting relations atomically; legacy implied constraints retain
  their existing no-op behavior.

## F-080 Native dimension targets omit the coordinate axis from sketch identity

- **Version:** v0.1.1-dev.20261026.
- **What:** native sketch targets hold sorted entity references, not the constraint index or its axis.
  Two dimensions on the same reference set (for example x/y centre pins) cannot be distinguished safely.
  SetDriver writes through check/set/name methods, explicitly solves/dirties a sketch target, then runs
  the app parameter-edit path. Literal edits need that explicit dirty marker even if no expression changes.
- **Evidence:** source: `model/params.rs:59–80,124–153,205–209,240–286`,
  `model/formulas.rs:295–319`, app `gui/panels_windows/params.rs:260–286,375–398`.
  `golden_dimensions_1` verifies literal/formula sketch and timeline-feature edits, discovery, cycles,
  collisions including ADR 0021 case twins, geometry rollback and ambiguous-ref refusal. Removing
  explicit sketch dirtying yields V=120 rather than expected 144 in the literal-driver test.
  `common::gui_edit_driver` reproduces the native app path separately from engine propagation.
- **Policy:** dimension_set rejects ambiguous native targets before mutation. Names and formulas retain
  exact case. Imported names, including stale/ambiguous targets, remain discoverable in doc_info.

## F-081 Item formulas conflict with full definition; named item edits can leave stale solids

- **Version:** v0.1.1-dev.20261026.
- **What:** native ellipse axes/turn and polygon turn item formulas move raw point coordinates but add no
  driving rows. Solving a fully defined sketch restores the old dimensional targets. Duplicating the
  item formula and dimensions creates competing edit sources. Polygon side count is different:
  `set_polygon_sides` retains existing points/first side and reconstructs only intrinsic figure ties.
- **Evidence:** source: `model/item_values.rs:193–267,269–361`, `model/params.rs:350–414`.
  `golden_figures_1::native_item_values_do_not_replace_driving_dimension_targets` shows ellipse full
  width 16→24 raw→16 solved, polygon turn 30→60 raw→30 solved, with DOF (0,0).
  `golden_dimensions_1::fully_dimensioned_ellipse_refuses_an_item_edit_that_solver_would_discard`
  proves refusal/rollback rather than reporting a discarded edit as success.
- **Additional app defect:** SetDriver on a Feature target marks only its item point id, not the owning
  sketch. The changed-name scheduler cannot see a new literal formula's own name, so the solid stays stale.
  `free_ellipse_item_formula_follows_globals_but_app_named_item_driver_keeps_old_body` reproduces
  V=113.09733552923257 instead of 150.79644737231007 after named item width 12→16; ordinary global
  parameter edits of the same item formula work. Source: `model/params.rs:277–292,309–326` and app
  `gui/panels_windows/params.rs:260–286` (only Sketch targets explicitly dirty their owner).
- **Stage-1 boundary:** ellipse axes/turn and polygon turn keep fully driving sketch dimension formulas;
  their competing native item-formula creation is stopped pending upstream/design action. New item driver
  names are refused. Imported item names remain discoverable and clearable, their values editable (a rename is refused like a new name); direct server item edits
  explicitly solve/dirty the owner and refuse values discarded by its constraints. This fixes server edits,
  not the pinned app's SetDriver. Options and evidence are in `tasks/review-features-1.md`.
  Reported upstream as QymIs-Tech/QymCAD#337 (2026-10-25).
- **Count policy:** native count imposition rounds, clamps only at three, and can allocate arbitrary counts
  (`item_values.rs:213–215,274–361`). Server initial and prospective edits require integral 3..=64 before
  native allocation. The upstream prospective name-graph evaluator is private (`model/formulas.rs:324`),
  so a bounded dependency evaluation over public parameter/driver formulas preflights counts without imposing
  geometry. `golden_figures_1` covers direct/transitive refusal, count increase/decrease, retained ids and
  retained radius/turn formulas. Item-dependent sketches must be solved and dirtied explicitly; native
  per-name scheduling provides the same owner propagation in app global parameter edits.

## F-082 Tangent-circle construction has no driving radius and chooses tied branches silently

- **Version:** v0.1.1-dev.20261026.
- **What:** `add_circle_touching` chooses the centre nearest the hint, stores both native tangency relations,
  and solves. It does not add a radius driver or reject distinct equidistant centres.
- **Evidence:** source: `model/sketch/touch.rs:18–36`, `model/curve/blend.rs:84–115`.
  `golden_tangent_circle_1` checks centre=(support_x+R,support_y+R), cylinder volume, support/radius edits,
  persistence and app edits, missing/tied solutions and selector ambiguity rollback. Removing the radius
  expression gives centre12 instead of13 after R=3; removing tie preflight accepts an ambiguous branch.
- **Policy:** preflight native `centres_touching`, refuse missing/distinct tied solutions before allocation,
  call the public constructor, then add the independent native radius dimension with the requested formula.
  Radius must exceed native 1e-9 mm. `near` is a one-time branch hint, not a persisted driver.

## F-083 Native trim and extend tie new ends only for lines and round curves

- **Version:** v0.1.1-dev.20261026.
- **What:** `trim_curve` keeps a line's or a circle's new ends on the cutter (`point_on` rows written by the
  specialised keep paths), but the generic path for ellipses and splines (`cut_curve`, then settle) writes no row,
  and `extend_curve_by` uses the boundary only to place the end. Without a tie, a trimmed or extended end does not
  follow its cutter or boundary when a parameter moves it.
- **Evidence:** source: `model/sketch/trim.rs:14–26`, `model/curve/cut.rs:15–89`, `model/sketch/crossings.rs:250–262,
  307–402`. `golden_curve_edits_2::an_extended_end_follows_its_boundary_when_the_boundary_moves` (mutation: no ties
  leaves the end at the old boundary).
- **Policy:** after the native edit, every new or moved end of the edited curve's pieces that lies on another curve
  gets the app's own membership row for that curve kind (`point_on`: PointOnLine, PointOnCircle, PointOnEllipse or
  PointOnCurve), unless an equivalent row exists or the row is dependent.

## F-084 Native extend keeps a dimension-held end

- **Version:** v0.1.1-dev.20261026.
- **What:** `curve_extension` reports `EndKept::Joined` and no growth for an end that is joined or held by a driving
  dimension (`trim.rs:50–79`). Every end the server dimensions counts as held, so `sketch_edit` extend applies to
  ends left free (`dimensioned: false`) and refuses held ends with the reason.
- **Evidence:** `golden_curve_edits_2::extend_free_end_attaches_to_formula_boundary_and_refuses_held_end`.

## F-085 Sketch corner sizes are separate native dimensions per mode

- **Version:** v0.1.1-dev.20261026.
- **What:** `cut_corner_set` writes the size as a `Diameter` (radius, `diam: false`), a `Distance` between the
  touching points (chord) or an `ArcLength` (fillet); a `Distance` along the cut (symmetric chamfer: the cut's
  length, not a setback), two setback `Distance`s (two_dist) or a setback and `AngleLines` (dist_angle). Each takes
  its own formula; storing a radius for a chord or arc-length request would change the requested measure when the
  corner angle changes.
- **Evidence:** source: `model/sketch/blends.rs:152–154,184–208`, `model/sketch_ops.rs:320–369`; `golden_corner_2`
  covers all six modes with app parameter and SetDriver edits (mutation: unwritten formula fails all six).

## F-086 The native point mirror leaves the copy free; the server ties it to its source

- **Version:** v0.1.1-dev.20261026.
- **What:** `mirror_about_point` copies curves with their own size rows but leaves the copy's position free (a
  mirrored triangle adds 6 freedoms). The server ties each copied point to its source with the centre as midpoint
  (`Midpoint`), falls back to keeping it on the line through source and centre (`PointOnLine`) when the copy's
  own dimensions already hold its distance (an ellipse axis end), and gives a copied circle its source's radius
  (`EqualRadius`). Only rows that remove freedom without redundancy are kept. The copy then follows its source's
  position and any formula they share; a size dimension the copy carried stays its own and can be edited alone.
- **Evidence:** source: `model/sketch_ops.rs:49–50`, `model/sketch/edit.rs:526–585`. `golden_mirror_point_2`
  (mutations: no ties, or no line fallback, leave freedom or redundancy).

## F-087 A hole's shift counts from the face's area centroid as the face stands

- **Version:** v0.1.1-dev.20261026.
- **What:** `Hole.shift` (`off_u`, `off_v`) is measured from the centroid of the drilled face as resolved at
  rebuild, along `PlaneFrame::from_origin_normal(centroid, normal, 0)`: u = +X for a face facing ±Z (v = normal ×
  u: +Y on a top face, −Y on a bottom one), otherwise u = Z × normal. The centroid is the area centroid of the face
  as it stands, so another cut-out in that face moves it, and a formula-driven hole moves with it. A shift
  without a formula holds the stored point.
- **Evidence:** source: `model/regen/prep_bodies.rs:326–342`, `feature/frames.rs:44–55,274–288`.
  `golden_sweep_loft_3::a_hole_offset_from_the_face_centre_follows_its_formulas`: a second hole offset (−10, −6)
  on a top face already holding a d4 hole at (15, 18) stands at the holed face's centroid
  ((1200·20 − 4π·15)/(1200 − 4π) − 10, …) = (10.053, 8.968), not (10, 9).

## F-088 A sweep keeps its profile where it is drawn when the path starts on the profile's plane

- **Version:** v0.1.1-dev.20261026.
- **What:** when a path end lies on the profile plane and leaves across it, the profile stays where it is drawn and
  that end starts the path; otherwise the sketch origin is carried to the path's start. A body-edge path is stored
  as the edges' persistent names (`EdgePath { body, edges: Ref::picks }`), so the path follows the body when it is
  rebuilt. Edges must join end to end (`ask::path_edges_join`, a self-gating door, called with no gate held).
  Sweep and loft have no size of their own (no `set_feat_dim` key); their sketches and planes carry the formulas.
- **Evidence:** source: `qymcad-kernel/src/occt_bridge.cpp:3256–3265`, `model/timeline/nodes.rs:234–296`,
  `qymcad-kernel/src/ask.rs:101–110`. `golden_sweep_loft_3` (straight, arc/Pappus, body-edge rail following T,
  ruled and smooth lofts), mutations S5/S6.

## F-089 Loft sections must be closed contours

- **Version:** v0.1.1-dev.20261026.
- **What:** `FeatureKind::Loft.surface` says a sheet's sections need not be closed, but `loft_section_contour` and
  the encoder take only closed contours with at least three points, as the app does. A sheet loft through closed
  sections is built only for a new body (`src` 0); with a source body the solid boolean branch is taken. The server
  allows `surface` only with `new_body`.
- **Evidence:** source: `feature/kind.rs:218–221`, `model/sketch/profiles.rs:316–350`, `model/regen/prep_ops.rs:524–539`.


## F-090 A sketch fillet's centre is laid on any point already standing there

- **Version:** v0.1.1-dev.20261026.
- **What:** `round_any` takes the fillet centre from `point_exactly_at` (`sketch_point_at`, 1e-9 mm), so a centre that
  lands on an existing point shares it. Rounding a corner of an outline offset by the same distance as the radius
  puts the centre on the source outline's corner: its rows already fix that point, and the arc's tangencies become
  redundant (DOF (0, 2)); a centre landing on another circle's centre would share its radius variable. The server
  moves such a point 1 µm aside while the cut is made and puts it back, so the arc gets a centre of its own and no
  existing row changes (an earlier fix that re-pointed rows afterwards also caught rows the cut had only rewritten).
- **Evidence:** source: `model/sketch/blends.rs:120–131`, `model/sketch/pieces.rs:27–29`,
  `model/sketch/entities.rs:960–982`. Live test 2026-10-26 D2;
  `golden_corner_2::a_corner_of_an_offset_outline_is_cut_without_redundancy` (refused before the fix),
  `a_fillet_centre_on_an_existing_point_leaves_that_points_rows_alone` (review round 2).
- **Upstream:** reported as #342.

## F-091 The app holds a broken line's split point on it only for a horizontal or vertical

- **Version:** v0.1.1-dev.20261026.
- **What:** a picked break (`break_line_at`) splits a horizontal or vertical of the line's ends into one per half and
  keeps every other row as it was; a break at intersections (`cut_curve`) keeps that row on the far ends only; a line held by distances or at an angle loses its straightness at the split point, which gets two
  free coordinates. The server puts each new split point on the original line's ends (`PointOnLine`), added only
  where independent, so one free place along the line remains (none at intersections). A split point that already
  existed (the pieces are welded to it) and is held by other rows cannot take that tie; unless it is already tied to
  the line or kept on it by the halves of a split horizontal or vertical, the break is refused, since the line would
  bend when those rows change; a line break that adds redundant rows (split halves over-holding a driven point) is
  refused too.
- **Evidence:** source: `model/curve/cut.rs:91–129`. Live test 2026-10-26 D1;
  `golden_curve_edits_2::the_pieces_of_a_broken_line_stay_on_the_line` (DOF (2, 0) before the fix),
  `a_line_broken_at_a_driven_point_is_refused_rather_than_bent` (review round 2),
  `a_horizontal_line_broken_at_a_driven_point_is_refused_both_ways` (review round 3).
- **Upstream:** reported as #343.

## F-092 A sketch item's values are addressed by its centre point, and the first item about it answers

- **Version:** v0.1.1-dev.20261026.
- **What:** `item_value` and the item formulas key an ellipse's or polygon's values by the id of its centre point;
  `ellipse_value`/`polygon_value` take the first item about that point. Concentric free ellipses share their centre
  (the adder lays it on the point standing there), so an item edit through that centre reaches only the first. The
  server refuses a value edit when more than one item about the centre has that key (a polygon has `sides`/`turn`,
  an ellipse `w`/`h`/`turn`; the arcs of one broken ellipse share its axis points and count once); a name can
  still be cleared.
- **Evidence:** source: `model/item_values.rs:29–60,96–116`. `golden_figures_1::an_item_value_on_a_shared_centre_is_refused`
  (the outer ellipse was resized before the fix), `item_values_on_a_shared_centre_are_set_where_one_item_answers`.
- **Upstream:** reported as #344.

## F-093 A body's export mesh repeats every point once per face

- **Version:** v0.1.1-dev.20261026.
- **What:** `kernel::ask::export_mesh` → `Shape::tessellate_merged` concatenates the faces' own triangulations, so the
  nodes of an edge appear once in each face beside it, with separate indices. 3MF, OBJ and GLB keep those indices, and
  a slicer then reads every edge between faces as open (Bambu Studio: "680 non-manifold edges" on a 756-triangle key).
  On every part checked the copies were bit-identical (the golden fixtures and the door hanger parts, curved faces
  included); the triangulation itself runs in the C++ bridge, so this is observed, not guaranteed. Joined exactly,
  they leave a few zero-area triangles with two corners at one point, laid along the degenerate edge at the pole of a
  rounded corner. The server joins the copies, drops those triangles, and gives a point once per fan of triangles
  around it: triangles hang together across an edge of two, and around an edge of more they are paired by the wedges
  of solid between them (faces lying on each other hold none). Solids of one body that only touch, and a surface
  closing on itself along an edge, so stay manifold (`export.rs` `weld`, `wedges`). The app's File > Export takes the same `export_mesh` with no join
  (`gui/io_jobs.rs` `write_mesh_to`; confirmed with the same calls on a box: 24 vertices, 24 of 30 edges open). Reported upstream as #346.
  This follows the 3MF Core Specification 4.1: every edge is shared by exactly two triangles, and vertices are
  duplicated only where joining them would make an edge non-manifold.
- **Known limit:** a solid pinched along an edge (zero thickness along a line, e.g. two cubes standing diagonally
  between two plates) keeps that edge with four triangles: both ends of it are single fans, so splitting points per
  fan cannot separate the wedges. The B-rep is itself non-manifold there; a slicer's repair fixes it. Resolving it
  needs the triangles re-paired across the gap with both ends duplicated (ignored test
  `export::tests::welding_a_solid_pinched_along_an_edge`).
- **Evidence:** source: `qymcad-kernel/src/shape/faces.rs:169–212`, `qymcad-kernel/src/ask.rs:46–49`.
  `golden_export::mesh_exports_are_closed_manifolds` reads 3MF, OBJ and GLB indices as written and STL joined by exact
  coordinates: every edge has two opposite triangles and every point one fan. Before the fix every edge between faces
  failed; joining alone left 16 doubled edges at the rounded block's corners. `export::tests::welding_*` cover solids
  touching at an edge, at a corner and face to face, four solids around one corner (every base edge of four triangles),
  a ring of cubes touching itself along an edge, and the pole slivers. Welding a million triangles takes about 0.25 s (release build).

## F-094 Native threads use material-side detection and may retain a stock skin at large print fits

- **Version:** v0.1.1-dev.20261026.
- **Recipe:** public `add_thread` creates a native subtractive feature. Its eleven numeric keys are
  `length`, `lead_in`, `lead_out`, `nominal`, `pitch`, `fit`, `crest_r`, `root_r`, `angle`, `depth`, `starts`.
  The app seeds fit=.2 and entry/exit runouts=1.5; ThreadSpec::default instead seeds fit=0.
  Source: `qymcad-core/src/model/timeline/add_ops.rs:579–591`, `feature/kind.rs:716–728`,
  `thread.rs:128–130`; app `qymcad-ui-state/src/lib.rs:116–118`, `qymcad-part/src/apply/solids.rs:189–201` and stage-0 source trace in
  `tasks/research-print.md` §1. Native regeneration decides the internal/external side from stock mesh
  (`model/regen/prep_bodies.rs:86–98`), regardless of the stored boolean. Public cylinder helpers allow
  the creation gesture to detect and persist the matching side without duplicating private preparation.
- **Boundary:** native starts evaluation rounds/clamps; the engine checks integral 1..64 before every
  rebuild. `MeshEdge::is_circular` includes arcs (`geom/mesh.rs:392–396`), so a thread rim also requires
  coincident endpoints, matching topology's full-circle classification. An unguarded half-disc arc builds
  a green half-thread. Evidence: `golden_thread_print::a_circular_arc_is_not_a_full_thread_rim`.
- **Fit geometry:** basic ISO diameters remain basic values. Groove widening caps at .49P; excess fit
  yields radial relief `max(0,fit-room)/tan(half_angle)`, where `room=max(0,.49P-(P-crest_flat)/2)`.
  Source: `thread.rs:399–433,506–516` (overshoot at `thread.rs:510`), `qymcad-kernel/src/occt_helical.cpp:225–249`. Relief shifts the
  cutter; narrow stock lands remain at their original diameter. In external M8/P1.25 fit=.2, relief
  .232744 exceeds cutter overshoot .15P=.1875: a section can cross an intact outer skin and an enclosed
  helical groove. This corrects stage-0's unmeasured assumption that the entire crest shrinks by 2*relief.
  This is an upstream geometry limitation, **not reported upstream yet**. The server retains the public
  adder and caps the default fit .2 so public `ThreadSpec::radial_relief()` stays <=.13P, leaving .02P of
  cutter overshoot exposed. Explicit fits above that limit and later edits are refused. For external M8
  the exact limit is `.0525P + .13P*tan(30°) = .15944441874331418 mm`; ADR's approximate .17 is too high
  for the stated margin. The raw native recipe remains covered separately from the safe server default.
  Native success still does not prove that a printed pair mates.
- **Tests:** `golden_thread_print::{metric_thread_sections_match_hand_dimensions,
  trapezoidal_thread_sections_match_hand_dimensions,internal_thread_detects_side_and_cuts_the_bore,
  upstream_metric_fit_relief_leaves_a_stock_skin_over_the_groove}` measure real mesh sections against
  hand-derived ISO/Tr groove depths, pitch periodicity, stock lands and relief equations. Parameter/formula,
  saved-file and unmodified app edit paths are covered by
  `thread_formulas_persist_and_follow_server_and_app_edits`; all native keys by
  `every_thread_key_is_reachable_by_dimension_set`; F-074 by
  `fillet_beside_a_threaded_rim_is_checked_before_and_after_thread`.

## F-095 Split output count and every output body's identity belong to the native recipe

- **Version:** v0.1.1-dev.20261026.
- **Source:** `model/timeline/add_ops.rs:421–435` allocates exactly the measured count, with the first body id
  also the feature id. `model/regen/prep_ops.rs:366–377` gives base operation normals XY +Z, XZ **+Y**, YZ +X;
  XZ differs from the sketch frame. `qymcad-kernel/src/ask.rs:16–20` measures pieces under its own kernel gate.
  Native regeneration refuses a changed count before landing outputs (`model/regen/prep_bodies.rs:414–459`,
  `qymcad-kernel/src/kernel.rs:756–784`). The app refuses direct count-changing edits for the same reason:
  downstream references already point to those ids (`qymcad-part/src/apply/update.rs:386–402,562–568`).
- **Integration:** native body names use `set_mesh_name` (`model/bodies.rs:209–213`), and all-output traversal
  uses `FeatureKind::bodies()`. Resolving ids/names, detecting missing B-reps on open, reporting and selected
  exports must include secondary bodies. The existing native deletion frontier already includes every output
  (`model/timeline/delete.rs:13–38`). The server keeps its existing tip-body default; use an explicit body
  after a split. Default piece aliases use the feature name and skip occupied positive suffixes;
  explicit aliases still refuse collisions (including ambiguous aliases). `review_round3_codex` verifies
  three successive unnamed splits and all six name resolutions plus cylinder piece volumes.
  Count-changing parameter edits restore all shapes, formulas, names and diagnostics atomically.
- **Tests:** `golden_split_print` verifies box volume conservation; U volumes1280/400/400 and the y12 edit;
  both count-change directions; native app edits; missing-secondary-B-rep recovery; native names and every id;
  selected STL/3MF/STEP independent volume/bounds readback; cap fillet; deletion and undo. `print_split`
  covers schema, named/numeric piece targets, strict plane arguments, per-piece export, batch and undo.
  Two production mutations (first-output-only open check; literal instead of offset formula) each fail a
  named test; evidence is `tasks/split-test-evidence.md`.
- **Placement:** preflight matches the stored native operation in the B-rep's local frame. Assembly-specific
  world-plane preview conversion is not added; applying it only to preflight would disagree with native rebuild.

## F-096 Native text stores baked glyphs and literal placement; counters are separate contours

- **Version:** v0.1.1-dev.20261026.
- **Source:** `qymcad-core/src/text.rs:86–118` outlines glyph loops, and silently skips an unsupported letter
  (`:102`). Public `family_name`/`can_write` (`:132–150`) validate the exact face and character mapping only.
  Mapping does not guarantee outlines: `:104` silently skips mapped bitmap-only glyphs too. The server
  requires each non-whitespace character to produce contours with the same native outliner; it lists
  failures before mutation. The synthetic mixed font in `tests/fixtures/glyph-gap.ttf` maps vector A and
  bitmap-only B; `review_round3_codex::mapped_bitmap_only_glyph_is_refused_atomically` proves AB is refused
  though `can_write` succeeds and the aggregate outline equals A. Its stdlib generator is adjacent.
  The app rotates each baked vertex about the anchor (`qymcad-ui-state/src/text.rs:50–57`)
  then calls `add_sketch_text` (`model/sketch/entities.rs:573–595`). `FontRef` and `SketchText` serialize path,
  face, family, literal placement/height and glyphs, without font bytes (`model/sketch_types.rs:484–525`).
- **Policy:** fixed numeric height, anchor and angle, deterministic Arial face0 at
  `/System/Library/Fonts/Supplemental/Arial.ttf`, or an explicit file/index; missing fonts and glyphs are errors.
  Reopening/rebuilding uses the saved loops without reading the font. Retyping needs its recorded path.
  Height is an em size, not the visible capital height. Construction text contributes no profiles.
- **Profiles:** empty native profile picks mean the first contour, and selecting all contours fills counters
  (`model/sketch/profiles.rs:222–243`; `qymcad-testkit/tests/text_of_any_font_extrudes.rs:58–66`). Existing server
  root-contour selection already picks every glyph outer with its holes; no special extrusion path is needed.
  Native `delete_sketch_text` takes a text index (`model/sketch/entities.rs:646`); the server resolves the exposed
  text id before calling it.
- **Tests:** `golden_text_print` computes shoelace areas with nesting parity directly from baked loops, then
  compares extrude/add/cut volumes. Rotation bounds, counters, Cyrillic, missing fonts/glyphs, construction,
  removal rollback, saved glyphs without a font, depth app edits and a glyph-edge fillet are covered.
  `print_text` checks numeric schemas, ADR refusal messages, info/remove ids, strict nested font arguments,
  all-or-nothing mixed additions, save/open/depth edits and batch/undo.

## F-097 App text-height edits can exchange glyph and counter contour identities

- **Version:** v0.1.1-dev.20261026; **reported upstream as #347**.
- **Cause:** text glyphs enter contour reconstruction with empty entity provenance
  (`model/sketch/solve.rs:469–487`, especially `:483`). `replace_contours` therefore falls back from provenance
  matching to greedy centroid distance plus area difference (`model/sketch/contours.rs:48–120`). A height edit
  scales every baked vertex correctly (`model/item_values.rs:200–210`), but another letter/counter can inherit
  the old contour id. Extrusion retains those old ids and builds the wrong regions without errors.
- **Evidence:** native `ABCDEFGHIJ`, Arial height10, depth2: area151.369854435325, volume302.739708870649.
  Native app-path H10→20 should give1210.958835482598, but gives1301.361804455519 with errors empty. It happens
  with an existing or missing recorded font. Even H10→11 exchanges some outer ids although aggregate volume
  happens to remain correct. Stage-0 probes are retained in `tasks/research-print.md` §3.
- **Boundary/test:** ADR0024 exposes fixed numeric text geometry and tells callers to recreate it to resize.
  Review regression `review_text_fix::review_text_height_edit` reproduced the same wrong volume through
  the server's generic Feature/item target before the fix. The server now refuses direct text-height
  writes and compares surviving text records across edit boundaries; inherited parameter/named-driver
  height formulas and stale height evaluation on open are refused atomically. Other text value paths
  were audited in F-102; accepted dependent edits are compared with independent fresh builds.
  `golden_text_print::upstream_native_text_height_h10_to_20_changes_saved_profile_identity` deliberately builds
  the raw native height formula and remains ignored in the normal suite. Explicit execution must fail on this
  pin. Re-picking roots only in the server would not repair a later app edit, so no such workaround is added.

## F-098 App SetDriver on a named text height changes metadata without dirtying its sketch

- **Version:** v0.1.1-dev.20261026; **not reported upstream yet** (text-specific reproduction).
- **Cause:** text height is a Feature item target, not a timeline node. `set_feat_dim` marks only that id
  (`model/params.rs:277–292`); the app's SetDriver explicitly solves/dirties only Sketch targets
  (`qymcad/src/gui/panels_windows/params.rs:260–286`). Changed-name scheduling cannot find the owner from a
  newly literal formula (`model/params.rs:309–326`). This is the text instance of F-081's item-owner gap.
- **Evidence:** the stage-0 native named height edit10→15 yields height15 and dirty=[] with the old solid
  volume; expected volume is2.25 times the old volume. `tasks/research-print.md` §3 and retained text probe
  outputs document the exact public SetDriver/evaluation/scheduler sequence. The passing characterization
  `golden_text_print::upstream_native_named_text_height_setdriver_changes_metadata_without_dirtying_solid`
  reproduces metadata15 with the old solid; `text_item_driver_name_is_refused_atomically` verifies server
  refusal. Ordinary extrusion-depth/global feature edits rebuild correctly.
- **Boundary:** ADR0023's refusal of new item-driver names remains in effect for text. Fixed text creation
  stores no height formula or invented placement driver. Neither the scheduler nor the native app is patched.


## F-099 ISO internal thread prep can bury a groove behind an undersized bore wall

- **Version:** v0.1.1-dev.20261026, f25f27d; upstream report pending.
- **Source:** `qymcad-core/src/model/regen/prep_bodies.rs:94–106` uses `geometry_in(Some(face_d))` and
  starts an ISO internal cutter at max(actual rim radius, D1/2), with D1 = D − 1.082532·P. Native accepts
  bore diameters [basic minor − .5·P, 1.05·D]; external stock accepts D ± max(.05·D, .5·P). The cutter
  has .15·P overshoot (`thread.rs:510`); `occt_helical.cpp:246–249` additionally translates its radius
  by public `ThreadSpec::radial_relief()` when relief >1e−6 mm (outwards internally, inwards externally).
- **Exact exposure interval:** let e be relief above that translation threshold, otherwise zero. The
  required .02·P exposure is `max(0,D1/2−r)+e <= .13P`. Alongside the independent fit guard e<=.13P,
  ISO internal rim diameters must lie in [D1−.26·P+2e, 1.05·D], intersected with the native size range.
  All other cutters start at the actual rim; non-ISO internal and external retain their native intervals
  above under the fit guard. Oversized external stock has no mirrored skin defect. Native checks on
  span, depth, minimum pitch and turns remain in effect.
- **Evidence:** review bore Ø6.1, annulus Ø16, M8/P1.25 fit.2 accepted but a ray at z6, angle.173 crossed
  radii [3.048990,3.231851,3.988098,7.998515]: intact bore wall plus enclosed groove. Independent basic
  diameter/relief equations give a safe minimum6.516690716 mm for this profile/fit. `review_thread_fix`
  checks ±.001 mm on both sides of all four internal/external interval boundaries; accepted sides have
  exactly 2 annular or 1 external radial crossings at two angles and 40 axial samples per pitch.
  Creation and post-regeneration guards use evaluated formulas/current source edges, so stock, nominal,
  pitch and fit edits cannot bypass the interval. Tests verify atomic recipe/B-rep rollback. Disabling
  only the post-regeneration open-margin check makes the stock-edit regression fail. Final edit acceptance
  repeats thread/split guards independently of the callers' exemption for previous node errors; a prior
  native error cannot hide a new refusal.

## F-100 Native split ordering can exchange same-side pieces without changing their count

- **Version:** v0.1.1-dev.20261026, f25f27d; upstream QymCAD#350 (2026-10-27).
- **Source:** `qymcad-kernel/src/kernel.rs:769–784` sorts pieces by their bounding-box centre projected
  onto the operation-plane normal, then zips them with fixed recipe ids. Separate pieces on the same
  side have no additional identity matching. `regen/prep_bodies.rs:414–459` checks count only.
- **Evidence:** reviewer U profile has left leg x[-10,-6], end y20, right leg x[6,10], end parameter22;
  split XZ offset10 produces bridge/left/right. Editing right_end22→18 retains3 pieces but native
  assigns the old `left` id to x[6,10] and the old `right` id to x[-10,-6]. `review_split_fix` reproduced
  the accepted swap before the fix and now checks a refusal naming both pieces and exact rollback.
- **Server boundary:** every rebuilt existing split compares all previous/new local B-rep boxes,
  including consumed outputs. Intersection-box volume is the geometric score; native ids must uniquely
  maximize the sum over a one-to-one assignment, with positive overlap for every retained id. Any
  alternative permutation decomposes into cycles; max-plus Floyd–Warshall detects nonnegative cycles
  of `score(i,j)−score(i,i)`, including three-way swaps. Zero/near-zero cycles are ambiguous and refused.
  This avoids greedy row matching (which can miss a global swap) without repeated geometric booleans.
  A changed/ambiguous assignment names affected pieces and rolls back through edit snapshots. Bounds
  can be ambiguous for complex interlocking pieces, and large moves can lose all overlap; these edits
  conservatively require recreating the split. Creation/missing-output recovery cannot compare absent
  previous geometry. Native app edits remain unguarded. Offset10→15 in a box and split/stock movements
  without reordering still pass. The production guard-disabled mutation accepts the U swap and fails
  the regression; the restored new tests and9 existing split golden tests pass. A saved wrong-count
  recipe can already have an error on the split node; `a_preexisting_split_error_cannot_exempt_an_identity_swap`
  verifies that repairing the count while swapping limbs still refuses atomically, while a safe repair passes.

## F-101 Custom thread angles at or below one degree silently become sixty degrees

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Source:** `qymcad-core/src/thread.rs:321` uses `custom_angle` only for Custom with angle>1.0;
  otherwise it uses the standard's default60°. The requested number remains stored.
- **Boundary/evidence:** `review_thread_fix::review_custom_angle_is_kept` stored.5 but observed60
  before the guard. Creation and evaluated edit/open checks require custom angle>1° and <180°.
  Tests cover.5, exactly1, 1.000001, parameter/dimension edits and atomic rollback. Relaxing the
  lower guard to0 makes the effective-angle test fail again.

## F-102 A text id exposes only numeric height and is not a constraint selector

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Source:** `model/item_values.rs:54–56` exposes only `height` for text; imposing item values first
  calls that getter and ignores unsupported keys. `SketchText` stores literal position/angle/font/string
  separately from sketch points/entities (`model/sketch_types.rs:507–525`). There is no item formula or
  sketch constraint selector for those values.
- **Evidence/boundary:** `review_text_fix` checks direct height/angle/turn/x/y/at/font/font_index/text
  item targets and distance/angle/diameter/fix constraints on text ids. Height writes explicitly refuse
  fixed text; unsupported values/selectors refuse as unavailable. Imported height formulas reached by
  parameter or another named dimension are also refused, as is stale height evaluation on open. Existing
  text height/anchor/angle/font/characters/glyphs are compared at edit boundaries; additions/removal are
  allowed. Depth parameter/dimension changes and an independent construction-circle constraint preserve
  those records and match volume/bounds of a fresh build. The comparator-disabled production mutation
  reopens the indirect parameter bypass and makes its regression red. F-097 explains why recreation
  is required; these guards cannot prevent subsequent edits in QymCAD.app.

## F-103 Native thread geometry silently replaces several accepted inputs

- **Version:** v0.1.1-dev.20261026, f25f27d. Found by the round-2 review and its audit (2026-10-27).
- **Source:** `qymcad-core/src/thread.rs`: angle used only when custom and above 1 (:321), half-angle clamped
  to 1.3 rad (:508), root half-width clamped at zero for deep profiles (:516; the public `profile_overflow`
  omits fit), crest radius clamped (:529), root radius clamped and snapped to the full radius within 1e-6 P
  (:554-575), pitch at or below 1e-9 replaced by the standard pitch (:301), buttress built as a symmetric
  52 degree profile (:75, :350-355). `qymcad-kernel/src/occt_helical.cpp`: the cutter side inferred from
  profile extents `py1 > -py0` (:222), which flips for profiles shallower than the 0.15 P overshoot and moves
  the fit relief the wrong way (:248) (upstream QymCAD#353); runouts at or below 1e-6 dropped and runouts above 0.95 of the length
  scaled (:134-136, :676-677).
- **Server boundary:** every such input is refused at creation and on every evaluated edit; explicit crest/root
  radii are checked only against arcs of their own kind. In groove traversal (:579–600), crest arcs lie
  between overshoot walls and flanks (ccw), root arcs between flanks (cw); internal mirroring reverses
  both y and winding (:615–624). An equal root radius must not validate an absent/clamped crest or vice
  versa. `review_round3_codex` measures arc positions independently and tests both false acceptances,
  valid equal radii on both sides, and atomic edit refusal. ISO internal geometry alone depends on the
  actual bore (:363–370); pre-regeneration checks defer that profile to the current source-rim check
  after regeneration. M8/P1.25, bore6.75, crest.26 passes and matches fresh native geometry although
  nominal geometry clamps to .248982303588. Actual crest.3 or bore6.6 clamps are refused. Tests measure
  the built profile/mesh and compare fresh builds; non-ISO profiles retain early geometry checks.

## F-104 A plain number set on a feature keeps only 4 decimals natively

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Source:** `qymcad-core/src/model/params.rs:203` writes a plain feature number through `expr::fmt_num`
  (`expr.rs:346`, `{v:.4}`). 1.234567891234 is stored as 1.2346 and 1e-6 as 0.
- **Numeric strings:** `set_dim_target_text` (:242) trims then parses with `parse::<f64>()`, so numeric
  strings take the same rounded path. Its separate formula checker rejects exponent notation even though
  the numeric path accepts it. The server uses exactly that trimmed numeric classification, canonicalizes
  finite literals to round-trippable decimal text, checks the target, and stores full precision directly.
  True expressions such as `1/1000000` still use the native formula path. `review_round3_codex` verifies
  string runout refusal, precise length, whitespace/sign/exponent forms and retained formula text.

## F-105 Native thread cutter diameters are not always the side-specific basic reference diameters

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Native source:** `qymcad-core/src/thread.rs:324–376` distinguishes internal ISO minor only; Tr reports
  external d3 on either side, ACME includes .01P cutter backlash in radial depth, and Rd omits internal
  crest/root clearance. Custom defines depth h (default .6P), pitch d-h, minor d-2h; it has no ISO standard.
- **Independent references:** [ISO 724:2023 §5](https://cdn.standards.iteh.ai/samples/85104/bd3a2c169cec4db3a4579ae25ff16daf/ISO-PRF-724.pdf),
  [ISO 2904:2020 §5](https://cdn.standards.iteh.ai/samples/78729/7d158726108b478c889fa887d460ba78/ISO-2904-2020.pdf),
  [NBS H28 Part III, ACME basic profile and clearance sections](https://nvlpubs.nist.gov/nistpubs/Legacy/hb/nbshandbook28supp1957pt3.pdf),
  [EMUGE-FRANKEN DIN 405 table §15.15](https://www.emuge-franken-bg.com/attachments/article/97/15%20Gewindetabellen.pdf).
  With nominal d and pitch P, basic major/pitch/minor references are:

  | Family | External | Internal |
  | --- | --- | --- |
  | ISO metric | d, d-3√3P/8, d-17√3P/24 (rounded root d3) | d, d-3√3P/8, d-5√3P/8 (D1) |
  | ISO Tr | d, d-P/2, d-P-2ac | d+2ac, d-P/2, d-P |
  | ACME | d, d-P/2, d-P | d, d-P/2, d-P |
  | DIN Rd | d, d-P/2, d-P | d+.1P, d-.5P, d-.9P |
  | Custom reference | d, d-h, d-2h | d, d-h, d-2h |

  Tr ac is .15 for P≤1.5, .25 for P≤5, .5 for P≤12, otherwise 1 mm.
- **Server boundary/evidence:** reporting corrects Tr internal, ACME and Rd internal independently of the
  pinned native cutter. Internal major means reference root diameter; minor means reference crest diameter.
  These numbers exclude modelling fit and are not measured cavity limits. `review_round3_diameters` checks
  all five accepted families on both sides against the formulas; `review_round3_codex` verifies internal
   Tr16x4 reports (16.5,14,12) and measures the actual zero-fit root diameter as 16.5000024 mm.

## F-106 Source-restored imports must be named before topology is exposed

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **What:** exact STEP/IGES parsing returns positional face/edge ids. If an import body has no saved B-rep,
  restoring the shape directly from its embedded source leaves saved selections unmatched (and can direct a
  modifier at the wrong edge). The bridge's silent naming assigns face names from their places in the document
  name table and edge names from adjacent named faces.
- **Evidence:** upstream source `crates/qymcad-kernel/src/imports.rs:86-106` calls
  `naming::silent` after source/solid/scale restoration; `naming.rs:77-92` derives placed faces and edges.
  The engine reproduces upstream `named_as_they_came_in` without calling `raise_imports`: that public helper stages
  the source with `fs::write` under a predictable temporary name, takes the file extension from the document and
  parses and scales outside the gate (a crash risk with concurrent kernel use, F-069). The engine keeps its own
  staging (`create_new`, 0600, normalized extension, parse under the gate) and then names each restored body with
  `OcctKernel::name_silent` under one gate of its own. Upstream's private `unminted` step (names the document does
  not hold go back to positional numbers) cannot be called; instead, a body whose names the document does not hold
  keeps its parsed positional ids, with an advisory warning when the document knew some of them.
  `crates/engine/tests/imports.rs` STEP/IGES source-only regressions compare saved face
  and edge ids and resolve each through `Session::select`. Before the fix each format restored edges as 1..12
  rather than the saved 16106127xx names; after the fix both pass. ADR 0019 source validation, warnings, and
   stored-B-rep authority remain unchanged.

  Tr16x4 reports (16.5,14,12) and measures the actual zero-fit root diameter as 16.5000024 mm.

## F-107 Sketch projections rebuild consumers outside the native dependency plan

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **What:** `Project::dependents` omits `Sketch.projections[].body`
  (`qymcad-core/src/model/timeline/delete.rs:13–42`). Native regeneration nevertheless follows projections
  unconditionally in timeline order and dirties consumers when projected geometry moves
  (`model/regen/run.rs:580–594`); `regen_plan` does not account for that edge (`:122–160`). Thus a successful
  parameter edit can move the cut correctly while a failed edit restores its Project but leaves changed B-reps.
  Projections see the source's consuming lineage **above** the sketch, rather than just the stored body id
  (`model/projection.rs:245–263`, private `body_seen_from`).
- **Disappearance is conditional:** a missing edge on a live source is `Gone`, releasing the association while
  retaining geometry and constraints. No live source edge geometry is `Unknown`: the association stays,
  including when the source feature was deleted and its B-rep removed. Source comments suggesting every
  deleted body detaches are broader than the implementation (`projection.rs:189–213,299–323`).
  `golden_projections::native_missing_edge_detaches_but_absent_body_retains_projection` observes both cases.
- **Engine policy:** `dependencies` extends native reachability through projections to a fixed point, including
  stored sources, effective source lineage, all native outputs and consumers. Parameter, sketch and dimension
  snapshot/retry plans and both halves of F-024 preflight use it. Deletion's structural closure uses it too;
  deleting a projection source refuses before changing anything, even with `cascade=true`, naming projecting
  sketches to delete explicitly first (ADR 0025).
- **Evidence:** `crates/engine/tests/golden_projections.rs` builds a public `Project::add_sketch_projection`
  of A's top Y-edge onto a fixed datum sketch. A constrained radius-1, centre-span-10 slot cuts 2 mm into
  independent 80×30×10 stock B: V = 24000 − 2(20 + π). Changing A's centred width 40→60 moves projected
  points, slot centres and pocket floor x=17→27. Save/open plus `common::gui_edit_param_faces` reproduces
  native app-path pocket movement and volume. A cached downstream extrusion with zero height forces failure
  after that cut rebuilds. Parameter, sketch and dimension regressions compared exact serialized Project and
  every live B-rep; each failed on B-rep bytes before the fix. Another regression edits a consuming push-face
  operation above the projection while its stored source remains A. F-024 regression refuses a stored fillet
  query over the cut when A has unnamed live edges. Source deletion tests both cascade modes and exact state.
  Commands, failing-before output and independent restored mutations: `tasks/cover-1-evidence.md`.

## F-108 Body booleans have dynamic outputs and both recipe inputs

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Native recipe:** `Project::add_body_boolean(a,b,op)` stores a parametric BodyBoolean; op 0 subtracts B from A, 1 unions and 2 intersects. Both operands are consumed and `FeatureKind::inputs()` includes both. Cut/intersect can split a single-solid A into output bodies; union stays one body even when disjoint solids form a compound. A failed no-op cut may retain pass-through geometry while recording a rebuild error; an empty intersection has no output. Source: pinned `model/timeline/add_ops.rs:257–263`, `feature/kind.rs:616–626,895–957`, `model/regen/prep_bodies.rs:357–408`, `model/regen/run.rs:398–409,661–687`.
- **Gate and failure policy:** `ask::boolean_gives_body` is a separate optional trial and acquires `kernel_gate()` itself (`qymcad-kernel/src/ask.rs:37–40`). The engine does not call it: native regeneration invokes the same checked boolean operation (`model/regen/run.rs` → `Shape::boolean_checked`), so preflight duplicates kernel work. `atomic_with_post` checks `RegenReport.errors` and restores recipe/shapes on failure, including no-op pass-through shapes (F-008).
- **Server boundary:** `body_boolean` requires two distinct current bodies in the same part; it creates the node inside the operands' owner component and restores the prior active part. It preserves both-input parametric dependencies. Every output body gets a name that resolves through id/name and is reported; omitted names use the first available `<feature> N` suffix. Parameter edits that change output-body count, solid count, or create an ambiguous global box-overlap identity assignment are refused atomically. This is server-side policy; the native app retains dynamic piece ids and greedy centre matching. Union compound solid count is guarded even though the body count is always one.
- **Evidence:** `tasks/cover-2-evidence.md`; `golden_body_boolean` independently checks overlapping box volumes/bounds for all ops, no-op cases, named split outputs, count-change rollback, and edits to both operands after save/open through the app-path emulator. `golden_review_r1` checks exact rollback after no-op rebuild refusal, first-available default names, explicit union-name reporting, and 508 mm³ imported-part union with active-part restoration. `import_aliases` covers imported aliases in topology/select/fillet/boolean/batch and collision refusal when the imported alias also names a timeline object. `tasks/fix1-report.md` records baseline observations and mutation status. The verified upstream probe is `tasks/research-cover/boolean/output.txt`.
- **App tree:** the app collapses a node and the union right after it into one tree row when the union's tool `b` is that node's only output and the node took no body as input (`qymcad-core/src/model/timeline/state.rs:259-283` `feature_op_span`, used by `qymcad/src/gui/panels_tree/build.rs:280-306`). It is meant for revolve/sweep/loft plus `finish_base_body`, but it also hides a server `body_boolean` union made straight after extruding its new tool body: GUI check 2026-10-29 (`tmp/gui-check-cover/REPORT.md`, `import_adapter.qcad`) showed AdapterBlock with AdapterUnion only as its dependent and no AdapterUnion row, while geometry, the cut built on it and parameter edits were correct. The node is stored normally (owner part, inputs, body); this is display only, and the server does not reorder the timeline to avoid it.

## F-109 Server projection selections and attached/detached inspection

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **What:** the app's projection command accepts an edge or a hosted face, and native projection records drive their derived entities. A missing edge on a live body releases its association while leaving ordinary geometry; a source with no live B-rep remains unknown and keeps its association (F-107). For new projections the server additionally requires the source body to be owned by the sketch's part and produced above the sketch in timeline order. Native `projected_curves` reads local source geometry without applying component placement; native regeneration visits sketches once in timeline order. `Session::open` checks stored projections for forward references, preventing an external timeline reorder from silently reviving stale geometry. Stored cross-part projections remain accepted in the source part's local coordinates, matching the native app; no placement transform is applied. Each record produces a rebuild warning with sketch name/id, projection id, body name/id and both owner parts, stating "projected in the source part's local coordinates; its placement is ignored, as in the QymCAD app". `sketch_info` marks associated cross-part entities with `cross_part: true`; same-part and ordinary entities omit it. Creation policy, warning and inspection use the same owner comparison. Warnings are derived from current records and persist across rebuilds without surviving record deletion.
- **Evidence:** upstream `crates/qymcad-sketch/src/extend.rs:8-32`; `crates/qymcad-core/src/model/projection.rs:83-105,186-213,295-324`; `crates/qymcad-core/src/model/regen/run.rs:580`; `golden_projections.rs` verifies the native missing-edge and absent-body distinction. Engine APIs reject stale topology ids, empty result, a face not hosting the sketch, cross-part or forward projections and unsolved/rebuild errors before successful commit. `golden_review_r1` tests atomic refusal for forward and placed cross-part scenarios, and open-time refusal of stored forward projection records. Source-proposed GUI placement repro, not GUI-executed: `tasks/fix1-report.md`. MCP `sketch_project` resolves each `SelArg` through topology before the edit, deduplicates edge ids and returns projection, entity and point ids. `sketch_info` marks attached entities driven and reports detached geometry after regeneration; source-less live association is unknown. Engine tests independently check a 40×30 face outline as [40,30,40,30], a circular edge radius of 5 mm matching the independently made hole, projection idempotence, parameter-following slot/cut and save/open app-path emulation. MCP registry, stdio protocol and batch/undo regressions cover the surface. Evidence: `tasks/cover-3-evidence.md`.

- **Inspection evidence (stage 3 continuation):** `session::tests::missing_live_projection_edge_is_reported_detached_in_session_detail` removes a real circular hole from a source profile: the edge vanishes, native records release, and `sketch_info` reports the retained circle as detached with stable provenance. Failed edits and undo restore metadata with recipe/B-reps. `projection_add_captures_detachment_before_rebuild` covers native resolution during insertion; unknown-source inspection leaves ordinary geometry unmarked. Metadata belongs to the Session: a file saved after native release has no projection record to reconstruct, so reopens as ordinary geometry. The app projects a hosted face using `live_body(host)` (`extend.rs:15`); `face_projection_requires_unique_host_face_and_uses_live_host_body` covers a consumed host with a live consumer. Slot movement is also checked through real stdio (`protocol::projected_edge_drives_slot_and_cut_after_parameter_edit_over_stdio`) and the native app path now returns the Project to assert projected point and slot-centre positions as well as the pocket floor.

- **Forward-source regression evidence:** `golden_review_r1::r1_1_forward_edge_projection_is_refused_atomically` and `r1_1_forward_face_projection_is_refused_atomically` create the XY sketch before the source extrusion, call `Session::sketch_project` and compare exact serialized recipe/live B-reps. MCP `projection_order::r1_1_forward_projection_tool_refuses_edge_and_face_without_history_changes` calls the real registry and compares every saved `.qcad` entry byte-for-byte, including recipe/B-reps, and preserves the undo head. Removing only the creation-path order check makes both engine cases and the MCP regression fail; restored tests pass (`tasks/fix1-logs/mut-1.log`, `mut-1-restored.log`). The separately named open-path tests cover externally stored forward references.

- **Stored cross-part regression evidence:** pinned `model/projection.rs:195–203,297–318` resolves source-local edges into the sketch frame, without component placement. `golden_review_r2` recreates the reviewer's public-native projection fixture: a 10×10×5 IGES part placed at world X=95..105 projects four top edges at local X=-5..5 into another part. Open reports four warnings and inspection marks all four attached entities; a subsequent extrusion remains local X=-5..5 with V=200 mm³. Same-part inspection stays unmarked and warning-free. Actual MCP stdio and independent RED/restored GREEN mutations: `tasks/fix2-report.md`.

- **Undo diagnostics:** `golden_review_r2::undo_restores_all_four_cross_part_projection_warnings` covers undo after extrusion, a grouped parameter/extrusion edit and sketch deletion. Restoration derives the same four warnings from current records through `Session::cross_part_projection_warnings`; it does not cache or accumulate them. Failing-before and mutation evidence: `tasks/fix3-report.md`.

## F-110 Exact imports must keep tree/source semantics and use safe parsing

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **What:** The app reads exact files with `read_exact_tree(format, path, 0.5)`, embeds original bytes, then uses `Project::import_tree_as_parts`. This creates Import bodies inside part components while retaining component names and placements; scale is a literal correction applied to shapes and component offsets, in addition to OCCT unit normalization to mm. IGES without solids may be an app drawing import, but the server's solid import refuses it.
- **Evidence:** source `qymcad/src/gui/file_doors.rs:223-247`, `qymcad/src/gui/import_scale.rs:123-181,228-257`, `qymcad-core/src/model/assembly/tree.rs:173-238`, `qymcad-kernel/src/shape/exchange.rs:206-271`; verified by `crates/engine/tests/import_tool.rs` and `crates/mcp/tests/protocol.rs::imports_step_through_mcp_and_batch_with_embedded_source`. Tests compare independent server-exported solid metrics, scale 2, topology availability, and reopen after deleting the external path.
- **How we handle it:** Engine import stages source bytes with exclusive 0600 files and normalized extensions, parses under the kernel gate, embeds bytes, preserves the native tree and component placements, and returns bodies addressable by id/name. Object names must be nonnumeric and free of leading/trailing whitespace (internal spaces remain valid; a blank name keeps meaning "use the default", as before); the shared validator covers explicit names and final imported component names derived from source metadata or filenames, refusing invalid imports atomically. Reported imported-body names resolve in topology, select, fillet, body_boolean and batch, while alias collisions retain the resolver's id-required ambiguity error. Do not call upstream `raise_imports`: it uses predictable staging and parses outside the gate. Import creates separate parts; default modelling stays scoped to the active part, so callers pass imported body references explicitly. Explicit body booleans create their node in the operands' part and restore the previous active part.
- **Follow-up regression evidence:** `import_tool` tests save then delete STEP/IGES sources, reopen both stored-B-rep and source-only documents, compare all 6 persistent face and 12 edge ids and resolve each through `select` (F-106). Distinct server-exported boxes prove names, original indices and selective geometry. Imported-body push (+200 mm³), one vertical R1 fillet (−10·(1−π/4) mm³), and a face-hosted 2×3×4 pocket (−24 mm³) are checked analytically and rebuilt after a source-only save/open. Those tests initially failed with `cross-component input` for modifiers and an unresolved sketch frame: native adders use the active component (`qymcad-core/src/model/timeline/add_ops.rs`), so these explicit edits now create nodes in the body/sketch owner's part and restore the previous active context. The app likewise enters the sketch owner's component before extruding (`qymcad-part/src/apply/solids.rs:417-428`). Real IGES MCP and batch tests use a server-built, inch-unit IGES fixture; the server export tool itself has no IGES format.
- **Optional naming policy:** `Session::optional_object_name` maps empty/whitespace-only names to omission for sketches, imports, all feature names and individual split/boolean piece names. Nonblank object names retain whitespace/numeric-alias refusal; parameter and dimension normalization is separate and unchanged. MCP `review_r3` compares omitted versus ASCII/Unicode blank names, all three boolean volumes, batch/undo, and mixed explicit/default piece aliases. Evidence: `tasks/fix3-report.md`.


## F-111 Native extrude down takes abs, drops tiny values and is ignored in symmetric/through modes

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Native source:** `qymcad-core/src/feature/matrix.rs:46–60` (`extrude_extent`) takes absolute height/down, uses the second side only when down > 1e-9, and ignores it for `Reach::BothWays`. Forward gives [-down,height], backward gives [-height,down]. `model/regen/apply.rs:225–255` bypasses down for through-all and uses exact kept ends for finite Two sides; ordinary one-sided cuts retain their existing clearance behavior.
- **Probe evidence (observed stage 0):** `research-medA/extrude-chamfer/edge-cases-output.txt` records h=-7/down=-3 becoming a positive 10 mm span, down=1e-12 and 1e-9 dropping out, 1.1e-9 retained, and symmetric ignoring down. `probe.rs`/`output.txt`, `named.rs`/`named-output.txt` and `placement.rs`/`placement-output.txt` verify native add/cut, both reaches, persisted global/named driver edits and cap positions. A 10×6 profile h=7,d=3 has V=600 mm³ and forward z=[-3,7], reverse z=[-7,3]; inside 20×16×20 stock on z=10, the cut leaves V=5800 mm³. Stage-0 script locations and exact replay commands: `tasks/research-medA.md` section 1.
- **Server policy (ADR 0029):** explicit `down` must evaluate finite and >1e-9, including later parameter/native dimension edits, rebuild and open. Refuse down with symmetric or through-all; zero does not switch an existing feature to one side. Both native Extrude and Combine paths retain the `down` formula, preserving native dimension naming and F-104 precision. Absent down preserves existing behavior. Implementation and red/restored mutation evidence: `tasks/A1-report.md` and `crates/engine/tests/golden_extrude_two_sides.rs`.

## F-112 Native sketch Point eligibility, reuse and coordinate dimension ambiguity

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Native source:** the app calls `Project::sketch_point_at(si,x,y,1e-6)` (`qymcad-sketch/src/clicks.rs:433–436`). The public adder (`qymcad-core/src/model/sketch/entities.rs:960–982`) reuses a non-system point within Euclidean epsilon and otherwise allocates a user point, even at the system origin. `model/sketch/profiles.rs:74–97` excludes system ids and every `entity_points` member from isolated points, including construction entities; constraints alone do not exclude them. Points add no entities or contours. Native `delete_points` (`entities.rs:924–944`) also removes incident entities, so isolated-only removal requires a preflight.
- **Dimension identity:** `model/params.rs:132–134` maps a plain `Distance` to its point reference set without axis. Two origin-to-point x/y dimensions therefore have ambiguous native SetDriver targets. Two `DistancePL` rows to different axes have distinct reference sets. Reversing the y-axis reference order gives a positive x driver for positive x. Literal zero coordinates use `PointOnLine`; formulas stay formulas under the established coordinate branch policy.
- **Probe evidence (observed stage 0):** `research-medA/point-hole/probe.rs` and `output.txt` record `AXIS_POINT_INITIAL` [8,6], then saved native app global edits to [12,10], named edits to [14,11], DOF=(0,0), and exactly one constraint matching each driver. `POINT_AT_ORIGIN` has a user id distinct from the system origin and is eligible; duplicate placements reuse ids, while construction line points are excluded. Full command/evidence: `tasks/research-medA.md` section 3.
- **Server policy (ADR 0030):** default-dimensioned Point uses native point-to-axis dimensions, returns its id on creation or reuse, and adds no constraints on reuse. Sketch inspection reports isolated point coordinates/dimension status. `sketch_remove point` refuses system/entity-owned points before native deletion, naming the entity to remove. The existing solve/rebuild/history paths provide atomicity, batch and undo. Golden, app-path and red/restored mutation evidence: `tasks/A1-report.md` and `crates/engine/tests/golden_sketch_point.rs`. Hole by sketch is outside A1.

- **A1 additional observed point behavior:** native `eval_parameters` (`model/params.rs:393–395`) and literal SetDriver (`:179–182`) preserve a DistancePL side with `d < 0`, so negative → zero → negative becomes positive after zero. A1 encodes the coordinate side in axis orientation, keeping d nonnegative. `golden_sketch_point::a_negative_coordinate_retains_its_side_after_touching_zero` verifies engine edits, saved zero state, load, and independent native app global/named edits. Its pre-fix failure measured x=12 instead of -12 (`tasks/A1-logs/s6-resume-baseline.log`). Magnitude-formula sign guards remain active after a curve adopts the point; the failing-first reuse test and exact archive rollback are recorded in `tasks/A1-logs/s6-reused-sign-failing-first.log`.
- **A1 numeric strings:** Rust Num accepts exponent literals but native formulas reject them. New Point coordinates and extrude down canonicalize these literals to full-precision decimals, retaining editable formula entries (also string zero); failing-first and saved native-driver tests are in `tasks/A1-logs/{p1-precision-first,s6-numeric-string-failing-first}.log` and both A1 golden suites.

- **R1 Point guards (observed):** native literal SetDriver clears the expression and takes `abs()` before a solve-only expression guard can inspect the request (`model/params.rs:179–182`). The engine validates Point magnitude inputs before mutation, keeps the public native formula check, and directly writes accepted Point dimensions without the SetDriver abs/clear fast path. Evaluated Point checks are shared across open/rebuild and parameter/dimension edits. Negative magnitudes refuse with sketch/point identity, including reused points and prior errors; zero keeps the axis-encoded side. `golden_medium_r1::{point_negative_literal_must_not_be_silently_absolute,open_must_not_silently_absolute_negative_point_formula}`, `sketch::r1_point_guards` and `tasks/R1-report.md` record failing-first, immediate archive equality and production mutation evidence.

- **R4 Point intent and cleanup (observed):** native `SketchPoint` has only id/x/y; A1 initially persisted no distinction from curve helpers. Engine `prune_debris` erased isolated explicit Points and their axis drivers after an unrelated entity removal, silently dropping one of two bores (V11899.469035085127→11949.734517542563). Native `prune_orphan_sketch_points` (`model/sketch/entities.rs:820`) also erases free Points during break/trim and offset relays (`offsets.rs:359`), including parameter solves and `qymcad-io`'s load-time `settle_loaded`. Newly created explicit Points persist `feat_dims[point]["explicit_point"]="1"`; this tag wins over helper classification and survives save/reopen. Legacy isolated non-system points are preserved unless positively identified as rectangle centres, angle-reference support points or materialized midpoints. Old files contain no intent for an already adopted endpoint/helper; that provenance cannot be recovered exactly. Engine deletion/helper cleanup excludes explicit Points, and native curve edits/solves/regeneration retain their records and coordinate drivers, rechecking the ordinary solve after restoration and rescheduling affected consumers after a native pass. Open recovers original Point records through the non-solving public parser before accepting native load cleanup. Deliberate point/constraint removal remains available; no dummy constraints or native source changes. Evidence: both ported reviewer fixtures, saved legacy archive, all curve-edit cleanup controls, offset parameter/open/rebuild tests, midpoint-helper regression and production mutations in `tasks/R4-report.md`.

## F-113 Native ancestor tilt changes can leave a child datum sketch's solid stale

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Probe evidence (observed stage 0):** `research-medA/plane/probe.rs`, `run.sh` and `output.txt` record an XY parent at d7/TiltX30°, child offset2 and a 10×6×4 extrusion. Native saved app ax30→40 updates both normals but leaves the old solid bbox with no errors. The expected bbox is [-5,-9.601686123,3.711494679,5,-2.434269026,10.632398110], derived from transformed corners; the volume stays 240 and cannot expose the defect. `PASS chain saved tilt barrier` confirms a persisted ancestor formula on the sketch fixes the saved app path. See `tasks/research-medA.md` section 4 for exact commands.
- **Source:** `qymcad-core/src/model/regen/run.rs:28–54` checks immediate sketch supports during parallel preparation, while `model/params.rs:309–324` dirties owners of any changed-name feature formula. `model/sketch.rs:319–367` reevaluates all four datum keys. Tilt rotates X then Y about the original support axes (`feature/frames.rs:187–206`); Turn rotates the sketch axes, and is not inherited as the child's Turn. `model/sketch/profiles.rs:25–42` projects world zero to obtain the sketch origin.
- **Server policy (ADR 0031):** extend the F-017 persisted barriers to `dist`, `tilt_x`, `tilt_y`, `turn` and named drivers of every ancestor. Reconcile after formula/name edits, propagation, open and before rebuild; remove obsolete reserved guards. All four evaluated values are finite and bounded (distance ±100000 mm, angles ±360°), refused without normalization, including previously broken nodes. Legacy absent-angle files retain placement. Independent corners and actual native saved global/named driver paths are tested in `golden_datum_tilt.rs`; isolated missing-ancestor-guard mutations and failing-first evidence are in `tasks/A2-report.md`.

## F-114 Native DistAngle chamfer accepts periodic angle aliases without warnings

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Probe evidence (observed stage 0):** `research-medA/extrude-chamfer/edge-cases.rs` and `edge-cases-output.txt` compare 1° and 181° on a 40×30×10 block, leg2. Both return no errors/warnings and V=11999.650898701435 with matching face areas. Several rejected angles return a pass-through shape despite a nonempty error report, so shape existence is not acceptance. `obtuse.rs`/`obtuse-output.txt` reproduce the alias at a 135° interior edge too. Commands are retained in `tasks/research-medA.md` section 2.
- **Source:** `qymcad-kernel/src/occt_io.cpp:1877` passes degrees converted to radians directly to OCCT AddDA; `model/regen/prep_ops.rs:77–109` evaluates native `dist` and `d2`. DistAngle is `FeatureKind::Chamfer` with mode `DistAngle`, not a separate feature kind. `d2` is degrees here and mm in TwoDist (`feature/kind.rs:339–359,683`).
- **Server policy (ADR 0032):** require `0 < angle < 90` and positive finite setbacks at creation and every evaluated edit/rebuild/open. The principal interval excludes aliases but does not promise geometric feasibility for every finite stock. Persist angle formulas under native `d2` through the app's public formula door, preserving F-104 precision. Branch-aware `dimension_set` reports mode/unit and refuses symmetric d2 even for naming. Tests use face setbacks/centroids and both saved app global/named drivers; the 135° golden derives V=11875−10(1+sqrt(3)) using the law of sines. Evidence: `golden_chamfer_angle.rs`, `golden_a2_dimensions.rs`, `tasks/A2-report.md`.

## F-115 Native asymmetric chamfer silently falls back when its reference face is unrelated

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Source:** `qymcad-kernel/src/occt_io.cpp:1830–1879` chooses the native adjacent face using flip, then overrides only if the picked persistent face belongs to that edge. An unrelated or vanished reference leaves the automatic side in force. A one-face boundary edge takes symmetric Add(a,e). A green native rebuild therefore does not establish that the requested support face was used.
- **Evidence:** stage-0 `research-medA/extrude-chamfer/probe.rs`/`output.txt` checks both valid support faces and flip, measuring surviving source face areas independently: on +x reference, leg2 changes area300→280; on +y it changes area400→380. The stage-A2 test `native_unrelated_reference_face_silently_uses_automatic_side` directly rebuilds native recipes with ref0 and an unrelated ref123456, observing no errors and identical volume/face areas. A separate boolean-intersection fixture changes a stock parameter and loses selected support/edge identity; the server refuses and restores the exact archive (native missing-edge detection can also refuse this fixture). Exact commands and runtime-red/restored-green logs: `tasks/A2-report.md` and `tasks/A2-logs/`.
- **Boundary observation:** stage-A2 `sheet_boundary_cannot_silently_become_symmetric` creates a rectangular surface loft. The public edge/face-pair list omits its one-sided boundaries; the guard checks whether a missing pair still has a live edge and refuses it explicitly. This fixture natively reports ChamferAsym failure, so it is evidence for omitted adjacency, not for a successful symmetric fallback. Runtime failing-first evidence: `tasks/A2-logs/p6-boundary-failing-first.log`.
- **Server policy (ADR 0032):** resolve exactly one reference face, require it adjacent to every explicitly picked non-seam edge, and require two distinct adjacent faces for asymmetric mode. Preserve native automatic/flip behavior when reference is absent. Recheck against current rebuilt source topology and refuse lost associations atomically, preserving the exact saved archive and original B-reps. Symmetric reference/true flip are refused. F-064 native tangent propagation beyond picks remains unchanged and has its own analytic regression. The F-074 overlapping-fillet validator remains restricted to fillets.

## F-116 Native accumulated rotation cannot preserve parameter-driven Move shifts

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Probe evidence (observed stage 0):** `research-medA/move/probe.rs:12–16` and `output.log` save/reload a 10×20×30 prism with a 90° Z rotation about (1,2,15) and native dx formula `Shift + 3`. The saved app path changes Shift from 10 to 25 and places it at bbox [16,-3,0,36,7,30], V=6000 mm³. Another +90° Z rotation about the origin must map (x,y) to (-y,x), giving bbox [-7,16,0,3,36,30]. Native `accumulate_move` instead produces [8,1,0,18,21,30], with dx rewritten to `(Shift + 3) + -14` and no dy formula. The log labels are `formula_then_accumulate_rotation` and `accumulated rotation formula`; bounds here omit native bbox padding. Exact replay command and saved app-edit algorithm: `tasks/research-medA.md` section 5.
- **Source:** `qymcad-core/src/model/timeline/nodes.rs:103–130` multiplies the stored matrix by the new operation, then adds numeric deltas to each existing shift expression. `model/regen/prep_ops.rs:382–398` substitutes evaluated dx/dy/dz only into a temporary matrix at rebuild, leaving the stored matrix unchanged. Rotation must mix parameter-dependent translation coefficients between axes; per-axis numeric delta addition cannot express that dependency, even if the stored and evaluated values currently coincide. The app calls this public accumulation path (`qymcad-part/src/apply.rs:177–192`).
- **Numeric override consequence (source-derived):** the temporary-matrix behavior also applies to a plain numeric `dimension_set` override. Its current value can differ from the stored matrix without carrying variable coefficients. Before an allowed numeric rotation accumulation the server synchronizes all three stored shift slots to their evaluated numeric values, then persists the composed numeric translations. `golden_move::edited_numeric_affine_translation_is_used_by_later_rotation_accumulation` checks dx 10→25.123456789 followed by a 90° rotation against independently transformed corners. This is the same stale-matrix mechanism, not a separate native finding.
- **Server policy (ADR 0033):** refuse rotation onto a Move with nonliteral translation expressions, naming the native formula-accumulation limitation. Arithmetic expressions are conservatively treated as formulas; plain numeric formulas remain eligible. Permit first rotation and repeated rotation of plain numeric transforms; do not erase dependencies or allocate a substitute node. Only native dx/dy/dz are editable dimensions: the app disables rotation naming, and the stage-0 probe observes that angle text can pass syntax checking while `set_dim_target_text` returns `Ok(false)` and `kind.dims()` lists only dx/dy/dz. These slots are total affine translation, including pivot correction, not angle drivers.
- **Identity and frame evidence:** the same probe retains all six face and twelve edge ids through Move and saved app parameter edits; a downstream selected-edge chamfer and a fillet before Move retain their independent analytic volumes. `Project::add_move` (`model/timeline/add_ops.rs:511–519`) uses the source's owner part. App gizmo pivot uses the part-local display-mesh bbox centre (`qymcad-ui-state/src/gizmos.rs:368–388,701–710`), sampled for that operation. Native transform naming passes through existing topology; Move has no newly generated face/edge role names.

## F-117 Native Move accumulation rounds formula deltas and can discard a microshift

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Probe evidence (observed stage 0):** `research-medA/move/probe.rs:17` applies +0.000001 mm X to the formula-driven saved Move from F-116. `output.log` labels `tiny_translation_accumulate` and `tiny translation formula` show unchanged bbox [16,-3,0,36,7,30] and dx expression `(Shift + 3) + 0`. The intended bbox is shifted +0.000001 mm along X; volume remains 6000 and cannot detect the loss.
- **Source:** `qymcad-core/src/model/timeline/nodes.rs:117–126` computes the matrix delta but formats its addition to existing dx/dy/dz formulas through `expr::fmt_num`, the four-decimal formatter also responsible for F-104. `model/regen/prep_ops.rs:384–388` then overrides the matrix translation with that rounded expression. Deltas at or below its 1e-12 update threshold can also leave the original expression unchanged. A successful accumulation/rebuild alone therefore does not establish that the requested displacement survived.
- **Server policy (ADR 0033):** retain the pre-accumulation expression for each affected axis, invoke the native accumulation path, then rebuild its formula as source expression plus full-precision requested delta through `Project::write_feat_formula` (`model/formulas.rs:270–281`). Preserve expression dependencies, native driver naming and reused node id. Numeric inputs use round-trippable decimal formulas, including numeric strings; do not accept native rounded `+ 0` as equivalent to the requested shift. Stage-A3 independent position, saved global/named driver and mutation results are recorded in `tasks/A3-report.md`.

## F-118 Native sketch holes omit their sketch from dependency scheduling

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Source:** `qymcad-core/src/feature/kind.rs:940` groups Hole with src-only modifiers in `FeatureKind::inputs`; the nonzero `Hole.sketch` is omitted. `model/regen/run.rs:28–52` derives dirty propagation and sketch-support ordering from those inputs. `model/timeline/state.rs:8–14` dirties only the sketch. Native `remap_ids` also omits `Hole.sketch` in its src/body-only arm (`feature/kind.rs:1137–1144`); component cloning remains outside this stage's scope.
- **Probe evidence:** retained stage-0 `research-medA/point-hole/probe.rs` and stage-A4 `crates/engine/tests/native_hole_sketch.rs` use actual saved `.qcad` B-reps and the native app global/SetDriver paths from `tests/common/mod.rs`, without engine propagation or repair. Unguarded global pitch 8→12 moves the point to x12 but leaves its bore wall at x8; unguarded named SetDriver 8→15 likewise leaves the wall at x8. Persisted `feat_dims[hole]["sketch_dep_..."]` expressions containing the coordinate expression and driver name respectively move the same walls to x12/x15. Both cases retain the independent analytic volume `12000−40π`, so volume/bbox cannot detect the stale placement. Native marking examines every `feat_dims` value (`model/params.rs:309–324`), including reserved keys.
- **Observed bridge limit:** an unnamed literal coordinate edit 8→13 followed by native solve/mark and dirty regeneration leaves the guarded bore at x8 (`RAW_UNNAMED_LITERAL`, no hole rebuilt). Structural app edits to points, constraints, formulas or driver names cannot reconcile server-created metadata. Reopen/rebuild through this engine repairs the recipe/cache; this is not full in-app associativity.
- **Observed ordering limit:** a datum-supported marker with guarded plane expression `height` has no native structural edge to that datum. The initial parallel wave can capture unresolved plane z0 rather than z10 and remove no material. After explicitly settling that fixture, save/load then native height10→11 updates the plane to z11 but leaves the blind-hole volume at `12000−16π = 11949.734517542562`; the independent target is `12000−12π = 11962.300888156922`. Re-dirtying gives native wave `[plane10,hole9]` together. Reserved expression reads schedule a rebuild but cannot order its preparation after a datum. Log: `tasks/A4-logs/native-probe.log`; initial independent expectation failure retained in `native-probe-datum-first-failure.log`.
- **Native operand limit (source and probe):** `add_hole_from_sketch` stores only sketch/flip (`model/timeline/add_ops.rs:556–570`); `prep_hole` enumerates every current isolated point (`model/regen/prep_bodies.rs:300–313`). There is no subset field. The probe confirms all isolated IDs are returned, including an off-stock marker. The server must refuse a proper `points` subset rather than silently drilling additional sites.
- **Server policy:** ADR 0034 defines persisted coordinate/global/named dependency reads plus the engine structural closure. Engine open refuses a marker sketch reordered after its hole; metadata cannot repair a forward recipe dependency. A native named depth edit persists an explicit `depth` entry while leaving server metadata alone (`model/params.rs:203`); engine reconciliation drops `sketch_through` on that explicit-depth signal, retaining the app's blind recipe (`hole_sketch_review::named_depth_edit_in_saved_app_converts_through_to_blind_on_engine_open`). Parameterized or named datum ancestors, projected sketches and independent hosted faces anywhere in the marker’s transitive sketch source graph are refused because metadata cannot establish native ordering; fixed settled datum frames and a face on the stock's own upstream lineage remain eligible. Coverage P8 stays partial with unnamed/structural app-edit and native operand/support limits explicit.

- **R1 named-source closure and numeric inputs (observed):** native deletion reachability omits names read by coordinate constraints and persisted bridge expressions. The engine resolves these reads through global aliases/named dimensions to their owning nodes, normalizes sketch-item owners to sketches and includes plain Point sketches without holes. Non-cascade deletion identifies dependents; cascade removes them under existing history/undo. Sketch-hole diameter/depth/dia2/depth2 now use canonical full-precision decimal formulas through the native formula door, accepting scientific numeric strings consistently with A1–A3. Evidence: `golden_medium_r1`, `dependencies::hole_tests::persisted_coordinate_reads_resolve_aliases_and_native_dimension_owners`, immediate (no helper rebuild) archive checks in `golden_hole_sketch`, and `tasks/R1-report.md`.

- **R2 expression closure and transitive support (observed):** scanning only `sketch_dep_*` missed ordinary feature formulas: deleting an independent Move’s named dx left another Move or extrusion with a dead expression and cached geometry. Shared closure now scans every feature-dimension expression, retaining the existing sketch constraint/item traversal, named owner normalization and global-alias resolution. Non-cascade deletion names those consumers; cascade removes them, and undo restores exact archives. Global aliases remain outside timeline cascade and still need separate cleanup. A named reference measurement of a projected edge also bypassed the marker-only support check: the reviewer’s native width40→60 edit updated measured_w to60 but left marker/bore X10 instead of15. The engine now checks bounded support for every sketch reached by `sketch_sources`, deterministically naming the unsupported source, both at creation and in evaluated edit/open/rebuild guards. Evidence: ported native fixture and immediate archive tests in `golden_medium_r2`, `dependencies::hole_tests::every_feature_formula_key_resolves_named_owners_without_owning_globals`, `hole_sketch::r2_tests::evaluated_rebuild_refuses_transitive_projected_named_source`, copied reviewer evidence `tasks/R2-logs/reviewer-confirm-projected-source.log`, and `tasks/R2-report.md`.

- **R3 sketch-array ordering limit (observed):** the native app solves sketches in array order. A world-plane reference-source sketch appearing after the marker can therefore update `measured_w` from40 to60 while leaving marker and bore at X10 rather than15, even when both sketches precede the hole in the timeline and neither has projections or datum support. Swapping only the sketches' array order yields bore X14.999999934 instead of X9.999999999999995. Evidence: independent review `review-medA-r3/REVIEW.md`, `probes/order/src/lib.rs` and `probes/resume-order.log`; ported refusal/control and mutation results: `tasks/R3-report.md`. The bounded-support guard now requires every source sketch reached through the named-expression graph to have an array index strictly less than the marker's. Creation, edits and evaluated open/rebuild refuse a later source atomically, naming it and saying it is solved after the marker; timeline precedence alone cannot establish support.

- **R4 same-sketch reference order (observed):** the same marker sketch is exempt from the earlier-sketch rule only for driving dimensions/named coordinates. A reference measurement is refreshed after its coordinate expressions even within that sketch: the reviewer saved-app w40→60 path left Point/bore X10 while measured_w became60 (expected X15). The guard now retains exact names while traversing global aliases and named driver formulas, refusing any marker expression reading its own non-driving measurement. Creation, coordinate/alias edits and evaluated open/rebuild refuse atomically, naming the measurement and saying it is solved after the marker's coordinates. Ordinary same-sketch driving dimensions/named Point coordinates remain eligible. Evidence: `golden_medium_r4`, saved `r4-same-sketch-reference.qcad`, `session::r4_tests` and `tasks/R4-report.md`.

## F-119 Native sketch-hole aggregate success does not establish every bore site

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Source:** `model/regen/prep_bodies.rs:308–313` passes every point ID as a site; `qymcad-kernel/src/kernel.rs:796–803` fuses the tools and performs one cut. Bore walls receive `GeoName{feature:hole, role:Hole, src:point_id}` (`qymcad-kernel/src/ops/holes.rs:9–12`); drill-tip/recess names use feature-wide `HoleTool` roles (`:15–17`), so recess names do not independently identify a site. The public reverse lookup is `NameTable::get` (`qymcad-core/src/names.rs:403`).
- **Probe evidence:** `native_hole_sketch::raw_partial_cut_succeeds_without_every_requested_site` drills Ø4 through a 40×30×10 box at one inside point and one x80 point. Native errors are empty and the volume is exactly `12000−40π`, yet the outside point has no nonzero-area bore face. `raw_partial_cut_also_ignores_site_over_existing_void` adds two sites to stock already containing one hole: one fresh site cuts, the site over the existing void does not; native errors remain empty and volume is `12000−80π`. The fresh site has its own Hole face, the void site does not. Run output: `tasks/A4-logs/native-probe.log`.
- **Server policy:** accept each requested site only when a positive finite-area face reverses through `p.names.get(face_id)` to that hole and point with role Hole. A missing site refuses the entire edit atomically and identifies its point ID; aggregate removed volume or a successful native report alone is insufficient. Through remains a fixed 10000 mm recipe (F-037), guarded by the full directed span from the marker plane over all eight current-source bbox corners, including air before the stock. These are server guards, not native through-all or upstream per-site acceptance.

## F-120 Native feature scope can precede refreshed named reference measurements

- **Version:** v0.1.1-dev.20261026, f25f27d.
- **Source:** `qymcad-core/src/model/regen/run.rs:193` snapshots feature-expression scope before projected geometry refresh updates named reference measurements. A feature rebuilt during that pass can consume the previous value and finish with an empty native error report. Another dirty mark is needed to rebuild that consumer against the refreshed scope; the inherited height behavior and newly exposed Move/down/tilt/chamfer formulas share this scheduler limit.
- **Observed evidence:** independent review `review-medA-r3/REVIEW.md` and retained `probes/move`, `probes/a1a2`, `probes/resume-move.log` and `probes/resume-a1a2-all.log` record width40→60 and `measured_w`40→60 with stale Move dx=`measured_w/4` (xmin105 instead of110), extrude down=`measured_w/10` (4 instead of6; V660 instead of780) and Tilt X=`measured_w/2` (20° instead of30°). An ordinary extrusion-height control reproduces the inherited defect. Pending native archives can also make evaluated down10→−10, Tilt X320→480° and chamfer angle20→100° during open while the old pre-regeneration guard passes. Engine regression, exact rollback and mutation results: `tasks/R3-report.md`.
- **Server policy:** compare evaluated scopes across regeneration and repeat every timeline node whose expressions read changed named values, using the shared expression-source/dependency closure; repeat consumers and their dependents until stable, with a four-pass bound. Non-convergence is a rebuild error. Recheck centralized evaluated guards against the settled post-regeneration scope in open, standalone rebuild, edits and `with_rebuild_copies`, after the last pass has no changed named values; do not reject a transient mixed scope between passes; guard/non-convergence refusals preserve exact pre-edit recipe, B-reps and diagnostics without an extra rebuild. Do not refuse general reference-measurement formulas: the engine settles them on edits/rebuild/reopen. Open also dirties the named dependency closure because an app-saved clean timeline may already contain stale feature caches (`session::r3_tests::reopen_repairs_clean_native_cache_after_reference_refresh`). The native app scheduler remains unchanged and may need a second regeneration; sketch-hole support retains its separate bounded scheduling contract (F-118, ADR 0034).

- **R4 settled-domain evidence:** an acyclic two-reference chain with down=`second_w-first_w+5` temporarily has first_w60/second_w40 (down−15), then both60 (down5). Intermediate validation falsely refused it. `golden_medium_r4::transient_guard_before_reference_settling` ports the native three-pass control and now verifies engine success, both references60 and back cap z=−5; the final-scope R3 down/tilt/chamfer refusals and non-convergence rollback remain enforced. Array limits join the same settled guard (F-033). Failing-first and mutation evidence: `tasks/R4-report.md`.
