//! Datum points: the app's `part.datum-point` command, by coordinates or at a body vertex.

use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::value::Num;
use qymcad_core::model::{FeatNumber, Id, PlainNumber, PointDef};
use serde::Serialize;

/// The app's field range for a datum coordinate (`start.rs`: ±1e7).
pub(crate) const DATUM_COORDINATE_LIMIT: f64 = 1.0e7;

/// Which end of an edge a vertex point stands at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexEnd {
    /// The edge's first end (`EdgeInfo.a`).
    Start,
    /// The edge's last end (`EdgeInfo.b`).
    End,
}

/// How the point is given.
#[derive(Clone, Debug, PartialEq)]
pub enum PointSpec {
    /// Coordinates x, y, z in the part's frame, mm (numbers or expressions kept as formulas).
    At([Num; 3]),
    /// A vertex of a body of the active part: an end of a persistent edge (ids from `topology`). The point follows the vertex.
    Vertex { edge: u32, end: VertexEnd, body: Option<Id> },
}

/// A datum point as it stands after the rebuild.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DatumPointInfo {
    pub id: Id,
    pub name: String,
    /// `manual` or `vertex`.
    pub def: &'static str,
    /// Position in the part's frame, mm.
    pub at: [f64; 3],
    /// Vertex points: the body, edge and end the point follows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertex: Option<VertexInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VertexInfo {
    pub body: Id,
    pub edge: u32,
    pub end: &'static str,
}

impl Session {
    /// Create a datum point (one timeline node, as the app's command does). Atomic like every feature.
    pub fn datum_point(&mut self, spec: &PointSpec, name: Option<&str>) -> Result<(DatumPointInfo, Rebuild)> {
        if let PointSpec::Vertex { .. } = spec {
            self.ensure_topology()?;
        }
        let (id, report) = self.atomic(|s| {
            let id = match spec {
                PointSpec::At(at) => {
                    let vars = s.p.param_map();
                    let mut values = [0.0; 3];
                    for ((key, input), value) in ["x", "y", "z"].into_iter().zip(at).zip(&mut values) {
                        *value = input.eval(&vars)?;
                        check_coordinate(key, *value)?;
                    }
                    let id = s.p.add_point_at(values);
                    for (key, input) in ["x", "y", "z"].into_iter().zip(at) {
                        if let Some(text) = input.expr().filter(|e| e.parse::<f64>().is_err()) {
                            s.p.write_feat_formula(&FeatNumber { node: id, key }, &text, PlainNumber::Stands)
                                .map_err(|e| Error::Expr(format!("datum point {key}: {e:?}")))?;
                        }
                    }
                    id
                }
                PointSpec::Vertex { edge, end, body } => {
                    let body = s.current_part_body(*body, "vertices")?;
                    let found = s.p.regen_edges.get(&body).and_then(|edges| edges.iter().find(|e| e.id == *edge)).cloned();
                    let found = found
                        .ok_or_else(|| Error::NotFound(format!("edge {edge} of body {body} (re-read topology after each feature)")))?;
                    let at = if *end == VertexEnd::End { found.b } else { found.a };
                    s.p.add_point_at_vertex(at, body, *edge, *end == VertexEnd::End)
                }
            };
            s.set_node_name(id, name)?;
            Ok(id)
        })?;
        Ok((self.datum_point_info(id)?, report))
    }

    /// A current body of the active Part: the app offers a datum's vertex, edge or face source only on those bodies
    /// (`viewport_3d.rs` filters `body_owner == ctx`). `what` names the pick in the refusal.
    pub(crate) fn current_part_body(&self, body: Option<Id>, what: &str) -> Result<Id> {
        let body = self.source_body(body)?;
        if self.has_assembly_structure() && self.p.body_owner(body) != self.p.active_component {
            return Err(Error::Invalid(format!(
                "body {body} belongs to another Part; the app offers only the {what} of the current Part's bodies (part_select first)"
            )));
        }
        Ok(body)
    }

    /// One datum point's current state.
    pub(crate) fn datum_point_info(&self, id: Id) -> Result<DatumPointInfo> {
        let p = self.p.datum_points.iter().find(|p| p.id == id).ok_or_else(|| Error::NotFound(format!("datum point {id}")))?;
        let (def, vertex) = match p.def {
            PointDef::Manual => ("manual", None),
            PointDef::AtVertex { body, edge, end } => ("vertex", Some(VertexInfo { body, edge, end: if end { "end" } else { "start" } })),
        };
        Ok(DatumPointInfo { id, name: self.node_name(id), def, at: p.at, vertex })
    }

    /// Refuse an evaluated coordinate outside the app's ±1e7 field (a formula's value is judged at every edit).
    pub(crate) fn check_datum_point_values(&self) -> std::result::Result<(), (Id, Error)> {
        let vars = self.p.param_map();
        for point in self.p.datum_points.iter().filter(|p| p.def == PointDef::Manual) {
            for (i, key) in ["x", "y", "z"].into_iter().enumerate() {
                let checked = (|| {
                    let value = match self.p.feat_dim(point.id, key) {
                        Some(expr) => Num::Expr(expr.into()).eval(&vars)?,
                        None => point.at[i],
                    };
                    check_coordinate(key, value)
                })();
                checked.map_err(|e| (point.id, e))?;
            }
        }
        Ok(())
    }
}

pub(crate) fn check_coordinate(key: &str, value: f64) -> Result<()> {
    if !value.is_finite() || value.abs() > DATUM_COORDINATE_LIMIT {
        return Err(Error::Invalid(format!("datum {key} must be finite within ±{DATUM_COORDINATE_LIMIT} mm, got {value}")));
    }
    Ok(())
}
