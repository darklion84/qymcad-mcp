//! Datum axes: the app's `part.datum-axis` command, from an edge, a round face, two datum points or by hand.

use crate::datum_point::check_coordinate;
use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::topology::EdgeKind;
use qymcad_core::model::{AxisDef, Id};
use serde::Serialize;

/// How the axis is given.
#[derive(Clone, Debug, PartialEq)]
pub enum AxisSpec {
    /// Along a straight, non-seam, non-smooth edge of a current body of the active Part (persistent id from `topology`).
    Edge { edge: u32, body: Option<Id> },
    /// Along the axis of a cylindrical or conical face of a current body of the active Part.
    Face { face: u32, body: Option<Id> },
    /// Through two distinct datum points (ids of `datum_point` nodes); follows them.
    Points([Id; 2]),
    /// Origin and direction by hand, in the Part's frame. Numbers only: the app ignores formulas typed into a manual axis (F-170).
    Manual { origin: [f64; 3], dir: [f64; 3] },
}

/// A datum axis as it stands after the rebuild.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DatumAxisInfo {
    pub id: Id,
    pub name: String,
    /// `manual`, `two_points`, `edge` or `face`.
    pub def: &'static str,
    /// A point on the axis, mm, in the Part's frame.
    pub origin: [f64; 3],
    /// Unit direction.
    pub dir: [f64; 3],
    /// What the axis follows: `{body, edge}`, `{body, face}` or `{points: [a, b]}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<serde_json::Value>,
}

/// The marker of an axis made on purpose by `datum_axis`: a revolve or array given `origin` + `dir` never reuses it.
pub(crate) const EXPLICIT_AXIS_KEY: &str = "__explicit";

impl Session {
    /// Create a datum axis (one timeline node). Atomic like every feature.
    pub fn datum_axis(&mut self, spec: &AxisSpec, name: Option<&str>) -> Result<(DatumAxisInfo, Rebuild)> {
        if matches!(spec, AxisSpec::Edge { .. } | AxisSpec::Face { .. }) {
            self.ensure_topology()?;
        }
        let (id, report) = self.atomic(|s| {
            let id = match spec {
                AxisSpec::Manual { origin, dir } => {
                    for (key, v) in ["ox", "oy", "oz"].into_iter().zip(origin).chain(["dx", "dy", "dz"].into_iter().zip(dir)) {
                        check_coordinate(key, *v)?;
                    }
                    if dir.iter().map(|v| v.abs()).sum::<f64>() < 1e-9 {
                        return Err(Error::Invalid("axis direction has no length".into()));
                    }
                    s.p.add_axis_manual(*origin, *dir)
                }
                AxisSpec::Points([a, b]) => {
                    if a == b {
                        return Err(Error::Invalid(format!("an axis needs two different points, got point {a} twice")));
                    }
                    let at = |id: &Id| {
                        s.p.datum_points.iter().find(|p| p.id == *id).map(|p| p.at).ok_or_else(|| {
                            Error::NotFound(format!("datum point {id} (make one with datum_point; vertices are not points)"))
                        })
                    };
                    let (pa, pb) = (at(a)?, at(b)?);
                    if (0..3).map(|i| (pb[i] - pa[i]).abs()).sum::<f64>() < 1e-9 {
                        return Err(Error::Invalid(format!("points {a} and {b} coincide; an axis needs a direction")));
                    }
                    s.p.add_axis_two_points(*a, *b)
                }
                AxisSpec::Edge { edge, body } => {
                    let body = s.current_part_body(*body, "edges")?;
                    s.check_straight_edge(body, *edge)?;
                    s.p.add_axis_from_edge(body, *edge)
                }
                AxisSpec::Face { face, body } => {
                    let body = s.current_part_body(*body, "faces")?;
                    if !s.p.regen_faces.get(&body).is_some_and(|fs| fs.iter().any(|f| f.id == *face)) {
                        return Err(Error::NotFound(format!("face {face} of body {body} (re-read topology after each feature)")));
                    }
                    let round = s.shapes.get(&body).is_some_and(|sh| {
                        let _g = qymcad_kernel::kernel_gate();
                        sh.face_axis(*face).is_some()
                    });
                    if !round {
                        return Err(Error::Invalid(format!("face {face} of body {body} is not a cylinder or cone")));
                    }
                    s.p.add_axis_from_face(body, *face)
                }
            };
            s.p.set_feat_dim(id, EXPLICIT_AXIS_KEY, "1".into());
            s.set_node_name(id, name)?;
            Ok(id)
        })?;
        Ok((self.datum_axis_info(id)?, report))
    }

