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
    /// Default: the face centroid. Not parametric (QymCAD stores the point as numbers).
    pub at: Option<[f64; 3]>,
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
            let id = s.p.add_fillet(src, r, ids);
            dim(s, id, "radius", radius);
            s.set_node_name(id, name);
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
            s.set_node_name(id, name);
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
            s.set_node_name(id, name);
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
            let id = s.p.add_push_face(src, key, d);
            if !face.is_ids() {
                let q = s.face_query(face, &[key.id])?;
                if let Some(FeatureKind::PushFace { face: r, .. }) = s.node_kind_mut(id) {
                    *r = Ref { query: q, expect: Cardinality::One, hint: Fingerprint { centroid: key.centroid, normal: key.normal } };
                }
            }
            dim(s, id, "dist", dist);
            s.set_node_name(id, name);
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
            let at = a.at.unwrap_or(key.centroid);
            let id = s.p.add_hole_at(src, key, at, HoleTool { kind, diameter, depth, dia2, depth2 });
            if !a.face.is_ids() {
                let q = s.face_query(&a.face, &[key.id])?;
                if let Some(FeatureKind::Hole { face: r, .. }) = s.node_kind_mut(id) {
                    r.query = q;
                }
            }
            dim(s, id, "diameter", &a.diameter);
            if let Some(n) = &a.depth {
                dim(s, id, "depth", n);
            }
            if kind != 0 {
                if let (Some(d2), Some(h2)) = (&a.dia2, &a.depth2) {
                    dim(s, id, "dia2", d2);
                    dim(s, id, "depth2", h2);
                }
            }
            s.set_node_name(id, a.name.as_deref());
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
            let _gate = qymcad_kernel::kernel_gate();
            // The bridge exposes persistent names, not IsSame identity (F-068).
            // Duplicate names cannot distinguish a real seam from two distinct faces.
            if self.ambiguous_face_warning(src).is_some() {
                Default::default()
            } else {
                self.shapes[&src].edge_face_pairs().into_iter().filter_map(|(edge, a, b)| (a == b).then_some(edge)).collect()
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
            let _gate = qymcad_kernel::kernel_gate();
            let shape = qymcad_kernel::Shape::cylinder_named(5.0, 10.0, [bottom, top, 9000]).unwrap();
            let (mesh, faces) = shape.tessellate_merged(0.05).unwrap();
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
