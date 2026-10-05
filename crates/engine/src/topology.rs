//! Topology of a built body (faces and edges with persistent ids) and selections: explicit ids or descriptive
//! queries that QymCAD re-evaluates on every rebuild (FINDINGS F-010).
//!
//! Ids are only valid after a rebuild and belong to one body: a feature creates a new body (F-009), so re-read
//! the topology of the current body after every feature.

use crate::error::{Error, Result};
use crate::session::Session;
use qymcad_core::feature::FaceKey;
use qymcad_core::geom::MeshFace;
use qymcad_core::model::Id;
use qymcad_core::names::Role as QRole;
use qymcad_core::refs::{Axis as QAxis, Query, Ref};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The geometric kind of a face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FaceKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Other,
}

/// The geometric kind of an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Line,
    /// A full circle (closed).
    Circle,
    /// Part of a circle.
    Arc,
    Other,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct FaceInfo {
    /// Persistent face id (valid for this body, after the latest rebuild).
    pub id: u32,
    pub kind: FaceKind,
    /// Area-weighted centre of the face, mm.
    pub centroid: [f64; 3],
    /// Outward unit normal; planes only (on curved faces an average normal is meaningless, F-011).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal: Option<[f64; 3]>,
    /// mm²
    pub area: f64,
    /// Cylinder/cone axis: a point on it and the unit direction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis: Option<[[f64; 3]; 2]>,
    /// Cylinder or sphere radius, mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    /// Sphere centre.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center: Option<[f64; 3]>,
    /// Ids of the edges bounding the face (with `adjacency`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<u32>>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct EdgeInfo {
    /// Persistent edge id (valid for this body, after the latest rebuild).
    pub id: u32,
    pub kind: EdgeKind,
    /// Start and end points (equal for a full circle).
    pub a: [f64; 3],
    pub b: [f64; 3],
    /// Midpoint along the edge.
    pub mid: [f64; 3],
    /// mm
    pub length: f64,
    /// Circle/arc centre, axis (unit normal of its plane) and radius.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    /// The two faces meeting at the edge (with `adjacency`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faces: Option<[u32; 2]>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Topology {
    pub body: Id,
    pub faces: Vec<FaceInfo>,
    pub edges: Vec<EdgeInfo>,
}

/// World axis, for `Sel::Extreme` and axis directions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// The role a face plays in the feature that made it (QymCAD's structural name).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The profile face of an extrude/revolve (at the sketch plane).
    CapStart,
    /// The far cap of an extrude (the translated profile), e.g. the top of a block extruded up from XY.
    CapEnd,
    /// A side face produced by a profile edge.
    Wall,
    /// A surface of revolution.
    Revolved,
    /// A hole's wall.
    Hole,
    /// A fillet or chamfer surface.
    Blend,
    /// The inner wall of a shell.
    ShellWall,
}

impl From<Role> for QRole {
    fn from(r: Role) -> QRole {
        match r {
            Role::CapStart => QRole::CapStart,
            Role::CapEnd => QRole::CapEnd,
            Role::Wall => QRole::Wall,
            Role::Revolved => QRole::Revolved,
            Role::Hole => QRole::Hole,
            Role::Blend => QRole::Blend,
            Role::ShellWall => QRole::ShellWall,
        }
    }
}

/// What a selection is evaluated against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    Faces,
    Edges,
}

impl Element {
    fn name(self) -> &'static str {
        match self {
            Element::Faces => "faces",
            Element::Edges => "edges",
        }
    }
}

