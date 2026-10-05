//! `SketchDetail`: everything `sketch_constrain` needs to refer to — entities, points, constraints — on top of
//! the compact `SketchInfo` summary.

use super::SketchInfo;
use crate::error::Result;
use crate::session::Session;
use qymcad_core::model::{Constraint, EntityKind, Id};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PointInfo {
    pub id: Id,
    pub x: f64,
    pub y: f64,
    /// Frame points: `origin` (0,0), `frame` (the axes' anchor at 0,0), `x_axis` (1,0), `y_axis` (0,1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct EntityInfo {
    pub id: Id,
    /// `line`, `arc`, `circle` or `ellipse`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// line: [start, end]; arc: [centre, start, end]; circle: [centre]; ellipse: [centre, major, minor].
    pub points: Vec<Id>,
    /// Radius of a circle or arc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r: Option<f64>,
    /// Arc direction from start to end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ccw: Option<bool>,
    /// Construction geometry (not part of any profile).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub construction: bool,
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
        let entities = s
            .entities
            .iter()
            .map(|e| {
                let (kind, points, r, ccw) = match e.kind {
                    EntityKind::Line { a, b } => ("line", vec![a, b], None, None),
                    EntityKind::Arc { center, a, b, ccw } => {
                        let r = xy(center).zip(xy(a)).map(|(c, p)| (p.0 - c.0).hypot(p.1 - c.1));
                        ("arc", vec![center, a, b], r, Some(ccw))
                    }
                    EntityKind::Circle { center, r } => ("circle", vec![center], Some(r), None),
                    EntityKind::Ellipse { c, ma, mi } => ("ellipse", vec![c, ma, mi], None, None),
                };
                EntityInfo { id: e.id, kind, points, r, ccw, construction: e.construction }
            })
            .collect();
        let role = |id: Id| match id {
            _ if id == s.origin => Some("origin"),
            _ if id == s.frame => Some("frame"),
            _ if id == s.axis_pts[0] => Some("x_axis"),
            _ if id == s.axis_pts[1] => Some("y_axis"),
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
