# ADR 0028: Import STEP and IGES as source-backed part trees

- Status: accepted (2026-10-28).

## Context

The pinned QymCAD app reads exact files through `read_exact_tree(format, path, 0.5)`, embeds original bytes, creates source-backed Import nodes through `Project::import_tree_as_parts`, and retains the file's component names and placements. The model server needs the same structure while preserving safe parsing and atomic edits. Import scale is a literal uniform correction beyond OCCT's unit conversion to millimetres.

## Decision
- Expose `import({path, scale?, solids?, name?})`; infer format strictly from `.step`, `.stp`, `.iges`, or `.igs`, and accept only finite positive scales.
- Read the external bytes first, stage them in a unique `create_new`, mode-0600 temporary file with normalized extension, and parse the exact tree under `kernel_gate`. Never call `raise_imports`; it stages insecurely and parses outside the gate.
- Preflight exact solids and selected zero-based source indexes before editing. Then atomically embed the unchanged bytes, add each built mesh/faces and live exact shape, transform selected tree nodes through `import_tree_as_parts`, and apply the literal scale to imported bodies and component translations about the source origin.
- Preserve native node names, tree, repeated products, colours and placements. Optional `name` renames the imported tree's first component. A source with a single imported part remains a separate part, consistent with app behavior; it does not become the server's active modelling part. All bodies are available by id and component name; reported imported-body names resolve in tools that accept body references, and features that require an explicit body must receive it. If an alias conflicts with another live object, resolution requires an explicit id.
- Explicit `fillet`, `push_face` and `body_boolean` create nodes in the input bodies' part; a face sketch belongs to its host body's part, and extrude creates its node in the sketch's part. Temporarily enter the owner context for native creation and restore the previous active context afterwards. This satisfies native component isolation without moving imported bodies or redirecting later implicit edits. Imported body names reported by import/doc_info resolve across body-reference tools; alias conflicts still require an id.
- Reports include source id, created root, selected source indexes, body ids/names, scaled volumes and world-space bounds. Save embeds source bytes; external paths are not retained as dependencies.

## Alternatives considered
- Flatten into the current part: conflicts with the app's component/part model and discards source tree placements/names.
- Call upstream `raise_imports`: unsafe predictable staging and parsing outside the non-reentrant kernel gate.
- Treat `scale` as unit conversion: OCCT already normalizes units, while the app presents scale as a separate literal correction.

## Consequences

Imported bodies are independent component parts and can be addressed explicitly by body id/name for topology, select, face sketches, modifiers, booleans, batches and export. Default-current-body features continue to target only the existing active part. Selective import keeps original source solid indexes, and embedded bytes are sufficient for restoration. Evidence and test/mutation commands are recorded in `tasks/cover-5-evidence.md` and `tasks/cover-5-report.md`.
