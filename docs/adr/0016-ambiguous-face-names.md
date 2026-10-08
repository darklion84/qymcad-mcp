# ADR 0016: Refuse ambiguous face selections while retaining usable geometry

- Status: accepted (2026-10-08)

## Context

A full-turn revolved conical cut followed by two mouth chamfers can produce pairs of native faces
sharing persistent names. Seam repair splits periodic surfaces and preserves original edge names;
blend naming restarts its generated-piece counter for each edge occurrence. Native face getters stop
at the first matching name. The affected recipe succeeds and has the correct volume (F-068).

## Decision

Keep all topology rows, flag each duplicated name with `ambiguous_id`, and report a topology warning.
Refuse face selections whose resolved ids include a duplicated name, including descriptive selections
and modifier selections. Suggest reversing construction-axis endpoints for a full turn, or using an
upward sketch axis with the profile at larger x than the line. Keep unique face selections, rendering,
export, and the original editable recipe available. Do not invent server-only persistent names.

Disprove sphere fits when the native aggregate reports zero spherical surfaces. Where native spheres
exist, also reject coplanar vertex sets and two axial levels about a circular boundary's axis (or the plane normal of its spline polyline): they
cannot distinguish a sphere from a conical band. Preserve the strict radial-residual validation.
The pinned kernel has no per-face surface-type getter; unrecognized spline patches stay `other`.

## Consequences

Some face operations require the axis/profile workaround. This is safer than allowing a name to
silently target multiple native faces. Sparse spherical patches with insufficient distinct support
may remain unclassified; a vertex fit is evidence of geometry only when it excludes these degeneracies.
Tests: `golden_round_i`; source and native probe evidence: F-068.
