//! Datum axes: the app's `part.datum-axis` command, from an edge, a round face, two datum points or by hand.

use crate::datum_point::{check_coordinate, zero_clean};
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
    /// The Part that owns the axis (and whose frame it is in). Omitted in a document with a single, unplaced Part.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_part: Option<Id>,
    /// A point on the axis, mm, in the Part's frame.
    pub origin: [f64; 3],
    /// Unit direction (a manual axis is normalised here; the app keeps what was typed).
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
        self.refuse_datum_at_assembly_root()?;
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
                        let at = s.p.datum_points.iter().find(|p| p.id == *id).map(|p| p.at).ok_or_else(|| {
                            Error::NotFound(format!("datum point {id} (make one with datum_point; vertices are not points)"))
                        })?;
                        // A point of another Part is in that Part's frame, as an axis of another Part is for a feature.
                        let owner = s.p.timeline.iter().find(|n| n.id == *id).and_then(|n| n.parent);
                        if s.has_assembly_structure() && owner != s.p.active_component {
                            return Err(Error::Invalid(format!(
                                "datum point {id} belongs to another Part; the axis would mix two Part frames (part_select first)"
                            )));
                        }
                        Ok(at)
                    };
                    let (pa, pb) = (at(a)?, at(b)?);
                    if (0..3).map(|i| (pb[i] - pa[i]).abs()).sum::<f64>() < 1e-9 {
                        return Err(Error::Invalid(format!("points {a} and {b} coincide; an axis needs a direction")));
                    }
                    s.p.add_axis_two_points(*a, *b)
                }
                AxisSpec::Edge { edge, body } => {
                    let body = s.current_part_body(*body, "edges", false)?;
                    s.check_straight_edge(body, *edge)?;
                    s.p.add_axis_from_edge(body, *edge)
                }
                AxisSpec::Face { face, body } => {
                    let body = s.current_part_body(*body, "faces", false)?;
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
        let length = a.dir().iter().map(|v| v * v).sum::<f64>().sqrt();
        let unit = if length > 0.0 { a.dir().map(|v| zero_clean(v / length)) } else { a.dir() };
        Ok(DatumAxisInfo {
            id,
            name: self.node_name(id),
            def,
            owner_part: self.datum_owner(id),
            origin: a.origin().map(zero_clean),
            dir: unit,
            source,
        })
    }

    /// An axis through two points that have come to coincide, or whose point is gone, keeps its last line silently when
    /// the app resolves it (F-172): reported here, and refused on every edit, so a parameter cannot collapse it.
    pub(crate) fn datum_axis_faults(&self) -> Vec<(Id, Error)> {
        let mut faults = Vec::new();
        for axis in &self.p.datum_axes {
            let AxisDef::TwoPoints { a, b } = axis.def else { continue };
            let at = |id: Id| self.p.datum_points.iter().find(|p| p.id == id).map(|p| p.at);
            let (Some(pa), Some(pb)) = (at(a), at(b)) else {
                faults.push((axis.id, Error::Invalid(format!("axis {} stands on a datum point that no longer exists", axis.id))));
                continue;
            };
            if (0..3).map(|i| (pb[i] - pa[i]).abs()).sum::<f64>() < 1e-9 {
                faults.push((
                    axis.id,
                    Error::Invalid(format!("axis {} would run through two coincident points {a} and {b} (F-172)", axis.id)),
                ));
            }
        }
        faults
    }

    /// Edge and face axes whose edge or face has vanished from a body that is still built (F-176): the app keeps the
    /// axis at its last line.
    pub(crate) fn axis_reference_faults(&self) -> Vec<(Id, Error)> {
        self.p
            .datum_axes
            .iter()
            .filter_map(|axis| {
                let (body, what) = match axis.def {
                    AxisDef::FromEdge { body, edge } if self.built_body(body) && !self.has_edge(body, edge) => (body, format!("edge {edge}")),
                    AxisDef::FromFace { body, face } if self.built_body(body) && !self.has_round_face(body, face) => {
                        (body, format!("face {face} (or its cylinder/cone axis)"))
                    }
                    _ => return None,
                };
                Some((
                    axis.id,
                    Error::Invalid(format!(
                        "datum axis {} follows {what} of body {body}, which this edit removes while the body stays; the app would keep the axis at its old line without any error (F-176)",
                        axis.id
                    )),
                ))
            })
            .collect()
    }

    /// Whether the face is still a cylinder or cone of the body's shape.
    fn has_round_face(&self, body: Id, face: u32) -> bool {
        self.shapes.get(&body).is_some_and(|sh| {
            let _g = qymcad_kernel::kernel_gate();
            sh.face_axis(face).is_some()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    /// A datum that was faulty at open and has become healthy loses its exemption with the next successful edit.
    #[test]
    fn a_healed_datum_leaves_the_open_fault_set() {
        let mut s = Session::new_part();
        let (axis, _) = s.datum_axis(&AxisSpec::Manual { origin: [0.0; 3], dir: [0.0, 0.0, 1.0] }, None).unwrap();
        s.open_node_faults.insert(axis.id);
        s.open_node_faults.insert(9999);
        s.param_set("unrelated", &1.0.into()).unwrap();
        assert!(s.open_node_faults.is_empty(), "{:?}", s.open_node_faults);
    }
}
