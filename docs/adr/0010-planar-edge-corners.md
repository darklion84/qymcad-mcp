# ADR 0010: Classify planar edge corners from local face geometry

- Status: accepted (2026-10-07)

## Context

Agents need to select the inner edge of an L profile for a fillet without manually inspecting every edge id.
The pinned QymCAD selection API provides adjacency and orientation queries but no concave/convex query.
The kernel exposes the pair of named faces adjacent to an edge, and tessellated planar triangles carry outward
winding. A face's average normal is unsuitable on curved faces (F-011), and its global centroid can be outside
a nonconvex face, so a centroid-to-edge vector cannot reliably identify the local interior.

## Decision

Expose `{"concave": true}` and `{"convex": true}` in edge selections, supporting straight edges between
two distinct planar faces. Exclude curved junctions, seams, and tangent junctions and describe that scope in
the selection schema. Refuse their use in face selections and values other than `true`.

Find a triangle of face A with a side touching the edge midpoint. Project its third vertex perpendicular to
the edge to get the local in-plane inward direction. Dot this with face B's outward unit normal: positive
means concave; negative means convex. Use planar normals from outward mesh winding, and allow relative float
rounding when identifying incident triangle sides. Near-parallel face normals are excluded.

Lower these engine filters to persistent ids before evaluating compositions through QymCAD's existing query
API, just as the engine already lowers `largest` edges by true length. Fillet/chamfer continue storing pick
lists (F-024); no new upstream file format, dependencies, or native stored query types are introduced.

## Consequences

The L-profile regression checks its one concave y-directed corner, five convex y-directed corners, and both
cap outlines. It proves the profile volume by `(20*15 - 16*11)*8 = 992 mm³`. Inverting the signed dot product
turns the corner-count assertion red. The nonconvex cap checks protect the local-interior calculation.

General curved-corner classification remains outside this scope. Supporting it would require local analytic
normals at the edge plus oriented dihedral support in the kernel, or a separately reviewed approximation.
Unavailable or numerically ambiguous junctions are not guessed from average normals.