/// A selection of faces or edges of a body: explicit persistent ids, or a description that QymCAD re-evaluates
/// on every rebuild (so it survives upstream edits — prefer it). Maps one-to-one onto `qymcad_core::refs::Query`.
#[derive(Clone, Debug, PartialEq)]
pub enum Sel {
    /// These ids (from `topology`).
    Ids(Vec<u32>),
    /// Faces made by a feature (node id), optionally only one role (e.g. `CapEnd`: the top of an extrude).
    OfFeature { feature: Id, role: Option<Role> },
    /// Faces whose outward normal is within `tol_deg` of `dir`.
    Facing { dir: [f64; 3], tol_deg: f64 },
    /// Edges running along `dir` (either sense) within `tol_deg`.
    Along { dir: [f64; 3], tol_deg: f64 },
    /// The elements whose centre (faces) or midpoint (edges) is extreme along a world axis (all ties).
    Extreme { axis: Axis, max: bool },
    /// The largest face by area / the longest edge (all ties).
    Largest,
    /// The edges bounding the selected faces.
    EdgesOf(Box<Sel>),
    /// The edges continuing the seed edges tangentially (within `tol_deg`).
    TangentChain { seed: Box<Sel>, tol_deg: f64 },
    /// The edges where a face of the first set meets a face of the second.
    Between(Box<Sel>, Box<Sel>),
    /// Everything in any of the sets.
    Union(Vec<Sel>),
    /// The first set without the second.
    Minus(Box<Sel>, Box<Sel>),
    /// The elements of the first set that are also in the second.
    And(Box<Sel>, Box<Sel>),
}

impl Sel {
    /// An explicit pick list (`Ids`), as opposed to a description.
    pub fn is_ids(&self) -> bool {
        matches!(self, Sel::Ids(_))
    }

    /// Check that the selection makes sense for `el` (faces or edges) and build the QymCAD query.
    pub(crate) fn query(&self, el: Element) -> Result<Query> {
        let bad = |what: &str, hint: &str| Err(Error::Invalid(format!("`{what}` cannot select {}: {hint}", el.name())));
        Ok(match self {
            Sel::Ids(ids) => {
                if ids.is_empty() {
                    return Err(Error::Invalid(format!("an empty id list selects no {}", el.name())));
                }
                match ids.as_slice() {
                    [one] => Query::Id(*one),
                    _ => Query::Ids(ids.clone()),
                }
            }
            Sel::OfFeature { feature, role } => {
                if el == Element::Edges {
                    return bad("of_feature", "it names faces; use {\"edges_of\": {\"of_feature\": ...}}");
                }
                Query::OfFeature { feature: *feature, role: role.map(Into::into) }
            }
            Sel::Facing { dir, tol_deg } => {
                if el == Element::Edges {
                    return bad("facing", "it tests face normals; use `along` for edge directions or {\"edges_of\": {\"facing\": ...}}");
                }
                Query::Oriented { dir: unit(*dir)?, tol_deg: *tol_deg }
            }
            Sel::Along { dir, tol_deg } => {
                if el == Element::Faces {
                    return bad("along", "it tests edge directions; use `facing` for faces");
                }
                let d = unit(*dir)?;
                let neg = [-d[0], -d[1], -d[2]];
                Query::Union(
                    Box::new(Query::Oriented { dir: d, tol_deg: *tol_deg }),
                    Box::new(Query::Oriented { dir: neg, tol_deg: *tol_deg }),
                )
            }
            Sel::Extreme { axis, max } => Query::Extreme {
                axis: match axis {
                    Axis::X => QAxis::X,
                    Axis::Y => QAxis::Y,
                    Axis::Z => QAxis::Z,
                },
                max: *max,
            },
            Sel::Largest => Query::Largest,
            Sel::EdgesOf(faces) => {
                if el == Element::Faces {
                    return bad("edges_of", "it yields edges");
                }
                Query::Adjacent(Box::new(faces.query(Element::Faces)?))
            }
            Sel::TangentChain { seed, tol_deg } => {
                if el == Element::Faces {
                    return bad("tangent_chain", "it yields edges");
                }
                Query::TangentChain { seed: Box::new(seed.query(Element::Edges)?), tol_deg: *tol_deg }
            }
            Sel::Between(a, b) => {
                if el == Element::Faces {
                    return bad("between", "it yields edges");
                }
                Query::Between(Box::new(a.query(Element::Faces)?), Box::new(b.query(Element::Faces)?))
            }
            Sel::Union(v) => {
                let mut it = v.iter();
                let first = it.next().ok_or_else(|| Error::Invalid("`union` needs at least one selection".into()))?.query(el)?;
                it.try_fold(first, |acc, s| Ok::<_, Error>(Query::Union(Box::new(acc), Box::new(s.query(el)?))))?
            }
            Sel::Minus(a, b) => Query::Minus(Box::new(a.query(el)?), Box::new(b.query(el)?)),
            Sel::And(a, b) => Query::Filter(Box::new(a.query(el)?), Box::new(b.query(el)?)),
        })
    }
}

