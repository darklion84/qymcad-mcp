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
| P8 | Hole | partial | By-sketch points | `hole` | `FeatureKind::Hole,PartTool::Hole` |
| P9 | Draft | not covered | Face draft up to 60° | none | `FeatureKind::Draft,PartTool::Draft` |
| P10 | Thread | partial | Auger mode | `thread` | `FeatureKind::Thread,FeatureKind::Auger,PartTool::Thread` |
| P11 | Push face | covered | — | `push_face` | `FeatureKind::PushFace,PartTool::PushFace` |
| P12 | Remove face | not covered | Heal body after face removal | none | `FeatureKind::RemoveFace,PartTool::RemoveFace` |
| P13 | Split body | covered | — | `split` | `FeatureKind::SplitBody,PartTool::SplitBody` |
| P14 | Split faces | not covered | Split a face without splitting the body | none | `FeatureKind::SplitFace,PartTool::SplitFace` |
| P15 | Thicken | not covered | Thicken from a face | none | `FeatureKind::Thicken,PartTool::Thicken` |
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
| P26 | Recognise mesh as body | not covered | Requires mesh import (I1) | none | `FeatureKind::MeshRecognised,PartTool::Recognise` |
| P27 | Offset surface | not covered | Surface operation | none | `FeatureKind::OffsetSurface,PartTool::OffsetSurface` |
| P28 | Move / rotate body | not covered | — | none | `FeatureKind::Move` |
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
| S8 | Move / copy / rotate sketch curves | not covered | — | none | — |
| S9 | Sketch arrays | not covered | Linked linear and circular copies | none | — |
| S10 | Delete sketch geometry | covered | — | `sketch_remove` | — |
| T1 | Parameters and formulas | covered | — | `param_set`, `param_delete` | — |
| T2 | Edit feature | partial | Re-pick edges and faces | `dimension_set` | — |
| T3 | Delete feature and undo | covered | — | `feature_delete`, `undo` | — |
| T4 | Rename feature | not covered | — | none | — |
| T5 | Reorder feature | not covered | — | none | — |
| T6 | Suppress / rollback history | not covered | — | none | — |
| T7 | Visibility and body color | not covered | Hide a body or sketch; set a body colour (it would reach 3MF) | none | — |
| T8 | Measurement | partial | No distances or angles between elements | `topology`, `doc_info` | — |
| I1 | STEP / IGES import | covered | Native source-backed part trees, selective solids, literal scale and embedded originals | `import` | `FeatureKind::Import` |
| I2 | Mesh import | not covered | STL, OBJ, PLY, glTF, 3MF, AMF | none | `FeatureKind::MeshSolid,FeatureKind::MeshPiece` |
| I3 | DXF / SVG sketch import | not covered | — | none | — |
| E1 | Export STEP, STL, 3MF, GLB, OBJ | covered | — | `export` | — |
| E2 | Export IGES, PLY, AMF | not covered | — | none | — |
| E3 | Export sketch DXF / SVG | not covered | — | none | — |
| X1 | Assemblies | out of scope | ADR 0022 deferred assemblies; Piece promotes a split/cut body into its own associative part, and MirrorPart creates an associative mirrored part with independent placement | none | `FeatureKind::PartInstance,FeatureKind::ComponentPattern,FeatureKind::Piece,FeatureKind::MirrorPart` |
| X2 | Part library | out of scope | Part of assemblies scope | none | — |
| X3 | GUI interface | out of scope | GUI-only languages, icons, layout, shortcuts, reports, updates and view | none | — |
