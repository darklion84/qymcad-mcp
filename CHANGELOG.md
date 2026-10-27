# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## Unreleased

### Added
- Native parametric `thread` with size aliases, persistent rim selection, side detection, all numeric formula
  keys and basic geometry reporting. Default fit is capped to leave the groove exposed; explicit unsafe fits
  and later parameter/dimension edits are refused, including unsafe recipes opened from the app.
- Fixed-count `split` on base/datum/face planes, native piece names and every output addressable by id/name.
  Each piece can be exported alone to STL/3MF/STEP. XZ split offsets point along +Y.
- Baked sketch text through `sketch_add`, with fixed numeric height/anchor/angle, deterministic Arial or an
  explicit font file/face and missing-glyph refusal. `sketch_info` lists text records; `sketch_remove` accepts
  text ids; default extrusion keeps every glyph and its counters. Native app text-height defects remain
  documented in FINDINGS; recreate text to resize it (ADR 0024).

## 0.1.1-dev.20261026.2 — closed mesh exports

Same QymCAD release as 0.1.1-dev.20261026; files open in that app.

### Fixed
- 3MF, OBJ and GLB exports write each body as a closed mesh: the copies of a point that the kernel writes once per
  face are joined, so slicers no longer report every edge between faces as non-manifold (Bambu Studio: 680 on a
  756-triangle part). Solids of one body that only touch at an edge or a corner keep their own points there. All mesh
  formats, STL included, drop the zero-area triangles at a rounded corner's pole. F-093.

### Known limits
- A solid pinched along an edge (zero thickness along a line) keeps that edge with four triangles; a slicer's repair
  fixes it (F-093, docs/BACKLOG.md).

### Docs
- FINDINGS cite the upstream reports: F-074 #322, F-075 #323, F-076 #341, F-090 #342, F-091 #343, F-092 #344,
  F-093 #346 (QymCAD's own mesh export has the same unwelded faces).

## 0.1.1-dev.20261026.1 — the modelling capabilities new in QymCAD v0.1.1-dev.20261026 (ADR 0022)

Same QymCAD release as 0.1.1-dev.20261026; files open in that app.

### Added
- Native open/closed interpolation splines, ellipses and ellipse arcs in `sketch_add`, with formula dimensions
  and spline nodes/closure in `sketch_info`. Native rectangle figures support rotated centre/corner anchors.
- Strict sketch curve/point selectors; point-on-curve, curve tangency and formula-driven curve distances;
  native `circle_tangent` with a driving radius and unique near-centre selection.
- Atomic, batchable and undoable `dimension_set` for native sketch/feature drivers; names, formulas and
  DOF are discoverable through `doc_info.named_dimensions`. Ambiguous native targets are refused.
- Polygon side-count formulas with integral/range checks, retained vertices/first side and app-compatible
  rebuilding. Ellipse axes and polygon turn retain sketch dimension formulas: native item-only formulas
  conflict with full definition. New sketch-item names are refused because app SetDriver leaves bodies stale.
- `sketch_edit` (ADR 0022 items 5–6): trim, extend and break any sketch curve, as the app does, with new ends
  tied to the curves they lie on; offset a chain or loop with the distance kept as a formula (circle and arc
  offsets use `CurveDistance`, F-076); round or bevel a corner in all six native size modes; mirror curves about a
  point with the copy tied to its source. The result lists created/removed ids, the size dimensions made, the
  formula dimensions an edit removed, released offsets and the remaining freedom.
- `sweep` (ADR 0022 item 7): closed profile contours along a contour of another sketch or a chain of body edges
  kept by their persistent names; add, cut, intersect or new_body.
- `loft` (item 8): a solid through closed section contours in order, ruled or smooth, or a sheet with
  `surface` and `new_body`.
- `profiles_at` on `extrude`, `revolve` and `sweep`, and `profile_at` on loft sections (item 9): pick contours
  by points, each taking the innermost closed contour around it.
