# QymCAD capability coverage

This matrix covers every variant of the four pinned enums `FeatureKind`, `PartTool`, `SketchShape`, and GUI `ConstraintKind`. The `keys` cells are machine-readable comma-separated, fully qualified enum variants; each key must occur exactly once. `—` means no key from those enums (for GUI capabilities without an enum variant). Statuses: `covered`, `partial`, `not covered`, `out of scope`, or `in progress (cover-high)`. MCP tools identify current server coverage; `none` means no tool.

| ID | Capability | Status | Missing / reason | MCP tool(s) | keys |
|---|---|---|---|---|---|
| P1 | Extrude | covered | — | `extrude` | `FeatureKind::Extrude,FeatureKind::Combine,PartTool::Extrude` |
| P2 | Revolve | covered | — | `revolve` | `FeatureKind::Revolve,PartTool::Revolve` |
| P3 | Sweep | covered | — | `sweep` | `FeatureKind::Sweep,PartTool::Sweep` |
| P4 | Loft | covered | — | `loft` | `FeatureKind::Loft,PartTool::Loft` |
| P5 | Fillet | partial | Variable radius at vertices | `fillet` | `FeatureKind::Fillet,PartTool::Fillet` |
| P6 | Chamfer | covered | Symmetric, TwoDist and Leg-and-angle; adjacent support face, evaluated angle/association guards (ADR 0032) | `chamfer` | `FeatureKind::Chamfer,PartTool::Chamfer` |
| P7 | Shell | covered | — | `shell` | `FeatureKind::Shell,PartTool::Shell` |
| P8 | Hole | partial | Sketch-point dependency bridge for global/named edits; unnamed/structural in-app edits can leave holes stale. Native recipe requires all isolated points; dynamic datum/projection or independent face support refused (ADR 0034) | `hole` | `FeatureKind::Hole,PartTool::Hole` |
| P9 | Draft | covered | App range 0 < abs(angle) ≤ 60 on creation and evaluated edits, flip, formula angle, re-pick; planar neutral and face-loss refusals (ADR 0052, F-148, F-149, F-151) | `draft` | `FeatureKind::Draft,PartTool::Draft` |
| P10 | Thread | partial | Auger mode | `thread` | `FeatureKind::Thread,FeatureKind::Auger,PartTool::Thread` |
| P11 | Push face | covered | — | `push_face` | `FeatureKind::PushFace,PartTool::PushFace` |
| P12 | Remove face | covered | Native face healing, persistent face witnesses, failure rollback, save/open and app-path parameter edits | `remove_face` | `FeatureKind::RemoveFace,PartTool::RemoveFace` |
| P13 | Split body | covered | — | `split` | `FeatureKind::SplitBody,PartTool::SplitBody` |
| P14 | Split faces | not covered | Split a face without splitting the body | none | `FeatureKind::SplitFace,PartTool::SplitFace` |
| P15 | Thicken | covered | Solid faces, planar or curved, united with the part; thickness > 0 only (F-150); single frozen face id, re-pick. Sheet sources: the server has no sheet bodies (ADR 0052) | `thicken` | `FeatureKind::Thicken,PartTool::Thicken` |
| P16 | Mirror body | covered | — | `mirror` | `FeatureKind::Mirror,PartTool::Mirror` |
| P17 | Linear pattern | partial | Third direction | `linear_array` | `FeatureKind::LinearArray,PartTool::LinearPattern` |
| P18 | Circular pattern | covered | — | `circular_array` | `FeatureKind::CircularArray,PartTool::CircularPattern` |
| P19 | Primitives | not covered | Box, cylinder, sphere, cone, torus, prism primitives not exposed directly | none | `FeatureKind::Box3,FeatureKind::Cylinder,FeatureKind::Sphere,FeatureKind::Cone,FeatureKind::Torus,FeatureKind::Prism,PartTool::Box,PartTool::Cylinder,PartTool::Sphere,PartTool::Cone,PartTool::Torus,PartTool::Prism` |
| P20 | Face copy | not covered | Surface operation | none | `FeatureKind::FaceCopy,PartTool::FaceCopy` |
| P21 | Surface replace | not covered | Surface operation | none | `FeatureKind::SurfaceReplace,PartTool::SurfaceReplace` |
| P22 | Patch | not covered | Surface operation | none | `FeatureKind::Patch,PartTool::Patch` |
| P23 | Stitch | not covered | Surface operation | none | `FeatureKind::Stitch,PartTool::Stitch` |
| P24 | Surface trim | not covered | Surface operation | none | `FeatureKind::Trim,PartTool::Trim` |
| P25 | Boolean between bodies | covered | — | `body_boolean` | `FeatureKind::BodyBoolean` |
| P26 | Recognise mesh as body | covered | Exact analytic recognition and polyhedron B-rep conversion; caps, ranges, closed-solid gate, formulas, persistence, downstream B-rep tools, batch and undo | `recognise` | `FeatureKind::MeshRecognised,FeatureKind::MeshSolid,PartTool::Recognise` |
| P27 | Offset surface | not covered | Surface operation | none | `FeatureKind::OffsetSurface,PartTool::OffsetSurface` |
| P28 | Move / rotate body | covered | Parametric dx/dy/dz; literal angle/pivot; accumulated rotation onto nonliteral shift formulas refused (ADR 0033) | `move` | `FeatureKind::Move` |
| D1 | Datum plane with offset | covered | — | `plane_offset` | `FeatureKind::Plane,PartTool::DatumPlane` |
| D2 | Tilted datum plane | covered | Tilt X/Y and Turn; ancestry barriers and global/named drivers (ADR 0031) | `plane_offset` | — |
| D3 | Datum axis | partial | Axis only inline in revolve and circular array; no standalone datum axis | `revolve`, `circular_array` | `FeatureKind::DatumAxis,PartTool::DatumAxis` |
| D4 | Datum point | not covered | Coordinates or vertex reference | none | `FeatureKind::DatumPoint,PartTool::DatumPoint` |
| S1 | Sketch geometry | covered | Line, rect, circle, arc, polygon, slot, ellipse, spline, text | `sketch_add` | `FeatureKind::Sketch,SketchShape::Line,SketchShape::Rect,SketchShape::Circle,SketchShape::Arc,SketchShape::Polygon,SketchShape::Slot,SketchShape::Ellipse,SketchShape::Spline,SketchShape::Text` |
| S2 | Construction geometry | partial | Cannot toggle an existing curve | `sketch_add` | — |
| S3 | Dimensions | covered | Linear, angular, radius, diameter | `dimension_set`, `sketch_constrain` | — |
| S4 | Sketch constraints | covered | All 12 GUI constraint actions represented | `sketch_constrain` | `ConstraintKind::Coincident,ConstraintKind::Horizontal,ConstraintKind::Vertical,ConstraintKind::Parallel,ConstraintKind::Perpendicular,ConstraintKind::Equal,ConstraintKind::Fix,ConstraintKind::Collinear,ConstraintKind::Concentric,ConstraintKind::Tangent,ConstraintKind::Symmetric,ConstraintKind::Midpoint` |
| S5 | Sketch edits | covered | Trim, extend, break, corner, offset, mirror | `sketch_edit` | — |
| S6 | Sketch point | covered | — | `sketch_add`, `sketch_info`, `sketch_remove` | `SketchShape::Point` |
| S7 | Project body geometry into sketch | covered | — | `sketch_project` | — |
| S8 | Move / copy / rotate sketch curves | covered | One-shot native edits; copy is unlinked; dependent rebuild and solver placement are checked atomically | `sketch_edit` | — |
| S9 | Sketch arrays | covered | Linked linear and circular copies; strict 2..=1000 counts and 1000 total-copy cap; native sector spacing retained | `sketch_pattern` | — |
| S10 | Delete sketch geometry | covered | — | `sketch_remove` | — |
| T1 | Parameters and formulas | covered | — | `param_set`, `param_delete` | — |
| T2 | Edit feature | covered | RemoveFace, fillet, chamfer, shell and push_face whole-selection re-picks; hole re-placement is out of scope (move a hole by editing its sketch/offset dimensions) | `remove_face`, `fillet`, `chamfer`, `shell`, `push_face`, `dimension_set` | — |
| T3 | Delete feature and undo | covered | — | `feature_delete`, `undo` | — |
| T4 | Rename feature, sketch, body, Part and joint | covered | Separate native labels, typed Part/joint refs, metadata-only history, resolver aliases, persistence and batch/undo (ADR 0042/0051) | `rename` | — |
| T5 | Reorder feature | partial | Checked native same-parent Up/Down for single-output B-rep features, including consumed/suppressed/tail nodes; rollback, exact refusal restore and batch/undo. Excludes mesh-born/multi-output movers, sketch/datum targets, cross-parent/arbitrary placement, moves that would release a projection and projected-spline geometry changes; native chain limitations remain (ADR 0050) | `feature_reorder` | — |
| T6 | Suppress / rollback history | covered | B-rep feature suppress/unsuppress and start/end/after rollback, native insertion/consumer remapping, exact refusal restore, batch/undo, persistence and saved-file app parameter edits (ADR 0046/0047). Move reuse preserved (ADR 0033); mesh-born Move suppression refused (F-128) | `feature_suppress`, `history_rollback`, `doc_info` | — |
| T7 | Visibility and body color | partial | Body visibility and lineage colour saved (ADR 0044); colour reaches 3MF/GLB/STEP with imported overrides, and export/render have opt-in `visible_only` (default false; ADR 0045). Sketch visibility not persisted by the pinned app | `appearance`, `doc_info`, `export`, `render` | — |
| T8 | Measurement | covered | Native support-geometry distances, acute unsigned angles, lengths and radii; arcs use full circles for pairs. No general trimmed B-rep minimum distance, area/mass or signed angles; spline pairs, unsupported curved faces and fallback anchor pairs refused (ADR 0043) | `measure`; `topology`, `doc_info` for existing area/volume reports | — |
| I1 | STEP / IGES import | covered | Native source-backed part trees, selective solids, literal scale and embedded originals | `import` | `FeatureKind::Import` |
| I2 | Mesh import | covered | STL, OBJ, PLY, glTF/GLB, 3MF and AMF import, declared units, caps, embedded source, mesh move/export and persistence | `import_mesh` | `FeatureKind::MeshPiece` |
| I3 | DXF / SVG sketch import | covered | Native DXF/SVG parsing, literal scaled editable sketches, closure, reports, units, and downstream profiles | `sketch_import`; `golden_sketch_import.rs` | — |
| E1 | Export STEP, STL, 3MF, GLB, OBJ | covered | — | `export` | — |
| E2 | Export IGES, PLY, AMF | not covered | — | none | — |
| E3 | Export sketch DXF / SVG | not covered | — | none | — |
| X1 | Assemblies | partial | Root-child multi-body Parts, placement/grounding, planar-face Rigid/Slider, absolute numeric/formula travel, release and native inspection/persistence/batch/undo (ADR 0051); live clones of single-body Parts (`part_instance`, F-146 refuses multi-body sources) and deep copies of any Part (`part_copy`) (ADR 0053). Part deletion as the app's tree Delete (`part_delete`; cloned sources and active rollbacks refused, F-154/F-156) (ADR 0054). Joint limits as the app's Limits section (`joint_limits`, `joint_create.limits`; clamping reported, min > max refused, F-157). Excludes ComponentPattern, Piece/MirrorPart, nested assembly edits, Revolute/other joint creation, Group/Width/Tangent/relations and collision checks. Existing unsupported records/limits are preserved and reported. Native app-data path tested; QymCAD.app check of the saved cabinet passed for .12 | `part_new`, `part_select`, `part_place`, `ground`, `joint_create`, `joint_set`, `joint_delete`, `assembly_info`, `rename`, `part_instance`, `part_copy`, `part_delete`, `joint_limits` | `FeatureKind::PartInstance,FeatureKind::ComponentPattern,FeatureKind::Piece,FeatureKind::MirrorPart` |
| X2 | Part library | out of scope | Library/external-file lifecycle remains excluded from root-level package A | none | — |
| X3 | GUI interface | out of scope | GUI-only languages, icons, layout, shortcuts, reports, updates and view | none | — |
