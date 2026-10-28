//! Modifiers of an existing body: fillet, chamfer, shell, push face, hole. Each call is atomic (see
//! `Session::atomic`): if the new feature does not build, the document is left unchanged and QymCAD's reason is
//! returned. Every numeric argument may be an expression; it is stored as a feature dimension so the GUI and
//! `param_set` keep the feature parametric.

use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::topology::{Element, Sel};
use crate::value::Num;
use qymcad_core::feature::{ChamferMode, FeatureKind, ShellSide};
use qymcad_core::model::{ChamferShape, HoleTool, Id};
use qymcad_core::refs::{Cardinality, Fingerprint, Ref};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Which side of the faces a shell's wall goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// The wall is taken out of the body; the outer surface stays.
    #[default]
    Inward,
    /// The wall is added outside; the original surface becomes the inner one.
    Outward,
    /// Half of the wall on each side.
    Centred,
}

/// The kind of a hole.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HoleKind {
    /// A flat-bottomed cylinder.
    #[default]
    Plain,
    /// Plus a wider cylinder (`dia2` × `depth2`) at the face.
    Counterbore,
    /// Plus a cone from `dia2` at the face down to `diameter` over `depth2`.
    Countersink,
}

/// Arguments of `Session::hole`.
#[derive(Clone, Debug, PartialEq)]
pub struct Hole {
    /// Body to drill (default: the current body).
    pub body: Option<Id>,
    /// The face to drill into; must resolve to exactly one planar face. The hole goes along −(outward normal).
    pub face: Sel,
    /// Centre, world mm. Projected onto the face plane along its normal, so only the in-plane position matters.
    /// Default: the face centroid. Not parametric (QymCAD stores the point as numbers). Excludes `offset`.
    pub at: Option<[f64; 3]>,
    /// Centre as a shift `[u, v]` from the face's area centroid as the face stands (other cut-outs in it move the
    /// centroid, F-087) along the face's axes, mm, numbers or formulas (QymCAD's
    /// `off_u`/`off_v`, which follow parameters). The axes: for a face facing ±Z, u = +X and v = normal × u (+Y
    /// on a top face, −Y on a bottom face); otherwise u = Z × normal (horizontal) and v = normal × u. Excludes `at`.
    pub offset: Option<[Num; 2]>,
    /// Diameter of the bore, mm (number or expression).
    pub diameter: Num,
    /// Depth from the face (counterbore/countersink included). `None` = 10000 mm; refused at creation when
    /// the body's bbox diagonal exceeds that bound. Later stock growth beyond this depth can make it blind.
    pub depth: Option<Num>,
    /// Plain, counterbore or countersink.
    pub kind: HoleKind,
    /// Counterbore/countersink only: diameter at the face, mm; must exceed `diameter`.
    pub dia2: Option<Num>,
    /// Counterbore/countersink only: depth of the recess (cylinder or cone), mm; less than the depth.
    pub depth2: Option<Num>,
    /// Name for the new feature, usable instead of its id later.
    pub name: Option<String>,
}

/// "Through all" for a hole. QymCAD's hole has no through-all extent, only a depth (F-037); the app's own hole
/// dialog goes up to 10000 mm. Creation refuses stock whose bbox diagonal exceeds this fixed depth; subsequent
/// stock growth beyond it can make the hole blind.
pub(crate) const THROUGH_DEPTH: f64 = 10000.0;

fn seam_note((id, mut report): (Id, Rebuild), dropped: usize) -> (Id, Rebuild) {
    if dropped > 0 {
        report.notes.push(format!("dropped {dropped} seam {}: not blendable", crate::topology::edge_word(dropped)));
    }
    (id, report)
}

/// Evaluate a dimension; positive values only.
fn positive(s: &Session, n: &Num, what: &str) -> Result<f64> {
    let v = n.eval(&s.p.param_map())?;
    if !(v.is_finite() && v > 0.0) {
        return Err(Error::Invalid(format!("{what} must be positive, got {v}")));
    }
    Ok(v)
}

/// Store the expression of `n` (if any) as feature dimension `key` of `node`.
pub(crate) fn dim(s: &mut Session, node: Id, key: &str, n: &Num) {
    if let Some(e) = n.expr() {
        s.p.set_feat_dim(node, key, e);
    }
}