    /// The app offers an edge as an axis only when it is straight and not smooth: the seam of a cylinder and the tangent
    /// seams of fillets are not offered (`refresh_axis_edges`), though the core would take them.
    fn check_straight_edge(&mut self, body: Id, edge: u32) -> Result<()> {
        let topo = self.topology(Some(body), false)?;
        let info = topo
            .edges
            .iter()
            .find(|e| e.id == edge)
            .ok_or_else(|| Error::NotFound(format!("edge {edge} of body {body} (re-read topology after each feature)")))?;
        if info.kind != EdgeKind::Line {
            return Err(Error::Invalid(format!("edge {edge} is not a straight line; use `face` for the axis of a round face")));
        }
        if info.seam {
            return Err(Error::Invalid(format!(
                "edge {edge} is the seam of a round face; the app does not offer it as an axis, use `face`"
            )));
        }
        let smooth = self.shapes.get(&body).is_some_and(|sh| {
            let _g = qymcad_kernel::kernel_gate();
            let (_, ids, _, smooth) = sh.edges_full_smooth();
            ids.iter().zip(smooth).any(|(id, smooth)| *id == edge && smooth)
        });
        if smooth {
            return Err(Error::Invalid(format!(
                "edge {edge} is smooth (tangent faces on both sides); the app does not offer it as an axis"
            )));
        }
        Ok(())
    }

    /// One datum axis's current state (origin and direction as resolved by the last rebuild).
    pub(crate) fn datum_axis_info(&self, id: Id) -> Result<DatumAxisInfo> {
        let a = self.p.datum_axes.iter().find(|a| a.id == id).ok_or_else(|| Error::NotFound(format!("datum axis {id}")))?;
        let (def, source) = match a.def {
            AxisDef::Manual { .. } => ("manual", None),
            AxisDef::TwoPoints { a, b } => ("two_points", Some(serde_json::json!({"points": [a, b]}))),
            AxisDef::FromEdge { body, edge } => ("edge", Some(serde_json::json!({"body": body, "edge": edge}))),
            AxisDef::FromFace { body, face } => ("face", Some(serde_json::json!({"body": body, "face": face}))),
        };
        Ok(DatumAxisInfo { id, name: self.node_name(id), def, origin: a.origin(), dir: a.dir(), source })
    }

    /// An axis through two points that have come to coincide, or whose point is gone, keeps its last line silently when
    /// the app resolves it (F-172): refused on every edit, so a parameter cannot collapse it.
    pub(crate) fn check_datum_axis_values(&self) -> std::result::Result<(), (Id, Error)> {
        for axis in &self.p.datum_axes {
            let AxisDef::TwoPoints { a, b } = axis.def else { continue };
            let at = |id: Id| self.p.datum_points.iter().find(|p| p.id == id).map(|p| p.at);
            let (Some(pa), Some(pb)) = (at(a), at(b)) else {
                return Err((axis.id, Error::Invalid(format!("axis {} stands on a datum point that no longer exists", axis.id))));
            };
            if (0..3).map(|i| (pb[i] - pa[i]).abs()).sum::<f64>() < 1e-9 {
                return Err((
                    axis.id,
                    Error::Invalid(format!("axis {} would run through two coincident points {a} and {b} (F-172)", axis.id)),
                ));
            }
        }
        Ok(())
    }
}
