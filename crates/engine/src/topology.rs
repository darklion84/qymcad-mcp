//! Topology of a built body (faces and edges with persistent ids) and selections: explicit ids or descriptive
//! queries that QymCAD re-evaluates on every rebuild (FINDINGS F-010).
//!
//! Ids are only valid after a rebuild and belong to one body: a feature creates a new body (F-009), so re-read
//! the topology of the current body after every feature.

use crate::error::{Error, Result};
use crate::session::Session;
use qymcad_core::feature::FaceKey;
use qymcad_core::geom::{MeshEdge, MeshFace};
use qymcad_core::model::Id;
use qymcad_core::names::Role as QRole;
use qymcad_core::refs::{Axis as QAxis, Query, Ref};
use qymcad_kernel::Shape;
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
    /// Plane, cylinder, cone, sphere or other.
    pub kind: FaceKind,
    /// Area-weighted centre of the face, mm.
    pub centroid: [f64; 3],
    /// Outward unit normal; planes only (on curved faces an average normal is meaningless, F-011).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal: Option<[f64; 3]>,
    /// mm²
    pub area: f64,
    /// Cylinder/cone axis: a point on it and the unit direction. For cylinders the point is the
    /// face centroid projected onto the axis, at the face's axial position (not on its curved surface).
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
    /// Line, full circle, arc or other (spline).
    pub kind: EdgeKind,
    /// Start and end points (equal for a full circle).
    pub a: [f64; 3],
    /// End point (equals `a` for a full circle).
    pub b: [f64; 3],
    /// Midpoint along the edge.
    pub mid: [f64; 3],
    /// mm
    pub length: f64,
    /// Circle/arc centre, axis (unit normal of its plane) and radius.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center: Option<[f64; 3]>,
    /// Circle/arc axis: the unit normal of its plane.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis: Option<[f64; 3]>,
    /// Circle/arc radius, mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    /// The two faces meeting at the edge (with `adjacency`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faces: Option<[u32; 2]>,
    /// A seam: the closing line of a round face, with that face on both sides. Not a real corner; fillets and
    /// chamfers ignore it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub seam: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Topology {
    /// The body the ids belong to.
    pub body: Id,
    /// Its faces.
    pub faces: Vec<FaceInfo>,
    /// Its edges.
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
/// on every rebuild (so it survives upstream edits — prefer it). Engine-only edge filters are lowered to ids
/// before mapping onto `qymcad_core::refs::Query`.
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
    /// Inward corners between two planar faces along a straight edge; excludes seams and tangent junctions.
    Concave,
    /// Outward corners between two planar faces along a straight edge; excludes seams and tangent junctions.
    Convex,
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
            Sel::Concave | Sel::Convex => {
                let what = if matches!(self, Sel::Concave) { "concave" } else { "convex" };
                if el == Element::Faces {
                    return bad(what, "it tests edge corners between two planar faces");
                }
                return Err(Error::Invalid(format!("`{what}` needs a body to resolve its edge corners")));
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
                if v.is_empty() {
                    return Err(Error::Invalid("`union` needs at least one selection".into()));
                }
                // Nested unions merge; a union of id lists is one flat list (as upstream `Ref::picks` does);
                // anything else becomes a balanced tree rather than a `Union(Union(..))` ladder, whose depth
                // grows with the count and breaks saving (F-032).
                let mut flat = Vec::new();
                flatten_union(v, &mut flat);
                if flat.iter().all(|s| matches!(s, Sel::Ids(_))) {
                    let mut ids: Vec<u32> = Vec::new();
                    for id in flat.iter().flat_map(|s| if let Sel::Ids(v) = s { v.clone() } else { Vec::new() }) {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                    return Sel::Ids(ids).query(el);
                }
                balanced(flat.iter().map(|s| s.query(el)).collect::<Result<Vec<_>>>()?)?
            }
            Sel::Minus(a, b) => Query::Minus(Box::new(a.query(el)?), Box::new(b.query(el)?)),
            Sel::And(a, b) => Query::Filter(Box::new(a.query(el)?), Box::new(b.query(el)?)),
        })
    }
}

/// Largest selection accepted, in parts (a union of id lists counts as one).
const MAX_SEL_PARTS: usize = 512;
/// Deepest query stored. A `Union` ladder 150 deep still saved, 300 did not (RON's recursion limit counts the
/// document's own nesting too, F-032); 48 leaves a wide margin and is far beyond any real description.
const MAX_QUERY_DEPTH: usize = 48;

