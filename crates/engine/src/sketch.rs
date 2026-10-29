//! Sketches: planar profiles. Every entity added here is fully dimensioned (size and position from the sketch
//! origin), with the given expressions as the driving values, so the sketch stays fully defined and editable
//! in the QymCAD GUI.

mod constrain;
mod corner_edit;
mod detail;
mod edits;
mod entities;
mod figures;
mod mirror_point;
mod offset_edit;
mod selectors;
mod splines;
mod text;

pub use corner_edit::{CornerSize, CornerSpec};
pub use edits::{BreakSpec, CurvePickSpec, EditedSketch, ExtendSpec};
pub use mirror_point::MirrorPointSpec;
pub use offset_edit::SketchOffsetSpec;

pub use constrain::{ConstrainSpec, Constrained, ConstraintKind, DistAxis, FrameRef, SketchRef};
pub use detail::{ConstraintInfo, EntityInfo, PointInfo, SketchDetail};
pub use entities::{Added, ArcSpec, LineSpec, PolygonSpec, PolylineSpec, SlotSpec, Xy};
pub use figures::{EllipseArcSpec, EllipseSpec, RectAnchor, RectSpec};
pub use selectors::{
    CurveQuery, EntityRef, OriginRef, PointRole, PointRoleSel, PointSel, SketchCurveKind, SketchCurveSel, TangentCircleSpec,
};
pub use splines::SplineSpec;
pub use text::{TextFont, TextInfo, TextSpec};

