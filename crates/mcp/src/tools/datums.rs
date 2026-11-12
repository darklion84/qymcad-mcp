//! Datum points and axes: the app's `part.datum-point` and `part.datum-axis` commands.

use super::common::{err, rebuild_json, ObjRef};
use super::{tool, Tool};
use qymcad_engine::{AxisSpec, Num, PointSpec, VertexEnd};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DatumPointArgs {
    /// Coordinates [x, y, z] in the active Part's frame, mm: numbers or expressions (kept as formulas on x, y and z,
    /// editable with dimension_set), each finite within ±1e7 including later evaluated edits. Give this or `vertex`.
    #[serde(default)]
    pub at: Option<[Num; 3]>,
    /// A vertex of a body of the active Part. The point follows the vertex when the body is rebuilt. Give this or `at`.
    #[serde(default)]
    pub vertex: Option<VertexArgs>,
    /// Name for the point (timeline node and the app's tree row), usable instead of its id later.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VertexArgs {
    /// Persistent edge id from `topology` of the body's current version.
    pub edge: u32,
    /// Which end of the edge: "start" (EdgeInfo.a) or "end" (EdgeInfo.b).
    pub end: VertexEndArg,
    /// Body id or name; default: the current body of the active Part. Must be a current body of the active Part.
    #[serde(default)]
    pub body: Option<ObjRef>,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VertexEndArg {
    Start,
    End,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DatumAxisArgs {
    /// A straight edge of a current body of the active Part (not a seam, not smooth): the axis follows the edge.
    #[serde(default)]
    pub edge: Option<EdgeArgs>,
    /// A cylindrical or conical face of a current body of the active Part: the axis follows the face.
    #[serde(default)]
    pub face: Option<FaceArgs>,
    /// Two different datum points (ids or names of datum_point nodes): origin at the first, direction towards the second; the axis follows them.
    #[serde(default)]
    pub points: Option<[ObjRef; 2]>,
    /// By hand, with `dir`: a point on the axis [x, y, z], mm, within ±1e7. NUMBERS ONLY: the app ignores a formula typed into a manual axis (F-170).
    #[serde(default)]
    pub origin: Option<[f64; 3]>,
    /// By hand, with `origin`: the direction [x, y, z], not zero (normalised by the app).
    #[serde(default)]
    pub dir: Option<[f64; 3]>,
    /// Name for the axis (timeline node and the app's tree row), usable instead of its id later.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdgeArgs {
    /// Persistent edge id from `topology` of the body's current version.
    pub edge: u32,
    /// Body id or name; default: the current body of the active Part.
    #[serde(default)]
    pub body: Option<ObjRef>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FaceArgs {
    /// Persistent face id from `topology` of the body's current version.
    pub face: u32,
    /// Body id or name; default: the current body of the active Part.
    #[serde(default)]
    pub body: Option<ObjRef>,
}

pub fn tools() -> Vec<Tool> {
    vec![tool(
        "datum_point",
        "Create a datum point, as the app's Datum point command does: by coordinates `at` [x, y, z] (numbers or formulas; formulas stay editable and follow parameters) or at a body vertex `vertex` {edge, end, body?} (the end of a persistent edge id from topology of a current body of the active Part; the point follows the vertex). Exactly one of the two. Coordinates are in the Part's frame and limited to ±1e7 mm, also after later edits. A point is used by datum_axis `points` (an axis through two points that follows them); the app does not use a point anywhere else (no sketch, plane or joint takes one). Deleting or suppressing a body that a vertex point stands on is refused without cascade; feature_delete of the point is refused while an axis stands on it. Returns point (its id), name, def (manual|vertex), at (after the rebuild), vertex {body, edge, end} for vertex points, and rebuild. Edit x, y, z with dimension_set (feature: point id, key x|y|z); a vertex point has no editable coordinates.",
        |st, a: DatumPointArgs| {
            let s = st.doc()?;
            let spec = match (a.at, a.vertex) {
                (Some(at), None) => PointSpec::At(at),
                (None, Some(v)) => PointSpec::Vertex {
                    edge: v.edge,
                    end: match v.end {
                        VertexEndArg::Start => VertexEnd::Start,
                        VertexEndArg::End => VertexEnd::End,
                    },
                    body: v.body.as_ref().map(|b| b.resolve(s)).transpose()?,
                },
                _ => return Err("give exactly one of `at` or `vertex`".into()),
            };
            let (info, r) = s.datum_point(&spec, a.name.as_deref()).map_err(err)?;
            let mut out = json!(info);
            out["point"] = json!(info.id);
            out["rebuild"] = rebuild_json(&r);
            Ok(out)
        },
    ),
        tool(
            "datum_axis",
            "Create a datum axis, as the app's Datum axis command does. Exactly one of: `edge` {edge, body?} (a straight, non-seam, non-smooth edge of a current body of the active Part; the axis follows the edge), `face` {face, body?} (a cylindrical or conical face; the axis follows it), `points` [a, b] (two different datum points from datum_point, direction a to b; the axis follows them, and an edit that makes them coincide is refused), or `origin` + `dir` (by hand, numbers only: the app ignores formulas in a manual axis, F-170; direction not zero). An axis made by two clicks on body vertices is not offered: make two datum points at the vertices instead. Use the axis as {\"datum\": id} in revolve and circular_array (the axis must belong to the active Part). Deleting an axis that a feature uses is refused without cascade (the app would silently turn the feature about another axis); a datum point under an axis is refused likewise. Returns axis (its id), name, def (manual|two_points|edge|face), origin and dir after the rebuild (a point on the axis and its unit direction in the Part's frame), source and rebuild. An axis made here is never reused by revolve or an array that gives `origin` + `dir`.",
            |st, a: DatumAxisArgs| {
                let s = st.doc()?;
                let body = |b: &Option<ObjRef>| b.as_ref().map(|b| b.resolve(s)).transpose();
                let spec = match (a.edge, a.face, a.points, a.origin, a.dir) {
                    (Some(e), None, None, None, None) => AxisSpec::Edge { edge: e.edge, body: body(&e.body)? },
                    (None, Some(f), None, None, None) => AxisSpec::Face { face: f.face, body: body(&f.body)? },
                    (None, None, Some([p, q]), None, None) => AxisSpec::Points([p.resolve(s)?, q.resolve(s)?]),
                    (None, None, None, Some(origin), Some(dir)) => AxisSpec::Manual { origin, dir },
                    _ => return Err("give exactly one of `edge`, `face`, `points`, or `origin` together with `dir`".into()),
                };
                let (info, r) = s.datum_axis(&spec, a.name.as_deref()).map_err(err)?;
                let mut out = json!(info);
                out["axis"] = json!(info.id);
                out["rebuild"] = rebuild_json(&r);
                Ok(out)
            },
        ),
    ]
}
