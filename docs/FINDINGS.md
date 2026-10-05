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

## F-3A-1 The constants `pi`, `tau`, `e` shadow parameters of the same name

- **Version:** v0.1.0-dev.20261001
- **What:** the expression evaluator resolves a bare name as a constant first and only then as a variable, so a
  parameter named `e` (or `pi`, `tau`) is never read: a dimension `e` evaluates to 2.718..., silently.
- **Evidence:**
  - observed: a hexagon dimensioned by a vertex at `["e", 0]` with parameter `e = 8` came out with circumradius
    2.718 (area 19.2 instead of 166.3).
  - source: `crates/qymcad-core/src/expr.rs` (~224: `constant(&name).or_else(|| self.vars.get(&name))`, ~256).
  - test: `crates/engine/tests/sketch_behaviour.rs` `constant_names_are_not_parameters`.
- **How we handle it:** `param_set` refuses these names (case-insensitive).

## F-3A-2 One sketch solve stops short when an angle dimension's arm must change length

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
  from a reference point instead of an angle dimension (F-3A-7): no arms, so a GUI edit settles in one solve.
  Plain-number arcs/rotations keep radius + angle dimensions (friendlier to edit by hand; only a hand edit in the
  app can change them). Angle dimensions added with `sketch_constrain` and driven by a parameter may still land
  slightly off after a GUI edit — not measured in the app.

## F-3A-3 A tangency at its own contact point is invisible to the rank analysis (QymCAD's slot reports 4 + 4)

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

## F-3A-4 Entity adders and the points they share

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
  `sketch_behaviour.rs` `circles_on_a_vertex_share_or_get_their_centre`.
- **How we handle it:** every entity, rect and circle included, adds its dimensions only when independent
  (`add_constraint_if_independent`, F-3A-11), so a shared point is not dimensioned twice.

## F-3A-5 Signs and units of dimension values

- **Version:** v0.1.0-dev.20261001
- **What:** `Angle`/`AngleLines` are unsigned, 0..180 (`atan2(|cross|, dot)`): the side comes from the geometry.
  `DistancePL.d` is signed (the side); `eval_parameters` writes `±|expr|` keeping the stored sign. A `Diameter`'s
  expression value goes to `d` unchanged — a diameter when `diam`, a radius otherwise.
- **Evidence:** source: `crates/qymcad-core/src/solver.rs` (~1093-1128, 1166-1180), `crates/qymcad-core/src/model.rs`
  `eval_parameters` (~2412-2431). test: `golden_sketch.rs` `hexagon_rotation_follows_an_expression`,
  `sketch_entities_and_constraints_end_to_end` (protocol, point-to-axis distance).
- **How we handle it:** angle dimensions are used only for plain numbers (folded into 0..180, the side from the
  geometry; 0/180 become horizontal and ±90 vertical constraints); parametric directions use `ArcLength` (F-3A-7).
  Point-line distances take their sign from the current geometry. (An earlier `angle_expr` that rewrote angle
  expressions was never reached and has been removed.)

## F-3A-6 A sketch edit does not rebuild the features built from it

- **Version:** v0.1.0-dev.20261001
- **What:** `mark_sketch_dirty` marks only the sketch node; the bodies change at the next regenerate.
  `dependents_of(sketch id)` lists the features that read the sketch.
- **Evidence:** source: `crates/qymcad-core/src/model/timeline.rs` (~1384, 1514); test: `sketch_behaviour.rs`
  `editing_a_sketch_rebuilds_its_features`.
- **How we handle it:** `Session::sketch_edit` rebuilds when the sketch has dependents and rolls the edit back if a
  feature that built before now fails; the sketch tools use it.

## F-3A-7 Dimensions keep their side: a coordinate expression cannot change sign; arc length is directed

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
    `a_coordinate_that_would_cross_zero_is_refused`, `a_coordinate_crosses_zero` (ignored: not supported).
