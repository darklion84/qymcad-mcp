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
    /// The Part that owns the point (and whose frame `at` is in). Omitted in a document with a single, unplaced Part.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_part: Option<Id>,
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
        self.refuse_datum_at_assembly_root()?;
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
                    let body = s.current_part_body(*body, "vertex", true)?;
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

    /// A current body of the active Part, for the source of a datum. `what` names the pick in the refusal. `app_limit`:
    /// the app itself offers it on the current Part's bodies only (a point's vertex, `viewport_3d.rs`
    /// `VertexOfThePart`); otherwise the app would take any visible body's edge or face (`pick_axis_at`) and the refusal
    /// is this server's, because the datum is stored in the active Part's frame.
    pub(crate) fn current_part_body(&self, body: Option<Id>, what: &str, app_limit: bool) -> Result<Id> {
        let body = self.source_body(body)?;
        if self.has_assembly_structure() && self.p.body_owner(body) != self.p.active_component {
            return Err(Error::Invalid(if app_limit {
                format!("body {body} belongs to another Part; the app takes a {what} pick only on the current Part's bodies (part_select first)")
            } else {
                format!(
                    "body {body} belongs to another Part; this server takes the {what} of the active Part's bodies only, because a datum is stored in its Part's frame (the app itself would offer any visible body); part_select first"
                )
            }));
        }
        Ok(body)
    }

    /// Datum points and axes made while the assembly root is the active context would be owned by the root assembly,
    /// which this server does not cover (the app's assembly bench has its own datum commands): refused.
    pub(crate) fn refuse_datum_at_assembly_root(&self) -> Result<()> {
        if self.p.active_component == Some(self.p.root) {
            return Err(Error::Invalid(
                "the assembly root is the active context; datums of the root assembly are not covered by this server (part_select a Part first)".into(),
            ));
        }
        Ok(())
    }

    /// The Part owning a datum node, reported only where the document has more than one Part or a placement.
    pub(crate) fn datum_owner(&self, id: Id) -> Option<Id> {
        self.p.timeline.iter().find(|n| n.id == id).and_then(|n| n.parent).filter(|_| self.has_assembly_structure())
    }

    /// One datum point's current state.
    pub(crate) fn datum_point_info(&self, id: Id) -> Result<DatumPointInfo> {
        let p = self.p.datum_points.iter().find(|p| p.id == id).ok_or_else(|| Error::NotFound(format!("datum point {id}")))?;
        let (def, vertex) = match p.def {
            PointDef::Manual => ("manual", None),
            PointDef::AtVertex { body, edge, end } => ("vertex", Some(VertexInfo { body, edge, end: if end { "end" } else { "start" } })),
        };
        Ok(DatumPointInfo { id, name: self.node_name(id), def, owner_part: self.datum_owner(id), at: p.at.map(zero_clean), vertex })
    }

    /// Evaluated coordinates outside the app's ±1e7 field (a formula's value is judged at every edit).
    fn datum_point_faults(&self) -> Vec<(Id, Error)> {
        let vars = self.p.param_map();
        let mut faults = Vec::new();
        for point in self.p.datum_points.iter().filter(|p| p.def == PointDef::Manual) {
            for (i, key) in ["x", "y", "z"].into_iter().enumerate() {
                let checked = (|| {
                    let value = match self.p.feat_dim(point.id, key) {
                        Some(expr) => Num::Expr(expr.into()).eval(&vars)?,
                        None => point.at[i],
                    };
                    check_coordinate(key, value)
                })();
                if let Err(e) = checked {
                    faults.push((point.id, e));
                    break;
                }
            }
        }
        faults
    }

    /// The datums that no longer stand where the app would resolve them, in one list: a coordinate outside ±1e7, a
    /// two-point axis whose points coincide or are gone (F-172), and a vertex, edge or face reference whose edge or
    /// face has vanished from a body that is still there (F-176). The geometry references are judged against the
    /// last build, so the check belongs after an edit's rebuild.
    pub(crate) fn datum_faults(&self) -> Vec<(Id, Error)> {
        let mut faults = self.datum_point_faults();
        faults.extend(self.datum_axis_faults());
        faults.extend(self.datum_reference_faults());
        faults
    }

    /// The first datum fault that is new: a datum that was already faulty when the file was opened (reported as a
    /// warning then) does not block unrelated edits.
    pub(crate) fn check_datum_nodes(&self) -> std::result::Result<(), (Id, Error)> {
        match self.datum_faults().into_iter().find(|(id, _)| !self.open_node_faults.contains(id)) {
            Some(fault) => Err(fault),
            None => Ok(()),
        }
    }

    /// A body whose edges and faces are the current build's: in the active prefix and with a shape.
    pub(crate) fn built_body(&self, body: Id) -> bool {
        self.is_active_body(body) && self.shapes.contains_key(&body)
    }

    /// Vertex points whose edge is gone from a body that is still built: natively the point keeps its last position and
    /// nothing turns red (`resolve_point_into`).
    fn datum_reference_faults(&self) -> Vec<(Id, Error)> {
        self.p
            .datum_points
            .iter()
            .filter_map(|p| match p.def {
                PointDef::AtVertex { body, edge, .. } if self.built_body(body) && !self.has_edge(body, edge) => Some((
                    p.id,
                    Error::Invalid(format!(
                        "datum point {} stands on edge {edge} of body {body}, which this edit removes while the body stays; the app would keep the point at its old place without any error (F-176)",
                        p.id
                    )),
                )),
                _ => None,
            })
            .chain(self.axis_reference_faults())
            .collect()
    }

    /// Whether the persistent edge is in the body's current edges.
    pub(crate) fn has_edge(&self, body: Id, edge: u32) -> bool {
        self.p.regen_edges.get(&body).is_none_or(|edges| edges.iter().any(|e| e.id == edge))
    }
}

/// Negative zero reads as noise in a reply.
pub(crate) fn zero_clean(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

pub(crate) fn check_coordinate(key: &str, value: f64) -> Result<()> {
    if !value.is_finite() || value.abs() > DATUM_COORDINATE_LIMIT {
        return Err(Error::Invalid(format!("datum {key} must be finite within ±{DATUM_COORDINATE_LIMIT} mm, got {value}")));
    }
    Ok(())
}
