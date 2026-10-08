# ADR 0010: Classify edge corners from local face geometry

- Status: accepted (2026-10-07)
- Extended to curved junctions (2026-10-08).

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

Find the shared tessellated triangle side nearest the edge midpoint, matching vertex coordinates even when
the faces duplicate vertex indices. Face A's outward winding orients the tangent; its outward normal crossed
with that tangent points into A. Dot this inward tangent with face B's outward unit normal: positive means
concave; negative means convex. Refine cylinder normals analytically from the axis/radial direction, using
triangle winding to distinguish a boss from a bore. Refine circle/arc tangents from their axis and centre;
other surfaces and edges use the adjacent triangles. Allow relative float rounding when matching sides.
Kernel-marked smooth edges and near-parallel normals are excluded. Missing or degenerate shared tessellation
does not produce a classification.

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
Local mesh normals approximate noncylindrical surfaces, so unavailable or numerically ambiguous junctions
are omitted. Face-wide average normals remain unsuitable.
