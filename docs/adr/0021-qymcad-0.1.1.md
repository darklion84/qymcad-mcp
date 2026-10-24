# ADR 0021: Follow QymCAD v0.1.1-dev.20261026

- Status: accepted (2026-10-24).

## Context
QymCAD published v0.1.1-dev.20261026 (f25f27d), 275 commits after the probe of 2026-10-14 and with eight of
our nine reported defects fixed. The release changes public kernel APIs (stage A, F-073), the document format
(`Constraint::Orientation`, B-rep QYMB v3, same `schema_version` 2), name matching, through-all extents and the
blend fallback. The server's rule since ADR 0003 is to match the app release the person has, and to keep only
workarounds whose cause still exists.

## Decision
- **Pin** all QymCAD crates to the tag; `QYMCAD_VERSION` 0.1.1-dev.20261026; server version 0.2.0 because files
  it saves do not open in QymCAD 0.1.0. `rust-version` 1.89 (the release's dependencies need it).
- **Names are case-sensitive, as in QymCAD** (5dcaa65). The server stores names and expressions as given. A new
  parameter whose name equals an existing parameter or named dimension ignoring case (Unicode lowercase mapping, not
  full case folding), or exactly equals a named dimension, is refused, saying which kind of name it collides with;
  an unknown name that matches a known one ignoring case is explained with a did-you-mean. Documents that
  already hold such twins open unchanged and `doc_info` warns; a parameter formula that no longer evaluates is
  explained there too. Lowercasing (F-001) existed only for the old GUI
  scheduling bug, now fixed.
- **Through-all follows QymCAD** (5ea7514): it goes to the stock boundary on the chosen side; the symmetric
  direction spans both sides. Schema text and instructions say so.
- **A fillet must be a fillet** (F-074): a join between the blends of two edges that share no vertex, sharp by the
  kernel's flag or (cylinders) more than 0.1° off tangent, is where overlapping blends cross; QymCAD 0.1.1 can
  return them without an error, and the server refuses them. Known gaps are listed in F-074. Mitres between
  blends of edges sharing a vertex and smooth limiting joins are accepted. The check runs after a fillet rebuilds
  and on open for fillets kept from the file. Creation and parameter edits roll back; found outside an edit, the
  refusal is kept in `regen_errors` in the same words, so doc_info shows it and export refuses, while unrelated
  parameter edits still go through. Chamfers past their faces are left to QymCAD (predictable plane intersection).
- **Workarounds removed** where a test proves the native fix: lowercasing (F-001), repeated sketch solves
  (F-040), slot and line-on-arc tangency rewrites (F-041), entity-removal markers (F-046), the arc reference
  radius refusal (F-047). Tests that pinned fixed upstream defects now assert the fixes (F-024, F-065, F-068).
- **Kept:** datum dependencies and barriers (F-017 still fails without them, 11 tests), duplicate-face-name
  guards (files saved by 0.1.0 still hold duplicates, F-068), the sphere residual check (F-065 tolerance unchanged
  for curved faces), expression and array limits as policy (F-031, F-033), pick-list storage for blends (F-024;
  descriptions in BACKLOG), stored-edge-query refusal on an empty pool for legacy documents.
- The test emulation of an app parameter edit follows the 0.1.1 app (`apply_param_edit`: solve sketches that
  read names, mark only what mentions a changed name).

- **Splines of 0.1.0 documents** are dropped by QymCAD 0.1.1 on load (F-075); the server warns on open.

## Consequences
- Files saved by 0.2.x need QymCAD 0.1.1; the app 0.1.0 refuses them (`Unexpected variant named Orientation`)
  or cannot read their B-reps. The startup check already warns when the installed app differs.
- The user's seven parts saved by the server on 0.1.0 open in 0.1.1 with identical volumes, bboxes and face/edge
  ids, and a full rebuild reproduces their volumes to 1e-11; QymCAD may add "a reference was found by place, not by
  name" or "n of the m edges could not be taken" (the old app dropped the same edges silently; volumes match the old
  server). This is evidence for those parts, not a guarantee: recipes whose semantics changed (a one-sided
  through-all from an interior plane, names in the wrong case, 0.1.0 splines, fillets/chamfers/planes that the release
  notes say may lie differently) can rebuild differently.
- A formula that relied on case-insensitive names (`T` read as `t`) now fails with an explanation.
- A forward or reverse through cut on an interior plane removes less than before (one side only).
