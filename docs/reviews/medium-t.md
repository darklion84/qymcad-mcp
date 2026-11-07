# Package T review log

Subject: `feature_reorder` (ADR 0050, F-142), branch medium-t on top of medium-f.

| Round | Subject | Verdict |
|---|---|---|
| 1 | `medium-f..9b6fbd5` | FIX: R1, R2, R3 medium; R4 low |
| 2 | `9b6fbd5..71390ae` | ACCEPT, no findings reproduced |

## Round 1 findings, fixed in 71390ae

- **R1:** a move could release an ordinary projected circle while reporting success. Any released live projection now refuses.
- **R2:** an unrelated forward measurement already in the file blocked a legal exchange. Only a forward relation the move introduces refuses.
- **R3:** a changed spline source id was refused as changed geometry, even for an identity Move. The rebuilt curve is compared instead; the source-id pre-check is gone.
- **R4:** no test covered the post-rebuild spline check. It is now the only spline guard, covered by the existing spline test and a new instance test.

## Evidence

- `scripts/check.sh` on 71390ae: OK, 1239 passed, 0 failed, 3 ignored. The commit adding this log changes documentation only.
- Removing or reversing each fix makes its new test fail, in my run and in the reviewer's.
- Live stdio run on the 71390ae binary with an id-picked face: 1937.168147 mm³ becomes 1968.584073 mm³ (2000−20π, 2000−10π by hand), the same after save and reopen; undo, bad direction, boundary no-op, failed batch and the R1 refusal all behaved as specified.
- Round 2 compared 16 saved models with fresh-process native full rebuilds: maximum volume, bound and projected-geometry differences were 0.

## Not covered

- QymCAD.app: no visible-app check of a reordered file or of the app's own undo.
- Round 2 could not construct a projected entity whose id changes while its association survives, or a sketch going from dormant to active. If the first case exists, the R1 guard refuses it, which is safe but a false refusal.
- External-reference and Piece transfers, component patterns and every datum-axis, datum-point and measured-value variant were not run individually.