- `hole` `offset: [u, v]`: the centre as a shift from the face centroid along the face's axes, numbers or
  formulas stored as QymCAD's `off_u`/`off_v`, so the hole follows parameters (F-087).

### Changed
- `sketch_add` `rect` now writes QymCAD's native rectangle figure, as the app does. The four side ids, the contour
  and full definition are the same as before; the sketch also gets the figure's two construction diagonals, the
  centre is their crossing point, and width and height are aligned `distance` dimensions along the sides instead
  of `distance_x`/`distance_y`.
- `dimension_set` refuses a non-positive feature diameter, depth, radius, thickness, dia2, depth2 or chamfer dist, and a
  non-positive sketch diameter, radius or arc length, by name, and a rotated
  figure's turn formula leaving 0..180 degrees is reported as such instead of as its guard expression.

### Fixed (from the live test, before release)
- The split points of a broken line stay on the line (F-091); the app holds them only for a horizontal or vertical.
- A corner of an outline offset by the fillet radius is rounded instead of refused as redundant: the fillet gets a
  centre point of its own (F-090).
- Review fixes: a point-mirrored copy is tied only to the curve it was made from; a concentric ellipse is returned and
  dimensioned as itself; a line break at a point held by other dimensions is refused rather than bending later; a
  circle offset takes the side nearer `toward` by exact radius; an item value on a centre shared by several
  ellipses or polygons is refused (F-092).

## 0.1.1-dev.20261026 — follows QymCAD v0.1.1-dev.20261026 (ADR 0021)

From this release on, the server's version number is the QymCAD release it works with; a server fix for the
same QymCAD release adds `.1`, `.2`, … (README, "Versions and compatibility"). The previous release, numbered
0.1.0, worked with QymCAD v0.1.0-dev.20261001.

### Changed
- Pin QymCAD v0.1.1-dev.20261026 (f25f27d) and Rust 1.89. Files saved now need QymCAD 0.1.1; QymCAD 0.1.0 cannot
  open them. Engine restoration, copies, mesh export and adjacency use the release's public APIs with
  non-recursive kernel-gate scopes (F-073).
- Parameter names are case-sensitive, as in QymCAD: they are stored as given, a new name differing from an
  existing one only by letter case is refused, and an unknown name in the wrong case is explained with a
  did-you-mean. Named driving dimensions count as twins too, and a parameter cannot take a named dimension's name. `doc_info` warns about case twins in documents made
  elsewhere and explains parameter formulas that no longer evaluate.
- `extrude` with `through` goes to the end of the stock on the chosen side; `symmetric` spans both sides.
- The test emulation of an app parameter edit follows QymCAD 0.1.1.

### Fixed
- A fillet too big for neighbouring edges is refused (also when a parameter grows it): QymCAD 0.1.1 returns
  overlapping blends with sharp joins and no error (F-074), also when a support face survives. Found on open, it
  stays an error in `doc_info` and export refuses until the radius fits; other parameter edits still work.
- Opening a QymCAD 0.1.0 document with splines warns that QymCAD 0.1.1 dropped them (F-075).