impl Sel {
    /// The QymCAD query for `el`, within the size and depth budget. Every stored or resolved selection goes
    /// through here.
    pub(crate) fn to_query(&self, el: Element) -> Result<Query> {
        let parts = self.parts();
        if parts > MAX_SEL_PARTS {
            return Err(Error::Invalid(format!("selection has {parts} parts, at most {MAX_SEL_PARTS}: use ids or a broader description")));
        }
        let q = self.query(el)?;
        let depth = query_depth(&q);
        if depth > MAX_QUERY_DEPTH {
            return Err(Error::Invalid(format!("selection nested {depth} levels deep, at most {MAX_QUERY_DEPTH}")));
        }
        Ok(q)
    }

    fn parts(&self) -> usize {
        match self {
            Sel::Union(v) if v.iter().all(|s| matches!(s, Sel::Ids(_))) => 1,
            Sel::Union(v) => 1 + v.iter().map(Sel::parts).sum::<usize>(),
            Sel::EdgesOf(a) | Sel::TangentChain { seed: a, .. } => 1 + a.parts(),
            Sel::Between(a, b) | Sel::Minus(a, b) | Sel::And(a, b) => 1 + a.parts() + b.parts(),
            _ => 1,
        }
    }
}

fn flatten_union<'a>(v: &'a [Sel], out: &mut Vec<&'a Sel>) {
    for s in v {
        match s {
            Sel::Union(inner) => flatten_union(inner, out),
            other => out.push(other),
        }
    }
}

/// A balanced `Union` tree over `qs` (non-empty): depth log2(n) instead of n.
fn balanced(mut qs: Vec<Query>) -> Result<Query> {
    match qs.len() {
        0 => Err(Error::Invalid("`union` needs at least one selection".into())),
        1 => qs.pop().ok_or_else(|| Error::Invalid("`union` needs at least one selection".into())),
        n => {
            let right = qs.split_off(n / 2);
            Ok(Query::Union(Box::new(balanced(qs)?), Box::new(balanced(right)?)))
        }
    }
}