impl Session {
    /// Round edges of a body. The selection (ids or a description) is resolved now and stored as a pick list of
    /// persistent edge names: QymCAD re-binds them across upstream edits (F-010) and warns `EdgesDropped` when
    /// some vanish. A stored edge *query* is not used: after the document is reopened in QymCAD.app it
    /// re-evaluates against an empty edge pool and rounds every edge (F-024).
    pub fn fillet(&mut self, body: Option<Id>, edges: &Sel, radius: &Num, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        let mut dropped_seams = 0;
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let r = positive(s, radius, "fillet radius")?;
            let (ids, dropped) = s.edges_now(src, edges)?;
            dropped_seams = dropped;
            let id = s.in_component(s.p.body_owner(src), |p| p.add_fillet(src, r, ids));
            dim(s, id, "radius", radius);
            s.set_node_name(id, name)?;
            Ok(id)
        })
        .map(|result| seam_note(result, dropped_seams))
    }

    /// Bevel edges of a body: symmetric (`dist`), or two distances (`dist` on one face, `d2` on the other).
    /// Edges are stored as a pick list, like `fillet`.
    pub fn chamfer(&mut self, body: Option<Id>, edges: &Sel, dist: &Num, d2: Option<&Num>, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        let mut revolve_source = false;
        let mut dropped_seams = 0;
        self.atomic(|s| {
            let src = s.source_body(body)?;
            revolve_source = s.body_ancestors(src).iter().any(|n| matches!(n.kind, FeatureKind::Revolve { .. }));
            let d = positive(s, dist, "chamfer distance")?;
            let d2v = d2.map(|n| positive(s, n, "chamfer d2")).transpose()?;
            let (ids, dropped) = s.edges_now(src, edges)?;
            dropped_seams = dropped;
            let mode = if d2v.is_some() { ChamferMode::TwoDist } else { ChamferMode::Symmetric };
            let id = s.p.add_chamfer_ex(src, d, ChamferShape { mode, d2: d2v.unwrap_or(0.0), flip: false, ref_face: 0 }, ids);
            dim(s, id, "dist", dist);
            if let Some(n) = d2 {
                dim(s, id, "d2", n);
            }
            s.set_node_name(id, name)?;
            Ok(id)
        })
        .map(|result| seam_note(result, dropped_seams))
        .map_err(|error| chamfer_failure_advice(error, revolve_source))
    }

    /// Hollow a body leaving walls of `thickness`, removing `open_faces` (at least one: QymCAD has no closed
    /// hollow shell, F-035).
    pub fn shell(&mut self, body: Option<Id>, open_faces: &Sel, thickness: &Num, side: Side, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let t = positive(s, thickness, "shell thickness")?;
            let ids = s.resolve_sel(src, Element::Faces, open_faces)?;
            if ids.is_empty() {
                return Err(Error::Invalid(format!("the open-face selection matched no face of body {src}")));
            }
            let qside = match side {
                Side::Inward => ShellSide::Inward,
                Side::Outward => ShellSide::Outward,
                Side::Centred => ShellSide::Centred,
            };
            let id = s.p.add_shell_mode(src, t, if open_faces.is_ids() { ids.clone() } else { Vec::new() }, qside);
            if !open_faces.is_ids() {
                let q = s.face_query(open_faces, &ids)?;
                if let Some(FeatureKind::Shell { faces, .. }) = s.node_kind_mut(id) {
                    *faces = Ref::many(q);
                }
            }
            dim(s, id, "thickness", thickness);
            s.set_node_name(id, name)?;
            Ok(id)
        })
    }

    /// Move one planar face along its outward normal by `dist` (negative pushes it in).
    pub fn push_face(&mut self, body: Option<Id>, face: &Sel, dist: &Num, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let d = dist.eval(&s.p.param_map())?;
            if d.is_nan() || d == 0.0 {
                return Err(Error::Invalid("push distance must not be zero".into()));
            }
            let key = s.planar_face(src, face)?;
            let id = s.in_component(s.p.body_owner(src), |p| p.add_push_face(src, key, d));
            if !face.is_ids() {
                let q = s.face_query(face, &[key.id])?;
                if let Some(FeatureKind::PushFace { face: r, .. }) = s.node_kind_mut(id) {
                    *r = Ref { query: q, expect: Cardinality::One, hint: Fingerprint { centroid: key.centroid, normal: key.normal } };
                }
            }
            dim(s, id, "dist", dist);
            s.set_node_name(id, name)?;
            Ok(id)
        })
    }

    /// Drill a hole into a planar face (see `Hole`).
    pub fn hole(&mut self, a: &Hole) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.source_body(a.body)?;
            let key = s.planar_face(src, &a.face)?;
            let diameter = positive(s, &a.diameter, "hole diameter")?;
            let depth = match &a.depth {
                Some(n) => positive(s, n, "hole depth")?,
                None => {
                    let bb = s.shapes.get(&src).and_then(|shape| shape.bbox())
                        .ok_or_else(|| Error::Invalid(format!("body {src} has no bounding box for a through hole")))?;
                    // The bbox diagonal bounds the body's extent along any drill axis.
                    let diagonal = (bb[3] - bb[0]).hypot(bb[4] - bb[1]).hypot(bb[5] - bb[2]);
                    if diagonal > THROUGH_DEPTH {
                        return Err(Error::Invalid(format!(
                            "through hole depth is limited to {THROUGH_DEPTH} mm; body {src} bbox diagonal is {diagonal} mm; use an explicit depth"
                        )));
                    }
                    THROUGH_DEPTH
                }
            };
            let (kind, dia2, depth2) = match a.kind {
                HoleKind::Plain => (0u8, 0.0, 0.0),
                k => {
                    let (Some(d2), Some(h2)) = (&a.dia2, &a.depth2) else {
                        return Err(Error::Invalid("a counterbore or countersink needs `dia2` and `depth2`".into()));
                    };
                    let (d2, h2) = (positive(s, d2, "dia2")?, positive(s, h2, "depth2")?);
                    if d2 <= diameter {
                        return Err(Error::Invalid(format!("dia2 ({d2}) must be larger than the diameter ({diameter})")));
                    }
                    if h2 >= depth {
                        return Err(Error::Invalid(format!("depth2 ({h2}) must be less than the hole depth ({depth})")));
                    }
                    (if k == HoleKind::Counterbore { 1 } else { 2 }, d2, h2)
                }
            };
            let at = match (a.at, &a.offset) {
                (Some(_), Some(_)) => return Err(Error::Invalid("give `at` or `offset`, not both".into())),
                (Some(at), None) => at,
                (None, Some([u, v])) => {
                    let vars = s.p.param_map();
                    let face = qymcad_core::feature::FaceAt { centre: key.centroid, normal: key.normal };
                    qymcad_core::feature::point_on_face(face, [u.eval(&vars)?, v.eval(&vars)?])
                }
                (None, None) => key.centroid,
            };
            let id = s.p.add_hole_at(src, key, at, HoleTool { kind, diameter, depth, dia2, depth2 });
            if !a.face.is_ids() {
                let q = s.face_query(&a.face, &[key.id])?;
                if let Some(FeatureKind::Hole { face: r, .. }) = s.node_kind_mut(id) {
                    r.query = q;
                }
            }
            dim(s, id, "diameter", &a.diameter);
            if let Some([u, v]) = &a.offset {
                dim(s, id, "off_u", u);
                dim(s, id, "off_v", v);
            }
            if let Some(n) = &a.depth {
                dim(s, id, "depth", n);
            }
            if kind != 0 {
                if let (Some(d2), Some(h2)) = (&a.dia2, &a.depth2) {
                    dim(s, id, "dia2", d2);
                    dim(s, id, "depth2", h2);
                }
            }
            s.set_node_name(id, a.name.as_deref())?;
            Ok(id)
        })
    }

    /// The edges `sel` resolves to on `src` now; an empty result is refused (QymCAD treats an empty edge list
    /// as "every edge", FINDINGS F-025).
    fn edges_now(&self, src: Id, sel: &Sel) -> Result<(Vec<u32>, usize)> {
        let mut ids = self.resolve_sel(src, Element::Edges, sel)?;
        if ids.is_empty() {
            let hint = self.empty_corner_hint(src, sel).map(|h| format!("; {h}")).unwrap_or_default();
            return Err(Error::Invalid(format!("the edge selection matched no edge of body {src}{hint}")));
        }
        let seams: std::collections::HashSet<_> = {
            // The bridge exposes persistent names, not IsSame identity (F-068).
            // Duplicate names cannot distinguish a real seam from two distinct faces.
            if self.ambiguous_face_warning(src).is_some() {
                Default::default()
            } else {
                crate::session::edge_face_pairs(&self.shapes[&src])?
                    .into_iter()
                    .filter_map(|(edge, a, b)| (a == b).then_some(edge))
                    .collect()
            }
        };
        let before = ids.len();
        ids.retain(|id| !seams.contains(id));
        let dropped = before - ids.len();
        if ids.is_empty() {
            return Err(Error::Invalid(format!(
                "the edge selection matched only seam edges of body {src}: not blendable; select a rim or another corner edge"
            )));
        }
        Ok((ids, dropped))
    }

    fn planar_face(&self, src: Id, sel: &Sel) -> Result<qymcad_core::feature::FaceKey> {
        let key = self.one_face(src, sel)?;
        if !self.face_is_planar(src, key.id) {
            return Err(Error::Invalid(format!("face {} of body {src} is not planar", key.id)));
        }
        Ok(key)
    }

    pub(crate) fn node_kind_mut(&mut self, id: Id) -> Option<&mut FeatureKind> {
        self.p.timeline.iter_mut().find(|n| n.id == id).map(|n| &mut n.kind)
    }
}

