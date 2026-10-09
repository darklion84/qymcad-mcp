# Upgrading the pinned QymCAD release

Do this only for a published QymCAD release (the app ships as release builds; ADR 0003).

1. Read the QymCAD changes between the old and new tag:
   `gh api repos/QymIs-Tech/QymCAD/compare/<old>...<new> --jq '.commits[].commit.message'`.
   Note anything touching `qymcad-core` model/regen/sketch, `qymcad-io` project files, `qymcad-kernel`, or the English `qymcad-i18n` catalogue/formatter.
2. Change the tag in all five QymCAD `[workspace.dependencies]` entries of `Cargo.toml` and `QYMCAD_VERSION` in
   `crates/engine/src/lib.rs`. Run `cargo update -p qymcad-core -p qymcad-io -p qymcad-kernel -p qymcad-testkit -p qymcad-i18n`.
3. `scripts/check.sh`. Fix compile errors in `crates/engine` only.
4. Re-verify every entry in docs/FINDINGS.md: the tests named as evidence must pass; re-read the `source:`
   references; update the **Version** line of each entry you confirmed, delete entries that no longer hold
   (and the workarounds for them).
5. Install the same QymCAD.app release; run the manual GUI check: open the golden parts saved by
   `cargo test -p qymcad-engine -- --ignored gui_artifacts` (phase 1+), change a parameter, confirm the rebuild.
6. Update README's compatibility table and CHANGELOG; release a new minor version.

Defect-pinning tests assert the old upstream failure deliberately. During step 4, review these before
interpreting a red result as a server regression; remove the defect-pinning assertion/test when the upstream
fix is confirmed, update its finding, and retain the companion server correctness regressions:

- `golden_acceptance::upstream_sphere_fit_accepts_a_single_coplanar_floor_mesh` — F-065's planar mesh
  misidentified as a sphere; keep `shelf_floor_is_planar_after_open_and_rebuild`.
- `golden_features::stored_edge_query_rounds_everything_after_reopen_upstream_bug` — F-024's reopened
  query broadening to both cylinder rims; keep the one-rim server open/edit and pick-persistence tests.
- `golden_revolve_chamfer::upstream_negative_side_shelf_refuses_each_mouth_but_axis_reversal_recovers` —
  F-067's indirect cone-frame mouth-chamfer failure; keep the equivalent-geometry successful controls.

Search `rg 'upstream_|pinned upstream|pins the upstream' crates/engine` on each upgrade to catch newly added
pinning tests. `descriptive_fillet_survives_an_upstream_edit` is a correctness regression about editing an
ancestor, not a test that expects an upstream defect; keep it.
