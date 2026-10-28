//! `SketchDetail`: everything `sketch_constrain` needs to refer to — entities, points, constraints — on top of
//! the compact `SketchInfo` summary.

use super::SketchInfo;
use crate::error::Result;
use crate::session::Session;
use qymcad_core::model::{entity_points, Constraint, EntityKind, Id};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PointInfo {
    pub id: Id,
    #[serde(serialize_with = "crate::session::serialize_coordinate")]
    pub x: f64,
    #[serde(serialize_with = "crate::session::serialize_coordinate")]
    pub y: f64,
    /// Frame points: `origin` (0,0), `frame` (the axes' anchor at 0,0), `x_axis` (1,0), `y_axis` (0,1);
    /// `angle_reference`: a construction point on the +x side of a centre, the zero of its direction dimensions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct EntityInfo {
    pub id: Id,
    /// `line`, `arc`, `circle`, `ellipse`, `ellipse_arc` or `spline`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// line: [start, end]; arc: [centre, start, end]; circle: [centre]; ellipse: [centre, major, minor].
    pub points: Vec<Id>,
    /// Ordered interpolation node ids of a spline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nodes: Option<Vec<Id>>,
    /// Whether a spline is closed; its ordered nodes are also listed in `points`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// Radius of a circle or arc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r: Option<f64>,
    /// Arc direction from start to end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ccw: Option<bool>,
    /// Construction geometry (not part of any profile).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub construction: bool,
    /// Native projection record still owns this curve (also true for unknown sources).
    pub driven: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// attached / unknown while associated; detached after a release observed in this Session.
    pub projection_state: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection_body: Option<Id>,
    /// Associated projection reads another part's local coordinates and ignores its placement.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cross_part: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ConstraintInfo {
    /// Position in the sketch's constraint list (`sketch_remove` takes it; later ones shift on removal).
    pub index: usize,
    /// e.g. `distance_x`, `radius`, `angle`, `horizontal`, `point_on_line`, `tangent`.
    pub kind: String,
    /// The point ids it acts on.
    pub points: Vec<Id>,
    /// A dimension's value (mm or degrees; a point-line distance is signed by side).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// A dimension's expression.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
    /// A reference dimension: measures, constrains nothing.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reference: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SketchDetail {
    #[serde(flatten)]
    pub summary: SketchInfo,
    pub entities: Vec<EntityInfo>,
    pub points: Vec<PointInfo>,
    pub constraints: Vec<ConstraintInfo>,
}

