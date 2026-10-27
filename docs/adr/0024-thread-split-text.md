# ADR 0024: Thread, split and sketch text for printing

- Status: accepted (2026-10-26). The user approved the plan and option 1 for text (fixed height).
- Related: tasks/PLAN-print.md, tasks/research-print.md (stage 0), ADR 0022, ADR 0023, F-024, F-069/F-073, F-074, F-081.

## Context

QymCAD v0.1.1-dev.20261026 has three capabilities the server does not expose, and each is useful for 3D
printing. A thread (`add_thread`) carries a print fit. A split (`add_split_body`) cuts a part that does not fit
the bed. Sketch text (`add_sketch_text`) labels a part. Stage 0 traced the app's recipe for each and ran release
probes through save, load and the app's parameter-edit path.

## Decisions

### Thread (`thread` tool)

1. **Canonical numeric arguments plus an alias.** `standard`, `nominal` and `pitch` are the canonical fields
   (Num; `pitch` 0 takes the standard's coarse pitch). `size: "M8" | "M8x1.25" | "Tr16x4"` is an exclusive
   alternative, normalised to the same native fields. A length is never implied by `size`.
2. **One persistent rim.** `edge` resolves to exactly one circular edge of the source body (by id or
   description, evaluated once). The native persistent edge id is stored, as the app stores it; there is no
   dynamic re-selection. Stage 0 verified that the rim id survives the stock's diameter and length edits.
3. **The side is detected.** `kind: "auto" | "external" | "internal"`, default `auto`. The kernel decides the
   side from the material on every rebuild. An explicit kind is an assertion: if it disagrees with the
   detected side, the call is refused. The saved bool matches the detected side.
4. **The app's print defaults, unless they bury the groove.** `lead_in`/`lead_out` 1.5 mm, as the app's
   command seeds them. `length` is required. The app's fit is 0.2 mm per side, but on the pinned release a fit
   larger than the groove's width can hold moves the whole groove into the stock by the radial relief
   (`occt_helical.cpp:225-249`). The groove overshoots the surface by only 0.15·P (`thread.rs:510`). When the
   relief reaches the overshoot, the groove ends up under a stock skin: an M8 thread at fit 0.2 has a relief of
   0.233 mm against an overshoot of 0.1875 mm, so its outside stays a plain Ø8 cylinder (stage-1 section test).
   So (amended 2026-10-26, after stage-1 evidence):
   - the default fit is 0.2, lowered to the largest fit whose relief stays at least 0.02·P inside the overshoot.
     The fit used and the reason are reported. For external M8 the exact bound is 0.159444 mm
     (stage-1 verification corrected the earlier approximate 0.17 estimate);
   - an explicit fit beyond that limit is refused, and the message names the limit;
   - every later edit is checked again, through the same rebuild validator as `starts`;
   - the relief comes from the public `thread::radial_relief`. The 0.15·P overshoot is a native constant and is
     recorded in FINDINGS.

   This is an upstream defect, a report candidate that goes out only with the user's yes.
5. **Formulas on every native key.** `length`, `lead_in`, `lead_out`, `nominal`, `pitch`, `fit`, `crest_r`,
   `root_r`, `angle`, `depth` and `starts` are written with the app's `write_feat_formula`. `starts` must
   evaluate to an integer 1..64, and this is checked again on every later edit.
6. **The result reports basic geometry, not tolerances.** Standard basic diameters, the effective pitch and
   lead, and the fit with its radial relief are reported as such. A fit is a modelling clearance, not an ISO
   tolerance class, and is not proof that a printed pair screws together.

### Split (`split` tool)

7. **The piece count is fixed at creation.** It is counted on the live B-rep through `ask::split_pieces`,
   called without an outer gate. A later edit that would change the count is refused atomically, as the app
   refuses it. Nothing is recounted or reallocated.
8. **Plane direction.** For `offset`, XY points along +Z, XZ along **+Y** and YZ along +X. This is the native
   operation plane; it differs from the XZ sketch normal (−Y). A datum plane or a face uses its own normal.
9. **Pieces are bodies in their own right.** Numeric ids and names resolve to any output body, not only the
   node's first body. Missing-B-rep detection on open walks every output. Piece names are native body names
   (`set_mesh_name`), given by `piece_names` or defaulting to `<feature name> 1..n`. `export` with `bodies`
   writes any piece on its own; one file per call.

### Sketch text (`sketch_add` type `text`)

10. **Fixed height (user's choice, option 1).** Text height is a number. A formula or a parameter name is
    refused. On the pinned release a height edit in the app re-matches the baked glyph contours by centroid,
    so an extruded text silently swaps letters and counters (stage 0: `ABCDEFGHIJ` H10→20 gives V 1301.36
    instead of 1210.96, errors empty). This is the app's own defect: no server-side repair can fix an edit
    made in the app. To change the size, the agent recreates the text. `at` and `angle` are numbers too: the
    native record has no formula path for them.
11. **A deterministic font.** When the font is omitted, it is Arial, face 0, at
    `/System/Library/Fonts/Supplemental/Arial.ttf`. `font: {path, index?}` chooses another font. A missing
    font is an error, with no substitution by family. This intentionally differs from the app's fc-match-first
    default. The glyphs are baked into the file, so it opens without the font. Retyping the text in the app
    needs the recorded path.
12. **Every glyph or nothing.** Text whose letters the font cannot write is refused, with the missing letters
    listed. The core outliner would otherwise skip them silently.
13. **Extrusion through the existing tools.** `extrude` with its usual ops. Profiles default to the glyph
    outlines with their counters kept as holes, never "first contour" or "all contours". Text ids are listed
    by `sketch_info` and removed by `sketch_remove` (`delete_sketch_text`). Text is not given an item name:
    ADR 0023's refusal of new item names still applies.

## Consequences

- The release cannot claim parametric text height. The defect goes upstream (only with the user's yes), with
  the stage-0 reproduction kept as a regression fixture for the day it is fixed.
- Split adds body resolution that later multi-output features also need.
- An auger, alignment pins at a split and text on a curve stay out of scope.