### Removed
- Workarounds for defects fixed upstream: lowercasing names (F-001), repeated sketch solves (F-040), slot and
  line-on-arc tangency rewrites (F-041; sketches keep QymCAD's own `Tangent`), entity-removal markers (F-046), the
  refusal of reference radii on arcs (F-047). Tests that pinned fixed upstream defects now assert the fixes.

### Backlog J
- Preserve literal feature names inside opaque selection/axis objects when decoding JSON-string arguments;
  bound structured argument nesting to 32 levels and document interop without widening tool schemas.
- Freeze the complete kind-containing face selection in hole/shell/push_face as persistent picks,
  preserving missing-opening checks; selections without kinds retain dynamic queries (ADR 0013).
- Parameter edits solve only reached sketches, including transitive parameters, imported uppercase
  dependencies and named reference measurements refreshed by solves. Retain blanket native rebuild
  dirtying and add scheduling barriers for sketches on face-derived datums, preventing stale placement.
  Bound reference propagation, reject follow-up expression errors, recheck array limits, and roll back
  non-stabilizing cycles. Reuse corner classification within empty hints.
- Failed partial rebuilds restore shape absence for planned bodies without pre-edit live shapes.
- Cover literal body names inside opaque Axis arguments and document the native named-feature-dimension scope limitation.
- Undo labels follow the node returned by the tool, preserving action labels for empty feature names.
- Undo includes a localized feature or action `label` alongside the original tool and arguments.
- Document lazy sketch reference points and how to inspect asymmetric chamfer setbacks. Torus remains
  `other`: the pinned kernel reports only aggregate torus counts, without a per-face getter.
- Apply four-decimal coordinate presentation to sketch frames/points while retaining full-precision
  reusable dimensions, parameters and undo arguments. Clarify post-open bounds and measured area errors
  up to ~2% on very small curved regions, including planar faces with curved boundaries.
- Carry ambiguous face-name warnings in document open/info and feature results; reject descriptive face
  operands inside edge selections when they resolve to ambiguous names. Preserve sphere-fit fallback for
  freeform surfaces, and guard fitted spheres directly in corner and face planar helpers.
- Drop seam edges automatically from fillet/chamfer picks, count them in `rebuild.notes`, and clearly
  refuse selections containing only seams. Infer/drop seams only on bodies with unique face names;
  ambiguous names cannot prove native face identity. Topology/selection previews retain proven seams.
- Give failed fillets geometric fit hints and smaller-radius/fewer-edge advice. Chamfer revolve advice
  applies only to native too-big failures, suggests a smaller distance first, and follows all consumed bodies.
- Correct corner singular/plural hints, require two solid-creating features for junction advice, and list
  vector directions in argument errors; kind-query errors identify faces versus edges.

### Round I fixes
- Unify all JSON bbox coordinates at four decimals with half-away rounding and normalized zero; document display-rounded render captions.
- Correct small curved-area approximation guidance and singular edge wording.
- Clarify kernel chamfer reasons and conditional, concrete full-turn revolve axis/profile workarounds; remove internal references from advice.
- Accept JSON-string object/array arguments centrally, with schema-aware preservation of plain strings and clear malformed-JSON errors.
- Show QymCAD's own English words instead of internal keys: default feature/plane names ("Extrusion", "Plane"), error texts, warnings and undo labels, via the pinned `qymcad-i18n` crate (stored documents keep the keys). A cut that removes nothing adds a direction/height hint.
- Reject false sphere fits on split conical chamfers, including meshes whose vertices lie on only two rings without native circle metadata; flag duplicated native face ids and refuse ambiguous face selections with a full-turn axis/profile workaround.

### Added
- `batch` runs 1–200 modelling steps in order with upfront argument validation, whole-batch rollback and
  one undo entry. Reuses individual tool handlers and rebuild checks; compact results include created
  nodes and final bodies/diagnostics, with full per-step results available through `verbose=true` (ADR 0020).
- Persistent local JSONL call logs on by default for stdio servers, recording received tool arguments before
  execution, timed results with image MIME type and `base64_bytes` instead of image data, protocol errors,
  panics and shutdown; configurable directory,
  opt-out, bounded records/files and startup retention (ADR 0018). Initialize instructions advertise the
  active log's path only after its startup record is written; unavailable cwd metadata is recorded as null
  without disabling logging. Truncation caches unchanged field sizes and caps work at 32 iterations.
- `doc_save` reports `replaced` when the target existed before saving, including explicit-path replacement of another model.
- Selection `and` accepts two or more operands; kind filters reuse topology's native/mesh classification.
  `curve` matches arc/other edges; kind-containing face modifiers store the whole resolved selection as persistent picks (ADR 0013).
- `feature_delete` refuses dependent nodes unless `cascade=true`; deletion is atomic and restores consumed
  source bodies. Session `undo` retains 16 successful modelling calls and restores exact Project/B-rep state;
  document replacement resets history, and saving/exporting files is outside modelling undo (ADR 0008).
- Advisory rebuild warnings identify features whose results acquire excess shells over solids (possible internal
  voids). Opening stale stored geometry warns when rebuilt volume/bounds or shell/solid/face counts differ,
  and checks stored bodies for voids even on clean reopen (ADR 0009).
- Edge selections accept `{"concave": true}` and `{"convex": true}` in compositions, including curved
  junctions such as boss/plate circles; seam and G1 tangent junctions are excluded (ADR 0010).
- Sketch inspection reports the native world frame. Extrude booleans show cut/add/intersect operations in
  the timeline, and cylinder axes report the point at the face's axial centroid (ADR 0011).
- Repository skeleton: Cargo workspace (`qymcad-engine`, `qymcad-mcp`), QymCAD pinned at `v0.1.0-dev.20261001`,
  kernel smoke test, `scripts/check.sh`, AGENTS.md, ADR 0001-0003, FINDINGS F-001..F-015, UPGRADING draft.
- Engine core: `Session` (new/open/save/info, rebuild pipeline, atomic edits, name resolution), parameters
  (lowercase, propagation, usage-checked delete), fully dimensioned rectangle/circle sketches on base/datum
  planes or faces, offset datum planes, extrude with add/cut/intersect/new body, direction and through.
- Golden plate tests incl. the QymCAD GUI parameter-edit path; engine contract tests. FINDINGS F-016, F-017.
- MCP server: stdio JSON-RPC transport (protocol 2024-11-05 .. 2025-06-18), tools `doc_new`, `doc_open`,
  `doc_save`, `doc_info`, `param_set`, `param_delete`, `sketch_create`, `sketch_add` (rect, circle),
  `sketch_info`, `plane_offset`, `extrude`; agent instructions; installed QymCAD.app release check (F-018);
  `--dump-tools` generating docs/TOOLS.md; protocol tests.
- `export` tool / `Session::export`: STEP (exact), STL, 3MF, GLB, OBJ of the result bodies (or selected ones), with
  the app's mesh quality presets; per-body mesh volume in the result. Golden read-back tests for every format.
- `render` tool / `Session::render`: shaded orthographic PNG (iso/top/bottom/front/back/left/right) with feature
  edges, returned as an MCP image item. CPU rasterizer adapted from QymCAD's thumbnail renderer; PNG via `flate2`
  (ADR 0004). FINDINGS F-019..F-021.
- Topology and finishing features (phase 3B). Engine: `topology` (face/edge kinds, geometry, adjacency, seams),
  selections (ids or descriptions mapped onto QymCAD's `refs::Query`) with `select`, `revolve` (sketch/world/datum/
  face axes, add/cut/intersect/new body), `fillet`, `chamfer` (symmetric or two distances), `hole` (plain/
  counterbore/countersink, blind or through), `shell`, `push_face`, `linear_array`, `circular_array`, `mirror`; all
  atomic, every dimension may be an expression. `Session::open` restores faces like the app. MCP tools of the same
  names. Golden tests with hand-computed volumes, GUI-path tests, protocol tests. FINDINGS F-023..F-029
  (notably: stored edge queries break after reopening, so edges are stored as pick lists).
- Sketch entities `line`, `polyline`, `arc` (radius + angles or start/end points), `polygon` (regular; radius +
  angle or a vertex), `slot`, each fully dimensioned from numbers/expressions (`dimensioned: false` leaves them
  free); tools `sketch_constrain` (coincident, horizontal, vertical, parallel, perpendicular, collinear, equal,
  tangent, concentric, midpoint, point_on_line, symmetric, fix; distance aligned/x/y, angle, diameter, radius;
  reference dimensions; refuses over-constraining) and `sketch_remove`; `sketch_info` lists entities, points and
  constraints. Sketch edits rebuild the features built from the sketch. Golden sketch tests incl. the GUI path,
  sketch contract tests, protocol test. FINDINGS F-039..F-044.

### Changed
- Export body result `mesh_volume` is renamed to `mesh_volume_mm3` (mm³), with no compatibility alias.

### Fixed
- Retain only unavailable-source advisories across import retessellation, preventing duplicate sealed-void
  warnings. Stored import B-reps with readable sources no longer require a matching source solid index.
- Restore imported STEP/IGES bodies from embedded sources when their saved B-reps are absent, before
  deciding whether opening requires a rebuild. Warn with the import body name/id for unavailable
  sources even when stored exact geometry exists; retain those warnings across retessellation and
  preserve the timeline, display geometry and source data through save (ADR 0019).
- Extrude and offset-plane descriptions explain why a reverse cut from the stock top gives an exact pocket floor.
- Failed chamfers on revolve-derived bodies include a conditional cone-mouth profile/axis workaround for the pinned OCCT parameterization issue.
- Report tight mesh-derived body bounds after open and rebuild, fixing inflated shelf rim bounds in doc_info, doc_open and render captions.
- Reject spurious spherical mesh fits on planar pocket floors; topology and kind selections report their plane normal.
- Empty composed corner selections report the body's actual inward/outward counts and explain that none
  survives the rest of the selection, including subtraction. A shared check adds "no edge is both" only
  when a positive intersection requires both signs; signs under any other composition do not count (G1c).
  Fillet/chamfer errors repeat the same hint (G1, R3).
- Corner omission notes honor other positive intersection conditions and report no omission for contradictory
  corner signs. Other compositions report only the uncertain body total and corner-filter treatment without
  claiming absence or causation (R1–R2). Coordinate-aware cone fitting excludes eight verified tangent chamfer-chain
  junctions while preserving genuinely uncertain shallow rims (G2, F-063).
- Direction errors list accepted unsigned axes as well as signed axes; incompatible kind errors use lowercase
  public names; sketch datum labels include the timeline name and native plane id (G6).
- Fillet/chamfer descriptions explain kernel tangent-chain propagation beyond selected-edge previews. A formula
  and six derived bevel-plane normals pin the extra blending without an inferred runtime warning; four selected
  long G1 boundaries have 180° internal dihedrals and remove no volume (G5, R4, F-064).
- Analytic cone meridian normals and verified native circle tangents classify 20°, 10°, 5° and 3° countersink
  rims correctly. Corner previews report uncertain omitted edges, including shallow native smoothness below
  ~1.5°; the 0.9° fixture remains omitted. Sparse curved faces retain conservative normal allowances (ADR 0010).
- Empty corner hints name inward/outward and opposite counts, offer `between` only on multi-feature bodies,
  and repeat in fillet/chamfer errors. Topology/adjacency/select ids sort stably.
- Body volume fields consistently use `volume_mm3` at native precision; curved bbox padding is documented
  up to about 0.3 mm. Pathless saves guard model lineage with an explicit `overwrite` override (ADR 0014).
- Extrude documents the native 0.001 mm entry clearance on internal planes; formula tests verify server/GUI
  agreement with top-face cuts. No geometry compensation changes native recipes.
- Sketch/parameter edits share one exact-geometry rollback helper; tests decode asymmetric GLB binary
  positions/indices and winding, and verify per-object geometry/global offsets in multi-body OBJ exports.
- Planar/straight concave/convex corners use numerical allowances, restoring omitted 45° chamfer boundaries
  and 30° wedges while retaining conservative curved uncertainty (ADR 0010, F-053).
- Concave/convex edge filters require a sign above normal/tangent uncertainty at five arc-length samples;
  ambiguous shallow junctions and edges with disagreeing signs are omitted. Cylinder normals preserve the
  facet normal when winding alignment is unreliable; native circle tangents have independent formula and mutation checks (ADR 0010, F-053).
- Undo preserves monotonic ids so removed-body references/picks cannot alias new nodes; snapshots omit embedded
  source payloads and restore them by id. Feature deletion shares the MCP history snapshot instead of copying twice.
- Export and render repeat current document warnings, including sealed-void diagnostics.
- Topology face areas and sketch contour areas are documented as tessellation-based, typically about
  0.1–0.2% below analytic areas for curved geometry.
- Undo returns the tool name/arguments, restored parameter expressions/values and bodies/diagnostics; exhausting evicted history reports
  `history limit reached (16): older calls cannot be undone`.
- Empty concave/convex previews return zero matches and compose as empty sets; fillet/chamfer still refuse
  empty final selections. Empty corner selections include a named-feature `between` hint on multi-feature bodies; fillet/chamfer
  errors repeat the same guidance.
- Disconnected arrays and other multi-solid results no longer trigger void warnings. Stale-open warnings use
  four-decimal metrics, show bbox changes only beyond F-016 tolerance, and identify rebuilt geometry as current.
- `feature_delete` returns every deleted node's id/name/kind, including cascades; its description explains
  dependent refusal versus app relinking. All `doc_info` operation labels use lowercase.
- Saving an empty document over a file containing bodies requires `allow_empty=true`, with both explicit
  and default save paths protected (ADR 0012).
- Array descriptions explicitly state whole-body copying, including holes/cuts, and suggest sketch circles
  plus a through cut for repeated holes. Pinned QymCAD has no native seed-feature/cut-tool pattern (F-027).
- `param_users` labels persisted datum guards as "datum dependency of sketch …" while retaining deletion checks.
- After GUI structural edits (new/re-hosted datum sketches or replacement datum-distance expressions), reopen
  and save through the server before further GUI parameter edits to refresh the persisted guards (ADR 0007).
  This limitation is also stated in `param_set` tool documentation.
- Face-sketch docs explain projected origins and axes; save docs require an existing directory; doc_info docs
  explain in-session OCCT bbox padding and potentially tighter reopened bounds (F-016).
- Datum regressions cover both parameterized ancestors; guard-refresh assertions check obsolete entries;
  top-face area checks allow tessellation inflation only, with corrected mm³-to-mm² explanation.
- Expression-driven offset datums now carry persisted sketch dependencies for server and native GUI parameter
  rebuilds, including chained offsets and legacy files opened in the server. Pocket regressions verify floor
  placement and a top opening; F-017's supposed depth drift was a sealed internal void. Volume tolerances are
  tightened and standalone datum extrusions check cap positions (F1, F-017, ADR 0007).
- Every rebuild path refuses stored edge queries dependent on live bodies whose named edges could not be
  restored after a clean open; errors identify the body and preserve exact Project/B-rep state, including
  unsafe full retries. Named queries and unrelated features remain usable (E1, F-023/F-024).
- Refuse opening unnamed live edges when dirty nodes or missing shapes require regeneration; clean documents
  remain usable and topology reports the issue later (D1, F-023).
- Parameter rollback restores exact project state and original B-reps without regeneration, including pending
  dirty flags on unsolved-sketch rejection (D2, F-034).
- Render tool text states document-wide regeneration-error refusal; architecture documents feature baseline
  repair through parameter/sketch edits and the integrated phase-3 status; protocol coverage includes every
  registered tool (D3/D4/D7).
- The planted-staging-entry regression uses an isolated counter and interleaves unrelated staging allocations
  to remain reliable under concurrent exports (D6).
- Array count expressions use the creation-time whole-number range on parameter edits; negative and fractional
  counts are refused before regeneration and rolled back (C6, F-033).
- Live edges without usable persistent names refuse topology restoration instead of a full rebuild (C4, F-023).
- Rollback goldens compare old body B-rep bytes and topology timeline entries (C2/C3, F-034/F-023).
- Singleton selection unions no longer use a panicking extraction; positive modifier dimensions explicitly
  require finite values (C5/C7).
- Formula-derived goldens cover intersect/through/symmetric extrusions and rectangles on previously pinned
  corners, including server and GUI parameter edits (C8/C11, F-003/F-042).
- Settled server solves refuse a negative-x direction reference; negative-turn operands are parenthesized
  explicitly (C9/C10, F-045).
- Sketch removal protects surviving helpers selectively, retains original self-Midpoints, drops deleted endpoint
  dimensions, and prunes unused angle references after direction removal (C12/C14/C19, F-046).
- Implied Parallel satisfaction uses an angle tolerance independent of line length (C13, F-049).
- Document the coordinate sign-crossing refusal decision, world-space render bbox, current export verification,
  and document-wide output refusal on any regeneration error (C15–C18).
- Line-line distances refuse contradictory rank-dependent parallelism and restore the whole sketch (A1).
- Removing unrelated sketch geometry preserves free direction helpers and their parametric ArcLength dimensions;
  helpers are pruned when their owner is removed (A2, F-046).
- Large finite numbers stay finite in rounded tool output, including nested topology JSON (A3).
- Through-hole creation refuses stock whose bbox diagonal exceeds the fixed 10000 mm depth; tool descriptions
  and findings document the limit and possible blindness after later stock growth (B1/B3, F-028/F-037).
- Feature edits rebuild pending dirty nodes before snapshotting; failed sketch edits restore original live shapes
  without rebuilding old bodies, preserving volume bits and project/shape consistency (B2, F-034).
- Tool argument fields are fully documented; fillet seam handling and asymmetric chamfer side selection now
  match QymCAD's limitations (B4/B5, F-029/F-036).
- Outward-shell golden coverage checks all four spherical corner radii against wall thickness (B6, F-038).
- OpenCASCADE's STEP-writer statistics no longer corrupt the stdio protocol: the server speaks JSON-RPC on a
  duplicate of stdout and redirects fd 1 to stderr (ADR 0005).
- Phase 3B review round 1: non-finite dimensions ("nan", "1e400") refused; extrude/revolve `target` must be a
  current body; edge `largest` ranks by true length; ids validated anywhere in a selection; unions balanced and
  selections budgeted so documents stay saveable; arrays capped at 1000 copies (also on parameter edits);
  a face axis without a body uses the active part's current body; strict `[x, y, z]` and tolerance parsing;
  `ensure_topology` reports a failed rebuild and undoes it; a failed or retried feature no longer rebuilds old
  bodies; fixed datum axes reused; `shell` requires open faces (QymCAD has no closed shell); through holes store
  10000 mm depth and refuse stock whose bbox diagonal exceeds it (later growth can make them blind).
  FINDINGS F-030..F-037.
- `param_set` refuses the names `pi`, `tau`, `e`: QymCAD reads them as constants (F-039).
- Sketches are solved until they settle (one QymCAD solve can stop short, F-040).
- A parameter edit that leaves a sketch unsolved is refused and rolled back (was committed silently).
- Parametric arc ends and polygon rotations are arc-length dimensions from a reference point, so they follow a
  parameter across 90°/180° (QymCAD dimensions keep their side, F-045) and settle in one GUI solve; this also
  covers a parametric radius with a literal angle.
- Removing a sketch entity keeps spline control points (F-046).
- A distance between two lines adds parallelism; reference radius/diameter on arcs is refused (F-047).
- Rect and circle add only independent dimensions (a circle centred on a dimensioned vertex was over-constrained).
- `param_delete` works for an old parameter named `pi`/`tau`/`e`; `sketch_add` refuses an empty list.

### Security
- Check generated dimension formulas against input expression length/nesting limits before storage; refuse
  overflowing negative coordinates, slot half-widths and directed-angle arc lengths atomically (D5, F-031).
- Exports are written via a temporary file and an atomic rename (no write-through of hard links); export and
  render refuse documents with features that did not build (review findings, phase 3C).
- Expressions are bounded (1000 characters, 64 levels of nesting) before QymCAD's recursive evaluator: a deep
  expression from an agent crashed the server (F-031).
- File paths accepted by tools are restricted to their file type (`.qcad`) and optionally confined to
  `QYMCAD_MCP_ROOT` (docs/SECURITY.md). `export` accepts only its format's extensions (.step/.stp, .stl, .3mf,
  .glb, .obj) under the same rules.
