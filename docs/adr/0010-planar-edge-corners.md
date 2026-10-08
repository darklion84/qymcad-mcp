# ADR 0010: Classify edge corners from local face geometry

- Status: accepted (2026-10-07)
- Extended to curved junctions (2026-10-08).
- Revised for uncertainty margins and multiple samples (2026-10-08).
- Corrected planar/straight uncertainty to preserve non-right-angle corners (2026-10-08).

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

Refine cylinder normals analytically from the axis/radial direction, using triangle winding to distinguish a
boss from a bore. If the radial normal and facet normal have an alignment magnitude below the numerical
threshold, preserve the facet normal. The pinned kernel exposes a cone's axis, but no cone apex/angle getter
or general surface-normal-at-point API. An axis alone does not determine a cone's normal. For faces identified
as planar by `planar_normal`, use the plane's outward unit normal. This reuses the engine's existing
`face_is_planar` check for face sketches (F-011), rather than introducing a different planarity rule.
Preserve topology's analytic cylinder/cone precedence: `face_axis` identifies these curved faces even
when a narrow trimmed patch passes mesh planarity, so they keep their existing curved normal path.
For zero-radius edges whose native polylines are collinear within `1e-9` times their endpoint chord length,
use the exact native endpoint direction, oriented by the shared side's winding. Two distinct points define a
line; degenerate or closed polylines do not. Other edges retain the shared triangle side's chord tangent;
the untested analytic circle-tangent refinement remains removed.

Convert angular uncertainty to unit-vector error by `delta(theta)=2*sin(theta/2)`. Plane normals, analytic
cylinder normals and straight-edge tangents receive a numerical allowance of `1e-6`.
Other face normals receive `delta(0.3)`, tied to the
kernel's full angular deflection (F-021). Circle/arc chord tangents receive `delta(0.15)` because a circular
chord bisects its angular sweep; other curved tangents receive `delta(0.3)`. Require the absolute signed
dot product to exceed the sum of both normal errors and the tangent error. For unit vectors, that sum bounds
the triple-product error when the assigned vector-error bounds hold, by expanding the difference one vector
at a time.

Lower these engine filters to persistent ids before evaluating compositions through QymCAD's existing query
API, just as the engine already lowers `largest` edges by true length. Fillet/chamfer continue storing pick
lists (F-024); no new upstream file format, dependencies, or native stored query types are introduced.

Empty corner previews return a hint with a named-feature `between` example; fillet documents the same
alternative. Noncorner empty selections remain ordinary empty results.

## Consequences

The L-profile regression checks its one concave y-directed corner, five convex y-directed corners, and both
cap outlines. It proves the profile volume by `(20*15 - 16*11)*8 = 992 mm³`. Inverting the signed dot product
turns the corner-count assertion red. The nonconvex cap checks protect the local-interior calculation.

`golden_curved_corners` checks boss/plate, through-hole and counterbore circles, two-cylinder wall junctions,
an elliptic cylinder/oblique-plane rim and cone/plane rims with formula-derived volumes and positions.
The boss-base fillet adds `2π[R*r²(1−π/4)+r³(5/6−π/4)]` mm³ by Pappus; independent radius, boss diameter,
boss height and plate thickness edits are checked through both the server and native GUI paths.
The smaller-radius counterbore shoulder is convex; the larger-radius recessed floor/wall circle is concave.
The shallow conical spotface regression is omitted by the uncertainty margin. A native solid made from
Bezier patches has top surfaces `z=q(y)` for `x<=0` and `z=q(y)+40*x*(y-1/4)` for `x>=0`, with
`q(y)=0.1*(y²-1)`, over `[-1,1]^2`, closed at `z=-60`. Its volume is `240-10-4/15 mm³`; the single crease
at `x=0, z=q(y)` changes sign at `y=1/4`, away from the midpoint. Its gentle curvature forces subdivision of
the shared boundary, and the regression verifies that all five sampled signs clear their uncertainty margins
and include both signs. Its midpoint-only mutation wrongly selects the crease. An oblique plane cutting a cylinder
does not supply a changing-sign example: intersecting convex bodies preserves convexity.

A controlled cylinder-normal regression uses a facet rotated by 8° that reverses a 4° corner sign, then
checks that the analytic radial normal recovers the sign. A second regression checks near-perpendicular
alignment fallback. The analytic tangent refinement is removed, resolving the untested-refinement backlog.

`golden_planar_corners` verifies a 20×16×10 block with a 2 mm, 45° chamfer along one 20 mm top edge:
V=3200−(2²/2)×20=3160 mm³ and both 135° interior boundaries are convex. A 90° V-groove with
width 4, depth 2 and length 16 has V=3200−(4×2/2)×16=3136 mm³ and a concave bottom edge.
A triangular wedge with base 10√3, height 10 and length 12 has V=600√3 mm³ and a convex 30° corner.
Each test also verifies bounds, edge length/position and adjacent planar faces. Reverting only plane-normal
allowances fails the wedge: `2*delta(0.3)+1e-6≈0.598 > sin(30°)=0.5`. The 45° chamfer still clears that
margin (`sin(45°)≈0.707`); reverting both plane and line allowances fails it with margin ≈0.897.
The collinearity test checks oblique two-point/subdivided lines, relative tolerances across three scales,
curved polylines and degeneracies. All existing curved regressions remain unchanged.
An additional regression revolves cylindrical and conical profiles by 0.1°: both native curved patches
pass mesh planarity but must remain outside the plane-normal path. Their volumes follow the frustum
formula times `0.1/360`. Removing the native-type guard fails this regression.

These margins deliberately omit uncertain shallow curved junctions. Planarity remains the engine's
established mesh-normal agreement check (dot product >0.99999, approximately 0.25°), not an analytic OCCT
surface-type proof; straightness likewise uses the native polyline. The numerical allowances assume those
engine classifications represent planes and lines. The 0.3-radian
deflection supplies an engineering uncertainty allowance; it is not a formal OCCT guarantee bounding every
surface's facet-normal error. Five samples reject observed sign changes but cannot guarantee the absence of
arbitrary oscillations between samples or normal variation along a single shared facet side. In an earlier
straight-seam version of the Bezier fixture, OCCT retained one shared side despite changing surface normals;
every sample consequently read the same facet normal. Face-wide average normals remain unsuitable on curved faces.