- **How we handle it:** parametric directions (arc ends, polygon rotation) are `ArcLength` dimensions from a
  construction point on the +x side of the centre (`Horizontal` + `PointOnCircle`, role `angle_reference` in
  `sketch_info`), `len = (r)*(a)*pi/180`, valid while the angle stays in the turn it was created in ([0°, 360°)
  for 0..359°). Linear coordinates still cannot cross zero: `param_set` refuses such an edit and rolls back
  (F-3A-2 settled solve + residual check); in the app the sketch would not solve. Design decision pending.

## F-3A-8 Deleting an entity drops every spline's control points

- **Version:** v0.1.0-dev.20261001
- **What:** `delete_entities` keeps only points used by entities, the frame and midpoints; spline control points
  are none of these, so removing any entity removes them (and the splines are left pointing at missing points).
- **Evidence:** source: `crates/qymcad-core/src/model/sketch.rs` `delete_entities` (~665-708); observed: "spline
  control point 4 was dropped"; test: `sketch_behaviour.rs` `removing_an_entity_keeps_splines`.
- **How we handle it:** `sketch_remove_entity` marks spline points as midpoints of themselves for the call (a
  protected kind) and removes the markers after.

## F-3A-9 Reference radius/diameter dimensions are refreshed from circles only

- **Version:** v0.1.0-dev.20261001
- **What:** `update_driven_dims` takes radii from `Circle` entities; a reference radius on an arc keeps its creation
  value (20 after the arc's radius became 25).
- **Evidence:** source: `crates/qymcad-core/src/model/sketch.rs` `update_driven_dims` (~2641-2650, 2681-2685);
  test: `sketch_behaviour.rs` `a_reference_radius_on_an_arc_does_not_go_stale`.
- **How we handle it:** `sketch_constrain` refuses reference radius/diameter dimensions on arcs.

## F-3A-10 Reference (driven) dimensions take no part in solving or counting

- **Version:** v0.1.0-dev.20261001
- **What:** `is_driven()` constraints are filtered out before `solver::dof` (`sketch_dof`), before the solve
  (`solve_sketch_inner`) and in `add_constraint_if_independent`; their values are rewritten from the geometry after
  each solve (`update_driven_dims`). Adding one never changes dof or redundancy.
- **Evidence:** source: `crates/qymcad-core/src/model.rs` `is_driven` (~1113), `crates/qymcad-core/src/model/sketch.rs`
  `sketch_dof` (~292-298), `add_constraint_if_independent` (~355-371), `solve_sketch_inner` (~2506-2515); test:
  `sketch_behaviour.rs` `a_conflicting_dimension_is_refused_and_rolled_back` (reference part: dof stays (0, 0)).
- **How we rely on it:** `sketch_constrain` skips the over-constraint check for reference dimensions.

## F-3A-11 Independence is the rank of a numeric Jacobian at the current geometry

- **Version:** v0.1.0-dev.20261001
- **What:** `add_constraint_if_independent` (and `sketch_dof`) compute the rank of a forward-difference Jacobian
  (`h = 1e-6`) of all non-driven constraints plus the entity intrinsics, at the current coordinates. A constraint is
  added only if the dof drops. It is local: a constraint that holds only after a large move, or a second-order one
  (F-3A-3), is judged by the current configuration; satisfaction is not checked.
- **Evidence:** source: `crates/qymcad-core/src/model/sketch.rs` `add_constraint_if_independent` (~355-371),
  `crates/qymcad-core/src/solver.rs` `dof` (~1217-1257); test: `golden_sketch.rs` `three_lines_close_a_triangle`,
  `sketch_behaviour.rs` `circles_on_a_vertex_share_or_get_their_centre`, `an_implied_constraint_is_not_added`.
- **How we rely on it:** entity dimensions are filtered through it; `sketch_constrain` compares `sketch_dof` before
  and after (more redundancy = refused; a fully implied geometric constraint is checked for satisfaction by its own
  residual before being skipped) and still requires the solve to reach residual ≤ 1e-6.