fn unit(d: [f64; 3]) -> Result<[f64; 3]> {
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if !l.is_finite() || l < 1e-12 {
        return Err(Error::Invalid(format!("direction {d:?} has no length")));
    }
    Ok([d[0] / l, d[1] / l, d[2] / l])
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

pub(crate) fn face_key(f: &MeshFace) -> FaceKey {
    FaceKey { index: 0, centroid: [f.centroid.x, f.centroid.y, f.centroid.z], normal: f.normal, id: f.id }
}

impl Session {
    /// The faces and edges of `body` (default: the current body of the part). `adjacency` adds the edges of each
    /// face and the two faces of each edge.
    pub fn topology(&mut self, body: Option<Id>, adjacency: bool) -> Result<Topology> {
        self.ensure_topology();
        let body = self.topo_body(body)?;
        let shape = self.shapes.get(&body).ok_or_else(|| Error::NotFound(format!("body {body} has no built shape")))?;
        let faces = self.p.regen_faces.get(&body).map(Vec::as_slice).unwrap_or_default();
        let medges = self.p.regen_edges.get(&body).map(Vec::as_slice).unwrap_or_default();
        let mesh = self.p.mesh_index(body).map(|i| &self.p.bodies[i].mesh);

        let _gate = qymcad_kernel::kernel_gate();
        let polylines: HashMap<u32, Vec<[f32; 3]>> = shape.edges_info().into_iter().map(|e| (e.id, e.poly)).rev().collect();
        let pairs: HashMap<u32, [u32; 2]> =
            if adjacency { shape.edge_face_pairs().into_iter().map(|(e, a, b)| (e, [a, b])).collect() } else { HashMap::new() };

        let mut out_faces = Vec::with_capacity(faces.len());
        for f in faces {
            let mut fi = FaceInfo {
                id: f.id,
                kind: FaceKind::Other,
                centroid: [f.centroid.x, f.centroid.y, f.centroid.z],
                normal: None,
                area: f.area,
                axis: None,
                radius: None,
                center: None,
                edges: adjacency.then(|| shape.face_edge_ids(f.id)),
            };
            if let Some((o, d, r)) = shape.face_cylinder(f.id) {
                fi.kind = FaceKind::Cylinder;
                fi.axis = Some([o, d]);
                fi.radius = Some(r);
            } else if let Some((o, d)) = shape.face_axis(f.id) {
                fi.kind = FaceKind::Cone;
                fi.axis = Some([o, d]);
            } else if let Some(n) = mesh.and_then(|m| planar_normal(f, m)) {
                fi.kind = FaceKind::Plane;
                fi.normal = Some(n);
            } else if let Some((c, r)) = self.p.face_sphere(body, &face_key(f)) {
                fi.kind = FaceKind::Sphere;
                fi.center = Some(c);
                fi.radius = Some(r);
            }
            out_faces.push(fi);
        }

        let mut out_edges = Vec::with_capacity(medges.len());
        for e in medges {
            let chord = norm(sub(e.b, e.a));
            let (kind, length) = if e.radius > 1e-9 {
                if chord < 1e-6 {
                    (EdgeKind::Circle, 2.0 * std::f64::consts::PI * e.radius)
                } else {
                    (EdgeKind::Arc, arc_angle(e.a, e.b, e.mid, e.center) * e.radius)
                }
            } else {
                let poly_len = polylines.get(&e.id).map(|p| polyline_length(p));
                match poly_len {
                    Some(l) if (l - chord).abs() <= 1e-4 * l.max(1.0) => (EdgeKind::Line, chord),
                    Some(l) => (EdgeKind::Other, l),
                    None => (EdgeKind::Line, chord),
                }
            };
            let round = matches!(kind, EdgeKind::Circle | EdgeKind::Arc);
            out_edges.push(EdgeInfo {
                id: e.id,
                kind,
                a: e.a,
                b: e.b,
                mid: e.mid,
                length,
                center: round.then_some(e.center),
                axis: round.then_some(e.axis),
                radius: round.then_some(e.radius),
                faces: pairs.get(&e.id).copied(),
            });
        }
        Ok(Topology { body, faces: out_faces, edges: out_edges })
    }

    /// Resolve a selection against `body` (default: the current body) now, as QymCAD will at the next rebuild.
    /// Returns the body and the matching ids.
    pub fn select(&mut self, body: Option<Id>, el: Element, sel: &Sel) -> Result<(Id, Vec<u32>)> {
        self.ensure_topology();
        let body = self.topo_body(body)?;
        Ok((body, self.resolve_sel(body, el, sel)?))
    }

    /// Resolve `sel` on `body`. Explicit ids that are not on the body are an error (stale or foreign ids).
    pub(crate) fn resolve_sel(&self, body: Id, el: Element, sel: &Sel) -> Result<Vec<u32>> {
        let q = sel.query(el)?;
        if let Sel::Ids(ids) = sel {
            let live: Vec<u32> = match el {
                Element::Faces => self.p.regen_faces.get(&body).map(|v| v.iter().map(|f| f.id).collect()).unwrap_or_default(),
                Element::Edges => self.p.regen_edges.get(&body).map(|v| v.iter().map(|e| e.id).collect()).unwrap_or_default(),
            };
            let unknown: Vec<u32> = ids.iter().copied().filter(|i| !live.contains(i)).collect();
            if !unknown.is_empty() {
                return Err(Error::NotFound(format!(
                    "{} {unknown:?} not on body {body} (ids belong to one body and change with every feature: call topology on the current body)",
                    el.name()
                )));
            }
        }
        let r = Ref::many(q);
        let found = match el {
            Element::Faces => self.p.resolve_face_refs(body, &r, "select"),
            Element::Edges => self.p.resolve_edge_refs(body, &r, "select"),
        }
        .map_err(|e| Error::Invalid(format!("selection did not resolve: {e:?}")))?;
        Ok(found)
    }

    /// Resolve a selection that must name exactly one face; returns its key (id, centroid, normal).
    pub(crate) fn one_face(&self, body: Id, sel: &Sel) -> Result<FaceKey> {
        let ids = self.resolve_sel(body, Element::Faces, sel)?;
        let [id] = ids.as_slice() else {
            return Err(Error::Invalid(format!("the face selection must match exactly one face of body {body}, it matched {ids:?}")));
        };
        let f = self
            .p
            .regen_faces
            .get(&body)
            .and_then(|fs| fs.iter().find(|f| f.id == *id))
            .ok_or_else(|| Error::NotFound(format!("face {id}")))?;
        Ok(face_key(f))
    }

    /// A document opened from a file has live B-reps but no edges (and, before `Session::open` restores them, no
    /// faces) until its nodes rebuild (F-3B-1): rebuild everything once if a current body lacks them.
    pub(crate) fn ensure_topology(&mut self) {
        let consumed = self.p.consumed_bodies();
        let missing = self.p.timeline.iter().filter(|n| !n.suppressed).flat_map(|n| n.kind.bodies()).any(|b| {
            !consumed.contains(&b)
                && self.shapes.contains_key(&b)
                && !(self.p.regen_faces.contains_key(&b) && self.p.regen_edges.contains_key(&b))
        });
        if missing {
            self.p.mark_all_dirty();
            self.rebuild();
        }
    }

    /// Whether face `id` of `body` is planar.
    pub(crate) fn face_is_planar(&self, body: Id, id: u32) -> bool {
        let f = self.p.regen_faces.get(&body).and_then(|fs| fs.iter().find(|f| f.id == id));
        let mesh = self.p.mesh_index(body).map(|i| &self.p.bodies[i].mesh);
        matches!((f, mesh), (Some(f), Some(m)) if planar_normal(f, m).is_some())
    }

    /// The body to read topology from: the given one (any built body) or the current body of the part.
    fn topo_body(&self, body: Option<Id>) -> Result<Id> {
        let b = match body {
            Some(b) => b,
            None => self.tip_body().ok_or_else(|| Error::Invalid("the part has no body yet".into()))?,
        };
        if !self.p.regen_faces.contains_key(&b) {
            return Err(Error::NotFound(format!("body {b} has no topology (not a built body, or not rebuilt yet)")));
        }
        Ok(b)
    }

    /// The body a modifier works on: the given one or the current body; it must be a current result (features
    /// consume their source, and branching from a consumed body makes a chain the GUI refuses, F-009).
    pub(crate) fn source_body(&self, body: Option<Id>) -> Result<Id> {
        let b = match body {
            Some(b) => b,
            None => self.tip_body().ok_or_else(|| Error::Invalid("the part has no body yet; extrude or revolve first".into()))?,
        };
        if !self.p.timeline.iter().any(|n| n.kind.bodies().contains(&b)) {
            return Err(Error::NotFound(format!("{b} is not a body")));
        }
        if self.p.consumed_bodies().contains(&b) {
            let cur = self.tip_body().map(|t| format!("; the current body is {t}")).unwrap_or_default();
            return Err(Error::Invalid(format!("body {b} was consumed by a later feature{cur}")));
        }
        if !self.shapes.contains_key(&b) || !self.p.regen_faces.contains_key(&b) {
            return Err(Error::Invalid(format!("body {b} is not built")));
        }
        Ok(b)
    }
}

/// The unit normal of a face if all its triangles are coplanar (within ~0.25°), else `None`.
fn planar_normal(f: &MeshFace, mesh: &qymcad_core::geom::Mesh) -> Option<[f64; 3]> {
    let mut ns: Vec<([f64; 3], f64)> = Vec::with_capacity(f.triangles.len());
    for &ti in &f.triangles {
        let t = mesh.tris.get(ti as usize)?;
        let p: Vec<[f64; 3]> = t.iter().map(|&i| mesh.verts.get(i as usize).map(|v| [v.x, v.y, v.z])).collect::<Option<_>>()?;
        let (u, v) = (sub(p[1], p[0]), sub(p[2], p[0]));
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = norm(n);
        if l > 1e-12 {
            ns.push(([n[0] / l, n[1] / l, n[2] / l], l));
        }
    }
    let total = ns.iter().fold([0.0; 3], |acc, (n, w)| [acc[0] + n[0] * w, acc[1] + n[1] * w, acc[2] + n[2] * w]);
    let avg = unit(total).ok()?;
    ns.iter().all(|(n, _)| dot(*n, avg) > 0.99999).then_some(avg)
}

/// The angle an arc spans, radians: from `a` to `b` through `mid` about `c`.
fn arc_angle(a: [f64; 3], b: [f64; 3], mid: [f64; 3], c: [f64; 3]) -> f64 {
    let (ca, cb) = (sub(a, c), sub(b, c));
    let cos = (dot(ca, cb) / (norm(ca) * norm(cb)).max(1e-300)).clamp(-1.0, 1.0);
    let theta = cos.acos();
    // The short way round passes near the chord's midpoint side; if `mid` is on the other side, it is the long way.
    let half = [(a[0] + b[0]) / 2.0 - c[0], (a[1] + b[1]) / 2.0 - c[1], (a[2] + b[2]) / 2.0 - c[2]];
    if dot(sub(mid, c), half) < 0.0 {
        2.0 * std::f64::consts::PI - theta
    } else {
        theta
    }
}

fn polyline_length(p: &[[f32; 3]]) -> f64 {
    p.windows(2)
        .map(|w| {
            let d = [(w[1][0] - w[0][0]) as f64, (w[1][1] - w[0][1]) as f64, (w[1][2] - w[0][2]) as f64];
            norm(d)
        })
        .sum()
}
