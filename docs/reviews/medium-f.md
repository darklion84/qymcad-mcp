# Review log: package F (pattern instances, sketch deletion metadata, active assembly, indexed dimensions, plane_offset docs)

Authors: Codex (gpt-6.1-sol) for research and most of F5; Claude (Opus 5.5) finished F5 and wrote F3, F1, F2, F4 and the fixes below.

| Round | Scope | Verdict | Findings | Fixed in |
|---|---|---|---|---|
| acceptance of F5 (Claude) | pattern settlement | fixed | Codex restored `next_id` on a failed batch, weakening F-056: reverted, the test normalises `next_id` | b4d6344 |
| live test (Claude) | old .9 against new on holder copies | fixed | schema text of `dimension_set` still promised unique references | 6d916bf |
| review F (Codex) | main..85e1efe | FIX | R1 high: projected pattern source moved by the rebuild leaves instances stale, success reported; R2 medium: explicit nested contour pick refused; R3 medium: root navigation saved as `None` not treated as root; R4 medium: formula on a second shared row checked under the first row's name, false cycle; R5 low: `plane_offset` text wrong for a turned datum parent | 6d916bf |

All findings were reproduced first: regressions in golden_pattern_source (`f5r_*`), golden_assembly_active, golden_dimension_index are red in `tasks/FR-logs/red-before-fix.log`, except `f5r_projected_refresh_undoes_in_one_step`, whose red there came from a test error (no undo snapshot); it was corrected and is not failing-first evidence. Own mutation: settling without the second rebuild turns two `f5r_*` tests red (`tasks/FR-logs/m1*`).

Live MCP test over stdio, copies of the holder, release .9 against the branch: `bed_frame_depth` 60→70 gives hex columns -24.55/-37.05 as computed by hand (.9 mixes -19.55/-24.55/-32.05/-37.05, same volume 109789.871); `param_delete floor_n` after deleting its sketch succeeds once `floor_z0` is gone (.9 refuses); cut in the GUI file with the assembly active 110149.274→109898.637, same after reopen (.9 refuses); indexed row of a triangle gives volume 70, failed batch restores `doc_info`; plane frames match the description — PASS.

QymCAD.app check: pending.
