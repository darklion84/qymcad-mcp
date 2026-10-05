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
    pub diameter: Num,
    /// Depth from the face (counterbore/countersink included). `None` = through all.
    pub depth: Option<Num>,
    pub kind: HoleKind,
    pub dia2: Option<Num>,
    pub depth2: Option<Num>,
    pub name: Option<String>,
}

/// Evaluate a dimension; positive values only.
fn positive(s: &Session, n: &Num, what: &str) -> Result<f64> {
    let v = n.eval(&s.p.param_map())?;
    if v.is_nan() || v <= 0.0 {
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
    /// re-evaluates against an empty edge pool and rounds every edge (F-3B-2).
    pub fn fillet(&mut self, body: Option<Id>, edges: &Sel, radius: &Num, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology();
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let r = positive(s, radius, "fillet radius")?;
            let ids = s.edges_now(src, edges)?;
            let id = s.p.add_fillet(src, r, ids);
            dim(s, id, "radius", radius);
            s.set_node_name(id, name);
            Ok(id)
        })
    }

    /// Bevel edges of a body: symmetric (`dist`), or two distances (`dist` on one face, `d2` on the other).
    /// Edges are stored as a pick list, like `fillet`.
    pub fn chamfer(&mut self, body: Option<Id>, edges: &Sel, dist: &Num, d2: Option<&Num>, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology();
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let d = positive(s, dist, "chamfer distance")?;
            let d2v = d2.map(|n| positive(s, n, "chamfer d2")).transpose()?;
            let ids = s.edges_now(src, edges)?;
            let mode = if d2v.is_some() { ChamferMode::TwoDist } else { ChamferMode::Symmetric };
            let id = s.p.add_chamfer_ex(src, d, ChamferShape { mode, d2: d2v.unwrap_or(0.0), flip: false, ref_face: 0 }, ids);
            dim(s, id, "dist", dist);
            if let Some(n) = d2 {
                dim(s, id, "d2", n);
            }
            s.set_node_name(id, name);
            Ok(id)
        })
    }

    /// Hollow a body leaving walls of `thickness`, removing `open_faces` (may be empty: a closed hollow body).
    pub fn shell(
        &mut self,
        body: Option<Id>,
        open_faces: Option<&Sel>,
        thickness: &Num,
        side: Side,
        name: Option<&str>,
    ) -> Result<(Id, Rebuild)> {
        self.ensure_topology();
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let t = positive(s, thickness, "shell thickness")?;
            let ids = match open_faces {
                Some(sel) => {
                    let ids = s.resolve_sel(src, Element::Faces, sel)?;
                    if ids.is_empty() {
                        return Err(Error::Invalid(format!("the open-face selection matched no face of body {src}")));
                    }
                    ids
                }
                None => Vec::new(),
            };
            let qside = match side {
                Side::Inward => ShellSide::Inward,
                Side::Outward => ShellSide::Outward,
                Side::Centred => ShellSide::Centred,
            };
            let id = s.p.add_shell_mode(src, t, if open_faces.is_some_and(Sel::is_ids) { ids } else { Vec::new() }, qside);
            if let Some(sel) = open_faces.filter(|s| !s.is_ids()) {
                let q = sel.query(Element::Faces)?;
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
        self.ensure_topology();
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let d = dist.eval(&s.p.param_map())?;
            if d.is_nan() || d == 0.0 {
                return Err(Error::Invalid("push distance must not be zero".into()));
            }
            let key = s.planar_face(src, face)?;
            let id = s.p.add_push_face(src, key, d);
            if !face.is_ids() {
                let q = face.query(Element::Faces)?;
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
        self.ensure_topology();
        self.atomic(|s| {
            let src = s.source_body(a.body)?;
            let key = s.planar_face(src, &a.face)?;
            let diameter = positive(s, &a.diameter, "hole diameter")?;
            let depth = match &a.depth {
                Some(n) => positive(s, n, "hole depth")?,
                None => {
                    // Through all: longer than anything the body can hold.
                    let bb = s.shapes.get(&src).and_then(|sh| sh.bbox()).unwrap_or([0.0; 6]);
                    ((bb[3] - bb[0]).powi(2) + (bb[4] - bb[1]).powi(2) + (bb[5] - bb[2]).powi(2)).sqrt() + 1.0
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
                let q = a.face.query(Element::Faces)?;
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
    /// as "every edge", FINDINGS F-3B-3).
    fn edges_now(&self, src: Id, sel: &Sel) -> Result<Vec<u32>> {
        let ids = self.resolve_sel(src, Element::Edges, sel)?;
        if ids.is_empty() {
            return Err(Error::Invalid(format!("the edge selection matched no edge of body {src}")));
        }
        Ok(ids)
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