/// Why fillet `node` that QymCAD built must still be refused, or `None` when it is acceptable (F-074).
///
/// QymCAD 0.1.1 builds a radius too big for neighbouring edges as overlapping prisms (`blend_limit`) without an
/// error: the blends of two edges that do not meet cross each other. Blends of edges that share a vertex may meet
/// at a sharp mitre (a rim corner), and blends of separate edges may meet tangentially at an exact limit (two R5
/// roundings across a 10 mm face). So a join between two blend faces of this node (`Role::Blend`, named after the
/// edge they round: a 1-based place in the pick list, or the edge's own name) whose source edges share no vertex is
/// refused when it is sharp by the kernel's flag, or, for two cylindrical blends, when their normals at the join
/// differ by more than `TANGENT_DEG` (the kernel's flag lets about 1.5° pass). Vertices are compared by their
/// native names (`MeshEdge::ends`), by position only when a name is missing. Blends of earlier fillets, unnamed join
/// edges, and blends whose source edge cannot be found (a pick renamed since it was stored) are not judged.
/// A source or result whose topology cannot be read is refused, not waved through.
/// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()`.
pub(crate) fn fillet_acceptance(
    p: &qymcad_core::model::Project,
    node: Id,
    source: &qymcad_kernel::Shape,
    result: &qymcad_kernel::Shape,
    radius: f64,
) -> Option<String> {
    use qymcad_core::names::{NameTable, Role, VertexName};
    use std::collections::HashMap;
    /// Two blends meeting at an exact limit are tangent; past it they cross at an angle (1.47° at R10.13 where the
    /// limit is R10, Codex review). Limits built a hair short stay far below this.
    const TANGENT_DEG: f64 = 0.1;
    let (Ok(pairs), Ok(edges)) = (crate::session::edge_face_pairs(result), crate::session::edges_with_ends(source)) else {
        return Some(format!("fillet R{radius:.2}: cannot read the faces of the result to check it"));
    };
    let picks: Vec<u32> = match p.timeline.iter().find(|n| n.id == node).map(|n| &n.kind) {
        Some(FeatureKind::Fillet { edges, .. }) if edges.query.is_pick_list() => edges.query.picked_descs(),
        _ => Vec::new(),
    };
    let ends: HashMap<u32, (VertexName, VertexName, [f64; 3], [f64; 3])> =
        edges.iter().map(|e| (e.id, (e.ends[0], e.ends[1], e.a, e.b))).collect();
    let (smooth, mids): (HashMap<u32, bool>, HashMap<u32, [f64; 3]>) = {
        let _gate = qymcad_kernel::kernel_gate();
        let info = result.edges_info();
        let mid = |poly: &[[f32; 3]]| poly.get(poly.len() / 2).map(|q| q.map(f64::from));
        (info.iter().map(|e| (e.id, e.smooth)).collect(), info.iter().filter_map(|e| Some((e.id, mid(&e.poly)?))).collect())
    };
    // The source edge a blend face of this node rounds.
    let rounds = |face: u32| -> Option<u32> {
        let name = p.names.get(face).filter(|n| n.feature == node && n.role == Role::Blend)?;
        let src = u32::try_from(name.src).ok()?;
        let edge = if NameTable::is_named(src) { src } else { *picks.get(usize::try_from(src).ok()?.checked_sub(1)?)? };
        ends.contains_key(&edge).then_some(edge)
    };
    let meet = |a: u32, b: u32| {
        let (a0, a1, pa, qa) = ends[&a];
        let (b0, b1, pb, qb) = ends[&b];
        let named = |v: VertexName| v != VertexName::default();
        let close = |x: [f64; 3], y: [f64; 3]| {
            (0..3).map(|k| (x[k] - y[k]).powi(2)).sum::<f64>().sqrt() <= 1e-6 * x.iter().fold(1.0, |m: f64, c| m.max(c.abs()))
        };
        [(a0, pa), (a1, qa)]
            .iter()
            .any(|&(va, xa)| [(b0, pb), (b1, qb)].iter().any(|&(vb, xb)| if named(va) && named(vb) { va == vb } else { close(xa, xb) }))
    };
    // Angle between two cylindrical faces' normals at `point`, degrees; `None` unless both are cylinders.
    let cylinder_angle = |fa: u32, fb: u32, point: [f64; 3]| -> Option<f64> {
        let _gate = qymcad_kernel::kernel_gate();
        let normal = |f: u32| -> Option<[f64; 3]> {
            let (origin, axis, _) = result.face_cylinder(f)?;
            let d = [point[0] - origin[0], point[1] - origin[1], point[2] - origin[2]];
            let along = d[0] * axis[0] + d[1] * axis[1] + d[2] * axis[2];
            let r = [d[0] - along * axis[0], d[1] - along * axis[1], d[2] - along * axis[2]];
            let len = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
            (len > 1e-9).then(|| r.map(|c| c / len))
        };
        let (na, nb) = (normal(fa)?, normal(fb)?);
        // Radial directions, not oriented normals: a convex blend beside a concave one has its axis on the other
        // side, so tangent faces give 180° here. The angle between the surfaces is the angle between the lines.
        Some((na[0] * nb[0] + na[1] * nb[1] + na[2] * nb[2]).abs().min(1.0).acos().to_degrees())
    };
    let cusps = pairs
        .into_iter()
        .filter(|&(edge, a, b)| a != b && a != 0 && b != 0 && edge != 0)
        .filter(|&(_, a, b)| matches!((rounds(a), rounds(b)), (Some(ea), Some(eb)) if ea != eb && !meet(ea, eb)))
        .filter(|&(edge, a, b)| match smooth.get(&edge) {
            Some(false) => true,
            Some(true) => mids.get(&edge).and_then(|&m| cylinder_angle(a, b, m)).is_some_and(|deg| deg > TANGENT_DEG),
            None => false,
        })
        .count();
    (cusps > 0).then(|| {
        format!(
            "fillet R{radius:.2} does not fit these edges together: the roundings of edges that do not meet cut into \
             each other ({cusps} {}; QymCAD returned it without an error); try a smaller radius or fewer edges",
            if cusps == 1 { "join" } else { "joins" },
        )
    })
}