fn query_depth(q: &Query) -> usize {
    1 + match q {
        Query::Adjacent(a) | Query::TangentChain { seed: a, .. } => query_depth(a),
        Query::Between(a, b) | Query::Union(a, b) | Query::Minus(a, b) | Query::Filter(a, b) => query_depth(a).max(query_depth(b)),
        _ => 0,
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
        self.ensure_topology()?;
        let body = self.topo_body(body)?;
        let shape = self.shapes.get(&body).ok_or_else(|| Error::NotFound(format!("body {body} has no built shape")))?;
        let faces = self.p.regen_faces.get(&body).map(Vec::as_slice).unwrap_or_default();
        let medges = self.p.regen_edges.get(&body).map(Vec::as_slice).unwrap_or_default();
        let mesh = self.p.mesh_index(body).map(|i| &self.p.bodies[i].mesh);

        let _gate = qymcad_kernel::kernel_gate();
        let polylines: HashMap<u32, Vec<[f32; 3]>> = shape.edges_info().into_iter().map(|e| (e.id, e.poly)).rev().collect();
        let pairs: HashMap<u32, [u32; 2]> = shape.edge_face_pairs().into_iter().map(|(e, a, b)| (e, [a, b])).collect();

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
                let t: f64 = (0..3).map(|i| (fi.centroid[i] - o[i]) * d[i]).sum();
                fi.axis = Some([std::array::from_fn(|i| o[i] + t * d[i]), d]);
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
            let (kind, length) = edge_kind_length(e, &polylines);
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
                faces: if adjacency { pairs.get(&e.id).copied() } else { None },
                seam: pairs.get(&e.id).is_some_and(|[a, b]| a == b),
            });
        }
        Ok(Topology { body, faces: out_faces, edges: out_edges })
    }

    /// Resolve a selection against `body` (default: the current body) now, as QymCAD will at the next rebuild.
    /// Returns the body and the matching ids.
    pub fn select(&mut self, body: Option<Id>, el: Element, sel: &Sel) -> Result<(Id, Vec<u32>)> {
        self.ensure_topology()?;
        let body = self.topo_body(body)?;
        Ok((body, self.resolve_sel(body, el, sel)?))
    }

    /// True lengths of the edges of `body`, as `topology` reports them.
    fn edge_lengths(&self, body: Id) -> HashMap<u32, f64> {
        let Some(shape) = self.shapes.get(&body) else { return HashMap::new() };
        let polylines: HashMap<u32, Vec<[f32; 3]>> = {
            let _gate = qymcad_kernel::kernel_gate();
            shape.edges_info().into_iter().map(|e| (e.id, e.poly)).rev().collect()
        };
        let medges = self.p.regen_edges.get(&body).map(Vec::as_slice).unwrap_or_default();
        medges.iter().map(|e| (e.id, edge_kind_length(e, &polylines).1)).collect()
    }

    /// A planar corner is concave when face A's local interior points outside face B's supporting plane.
    /// Use an incident triangle, not the face centroid: a concave face's centroid can lie outside the face.
    fn corner_edges(&self, body: Id, concave: bool) -> Vec<u32> {
        let Some(shape) = self.shapes.get(&body) else { return Vec::new() };
        let Some(mesh) = self.p.mesh_index(body).map(|i| &self.p.bodies[i].mesh) else { return Vec::new() };
        let faces: HashMap<_, _> = self.p.regen_faces.get(&body).into_iter().flatten().map(|f| (f.id, f)).collect();
        let normals: HashMap<_, _> = faces.iter().filter_map(|(id, f)| planar_normal(f, mesh).map(|n| (*id, n))).collect();
        let (polylines, pairs) = {
            let _gate = qymcad_kernel::kernel_gate();
            (
                shape.edges_info().into_iter().map(|e| (e.id, e.poly)).rev().collect::<HashMap<_, _>>(),
                shape.edge_face_pairs().into_iter().map(|(e, a, b)| (e, [a, b])).collect::<HashMap<_, _>>(),
            )
        };
        self.p
            .regen_edges
            .get(&body)
            .into_iter()
            .flatten()
            .filter_map(|e| {
                if edge_kind_length(e, &polylines).0 != EdgeKind::Line {
                    return None;
                }
                let [a, b] = *pairs.get(&e.id)?;
                if a == b {
                    return None;
                }
                let (na, nb) = (*normals.get(&a)?, *normals.get(&b)?);
                if dot(na, nb).abs() > 0.99999 {
                    return None;
                }
                let inward = edge_face_inward(e, faces.get(&a)?, mesh)?;
                let sign = dot(inward, nb);
                ((sign > 1e-6 && concave) || (sign < -1e-6 && !concave)).then_some(e.id)
            })
            .collect()
    }

    /// Lower engine-only edge filters to ids. QymCAD ranks `Largest` by chord rather than true length (F-030)
    /// and has no corner-sign query. All edge selections are stored as pick lists anyway (F-024).
    fn lower_edge_filters(&self, body: Id, el: Element, sel: &Sel, lengths: &mut Option<HashMap<u32, f64>>) -> Sel {
        let sub = |x: &Sel, el: Element, lengths: &mut Option<HashMap<u32, f64>>| Box::new(self.lower_edge_filters(body, el, x, lengths));
        match sel {
            Sel::Concave | Sel::Convex if el == Element::Edges => Sel::Ids(self.corner_edges(body, matches!(sel, Sel::Concave))),
            Sel::Largest if el == Element::Edges => {
                let l = lengths.get_or_insert_with(|| self.edge_lengths(body));
                let best = l.values().copied().fold(f64::MIN, f64::max);
                let mut ids: Vec<u32> =
                    l.iter().filter(|(_, v)| (best - **v).abs() <= 1e-9 * best.abs().max(1.0)).map(|(k, _)| *k).collect();
                ids.sort_unstable();
                Sel::Ids(ids)
            }
            Sel::EdgesOf(f) => Sel::EdgesOf(sub(f, Element::Faces, lengths)),
            Sel::TangentChain { seed, tol_deg } => Sel::TangentChain { seed: sub(seed, Element::Edges, lengths), tol_deg: *tol_deg },
            Sel::Between(a, b) => Sel::Between(sub(a, Element::Faces, lengths), sub(b, Element::Faces, lengths)),
            Sel::Union(v) => Sel::Union(v.iter().map(|x| *sub(x, el, lengths)).collect()),
            Sel::Minus(a, b) => Sel::Minus(sub(a, el, lengths), sub(b, el, lengths)),
            Sel::And(a, b) => Sel::And(sub(a, el, lengths), sub(b, el, lengths)),
            other => other.clone(),
        }
    }

    /// Every explicit id anywhere in `sel` must be a face or an edge of `body`, as its position requires (inside
    /// `edges_of`/`between` ids are faces; a `tangent_chain` seed is edges). QymCAD would drop an unknown id
    /// silently.
    fn check_ids(&self, body: Id, el: Element, sel: &Sel) -> Result<()> {
        let b = |x: &Sel, el| self.check_ids(body, el, x);
        match sel {
            Sel::Ids(ids) => {
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
                Ok(())
            }
            Sel::EdgesOf(f) => b(f, Element::Faces),
            Sel::TangentChain { seed, .. } => b(seed, Element::Edges),
            Sel::Between(x, y) => b(x, Element::Faces).and(b(y, Element::Faces)),
            Sel::Union(v) => v.iter().try_for_each(|x| b(x, el)),
            Sel::Minus(x, y) | Sel::And(x, y) => b(x, el).and(b(y, el)),
            Sel::OfFeature { .. }
            | Sel::Facing { .. }
            | Sel::Along { .. }
            | Sel::Concave
            | Sel::Convex
            | Sel::Extreme { .. }
            | Sel::Largest => Ok(()),
        }
    }

    /// Resolve `sel` on `body`. Explicit ids that are not on the body are an error (stale or foreign ids).
    pub(crate) fn resolve_sel(&self, body: Id, el: Element, sel: &Sel) -> Result<Vec<u32>> {
        let lowered = self.lower_edge_filters(body, el, sel, &mut None);
        let sel = &lowered;
        let q = sel.to_query(el)?;
        self.check_ids(body, el, sel)?;
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

    /// A document opened from a file has live B-reps but no edges, and no faces when the file did not store them
    /// (F-023). Edges come back from the B-reps (`restore_edges`, no rebuild, so stored edge queries keep their
    /// meaning, F-024); missing faces need one full rebuild.
    pub(crate) fn ensure_topology(&mut self) -> Result<()> {
        self.restore_edges()?;
        let consumed = self.p.consumed_bodies();
        let missing = self.p.timeline.iter().filter(|n| !n.suppressed).flat_map(|n| n.kind.bodies()).any(|b| {
            !consumed.contains(&b)
                && self.shapes.contains_key(&b)
                && !(self.p.regen_faces.contains_key(&b) && self.p.regen_edges.contains_key(&b))
        });
        if !missing {
            return Ok(());
        }
        // A rebuild that fails would pass sources through (F-008) and change the bodies: keep the document and
        // the shapes (as B-rep bytes; a Shape cannot be cloned) to put back. Only files saved without faces get here.
        let before = self.p.clone();
        let before_warnings = self.advisory_warnings.clone();
        let saved: Vec<(Id, Vec<u8>)> = {
            let _gate = qymcad_kernel::kernel_gate();
            self.shapes.iter().filter_map(|(id, sh)| sh.to_brep_bytes().map(|b| (*id, b))).collect()
        };
        self.p.mark_all_dirty();
        let r = self.rebuild();
        if r.errors.is_empty() {
            return Ok(());
        }
        self.p = before;
        self.advisory_warnings = before_warnings;
        self.shapes = {
            let _gate = qymcad_kernel::kernel_gate();
            saved.into_iter().filter_map(|(id, b)| Shape::from_brep_bytes(&b).map(|sh| (id, sh))).collect()
        };
        let mut lines = vec!["the document has no stored faces and needs a full rebuild to read its topology; it failed".to_string()];
        lines.extend(r.errors.iter().map(|i| format!("{} ({}): {}", i.name, i.node, i.message)));
        Err(Error::Rebuild(lines))
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

/// In-plane direction into the triangle sharing the edge midpoint, perpendicular to the edge.
fn edge_face_inward(e: &MeshEdge, face: &MeshFace, mesh: &qymcad_core::geom::Mesh) -> Option<[f64; 3]> {
    let dir = unit(sub(e.b, e.a)).ok()?;
    // Kernel tessellation carries float coordinates; allow their relative rounding error.
    let scale = e.a.into_iter().chain(e.b).map(f64::abs).fold(1.0, f64::max);
    let eps = 1e-6 * scale;
    for &ti in &face.triangles {
        let t = mesh.tris.get(ti as usize)?;
        let p: Vec<_> = t.iter().map(|&i| mesh.verts.get(i as usize).map(|v| [v.x, v.y, v.z])).collect::<Option<_>>()?;
        for i in 0..3 {
            let (a, b) = (p[i], p[(i + 1) % 3]);
            let ab = sub(b, a);
            let length = norm(ab);
            if length <= eps || dot(unit(ab).ok()?, dir).abs() < 0.99999 {
                continue;
            }
            let am = sub(e.mid, a);
            let fraction = dot(am, ab) / (length * length);
            let offset = sub(am, ab.map(|x| x * fraction));
            if fraction < -eps / length || fraction > 1.0 + eps / length || norm(offset) > eps {
                continue;
            }
            let interior = sub(p[(i + 2) % 3], e.mid);
            return unit(sub(interior, dir.map(|x| x * dot(interior, dir)))).ok();
        }
    }
    None
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

/// An edge's kind and true length: exact for lines and circles/arcs, the polyline length otherwise.
fn edge_kind_length(e: &MeshEdge, polylines: &HashMap<u32, Vec<[f32; 3]>>) -> (EdgeKind, f64) {
    let chord = norm(sub(e.b, e.a));
    if e.radius > 1e-9 {
        if chord < 1e-6 {
            (EdgeKind::Circle, 2.0 * std::f64::consts::PI * e.radius)
        } else {
            (EdgeKind::Arc, arc_angle(e.a, e.b, e.mid, e.center) * e.radius)
        }
    } else {
        match polylines.get(&e.id).map(|p| polyline_length(p)) {
            Some(l) if (l - chord).abs() <= 1e-4 * l.max(1.0) => (EdgeKind::Line, chord),
            Some(l) => (EdgeKind::Other, l),
            None => (EdgeKind::Line, chord),
        }
    }
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
