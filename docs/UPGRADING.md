# Upgrading the pinned QymCAD release

Do this only for a published QymCAD release (the app ships as release builds; ADR 0003).

1. Read the QymCAD changes between the old and new tag:
   `gh api repos/QymIs-Tech/QymCAD/compare/<old>...<new> --jq '.commits[].commit.message'`.
   Note anything touching `qymcad-core` model/regen/sketch, `qymcad-io` project files, or `qymcad-kernel`.
2. Change the tag in all four `[workspace.dependencies]` entries of `Cargo.toml` and `QYMCAD_VERSION` in
   `crates/engine/src/lib.rs`. Run `cargo update -p qymcad-core -p qymcad-io -p qymcad-kernel -p qymcad-testkit`.
3. `scripts/check.sh`. Fix compile errors in `crates/engine` only.
4. Re-verify every entry in docs/FINDINGS.md: the tests named as evidence must pass; re-read the `source:`
   references; update the **Version** line of each entry you confirmed, delete entries that no longer hold
   (and the workarounds for them).
5. Install the same QymCAD.app release; run the manual GUI check: open the golden parts saved by
   `cargo test -p qymcad-engine -- --ignored gui_artifacts` (phase 1+), change a parameter, confirm the rebuild.
6. Update README's compatibility table and CHANGELOG; release a new minor version.