fn chamfer_failure_advice(mut error: Error, revolve_source: bool) -> Error {
    if revolve_source {
        if let Error::Rebuild(lines) = &mut error {
            let mut too_big = false;
            for line in lines.iter_mut() {
                // Atomic errors are "name (id): native message". Annotate the native reason only.
                if let Some((label, native)) = line.rsplit_once(": ") {
                    if native.starts_with("chamfer ") && native.ends_with(" too big") {
                        *line = format!("{label}: kernel's reason: {native}");
                        too_big = true;
                    }
                }
            }
            if too_big {
                lines.push("try a smaller distance first. This reason does not prove the distance is too large: cone-mouth chamfers can fail because of surface parameterization, including overlapping cones; orientation alone does not predict failure. If the distance fits, for a full-turn revolve draw the axis line toward the sketch's +y with the profile at larger x than the line, or reverse the construction-axis line endpoints. For a partial turn, reversing endpoints changes the sweep".into());
            }
        }
    }
    error
}

#[cfg(test)]
mod hint_tests {
    use super::*;
    #[test]
    fn kind_shell_refuses_partial_loss_of_frozen_faces() {
        let mut s = Session::new_part();
        let sk = s.sketch_create(&crate::PlaneRef::Base(crate::BaseName::XY), None).unwrap();
        s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &16.0.into(), false).unwrap();
        let (src, _) = s
            .extrude(&crate::Extrude {
                sketch: sk,
                profiles: None,
                height: 10.0.into(),
                op: crate::Op::NewBody,
                direction: crate::Direction::Normal,
                through: false,
                target: None,
                name: None,
            })
            .unwrap();
        let sel = Sel::And(
            Box::new(Sel::Kind(crate::SelectionKind::Plane)),
            Box::new(Sel::Union(vec![
                Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 5.0 },
                Sel::Facing { dir: [1.0, 0.0, 0.0], tol_deg: 5.0 },
                Sel::Facing { dir: [-1.0, 0.0, 0.0], tol_deg: 5.0 },
            ])),
        );
        let picks = s.select(Some(src), Element::Faces, &sel).unwrap().1;
        assert_eq!(picks.len(), 3, "box top and two opposing sides");
        let (shell, report) = s.shell(Some(src), &sel, &1.0.into(), Side::Inward, None).unwrap();
        assert!(report.errors.is_empty());
        // Three open faces leave a cavity spanning all 20 mm of X, 16-2 mm of Y,
        // and 10-1 mm of Z. Stock minus cavity gives the shell's exact volume.
        let expected = 20.0 * 16.0 * 10.0 - 20.0 * 14.0 * 9.0;
        assert!((report.bodies[0].volume - expected).abs() < 1e-8);
        // Replace upstream box geometry with a cylinder. The top and bottom cap names
        // survive, but neither planar side exists anymore: name healing cannot recover
        // them. The resulting stock has exact V=pi*5²*10, with just one requested opening.
        let top = s.p.regen_faces[&src].iter().find(|f| f.normal[2] > 0.9).unwrap().id;
        let bottom = s.p.regen_faces[&src].iter().find(|f| f.normal[2] < -0.9).unwrap().id;
        {
            let shape = crate::test_support::cylinder_named(5.0, 10.0, [bottom, top, 9000]);
            let built = qymcad_kernel::ask::export_mesh(&shape, 0.05).unwrap();
            let (mesh, faces) = (built.mesh, built.faces);
            let _gate = qymcad_kernel::kernel_gate();
            assert!((shape.volume() - std::f64::consts::PI * 25.0 * 10.0).abs() < 1e-8);
            let i = s.p.mesh_index(src).unwrap();
            s.p.bodies[i].mesh = mesh;
            s.p.set_body_faces(src, faces.clone());
            s.p.regen_faces.insert(src, faces);
            s.shapes.insert(src, shape);
        }
        s.p.timeline.iter_mut().find(|n| n.id == shell).unwrap().dirty = true;
        // Only the shell is scheduled: retry must not recreate the superseded box recipe.
        let report = s.rebuild_retrying(Some(&std::collections::HashSet::from([shell])));
        assert!(
            report.errors.iter().chain(&report.warnings).any(|issue| issue.node == shell),
            "lost frozen openings must warn or refuse, rather than silently shelling one face: {report:?}"
        );
    }

    #[test]
    fn ambiguous_body_blend_picks_are_not_dropped_as_seams() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cone_negative_chamfer.qcad");
        let (mut s, _) = Session::open(&path).unwrap();
        let topo = s.topology(None, true).unwrap();
        let ids: Vec<_> = topo.edges.iter().map(|e| e.id).collect();
        let (picks, dropped) = s.edges_now(topo.body, &Sel::Ids(ids.clone())).unwrap();
        assert_eq!(dropped, 0, "duplicate face names must not drop blend picks");
        assert_eq!(picks, ids);
    }

    #[test]
    fn chamfer_advice_only_wraps_too_big_native_message() {
        let error = chamfer_failure_advice(Error::Rebuild(vec!["Bevel (42): chamfer 0.50 too big".into()]), true).to_string();
        assert!(error.contains("Bevel (42): kernel's reason: chamfer 0.50 too big"), "{error}");
        assert!(error.find("smaller distance").unwrap() < error.find("axis line").unwrap(), "{error}");
        let other = chamfer_failure_advice(Error::Rebuild(vec!["Bevel (42): all edges smooth".into()]), true).to_string();
        assert!(!other.contains("axis") && !other.contains("kernel's reason"), "{other}");
        let no_revolve = chamfer_failure_advice(Error::Rebuild(vec!["Bevel (42): chamfer 0.50 too big".into()]), false).to_string();
        assert!(!no_revolve.contains("axis"), "{no_revolve}");
    }
}
