use crate::error::{Error, Result};
use crate::mesh_import::MAX_TRIANGLES;
use crate::session::{Rebuild, Session};
use crate::value::Num;
use qymcad_core::feature::FeatureKind;
use qymcad_core::model::Id;
use serde::Serialize;

const MAX_EXACT_TRIANGLES: usize = 50_000;
const MIN_TOLERANCE: f64 = 0.1;
const MAX_TOLERANCE: f64 = 1000.0;
const MIN_SHARP: f64 = 5.0;
const MAX_SHARP: f64 = 90.0;
const MAX_SIMPLIFY: f64 = 10.0;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MeshRecogniseMode {
    Exact,
    Polyhedron,
}

#[derive(Clone, Debug, Serialize)]
pub struct MeshRecogniseResult {
    pub feature: Id,
    pub body: Id,
    pub name: String,
    pub mode: MeshRecogniseMode,
    pub source_mesh_body: Id,
    pub tolerance: f64,
    pub sharp: Option<f64>,
    pub simplify: f64,
    pub tolerance_formula: String,
    pub sharp_formula: Option<String>,
    pub simplify_formula: String,
    pub faces: u32,
    pub solids: u32,
    pub shells: u32,
    pub volume: f64,
    #[serde(serialize_with = "crate::session::serialize_coordinates")]
    pub bbox: [f64; 6],
    pub warnings: Vec<crate::session::NodeIssue>,
    pub rebuild: Rebuild,
}

fn stored_num(num: &Num, value: f64) -> Result<String> {
    crate::value::check_expr(num.expr().as_deref().unwrap_or(&value.to_string()))?;
    Ok(num.expr().filter(|expr| expr.parse::<f64>().is_err()).unwrap_or_else(|| value.to_string()))
}

fn range(key: &str, value: f64, min: f64, max: f64) -> Result<()> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(Error::Invalid(format!("{key} must be finite and in {min}..={max}, got {value}")));
    }
    Ok(())
}

fn mesh_triangles(session: &Session, body: Id) -> Result<usize> {
    let index = session.project().mesh_index(body).ok_or_else(|| Error::Invalid(format!("body {body} is not a mesh body")))?;
    Ok(session.project().bodies[index].mesh.tris.len())
}

fn mesh_bbox(session: &Session, body: Id) -> Result<[f64; 6]> {
    let index = session.project().mesh_index(body).ok_or_else(|| Error::Invalid(format!("body {body} is not a mesh body")))?;
    let mesh = &session.project().bodies[index].mesh;
    let mut bbox = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let transform = session.project().body_world_transform(body);
    for vertex in &mesh.verts {
        let point = [
            transform[0] * vertex.x + transform[1] * vertex.y + transform[2] * vertex.z + transform[3],
            transform[4] * vertex.x + transform[5] * vertex.y + transform[6] * vertex.z + transform[7],
            transform[8] * vertex.x + transform[9] * vertex.y + transform[10] * vertex.z + transform[11],
        ];
        for (axis, value) in point.into_iter().enumerate() {
            bbox[axis] = bbox[axis].min(value);
            bbox[axis + 3] = bbox[axis + 3].max(value);
        }
    }
    for value in &mut bbox {
        *value = (*value * 1_000_000.0).round() / 1_000_000.0;
    }
    Ok(bbox)
}

fn validate_values(session: &Session, mode: MeshRecogniseMode, tolerance: &Num, sharp: &Num, simplify: &Num) -> Result<(f64, f64, f64)> {
    let vars = session.project().param_map();
    let t = tolerance.eval(&vars)?;
    let s = sharp.eval(&vars)?;
    let z = simplify.eval(&vars)?;
    range("tolerance", t, MIN_TOLERANCE, MAX_TOLERANCE)?;
    range("sharp", s, MIN_SHARP, MAX_SHARP)?;
    range("simplify", z, 0.0, MAX_SIMPLIFY)?;
    if mode == MeshRecogniseMode::Polyhedron && (tolerance.expr().is_some() || sharp.expr().is_some() || t != 1.0 || s != 30.0) {
        return Err(Error::Invalid("tolerance/sharp are refused for polyhedron mode".into()));
    }
    Ok((t, s, z))
}

