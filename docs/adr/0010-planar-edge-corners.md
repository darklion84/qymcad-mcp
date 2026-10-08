# ADR 0010: Classify edge corners from local face geometry

- Status: accepted (2026-10-07)
- Extended to curved junctions (2026-10-08).
- Revised for uncertainty margins and multiple samples (2026-10-08).
- Corrected planar/straight uncertainty to preserve non-right-angle corners (2026-10-08).
- Added verified cone/circle refinements, sparse-face allowances and omission reporting (2026-10-08).
- Corrected composed hint/count scope and cone mesh rounding for tangent chamfer chains (2026-10-08).

## Context

Agents need to select the inner edge of an L profile for a fillet without manually inspecting every edge id.
The pinned QymCAD selection API provides adjacency and orientation queries but no concave/convex query.
The kernel exposes the pair of named faces adjacent to an edge, and tessellated triangles carry outward
winding. A face's average normal is unsuitable on curved faces (F-011), and its global centroid can be outside
a nonconvex face, so a centroid-to-edge vector cannot reliably identify the local interior.

## Decision

Expose `{"concave": true}` and `{"convex": true}` in edge selections, supporting manifold edges between
two distinct faces, including curved junctions such as a boss on a plate. Exclude seams and G1 tangent junctions and describe that scope in
the selection schema. Refuse their use in face selections and values other than `true`.

Sample five fractions (0.1, 0.3, 0.5, 0.7, 0.9) of the native edge polyline's arc length. At each target, find
the nearest shared tessellated triangle side, matching vertex coordinates even when faces duplicate vertex
indices. Face A's outward winding orients the tangent; its outward normal crossed with that tangent points
into A. Dot this inward tangent with face B's outward unit normal: positive means concave; negative means
convex. Allow relative float rounding when matching sides. Every sample must have a sufficiently large signed
dot product, and every sample must agree; otherwise omit the whole edge. Kernel-marked smooth edges, seams,
and missing or degenerate shared tessellation are also omitted.

Refine cylinder normals from the axis/radial direction and cone normals from the axis plus the face's
own meridian vertices. Project every vertex onto the axis to obtain z and radial distance r. The widest
axial span determines s=dr/dz; all vertices must fit r(z)=r0+s*(z-z0) within 1e-6 of the larger of
meridian and absolute mesh coordinate scales. Native vertices are f32 promoted to f64; a small cone at a large coordinate retains that rounding (F-063).
An axial span below 1e-9 of the meridian scale falls back to facets. The generator axis+s*radial has perpendicular
normal radial-s*axis. Normalize and orient it by triangle winding; unreliable alignment falls back to facets.
This uses no guessed apex or surface angle (F-059).

Native cylinder/cone identification and the Project's sphere fit precede corner mesh planarity. The kernel's
aggregate surface counts prove an axis-free face is a native plane only when all other native faces are
planes/cylinders/cones; no planar surfaces means the mesh-plane path is refused. In mixed bodies, use the
existing triangle agreement check but give mesh-only normals delta(max(0.3/sqrt(n),acos(0.99999))) allowance,
where n is the triangle count. The minimum corresponds to the planarity check's ~0.25° agreement; it never
claims numerical certainty from a few triangles. The sphere fit alone requires at least 12 vertices and
cannot catch single-triangle curved patches (F-060). No per-id native plane/type getter is exposed.

Native collinear zero-radius polylines use exact endpoint directions; native circles/arcs use
unit(axis × (point-centre)). Shared-side winding orients either analytic tangent. The circle formula is
verified independently at radially inward chord points, so shallow cone rims no longer need a chord-tangent
allowance. Other edges retain shared-side tangents. Lines and cylinders and proven planes receive `1e-6`;
cone normals retain a one-degree engineering allowance. Other surface normals and curved tangents receive
delta(0.3 rad), where delta(theta)=2*sin(theta/2). The absolute triple product must exceed the sum of both
normal errors and tangent error at every sample. That sum bounds the vector error when assigned bounds hold.

Native smooth flags classify angles below about 1.5° as smooth, including shallow sharp rims. Continue
omitting them from selection. Count them as uncertain unless all five sampled normals agree within 1e-6;
seams and such sampled G1 junctions do not contribute to the count. Unknown G1 facets can still be counted
as uncertain because their exact normal is unavailable. Corner previews report "omitted N uncertain edges"
when nonzero, even if other corners matched. For bare filters and positive `and` compositions, evaluate
the selection with corner leaves replaced by uncertain ids. This counts only edges satisfying the other
conditions. Union/subtraction/tangent-chain compositions instead report the body total and the absent
subset, distinguishing them when some uncertain edges appear in the actual result: counterfactual substitution need not represent an omission under negation or expansion.
Analytic circular countersinks at ≥3° are verified; a 0.9° spotface remains omitted. Unrefined surfaces retain an effective ~20–30° or larger threshold depending on
the other face and tangent; this is stated in select/fillet descriptions rather than hidden.