impl Session {
    /// The sketch summary plus its entities, points and constraints.
    pub fn sketch_detail(&self, sketch: Id) -> Result<SketchDetail> {
        let summary = self.sketch_info(sketch)?;
        let si = self.sketch_si(sketch)?;
        let s = &self.p.sketches[si];
        let xy = |id: Id| s.points.iter().find(|p| p.id == id).map(|p| (p.x, p.y));
        let projections = &s.projections;
        let detached = &self.detached_projections;
        let entities = s.entities.iter().map(|e| {
            let (kind, r, ccw) = match &e.kind {
                EntityKind::Line { .. } => ("line", None, None),
                EntityKind::Arc { center, a, ccw, .. } => {
                    let r = xy(*center).zip(xy(*a)).map(|(c, p)| (p.0 - c.0).hypot(p.1 - c.1));
                    ("arc", r, Some(*ccw))
                }
                EntityKind::Circle { r, .. } => ("circle", Some(*r), None),
                EntityKind::Ellipse { span, .. } => (if span.is_some() { "ellipse_arc" } else { "ellipse" }, None, span.map(|s| s.winding == qymcad_core::feature::Winding::Ccw)),
                EntityKind::Spline { .. } => ("spline", None, None),
            };
            let projection = projections.iter().find(|p| p.entities.contains(&e.id));
            let detached_projection = detached.get(&(sketch, e.id));
            let source = projection.map(|p| (p.id, p.body, p.src))
                .or_else(|| detached_projection.map(|p| (p.id, p.body, p.source)));
            let source_label = |src| match src {
                qymcad_core::model::ProjSource::Edge(id) => format!("edge:{id}"),
                qymcad_core::model::ProjSource::Face(id) => format!("face:{id}"),
            };
            let state = if let Some(p) = projection {
                let body = crate::dependencies::projection_source(&self.p, sketch, p.body);
                let live = self.shapes.contains_key(&body) && match p.src {
                    qymcad_core::model::ProjSource::Edge(id) => self.p.regen_edges.get(&body).is_some_and(|edges| edges.iter().any(|e| e.id == id)),
                    qymcad_core::model::ProjSource::Face(id) => self.p.regen_faces.get(&body).is_some_and(|faces| faces.iter().any(|f| f.id == id)),
                };
                Some(if live { "attached" } else { "unknown" })
            } else {
                detached_projection.map(|_| "detached")
            };
            let geometry = source.map(|_| match &e.kind {
                EntityKind::Line { a, b } => serde_json::json!({"start":xy(*a),"end":xy(*b)}),
                EntityKind::Arc { center, a, b, ccw } => serde_json::json!({"center":xy(*center),"start":xy(*a),"end":xy(*b),"ccw":ccw}),
                EntityKind::Circle { center, r } => serde_json::json!({"center":xy(*center),"radius":r}),
                EntityKind::Ellipse { c, ma, mi, span } => serde_json::json!({"center":xy(*c),"major":xy(*ma),"minor":xy(*mi),"span":span}),
                EntityKind::Spline { nodes, ends, .. } => serde_json::json!({"nodes":nodes.iter().map(|id| xy(*id)).collect::<Vec<_>>(),"closed":ends == &qymcad_core::feature::Ends::Closed}),
            });
            EntityInfo {
                id: e.id,
                kind,
                points: entity_points(e),
                r,
                ccw,
                construction: e.construction,
                driven: projection.is_some(),
                projection: source.map(|s| s.0),
                projection_state: state,
                projection_body: source.map(|s| s.1),
                cross_part: projection.is_some_and(|p| self.projection_cross_part(sketch, p.body)),
                projection_source: source.map(|s| source_label(s.2)),
                geometry,
                nodes: match &e.kind { EntityKind::Spline { nodes, .. } => Some(nodes.clone()), _ => None },
                closed: match &e.kind { EntityKind::Spline { ends, .. } => Some(*ends == qymcad_core::feature::Ends::Closed), _ => None },
            }
        }).collect();
        let angle_refs: Vec<Id> = s
            .constraints
            .iter()
            .filter_map(|c| match *c {
                Constraint::ArcLength { a, .. } if !s.entities.iter().any(|e| entity_points(e).contains(&a)) => Some(a),
                _ => None,
            })
            .collect();
        let role = |id: Id| match id {
            _ if id == s.origin => Some("origin"),
            _ if id == s.frame => Some("frame"),
            _ if id == s.axis_pts[0] => Some("x_axis"),
            _ if id == s.axis_pts[1] => Some("y_axis"),
            _ if angle_refs.contains(&id) => Some("angle_reference"),
            _ => None,
        };
        let points = s.points.iter().map(|p| PointInfo { id: p.id, x: p.x, y: p.y, role: role(p.id) }).collect();
        let constraints = s
            .constraints
            .iter()
            .enumerate()
            .map(|(index, c)| ConstraintInfo {
                index,
                kind: constraint_kind(c),
                points: c.points(),
                value: c.dim_value(),
                expr: c.dim_expr().filter(|e| !e.trim().is_empty()).map(str::to_string),
                reference: c.is_driven(),
            })
            .collect();
        Ok(SketchDetail { summary, entities, points, constraints })
    }
}

fn constraint_kind(c: &Constraint) -> String {
    match c {
        Constraint::Distance { axis: 1, .. } => "distance_x".into(),
        Constraint::Distance { axis: 2, .. } => "distance_y".into(),
        Constraint::Distance { .. } => "distance".into(),
        Constraint::Diameter { diam: true, .. } => "diameter".into(),
        Constraint::Diameter { .. } => "radius".into(),
        Constraint::DistancePL { .. } => "distance_point_line".into(),
        Constraint::Angle { .. } => "angle_at_vertex".into(),
        Constraint::AngleLines { .. } => "angle".into(),
        Constraint::PointOnCircle { .. } => "point_on_circle".into(),
        Constraint::Fixed { .. } => "fix".into(),
        other => snake(format!("{other:?}").split(|ch: char| !ch.is_alphanumeric()).next().unwrap_or("")),
    }
}

fn snake(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}
