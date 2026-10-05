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
  TangentChain, Oriented, Extreme, OfFeature, ...}`) via `add_fillet_ref` / `add_chamfer_ref` are the robust
  form. Topology must be re-read after every regenerate; ids live on the specific body.
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