Lower these engine filters to persistent ids before evaluating compositions through QymCAD's existing query
API, just as the engine already lowers `largest` edges by true length. Fillet/chamfer continue storing pick
lists (F-024); no new upstream file format, dependencies, or native stored query types are introduced.

Empty corner previews name the requested inward/outward corner count and the opposite count for the body.
When requested corners exist, say they were eliminated by the selection's other conditions. Bare empty
filters retain their zero-count wording.
Only multi-feature source chains receive a named-feature `between` example. Fillet/chamfer repeat the hint
in their empty-match errors. Noncorner empty selections remain ordinary empty results.

## Consequences

The L-profile regression checks its one concave y-directed corner, five convex y-directed corners, and both
cap outlines. It proves the profile volume by `(20*15 - 16*11)*8 = 992 mm³`. Inverting the signed dot product
turns the corner-count assertion red. The nonconvex cap checks protect the local-interior calculation.

`golden_curved_corners` checks boss/plate, through-hole and counterbore circles, two-cylinder wall junctions,
an elliptic cylinder/oblique-plane rim and cone/plane rims with formula-derived volumes and positions.
The boss-base fillet adds `2π[R*r²(1−π/4)+r³(5/6−π/4)]` mm³ by Pappus; independent radius, boss diameter,
boss height and plate thickness edits are checked through both the server and native GUI paths.
The smaller-radius counterbore shoulder is convex; the larger-radius recessed floor/wall circle is concave.
The 0.9° shallow conical spotface regression is omitted and reports one uncertain rim. A native solid made from
Bezier patches has top surfaces `z=q(y)` for `x<=0` and `z=q(y)+40*x*(y-1/4)` for `x>=0`, with
`q(y)=0.1*(y²-1)`, over `[-1,1]^2`, closed at `z=-60`. Its volume is `240-10-4/15 mm³`; the single crease
at `x=0, z=q(y)` changes sign at `y=1/4`, away from the midpoint. Its gentle curvature forces subdivision of
the shared boundary, and the regression verifies that all five sampled signs clear their uncertainty margins
and include both signs. Its midpoint-only mutation wrongly selects the crease. An oblique plane cutting a cylinder
does not supply a changing-sign example: intersecting convex bodies preserves convexity.

A controlled cylinder-normal regression uses a facet rotated by 8° that reverses a 4° corner sign, then
checks that the analytic radial normal recovers the sign. A second regression checks near-perpendicular
alignment fallback. The former untested tangent refinement was removed; its replacement now has a direct
formula proof and a reversed-cross-product mutation. Native cone formula tests cover six slopes, four
azimuths and reversed axis/winding within 1e-7. Countersinks at 20/10/5/3/30/45° follow formula volumes through
server and native GUI depth edits; restoring full cone uncertainty makes the 10° rim regression red.

`golden_planar_corners` verifies a 20×16×10 block with a 2 mm, 45° chamfer along one 20 mm top edge:
V=3200−(2²/2)×20=3160 mm³ and both 135° interior boundaries are convex. A 90° V-groove with
width 4, depth 2 and length 16 has V=3200−(4×2/2)×16=3136 mm³ and a concave bottom edge.
A triangular wedge with base 10√3, height 10 and length 12 has V=600√3 mm³ and a convex 30° corner.
A 20° plane/plane wedge with base 10/tan(20°) remains convex with V=600/tan(20°) mm³.
Each test also verifies bounds, edge length/position and adjacent planar faces. Reverting only plane-normal
allowances fails the wedge: `2*delta(0.3)+1e-6≈0.598 > sin(30°)=0.5`. The 45° chamfer still clears that
margin (`sin(45°)≈0.707`); reverting both plane and line allowances fails it with margin ≈0.897.
The collinearity test checks oblique two-point/subdivided lines, relative tolerances across three scales,
curved polylines and degeneracies. All existing curved regressions remain unchanged.
An additional regression revolves cylindrical and conical profiles by 0.1°: both native curved patches
pass mesh planarity but must remain outside the plane-normal path. Their volumes follow the frustum
formula times `0.1/360`. Removing the native-type guard fails this regression.

These margins deliberately omit uncertain shallow curved junctions. Aggregate native counts cannot assign
a particular torus/freeform type to a face in a mixed body, so triangle-count allowances remain engineering
estimates, not a formal OCCT normal-error guarantee. Five samples reject observed sign changes but cannot
exclude arbitrary oscillations between samples or variation within a single facet side. In an earlier
straight-seam Bezier fixture, every fraction read the same facet despite varying surface derivatives;
face-wide average normals remain unsuitable on curved faces.
