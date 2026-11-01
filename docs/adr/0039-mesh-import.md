# ADR 0039: Mesh import as embedded MeshPiece bodies

- Status: accepted (2026-11-01).
- Related: ADR 0008, ADR 0019, ADR 0020, ADR 0037; F-008, F-024.

## Context

QymCAD's import door reads STL, OBJ, PLY, glTF/GLB, 3MF and AMF into display meshes. Unitless STL/OBJ/PLY files are interpreted using the unit selected by the person; glTF uses metres and +Y-up, while 3MF and AMF apply their declared unit. The native project stores the original source and each piece as a `MeshPiece` with its mesh, faces, placement and group ancestry. A mesh piece has no B-rep and is therefore not a recognised solid.

A synchronous MCP server must reject oversized or malicious input before reader and project work. The limits are 16 MiB input metadata, 64 MiB total declared uncompressed 3MF central-directory sizes, and 200,000 decoded triangles. Binary STL count and length are checked before the STL reader; ZIP sizes are checked before decompression; other triangle totals are checked before project mutation.

## Decisions

1. Expose `import_mesh` with `path`, `format` (`auto`, `stl`, `obj`, `ply`, `gltf`, `glb`, `3mf`, `amf`), optional unit (`mm`, `cm`, `m`, `in`, `ft`), positive finite one-shot `scale`, and optional group name. Auto and explicit format selection follow the extension exactly; mismatches refuse.
2. Apply the requested unit factor only to STL/OBJ/PLY. glTF/GLB uses the pinned reader's metre and Y-up conversion. 3MF/AMF uses the pinned reader's declared unit. An explicit unit for glTF/GLB is refused because it has no additional unit choice.
3. Preflight metadata and container limits before calling a reader. Count decoded triangles before inserting anything into the project. Empty meshes, non-finite coordinates and invalid reader results refuse atomically.
4. Insert one `MeshPiece` per reader piece through `add_source`, `add_mesh`, `set_body_faces` and `import_tree_as_parts`. Preserve piece names, colours, placements and `FileGroup` ancestry. Store source bytes in the project and retain `MeshPiece.scale`.
5. Report source and tree ids, piece body/name, triangle and vertex counts, unit, scale, bbox, warnings and rebuild diagnostics. Mesh bodies are available for move, export and render. Booleans, fillets and exact STEP require stage C3 recognition and are not promised by this stage.
6. The existing registry modelling boundary supplies batch and undo. `doc_info` reports raw mesh bodies from their mesh volume and world-space bounds even though they have no live B-rep.

## Consequences

Saved `.qcad` files retain both the mesh and original source bytes, so reopening does not need the external mesh path. Unit and scale are literal import decisions, not parametric drivers. The 200,000-triangle cap admits the measured 200,000-triangle binary STL probe while rejecting larger decoded meshes; the 64 MiB central-directory cap prevents a small compressed 3MF from declaring an unbounded expansion.

I2 is complete for mesh import; C3 recognition is specified separately in [ADR 0040](0040-mesh-recognition.md). No B-rep is created by this tool, and every B-rep tool refuses a raw mesh body until `recognise` is called (see below).

Evidence: `tasks/research-medC.md` I2, `tasks/research-medC-probes.md` sections 2–3, `crates/engine/tests/mesh_import.rs`, and the generated `import_mesh` schema in `docs/TOOLS.md`.

Every B-rep operation on a mesh body (topology, select, modifiers, arrays, mirror, split, boolean, sketch targets)
is refused at one point, `Session::source_body`/`topo_body`, with one text naming the body as an imported mesh.
Only `move` takes a mesh body (`source_body_or_mesh`); render and mesh export read the stored triangles.

Security addendum (F-126): QymCAD's readers open files named inside a mesh and inflate ZIP parts without a limit.
`preflight` reads the mesh once (bounded), confines glTF buffer URIs (from every GLB JSON chunk) and OBJ material
libraries (parsed exactly like the reader) to single names in the mesh's folder opened with `O_NOFOLLOW|O_NONBLOCK`,
reads them once, counts them toward the 16 MiB cap, and inflates 3MF/zipped AMF through a counter capped at 64 MiB.
The native reader, which only takes a path, then runs on a private 0700 copy of exactly those bytes in the temp dir,
so nothing swapped in after the checks reaches it. Secondary files in subfolders are refused (rare; flatten them).

