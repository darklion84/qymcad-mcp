//! Native interpolation splines with formula-carrying node dimensions.
use super::{purpose, Added, Xy};
use crate::{Error, Id, Result, Session};
use qymcad_core::feature::Ends;
use qymcad_core::geom::Point2;
use qymcad_core::model::EntityKind;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn yes() -> bool {
    true
}

/// An interpolating spline through ordered sketch points, with native automatic tangent handles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplineSpec {
    /// Interpolation nodes `[[x,y],…]`: at least two, or three for a closed spline. Do not repeat the first node.
    pub points: Vec<Xy>,
    /// Join the last node to the first with a smooth periodic curve (default false).
    #[serde(default)]
    pub closed: bool,
    /// Construction geometry, excluded from profiles (default false).
    #[serde(default)]
    pub construction: bool,
    /// Add independent x/y driving dimensions for the nodes (default true).
    #[serde(default = "yes")]
    pub dimensioned: bool,
}

impl Session {
    /// Add one native spline, retaining each node's coordinate formulas; refusal restores the sketch.
    pub fn sketch_spline(&mut self, sketch: Id, spec: &SplineSpec) -> Result<Added> {
        self.transaction(|s| {
            let si = s.sketch_si(sketch)?;
            let vars = s.p.param_map();
            let pts = spec.points.iter().map(|Xy(x, y)| Ok(Point2::new(x.eval(&vars)?, y.eval(&vars)?))).collect::<Result<Vec<_>>>()?;
            let need = if spec.closed { 3 } else { 2 };
            if pts.len() < need {
                return Err(Error::Invalid(format!(
                    "a {} spline needs at least {need} points, got {}",
                    if spec.closed { "closed" } else { "open" },
                    pts.len()
                )));
            }
            for (i, p) in pts.iter().enumerate() {
                if let Some(j) = pts[..i].iter().position(|q| p.dist(*q) <= 1e-6) {
                    return Err(Error::Invalid(format!("spline nodes {j} and {i} coincide (must be more than 1e-6 mm apart)")));
                }
            }
            let eid =
                s.p.add_spline(si, pts.clone(), if spec.closed { Ends::Closed } else { Ends::Open }, purpose(spec.construction))
                    .ok_or_else(|| Error::Invalid("native spline constructor refused these nodes".into()))?;
            let nodes = match &s.p.sketches[si]
                .entities
                .iter()
                .find(|e| e.id == eid)
                .ok_or_else(|| Error::Invalid("spline was not created".into()))?
                .kind
            {
                EntityKind::Spline { nodes, .. } => nodes.clone(),
                _ => return Err(Error::Invalid("native spline constructor returned another kind".into())),
            };
            if spec.dimensioned {
                for ((&id, pt), Xy(x, y)) in nodes.iter().zip(&pts).zip(&spec.points) {
                    let dims = s.pin_dims(si, id, x, pt.x, y, pt.y)?;
                    s.add_independent(si, dims);
                }
            }
            s.finish_sketch_edit(si)?;
            Ok(Added { entities: vec![eid], points: nodes })
        })
    }
}