use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::value::Num;
use qymcad_core::feature::{BasePlane, FaceKey, Purpose, SketchPlane};
use qymcad_core::model::{entity_points, Constraint, EntityKind, Id, PlaneDef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionResult {
    pub projections: Vec<Id>,
    pub entities: Vec<Id>,
    pub points: Vec<Id>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct AddedPoint {
    pub point: Id,
    pub reused: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct IsolatedPointInfo {
    pub id: Id,
    #[serde(serialize_with = "crate::session::serialize_coordinate")]
    pub x: f64,
    #[serde(serialize_with = "crate::session::serialize_coordinate")]
    pub y: f64,
    /// Whether native constraints leave this point no freedom to move.
    pub dimensioned: bool,
}

/// Where a sketch (or an offset plane) sits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaneRef {
    /// A base plane: `XY` (normal +Z), `XZ` (normal −Y), `YZ` (normal +X).
    Base(BaseName),
    /// A datum plane by id (from `plane_offset`).
    Plane(Id),
    /// A planar face of a body (face id from `topology`).
    Face { body: Id, face: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum BaseName {
    XY,
    XZ,
    YZ,
}

impl From<BaseName> for BasePlane {
    fn from(b: BaseName) -> Self {
        match b {
            BaseName::XY => BasePlane::XY,
            BaseName::XZ => BasePlane::XZ,
            BaseName::YZ => BasePlane::YZ,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ContourInfo {
    pub id: Id,
    /// The enclosing contour, if any. Extruding a contour extrudes it minus its direct children (holes).
    pub parent: Option<Id>,
    /// mm²
    pub area: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SketchWorldFrame {
    /// World origin in mm (including component placement and any GUI origin shift).
    #[serde(serialize_with = "crate::session::serialize_coordinates")]
    pub origin: [f64; 3],
    /// World unit direction of positive sketch x.
    #[serde(serialize_with = "crate::session::serialize_coordinates")]
    pub x_axis: [f64; 3],
    /// World unit direction of positive sketch y.
    #[serde(serialize_with = "crate::session::serialize_coordinates")]
    pub y_axis: [f64; 3],
    /// World unit normal, x cross y.
    #[serde(serialize_with = "crate::session::serialize_coordinates")]
    pub normal: [f64; 3],
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SketchInfo {
    pub id: Id,
    pub name: String,
    pub plane: String,
    /// Resolved world frame, or null when the sketch's host cannot be resolved.
    pub world_frame: Option<SketchWorldFrame>,
    /// Remaining degrees of freedom (0 = fully defined) and redundant constraints.
    pub dof: (i32, i32),
    pub contours: Vec<ContourInfo>,
    /// Non-system points that belong to no entity.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub isolated_points: Vec<IsolatedPointInfo>,
    /// Native texts, including construction texts and their baked glyph contour counts.
    pub texts: Vec<TextInfo>,
}

impl Session {
    /// Add a native isolated point or reuse an existing non-system point within 1e-6 mm.
    pub fn sketch_point(&mut self, sketch: Id, x: &Num, y: &Num, dimensioned: bool) -> Result<AddedPoint> {
        self.transaction(|s| {
            let si = s.sketch_si(sketch)?;
            let vars = s.p.param_map();
            let (vx, vy) = (x.eval(&vars)?, y.eval(&vars)?);
            let count = s.p.sketches[si].points.len();
            let point = s.p.sketch_point_at(si, vx, vy, 1e-6);
            let reused = s.p.sketches[si].points.len() == count;
            if reused {
                return Ok(AddedPoint { point, reused });
            }
            if dimensioned {
                // Reverse the y axis so positive x has a positive signed DistancePL driver. The two
                // reference sets are distinct, unlike origin Distance{axis}, so native names are unique.
                let (ya, yb) = s.p.ensure_axis(si, 1);
                let (xa, xb) = s.p.ensure_axis(si, 0);
                for (coordinate, value, a, b) in [(x, vx, yb, ya), (y, vy, xa, xb)] {
                    let constraint = if coordinate.expr().is_none() && value == 0.0 {
                        Constraint::PointOnLine { p: point, a, b }
                    } else {
                        // Native preserves side with `d < 0`, which loses a negative side at zero.
                        // Encode the side in the axis orientation instead: d always stays nonnegative,
                        // including after save/load and the app's own parameter/driver edits.
                        let (a, b) = if value < 0.0 { (b, a) } else { (a, b) };
                        Constraint::DistancePL {
                            p: point,
                            a,
                            b,
                            d: value.abs(),
                            off: 0.0,
                            // Numeric strings can use exponent notation; the native formula parser cannot.
                            // Keep them as editable decimal formulas, including a string evaluating to zero.
                            expr: if coordinate.expr().is_some_and(|e| e.parse::<f64>().is_ok()) {
                                value.abs().to_string()
                            } else {
                                coordinate.magnitude_expr(value)?
                            },
                            driven: false,
                            at: None,
                        }
                    };
                    s.p.add_constraint_if_independent(si, constraint);
                }
            }
            s.finish_sketch_edit(si)?;
            Ok(AddedPoint { point, reused })
        })
    }

    /// Delete only an isolated non-system point, preserving any entity incident to a refused point.
    pub fn sketch_remove_point(&mut self, sketch: Id, point: Id) -> Result<()> {
        self.transaction(|s| {
            let si = s.sketch_si(sketch)?;
            let sk = &s.p.sketches[si];
            if !sk.points.iter().any(|p| p.id == point) {
                return Err(Error::NotFound(format!("point {point} in sketch {sketch} (see sketch_info)")));
            }
            if sk.system_ids().contains(&point) {
                return Err(Error::Invalid(format!("point {point} anchors the sketch's origin/axes and cannot be removed")));
            }
            if let Some(entity) = sk.entities.iter().find(|e| entity_points(e).contains(&point)) {
                return Err(Error::Invalid(format!(
                    "point {point} belongs to entity {}; remove that entity with sketch_remove instead",
                    entity.id
                )));
            }
            s.p.delete_points(si, &[point]);
            s.finish_sketch_edit(si)
        })
    }

    /// Create an empty sketch on `plane`. Returns the sketch id (also its timeline node id).
    pub fn sketch_create(&mut self, plane: &PlaneRef, name: Option<&str>) -> Result<Id> {
        let name = Self::optional_object_name(name);
        if let Some(name) = name {
            Self::check_object_name(name)?;
        }
        let sp = self.sketch_plane(plane)?;
        let n = self.p.sketches.len() + 1;
        let name = name.map(str::to_string).unwrap_or_else(|| format!("Sketch {n}"));
        let owner = match plane {
            PlaneRef::Face { body, .. } => self.p.body_owner(*body),
            _ => self.p.active_component,
        };
        let sid = self.in_component(owner, |p| {
            let sid = p.add_sketch(name.clone(), vec![], None);
            p.add_sketch_node(sid, name);
            sid
        });
        let si = self.sketch_si(sid)?;
        self.p.sketches[si].plane = sp;
        self.track_datum_dependencies(sid);
        Ok(sid)
    }

    /// Native feature dimensions make a datum sketch dirty before parallel preparation (F-017).
    /// The pinned scheduler otherwise misses the datum dependency and can capture an old sketch frame.
    /// Persist every placement formula and named driver of every ancestor. A child's normal can change
    /// without the native scheduler seeing its ancestor's tilt (D2); the guards retain the app barrier.
    pub(crate) fn track_datum_dependencies(&mut self, sid: Id) {
        let Some(si) = self.p.sketch_index(sid) else { return };
        let mut desired = HashMap::new();
        if let SketchPlane::Datum(mut plane) = self.p.sketches[si].plane {
            // Bound traversal even for malformed cycles in externally produced documents.
            for _ in 0..self.p.planes.len() {
                for key in ["dist", "tilt_x", "tilt_y", "turn"] {
                    if let Some(expr) = self.p.feat_dim(plane, key) {
                        desired.insert(format!("datum_{key}_{plane}"), expr.to_string());
                    }
                    let name = self.p.name_of_target(&qymcad_core::model::DimTarget::Feature { node: plane, key: key.into() });
                    if !name.is_empty() {
                        desired.insert(format!("datum_driver_{key}_{plane}"), name);
                    }
                }
                match self.p.planes.iter().find(|p| p.id == plane).map(|p| &p.def) {
                    Some(PlaneDef::OffsetPlane { plane: parent, .. }) => plane = *parent,
                    _ => break,
                }
            }
        }
        // A GUI edit may change the datum expression or reattach the sketch. Drop only obsolete guards.
        let obsolete: Vec<String> = self
            .p
            .feat_dims
            .get(&sid)
            .into_iter()
            .flat_map(|m| m.keys())
            .filter(|k| is_datum_guard(k) && !desired.contains_key(*k))
            .cloned()
            .collect();
        for key in obsolete {
            self.p.set_feat_dim(sid, &key, String::new());
        }
        for (key, expr) in desired {
            if self.p.feat_dim(sid, &key) != Some(expr.as_str()) {
                self.p.set_feat_dim(sid, &key, expr);
            }
        }
    }

    /// Refresh persisted barriers after a formula, driver name or host changes, including after open.
    pub(crate) fn reconcile_datum_dependencies(&mut self) {
        let sketches: Vec<Id> = self.p.sketches.iter().map(|sketch| sketch.id).collect();
        for sketch in sketches {
            self.track_datum_dependencies(sketch);
        }
    }

    /// A source must already exist when native regeneration visits the sketch (run.rs:580).
    /// Missing sources retain native Unknown inspection behavior when opening older documents.
    pub(crate) fn check_projection_order(&self, sketch: Id, body: Id) -> Result<()> {
        let source = self.p.timeline.iter().position(|node| node.kind.bodies().contains(&body));
        let target = self.p.timeline.iter().position(|node| node.id == sketch);
        if let (Some(source), Some(target)) = (source, target) {
            if source >= target {
                return Err(Error::Invalid(format!("body {body}: body must be built above the sketch {sketch} in the timeline")));
            }
        }
        Ok(())
    }

    /// Native projection reads source-part local geometry without applying component placement.
    /// Share this owner comparison between creation policy and stored-projection diagnostics.
    pub(crate) fn projection_cross_part(&self, sketch: Id, body: Id) -> bool {
        self.p.body_owner(body) != self.p.sketch_owner(sketch)
    }

    /// Project body geometry into a sketch and rebuild its consumers as one atomic edit.
    pub fn sketch_project(
        &mut self,
        sketch: Id,
        body: Id,
        sources: &[qymcad_core::model::ProjSource],
    ) -> Result<(ProjectionResult, Option<Rebuild>)> {
        if sources.is_empty() {
            return Err(Error::Invalid("projection selection resolved to nothing".into()));
        }
        let si = self.sketch_si(sketch)?;
        if !self.p.timeline.iter().any(|node| node.kind.bodies().contains(&body)) {
            return Err(Error::NotFound(format!("body {body}")));
        }
        self.check_projection_order(sketch, body)?;
        if self.projection_cross_part(sketch, body) {
            return Err(Error::Invalid("project only bodies of the sketch's own part".into()));
        }
        if !self.shapes.contains_key(&body) {
            return Err(Error::Invalid(format!("body {body} has no built shape")));
        }
        if self.p.consumed_bodies().contains(&body) {
            return Err(Error::Invalid(format!("body {body} is consumed; re-read current topology")));
        }
        let mut unique_sources = Vec::new();
        for source in sources {
            if !unique_sources.contains(source) {
                unique_sources.push(*source);
            }
        }
        for source in &unique_sources {
            match source {
                qymcad_core::model::ProjSource::Edge(edge) => {
                    if self.p.regen_edges.get(&body).is_none_or(|edges| edges.iter().filter(|candidate| candidate.id == *edge).count() != 1)
                    {
                        return Err(Error::NotFound(format!("edge {edge} of body {body} (re-read topology after each rebuild)")));
                    }
                }
                qymcad_core::model::ProjSource::Face(face) => {
                    let hosted = matches!(self.p.sketches[si].plane, qymcad_core::feature::SketchPlane::Face(host, key) if self.p.live_body(host) == body && key.id == *face);
                    if !hosted {
                        return Err(Error::Invalid(format!(
                            "face projection requires sketch {sketch} to be hosted on face {face} of body {body}"
                        )));
                    }
                    if self.p.regen_faces.get(&body).is_none_or(|faces| faces.iter().filter(|candidate| candidate.id == *face).count() != 1)
                    {
                        return Err(Error::NotFound(format!("face {face} of body {body} (missing or ambiguous; re-read topology)")));
                    }
                }
            }
        }
        let before = self.p.clone();
        self.p.mark_sketch_dirty(sketch);
        let nodes = crate::dependencies::rebuild_nodes(&self.p);
        self.with_rebuild_copies(before, &nodes, "sketch projection", |session| {
            let si = session.sketch_si(sketch)?;
            let provenance = session.projection_provenance();
            let kernel =
                qymcad_kernel::OcctKernel { shapes: std::cell::RefCell::new(std::mem::take(&mut session.shapes)), ..Default::default() };
            let mut projections = Vec::new();
            {
                let _gate = qymcad_kernel::kernel_gate();
                for source in &unique_sources {
                    let id = session.p.add_sketch_projection(si, body, *source, &kernel);
                    let projection = session.p.sketches[si].projections.iter().find(|p| p.id == id && !p.entities.is_empty());
                    if projection.filter(|_| id != 0).is_none() {
                        session.shapes = kernel.shapes.into_inner();
                        return Err(Error::Invalid(format!("projection source {source:?} yielded no geometry")));
                    }
                    projections.push(id);
                }
            }
            session.shapes = kernel.shapes.into_inner();
            session.remember_detached_projections(provenance);
            let residual = session.solve_settled(si);
            session.p.mark_sketch_dirty(sketch);
            if residual > 1e-6 {
                return Err(Error::Invalid(format!("sketch {sketch} does not solve after projection (residual {residual:.3e})")));
            }
            let report = session.rebuild_retrying(Some(&nodes));
            if !report.errors.is_empty() {
                return Err(Error::Rebuild(report.errors.iter().map(|e| format!("{} ({}): {}", e.name, e.node, e.message)).collect()));
            }
            let mut created_entities = std::collections::BTreeSet::new();
            let mut created_points = std::collections::BTreeSet::new();
            for id in &projections {
                let projection = session.p.sketches[si]
                    .projections
                    .iter()
                    .find(|p| p.id == *id && !p.entities.is_empty())
                    .ok_or_else(|| Error::Invalid("projection lost its geometry during rebuild".into()))?;
                created_entities.extend(projection.entities.iter().copied());
                created_points.extend(projection.points.iter().copied());
            }
            Ok((
                ProjectionResult {
                    projections,
                    entities: created_entities.into_iter().collect(),
                    points: created_points.into_iter().collect(),
                },
                Some(report),
            ))
        })
    }

    /// Add a rectangle centred at (`cx`, `cy`) of size `w` × `h` in sketch coordinates. Returns its four line
    /// ids (bottom, right, top, left).
    pub fn sketch_rect(&mut self, sketch: Id, cx: &Num, cy: &Num, w: &Num, h: &Num, construction: bool) -> Result<Vec<Id>> {
        self.sketch_rectangle(
            sketch,
            &RectSpec {
                cx: cx.clone(),
                cy: cy.clone(),
                w: w.clone(),
                h: h.clone(),
                turn: Num::Value(0.0),
                anchor: RectAnchor::Centre,
                construction,
            },
        )
    }

    /// Add a circle centred at (`cx`, `cy`) with diameter `d`. Returns the circle entity id.
    pub fn sketch_circle(&mut self, sketch: Id, cx: &Num, cy: &Num, d: &Num, construction: bool) -> Result<Id> {
        self.transaction(|s| s.sketch_circle_inner(sketch, cx, cy, d, construction))
    }

    fn sketch_circle_inner(&mut self, sketch: Id, cx: &Num, cy: &Num, d: &Num, construction: bool) -> Result<Id> {
        let si = self.sketch_si(sketch)?;
        let vars = self.p.param_map();
        let (vx, vy, vd) = (cx.eval(&vars)?, cy.eval(&vars)?, d.eval(&vars)?);
        if vd <= 0.0 {
            return Err(Error::Invalid(format!("circle diameter must be positive, got {vd}")));
        }
        let eid = self.p.add_circle_entity(si, vx, vy, vd / 2.0, purpose(construction));
        let center = self.p.sketches[si]
            .entities
            .iter()
            .find_map(|e| match e.kind {
                EntityKind::Circle { center, .. } if e.id == eid => Some(center),
                _ => None,
            })
            .ok_or_else(|| Error::Invalid("circle was not created".into()))?;
        let idx = self.p.ensure_diameter(si, center, true).ok_or_else(|| Error::Invalid("cannot dimension the circle".into()))?;
        if let Constraint::Diameter { expr, d: dd, .. } = &mut self.p.sketches[si].constraints[idx] {
            *expr = d.magnitude_expr(vd)?;
            *dd = vd;
        }
        self.pin_point(si, center, cx, vx, cy, vy)?;
        self.finish_sketch_edit(si)?;
        Ok(eid)
    }

    pub fn sketch_info(&self, sketch: Id) -> Result<SketchInfo> {
        let si = self.sketch_si(sketch)?;
        let s = &self.p.sketches[si];
        let contours = s
            .contour_ids
            .iter()
            .map(|&cid| ContourInfo {
                id: cid,
                parent: self.p.contours.parent_of(cid).filter(|&x| x != 0),
                area: self.p.contours.index_of(cid).map(|i| self.p.contours[i].signed_area().abs()).unwrap_or(0.0),
            })
            .collect();
        let world_frame = self.p.sketch_frame(si).map(|f| {
            let f = self.p.sketch_owner(sketch).map(|owner| f.transformed(&self.p.world_transform(owner))).unwrap_or(f);
            SketchWorldFrame { origin: f.origin, x_axis: f.x, y_axis: f.y, normal: f.normal() }
        });
        let isolated: HashSet<Id> = self.p.sketch_hole_point_ids(sketch).into_iter().collect();
        let free = self.p.sketch_free_points(si);
        let isolated_points = s
            .points
            .iter()
            .zip(free)
            .filter(|(p, _)| isolated.contains(&p.id))
            .map(|(p, free)| IsolatedPointInfo { id: p.id, x: p.x, y: p.y, dimensioned: !free })
            .collect();
        Ok(SketchInfo {
            id: s.id,
            name: crate::localization::name(&s.name),
            plane: plane_desc(self, &s.plane),
            world_frame,
            dof: self.p.sketch_dof(si),
            contours,
            isolated_points,
            texts: s
                .texts
                .iter()
                .map(|t| TextInfo {
                    id: t.id,
                    text: t.text.clone(),
                    font: TextFont { path: t.font.path.clone(), index: t.font.index },
                    family: t.font.family.clone(),
                    height: t.height,
                    at: [t.x, t.y],
                    angle: t.angle,
                    construction: t.construction,
                    contours: t.glyphs.len(),
                })
                .collect(),
        })
    }

    pub fn sketches(&self) -> Vec<SketchInfo> {
        self.p.sketches.iter().filter_map(|s| self.sketch_info(s.id).ok()).collect()
    }

    /// Contours that are not inside another: extruding them gives each region minus its holes.
    pub(crate) fn root_contours(&self, sketch: Id) -> Result<Vec<Id>> {
        let si = self.sketch_si(sketch)?;
        Ok(self.p.sketches[si].contour_ids.iter().copied().filter(|&c| self.p.contours.parent_of(c).is_none_or(|x| x == 0)).collect())
    }

    /// The contours picked by points in the sketch plane, as a click in the app picks them: for each point,
    /// the innermost closed contour that contains it (its region, minus the contours inside it, is what a
    /// feature takes). Points are evaluated once; the ids are what the feature stores. A point outside every
    /// contour, or within 1e-6 mm of a contour's edge, is refused. Repeated contours are listed once.
    pub fn contours_at(&self, sketch: Id, points: &[[Num; 2]]) -> Result<Vec<Id>> {
        let si = self.sketch_si(sketch)?;
        let vars = self.p.param_map();
        let loops: Vec<(Id, Vec<[f64; 2]>)> = self.p.sketches[si]
            .contour_ids
            .iter()
            .filter_map(|&c| self.p.contour_profile_xy(c).map(|xy| (c, xy.as_chunks::<2>().0.to_vec())))
            .collect();
        let mut picked = Vec::new();
        for [x, y] in points {
            let at = [x.eval(&vars)?, y.eval(&vars)?];
            if loops.iter().any(|(_, poly)| polygon_edge_distance(poly, at) <= 1e-6) {
                return Err(Error::Invalid(format!("point {at:?} lies on a contour's edge; pick a point inside the region")));
            }
            let inner = loops
                .iter()
                .filter(|(_, poly)| polygon_contains(poly, at))
                .min_by(|a, b| polygon_area(&a.1).total_cmp(&polygon_area(&b.1)))
                .map(|(c, _)| *c)
                .ok_or_else(|| Error::Invalid(format!("point {at:?} lies in no closed contour of sketch {sketch}")))?;
            if !picked.contains(&inner) {
                picked.push(inner);
            }
        }
        if picked.is_empty() {
            return Err(Error::Invalid("give at least one point".into()));
        }
        Ok(picked)
    }

    pub(crate) fn sketch_si(&self, sketch: Id) -> Result<usize> {
        self.p.sketch_index(sketch).ok_or_else(|| Error::NotFound(format!("sketch {sketch}")))
    }

    fn sketch_plane(&self, plane: &PlaneRef) -> Result<SketchPlane> {
        Ok(match plane {
            PlaneRef::Base(b) => SketchPlane::World((*b).into()),
            PlaneRef::Plane(id) => {
                if !self.p.planes.iter().any(|w| w.id == *id) {
                    return Err(Error::NotFound(format!("datum plane {id}")));
                }
                SketchPlane::Datum(*id)
            }
            PlaneRef::Face { body, face } => {
                let f = self
                    .p
                    .regen_faces
                    .get(body)
                    .and_then(|fs| fs.iter().find(|f| f.id == *face))
                    .ok_or_else(|| Error::NotFound(format!("face {face} of body {body} (re-read topology after each rebuild)")))?;
                SketchPlane::Face(
                    *body,
                    FaceKey { index: 0, centroid: [f.centroid.x, f.centroid.y, f.centroid.z], normal: f.normal, id: f.id },
                )
            }
        })
    }

    /// Fix point `p` at (`x`, `y`) from the sketch origin with driving dimensions. A plain zero puts the point
    /// on the axis instead (a zero distance has no side). Distances are magnitudes `|Δ|`: the side comes from
    /// the initial geometry and the expression is negated when its value is negative (see `Num::magnitude_expr`).
    /// Only the independent dimensions are added: the point may be shared with earlier, already dimensioned
    /// geometry (a circle centred on a polyline vertex takes that vertex as its centre, FINDINGS F-042).
    fn pin_point(&mut self, si: usize, p: Id, x: &Num, vx: f64, y: &Num, vy: f64) -> Result<()> {
        let dims = self.pin_dims(si, p, x, vx, y, vy)?;
        self.add_independent(si, dims);
        Ok(())
    }

    /// The two dimensions of `pin_point` ([x, y]), not yet added.
    pub(crate) fn pin_dims(&mut self, si: usize, p: Id, x: &Num, vx: f64, y: &Num, vy: f64) -> Result<[Constraint; 2]> {
        let origin = self.p.ensure_origin(si);
        let x_on_axis = x.expr().is_none() && vx == 0.0;
        let y_on_axis = y.expr().is_none() && vy == 0.0;
        let yaxis = if x_on_axis { Some(self.p.ensure_axis(si, 1)) } else { None };
        let xaxis = if y_on_axis { Some(self.p.ensure_axis(si, 0)) } else { None };
        let cx = match yaxis {
            Some((a, b)) => Constraint::PointOnLine { p, a, b },
            None => dist(p, origin, 1, vx.abs(), x.magnitude_expr(vx)?),
        };
        let cy = match xaxis {
            Some((a, b)) => Constraint::PointOnLine { p, a, b },
            None => dist(p, origin, 2, vy.abs(), y.magnitude_expr(vy)?),
        };
        Ok([cx, cy])
    }

    /// Add each candidate dimension only if it removes a degree of freedom (`add_constraint_if_independent`).
    /// A point that is already determined (shared with earlier geometry, or fixed by the entity's own
    /// constraints) is not dimensioned twice, so the sketch never becomes over-constrained.
    pub(crate) fn add_independent(&mut self, si: usize, dims: impl IntoIterator<Item = Constraint>) {
        for c in dims {
            self.p.add_constraint_if_independent(si, c);
        }
    }

    /// Apply a sketch edit as one unit (restored on error). When features are built from the sketch they are
    /// rebuilt; if a feature that built before fails now, the edit is rolled back and the errors returned.
    pub fn sketch_edit<T>(&mut self, sketch: Id, edit: impl FnOnce(&mut Session) -> Result<T>) -> Result<(T, Option<Rebuild>)> {
        let before = self.p.clone();
        // Include pending dirty nodes and consumers reached through sketch projections.
        let mut planned = before.clone();
        planned.mark_sketch_dirty(sketch);
        let nodes = crate::dependencies::closure(&planned, planned.regen_plan().nodes.into_iter().chain([sketch]).collect());
        let had: HashSet<Id> = before.regen_errors.keys().copied().collect();
        let copies = if before.dependents_of(sketch).is_empty() { HashSet::new() } else { nodes.clone() };
        self.with_rebuild_copies(before, &copies, "sketch edit", |s| {
            let value = edit(s)?;
            if s.p.dependents_of(sketch).is_empty() {
                return Ok((value, None));
            }
            let r = s.rebuild_retrying(Some(&nodes));
            let broken: Vec<String> =
                r.errors.iter().filter(|i| !had.contains(&i.node)).map(|i| format!("{} ({}): {}", i.name, i.node, i.message)).collect();
            if !broken.is_empty() {
                return Err(Error::Rebuild(broken));
            }
            Ok((value, Some(r)))
        })
    }

    /// Solve sketch `si` and return the residual. QymCAD 0.1.1 settles angle-dimension arms in one solve (c899ea7;
    /// the repeated calls of F-040 are gone). Its own "solved" bar is 1e-4 while edits here are refused above 1e-6,
    /// so a solve that stops in between gets one more before a refusal. A wrong branch is checked below.
    pub(crate) fn solve_settled(&mut self, si: usize) -> f64 {
        // Native DistancePL silently takes abs(expr), unlike the origin Distance coordinate pins. Keep
        // the same coordinate-side contract for axis-pinned points, including ones later reused by
        // entities: a magnitude formula cannot turn negative.
        let sketch = &self.p.sketches[si];
        let vars = self.p.param_map();
        if sketch.constraints.iter().any(|c| match c {
            Constraint::DistancePL { p, a, b, expr, driven: false, .. }
                if !expr.trim().is_empty()
                    && !sketch.system_ids().contains(p)
                    && sketch.system_ids().contains(a)
                    && sketch.system_ids().contains(b) =>
            {
                !Num::Expr(expr.clone()).eval(&vars).is_ok_and(|value| value >= 0.0)
            }
            _ => false,
        }) {
            return f64::INFINITY;
        }
        let mut residual = self.p.solve_sketch(si);
        if residual > 1e-6 {
            residual = self.p.solve_sketch(si);
        }
        // Horizontal + PointOnCircle also admit a reference on the -x side. That alternate branch
        // satisfies ArcLength while rotating every direction by 180°, so reject it through the same
        // residual/rollback path as a failed solve.
        let sketch = &self.p.sketches[si];
        if sketch.constraints.iter().any(|constraint| match *constraint {
            Constraint::ArcLength { c, a, .. } if !sketch.entities.iter().any(|e| entity_points(e).contains(&a)) => {
                let center = sketch.points.iter().find(|p| p.id == c);
                let reference = sketch.points.iter().find(|p| p.id == a);
                center.zip(reference).is_some_and(|(c, a)| a.x < c.x)
            }
            _ => false,
        }) {
            return f64::INFINITY;
        }
        residual
    }

    pub(crate) fn finish_sketch_edit(&mut self, si: usize) -> Result<()> {
        self.p.eval_parameters();
        let residual = self.solve_settled(si);
        let sid = self.p.sketches[si].id;
        self.p.mark_sketch_dirty(sid);
        if residual > 1e-6 {
            let conflicts = self.p.sketch_conflicts(si);
            return Err(Error::Invalid(format!(
                "sketch {sid} does not solve (residual {residual:.3e}): conflicting dimensions{}",
                if conflicts.is_empty() { String::new() } else { format!(" (constraints {conflicts:?} disagree)") }
            )));
        }
        Ok(())
    }
}

fn is_datum_guard(key: &str) -> bool {
    ["datum_dist_", "datum_tilt_x_", "datum_tilt_y_", "datum_turn_", "datum_driver_"].iter().any(|prefix| key.starts_with(prefix))
}

pub(crate) fn dist(a: Id, b: Id, axis: u8, d: f64, expr: String) -> Constraint {
    Constraint::Distance { a, b, d, off: 0.0, expr, driven: false, axis, at: None }
}

pub(crate) fn purpose(construction: bool) -> Purpose {
    if construction {
        Purpose::Construction
    } else {
        Purpose::Real
    }
}

fn plane_desc(session: &Session, p: &SketchPlane) -> String {
    match p {
        SketchPlane::World(b) => format!("{b:?}"),
        SketchPlane::Datum(id) => {
            let name = session.node_name(*id);
            if name.is_empty() {
                format!("plane {id}")
            } else {
                format!("{name} (plane {id})")
            }
        }
        SketchPlane::Face(body, k) => format!("face {} of body {body}", k.id),
    }
}

/// Even-odd test of a point against a closed polygon.
fn polygon_contains(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    for (i, a) in poly.iter().enumerate() {
        let b = poly[(i + 1) % poly.len()];
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]) {
            inside = !inside;
        }
    }
    inside
}

/// Absolute area of a closed polygon (shoelace).
fn polygon_area(poly: &[[f64; 2]]) -> f64 {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

/// Distance from a point to the nearest edge of a closed polygon.
fn polygon_edge_distance(poly: &[[f64; 2]], p: [f64; 2]) -> f64 {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
        })
        .fold(f64::INFINITY, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flipped_angle_reference_is_refused_and_rolled_back() {
        let mut s = Session::new_part();
        s.param_set("rot", &Num::Value(30.0)).unwrap();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let added = s
            .sketch_polygon(
                sk,
                &PolygonSpec {
                    cx: Num::Value(20.0),
                    cy: Num::Value(30.0),
                    sides: 3,
                    r: Some(Num::Value(10.0)),
                    angle: Some(Num::Expr("rot".into())),
                    vertex: None,
                    construction: false,
                    dimensioned: true,
                },
            )
            .unwrap();
        let before = s.sketch_detail(sk).unwrap();
        let reference = before.points.iter().find(|p| p.role == Some("angle_reference")).unwrap().id;
        let si = s.sketch_si(sk).unwrap();
        let err = s
            .transaction(|s| {
                // A 180° rotation about C preserves radius, equal sides, Horizontal, PointOnCircle and
                // directed ArcLength when the reference rotates too: this is the alternate satisfied branch.
                for p in &mut s.p.sketches[si].points {
                    if added.points[1..].contains(&p.id) || p.id == reference {
                        p.x = 2.0 * 20.0 - p.x;
                        p.y = 2.0 * 30.0 - p.y;
                    }
                }
                assert!(s.p.sketch_residuals(si).iter().all(|r| r.abs() < 1e-6));
                s.finish_sketch_edit(si)
            })
            .unwrap_err();
        assert!(err.to_string().contains("does not solve"), "{err}");
        assert_eq!(s.sketch_detail(sk).unwrap(), before, "the flipped branch must roll back");
    }
}
