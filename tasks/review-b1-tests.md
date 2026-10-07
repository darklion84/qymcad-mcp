# B1 curved-corner golden tests

Files: `crates/engine/tests/golden_curved_corners.rs` (five tests).

First run: `cargo test -q -p qymcad-engine --test golden_curved_corners`, exit 101, 0 passed and 4 failed on the original planar-only classifier.

Exact assertion/error messages:

```text
called `Result::unwrap()` on an `Err` value: Invalid("the edge selection matched no edge of body 28")

both hole rims are convex: [1610612748, 1610612749], got [1610612737, 1610612736, 1610612738, 1610612739, 1610612743, 1610612746, 1610612747, 1610612740, 1610612741, 1610612742, 1610612744, 1610612745]

assertion `left == right` failed: boss/plate circle is the sole concave corner
  left: []
 right: [1610612748]

assertion `left == right` failed: recessed floor-to-counterbore-wall circle is concave
  left: []
 right: [1610612754]
```

Formula checks: boss volume `W*L*t+π*(d/2)²*h`; through-hole volume `W*L*t-π*R²*t`; counterbore volume `W*L*t-π*Rsmall²*t-π*(Rlarge²-Rsmall²)*depth`. Circle identities use analytic radius, position and circumference `2πR`, independent of selection ids.

The boss-base fillet adds a revolved quarter-circle spandrel: area `A=r²*(1-π/4)`, radial first moment `r³*(5/6-π/4)`, centroid `R+r*(5/6-π/4)/(1-π/4)`, added volume `2π*centroid*A` (Pappus). The test checks G1 boundaries and seams excluded, top rim remains sharp, and radius/boss diameter/boss height/plate thickness follow independently through both reopened server and native GUI parameter paths.

Counterbore wording: the concave recessed step circle is the **larger-radius floor-to-counterbore-wall** circle. The smaller-radius floor-to-through-bore circle is convex. Material occupies 270° around the larger-radius corner and 90° around the smaller-radius shoulder in a radial section; both signs are tested. No requested capability refused; the phrase "step's inner circle" is interpreted as the recessed circle inside the plate, and not the smaller-radius shoulder.

General manifold coverage adds a curved/curved test: union/intersection of two overlapping equal cylinders. Disc overlap area is `2R²*acos(c/(2R))-c*sqrt(4R²-c²)/2`; union/intersection volumes multiply the appropriate disc areas by height. Junctions are at `x=c/2`, `|y|=sqrt(R²-(c/2)²)` and have length equal to height. For union these two-cylinder wall junctions are concave; for intersection they are convex. First run `cargo test -q -p qymcad-engine --test golden_curved_corners junction_of_two_curved_cylinder_walls_has_the_material_corner_sign` failed, exit 101, with exact message:

```text
two-cylinder Add corner has concave=true: []
```

After-fix run and classifier mutation evidence: coordinator owns implementation/mutation; pending.