impl Session {
    pub fn recognise_mesh(
        &mut self,
        body: Option<Id>,
        mode: MeshRecogniseMode,
        tolerance: &Num,
        sharp: &Num,
        simplify: &Num,
        name: Option<&str>,
    ) -> Result<(MeshRecogniseResult, Rebuild)> {
        let source = self.source_body_mesh(body)?;
        let triangles = mesh_triangles(self, source)?;
        let cap = match mode {
            MeshRecogniseMode::Exact => MAX_EXACT_TRIANGLES,
            MeshRecogniseMode::Polyhedron => MAX_TRIANGLES,
        };
        if triangles > cap {
            return Err(Error::Invalid(format!(
                "{} recognition triangle limit is {cap}, measured {triangles}",
                match mode {
                    MeshRecogniseMode::Exact => "exact",
                    MeshRecogniseMode::Polyhedron => "polyhedron",
                }
            )));
        }
        let (t, s, z) = validate_values(self, mode, tolerance, sharp, simplify)?;
        let tolerance_formula = stored_num(tolerance, t)?;
        let sharp_formula = (mode == MeshRecogniseMode::Exact).then(|| stored_num(sharp, s)).transpose()?;
        let simplify_formula = stored_num(simplify, z)?;
        if let Some(name) = name {
            Self::check_object_name(name)?;
        }
        let result = self.atomic_with_post(
            |session| {
                let feature = session.in_component(session.project().body_owner(source), |p| match mode {
                    MeshRecogniseMode::Exact => p.add_mesh_recognised(source, t, s, z),
                    MeshRecogniseMode::Polyhedron => p.add_mesh_solid(source, z),
                });
                session.p.set_feat_dim(feature, "simplify", simplify_formula.clone());
                if mode == MeshRecogniseMode::Exact {
                    session.p.set_feat_dim(feature, "tol", tolerance_formula.clone());
                    session.p.set_feat_dim(feature, "sharp", sharp_formula.clone().unwrap());
                }
                session.set_node_name(feature, name)?;
                Ok((feature, source))
            },
            |session, (feature, source), rebuild| {
                let shape = session.shapes.get(&feature).ok_or_else(|| Error::Invalid("recognition produced no B-rep shape".into()))?;
                let shells = shape.shell_count();
                let solids = shape.solid_count();
                if !rebuild.errors.is_empty() || solids == 0 {
                    return Err(Error::Rebuild(vec![format!(
                        "recognition result has {shells} shells and {solids} solids; use polyhedron mode or a repaired mesh"
                    )]));
                }
                let faces = shape.face_kinds().map(|k| k.into_iter().sum()).unwrap_or(0);
                let body = feature;
                let warnings = rebuild.warnings.clone();
                let bbox = mesh_bbox(session, source)?;
                let output = MeshRecogniseResult {
                    feature,
                    body,
                    name: session.body_name(body),
                    mode,
                    source_mesh_body: source,
                    tolerance: t,
                    sharp: (mode == MeshRecogniseMode::Exact).then_some(s),
                    simplify: z,
                    tolerance_formula: tolerance_formula.clone(),
                    sharp_formula: sharp_formula.clone(),
                    simplify_formula: simplify_formula.clone(),
                    faces,
                    solids,
                    shells,
                    volume: shape.volume(),
                    bbox,
                    warnings,
                    rebuild: rebuild.clone(),
                };
                Ok(output)
            },
        )?;
        Ok(result)
    }

    pub(crate) fn source_body_mesh(&self, body: Option<Id>) -> Result<Id> {
        let b = self.source_body_or_mesh(body)?;
        if !self.project().mesh_born(b) {
            return Err(Error::Invalid(format!("body {b} is already a B-rep")));
        }
        Ok(b)
    }

    pub(crate) fn check_mesh_recognise_values(&self) -> std::result::Result<(), (Id, Error)> {
        for node in &self.p.timeline {
            let (mode, source, tolerance, sharp, simplify) = match &node.kind {
                FeatureKind::MeshSolid { src, simplify, .. } => (
                    MeshRecogniseMode::Polyhedron,
                    *src,
                    Num::Value(1.0),
                    Num::Value(30.0),
                    Num::Expr(self.p.feat_dim(node.id, "simplify").unwrap_or(&simplify.to_string()).into()),
                ),
                FeatureKind::MeshRecognised { src, tol, sharp, simplify, .. } => (
                    MeshRecogniseMode::Exact,
                    *src,
                    Num::Expr(self.p.feat_dim(node.id, "tol").unwrap_or(&tol.to_string()).into()),
                    Num::Expr(self.p.feat_dim(node.id, "sharp").unwrap_or(&sharp.to_string()).into()),
                    Num::Expr(self.p.feat_dim(node.id, "simplify").unwrap_or(&simplify.to_string()).into()),
                ),
                _ => continue,
            };
            let cap = if mode == MeshRecogniseMode::Exact { MAX_EXACT_TRIANGLES } else { MAX_TRIANGLES };
            let triangles = self.project().mesh_index(source).map(|i| self.project().bodies[i].mesh.tris.len()).unwrap_or(0);
            if triangles > cap {
                return Err((
                    node.id,
                    Error::Invalid(format!(
                        "{} recognition triangle limit is {cap}, measured {triangles}",
                        if mode == MeshRecogniseMode::Exact { "exact" } else { "polyhedron" }
                    )),
                ));
            }
            if let Err(e) = validate_values(self, mode, &tolerance, &sharp, &simplify) {
                return Err((node.id, e));
            }
        }
        Ok(())
    }
}
