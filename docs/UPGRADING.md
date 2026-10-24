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

- (v0.1.1: F-065's and F-024's pins became `native_sphere_fit_rejects_a_single_coplanar_floor_mesh` and
  `stored_edge_query_rounds_one_rim_after_reopen_in_the_app`, which now guard the upstream fixes.)
- `golden_revolve_chamfer::upstream_negative_side_shelf_refuses_each_mouth_but_axis_reversal_recovers` —
  F-067's indirect cone-frame mouth-chamfer failure; keep the equivalent-geometry successful controls.

Lessons from v0.1.1-dev.20261026 (ADR 0021):
- Public kernel doors may take the non-reentrant kernel gate themselves (F-073): list every gated public function
  you call and make sure no caller holds the gate around it.
- Re-check `tests/common/mod.rs`'s emulation of the app's parameter edit against the new app
  (`apply_param_edit`, `mark_changed_params_dirty`); an over-generous emulation hides app-side defects.
- Before removing a workaround, disable it and run the whole suite on the new release; remove it only when its
  tests stay green, and add an assertion that is red with the workaround restored.
- Capture the user's real saved parts with the old server first (volumes, bboxes, face/edge ids), then compare
  after opening and after a full rebuild on the new release.
- A `schema_version` that does not change says nothing about compatibility: load a file written by the new
  release with the old release's loader.

Search `rg 'upstream_|pinned upstream|pins the upstream' crates/engine` on each upgrade to catch newly added
pinning tests. `descriptive_fillet_survives_an_upstream_edit` is a correctness regression about editing an
ancestor, not a test that expects an upstream defect; keep it.
