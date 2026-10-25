//! Strict selectors for native sketch curves and their owned points.
use crate::error::{Error, Result};
use crate::{Id, Num, Session};
use qymcad_core::feature::Ends;
use qymcad_core::geom::Point2;
use qymcad_core::model::{curve::Curve, entity_centre, entity_ends, EntityKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A radius-driven native circle touching two uniquely selected curves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TangentCircleSpec {
    /// Two different curves in the sketch.
    pub curves: [EntityRef; 2],
    /// Positive driving radius in mm, or its formula.
    pub radius: Num,
    /// One-time centre-selection hint in sketch coordinates; tied solutions are refused.
    pub near: [Num; 2],
    /// Construction geometry: excluded from profiles.
    #[serde(default)]
    pub construction: bool,
}

/// Native sketch curves selected by ids or a geometric description.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SketchCurveSel {
    /// One native curve id.
    Id(Id),
    /// A nonempty set of native curve ids; duplicates are removed.
    Ids(Vec<Id>),
    /// Kind/construction filters, optionally choosing the unique nearest curve.
    Query(CurveQuery),
}

/// A curve selection used at a site that requires exactly one curve.
pub type EntityRef = SketchCurveSel;

/// The native kind of a selected sketch curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SketchCurveKind {
    /// Straight segment.
    Line,
    /// Circular arc.
    Arc,
    /// Whole circle.
    Circle,
    /// Whole ellipse.
    Ellipse,
    /// Directed elliptical arc.
    EllipseArc,
    /// Interpolating spline.
    Spline,
}

/// A geometric description of curves in one sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CurveQuery {
    /// Native curve kind. A kind or nearest point is required.
    #[serde(default)]
    pub kind: Option<SketchCurveKind>,
    /// Limit to construction or profile geometry.
    #[serde(default)]
    pub construction: Option<bool>,
    /// A point in sketch coordinates; chooses the unique nearest analytic curve.
    #[serde(default)]
    pub nearest: Option<[Num; 2]>,
    /// Positive maximum distance in mm; required exactly when nearest is supplied.
    #[serde(default)]
    pub within: Option<Num>,
}

/// A native sketch point or an owned role on exactly one curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PointSel {
    /// A point id belonging to the sketch.
    Id(Id),
    /// The sketch origin, when materialized.
    Origin(OriginRef),
    /// An owned point on a uniquely selected curve.
    Role(PointRoleSel),
}

/// The named sketch origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OriginRef {
    /// Sketch coordinate (0, 0).
    Origin,
}

/// Which owned point of a curve is requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PointRole {
    /// The start of an open curve.
    Start,
    /// The end of an open curve.
    End,
    /// Circle, arc or ellipse centre.
    Centre,
    /// Ellipse major semi-axis endpoint.
    Major,
    /// Ellipse minor semi-axis endpoint.
    Minor,
    /// A spline interpolation node; requires node_index.
    Node,
}

/// One curve and an available owned point role.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PointRoleSel {
    /// Select exactly one native curve.
    pub curve: EntityRef,
    /// Point role available on that native kind.
    pub role: PointRole,
    /// Zero-based spline node index; required for node and forbidden otherwise.
    #[serde(default)]
    pub node_index: Option<usize>,
}

impl Session {
    /// Add the native touching-circle recipe and its driving radius, refusing missing or tied solutions.
    pub fn sketch_circle_tangent(&mut self, sketch: Id, spec: &TangentCircleSpec) -> Result<super::Added> {
        use qymcad_core::model::{curve::centres_touching, Constraint, Touching};
        let si = self.sketch_si(sketch)?;
        // Resolve and evaluate everything before the native constructor allocates ids or solves.
        let curves = [self.sketch_curve_id(sketch, &spec.curves[0])?, self.sketch_curve_id(sketch, &spec.curves[1])?];
        if curves[0] == curves[1] {
            return Err(Error::Invalid("a tangent circle needs two different curves".into()));
        }
        let vars = self.p.param_map();
        let r = spec.radius.eval(&vars)?;
        if r <= 1e-9 {
            return Err(Error::Invalid("a tangent circle radius must be greater than 1e-9 mm".into()));
        }
        let near = Point2::new(spec.near[0].eval(&vars)?, spec.near[1].eval(&vars)?);
        let s = &self.p.sketches[si];
        let native = curves.map(|id| s.entity(id).and_then(|e| Curve::of(s, e)));
        let [Some(a), Some(b)] = native else {
            return Err(Error::Invalid("cannot read the selected native curves".into()));
        };
        let mut solutions = centres_touching([&a, &b], r);
        solutions.retain(|p| p.x.is_finite() && p.y.is_finite());
        solutions.sort_by(|a, b| a.dist(near).total_cmp(&b.dist(near)));
        let Some(best) = solutions.first() else {
            return Err(Error::Invalid("no tangent circle of this radius touches both curves".into()));
        };
        // Different offset pairs can return the same geometric solution. Only distinct centres tie.
        let tolerance = 1e-7 * (1.0 + r + best.dist(near));
        if solutions.iter().skip(1).any(|p| p.dist(*best) > tolerance && (p.dist(near) - best.dist(near)).abs() <= tolerance) {
            return Err(Error::Invalid("tangent circle solution is ambiguous; move `near` towards the desired centre".into()));
        }
        self.transaction(|session| {
            let before = session.p.sketch_dof(si);
            let id = session
                .p
                .add_circle_touching(si, Touching { curves, r, near }, super::purpose(spec.construction))
                .ok_or_else(|| Error::Invalid("no tangent circle of this radius touches both curves".into()))?;
            let centre = session.p.sketches[si]
                .entity(id)
                .and_then(|e| e.kind.as_circle())
                .map(|c| c.center)
                .ok_or_else(|| Error::Invalid("native touching-circle construction did not return a circle".into()))?;
            session.p.sketches[si].constraints.push(Constraint::Diameter {
                c: centre,
                d: r,
                off: 0.0,
                expr: spec.radius.expr().unwrap_or_default(),
                driven: false,
                diam: false,
                at: None,
            });
            session.finish_sketch_edit(si)?;
            let after = session.p.sketch_dof(si);
            if after.1 > before.1 || after.0 > before.0 {
                return Err(Error::Invalid(format!("the tangent circle is not independently defined: sketch DOF {after:?}")));
            }
            Ok(super::Added { entities: vec![id], points: vec![centre] })
        })
    }

    /// Resolve a curve selector without modifying the document; ambiguous nearest choices refuse.
    pub fn sketch_curve_ids(&self, sketch: Id, sel: &SketchCurveSel) -> Result<Vec<Id>> {
        let si = self.sketch_si(sketch)?;
        let s = &self.p.sketches[si];
        let ids = match sel {
            SketchCurveSel::Id(id) => vec![*id],
            SketchCurveSel::Ids(ids) => ids.clone(),
            SketchCurveSel::Query(q) => {
                if q.kind.is_none() && q.nearest.is_none() {
                    return Err(Error::Invalid(
                        "a curve description needs `kind` or `nearest`; construction alone is not a selector".into(),
                    ));
                }
                if q.nearest.is_some() != q.within.is_some() {
                    return Err(Error::Invalid("`nearest` requires positive `within`, and `within` requires `nearest`".into()));
                }
                let mut candidates: Vec<_> = s
                    .entities
                    .iter()
                    .filter(|e| {
                        let kind = match e.kind {
                            EntityKind::Line { .. } => SketchCurveKind::Line,
                            EntityKind::Arc { .. } => SketchCurveKind::Arc,
                            EntityKind::Circle { .. } => SketchCurveKind::Circle,
                            EntityKind::Ellipse { span: None, .. } => SketchCurveKind::Ellipse,
                            EntityKind::Ellipse { span: Some(_), .. } => SketchCurveKind::EllipseArc,
                            EntityKind::Spline { .. } => SketchCurveKind::Spline,
                        };
                        q.kind.is_none_or(|k| k == kind) && q.construction.is_none_or(|c| c == e.construction)
                    })
                    .map(|e| e.id)
                    .collect();
                if let (Some(near), Some(within)) = (&q.nearest, &q.within) {
                    let vars = self.p.param_map();
                    let p = Point2::new(near[0].eval(&vars)?, near[1].eval(&vars)?);
                    let max = within.eval(&vars)?;
                    if max <= 0.0 {
                        return Err(Error::Invalid("`within` must be positive".into()));
                    }
                    let mut ranked: Vec<_> = candidates
                        .iter()
                        .filter_map(|id| {
                            let e = s.entities.iter().find(|e| e.id == *id)?;
                            let d = Curve::of(s, e)?.nearest(p).off;
                            (d <= max).then_some((*id, d))
                        })
                        .collect();
                    ranked.sort_by(|a, b| a.1.total_cmp(&b.1));
                    if ranked.get(1).is_some_and(|b| (b.1 - ranked[0].1).abs() <= 1e-9 * (1.0 + ranked[0].1)) {
                        return Err(Error::Invalid(format!(
                            "nearest curve selection is ambiguous between entities {} and {}; use an id",
                            ranked[0].0, ranked[1].0
                        )));
                    }
                    candidates = ranked.first().map(|r| vec![r.0]).unwrap_or_default();
                }
                candidates
            }
        };
        if ids.is_empty() {
            return Err(Error::NotFound(format!("curve selector matches no curves in sketch {sketch}")));
        }
        let mut seen = HashSet::new();
        let mut unique = Vec::new();
        for id in ids {
            if !s.entities.iter().any(|e| e.id == id) {
                return Err(Error::NotFound(format!("no curve {id} in sketch {sketch} (see sketch_info)")));
            }
            if seen.insert(id) {
                unique.push(id);
            }
        }
        Ok(unique)
    }

    /// Resolve exactly one curve before any edit takes place.
    pub fn sketch_curve_id(&self, sketch: Id, sel: &EntityRef) -> Result<Id> {
        let ids = self.sketch_curve_ids(sketch, sel)?;
        if ids.len() != 1 {
            return Err(Error::Invalid(format!("curve selector is ambiguous: matched {} curves {ids:?}; select exactly one", ids.len())));
        }
        Ok(ids[0])
    }

    /// Resolve an existing sketch point or curve role without changing the document.
    pub fn sketch_point_id(&self, sketch: Id, sel: &PointSel) -> Result<Id> {
        let si = self.sketch_si(sketch)?;
        let s = &self.p.sketches[si];
        match sel {
            PointSel::Id(id) => s
                .points
                .iter()
                .find(|p| p.id == *id)
                .map(|p| p.id)
                .ok_or_else(|| Error::NotFound(format!("no point {id} in sketch {sketch}"))),
            PointSel::Origin(_) => (s.origin != 0)
                .then_some(s.origin)
                .ok_or_else(|| Error::NotFound(format!("origin of sketch {sketch} has not been materialized"))),
            PointSel::Role(r) => {
                if (r.role == PointRole::Node) != r.node_index.is_some() {
                    return Err(Error::Invalid("`node_index` is required for role `node` and forbidden for every other role".into()));
                }
                let id = self.sketch_curve_id(sketch, &r.curve)?;
                let e = s.entities.iter().find(|e| e.id == id).ok_or_else(|| Error::NotFound(format!("curve {id}")))?;
                let p = match r.role {
                    PointRole::Centre => entity_centre(e),
                    PointRole::Start | PointRole::End => entity_ends(e).map(|ends| ends[usize::from(r.role == PointRole::End)]),
                    PointRole::Major => match e.kind {
                        EntityKind::Ellipse { ma, .. } => Some(ma),
                        _ => None,
                    },
                    PointRole::Minor => match e.kind {
                        EntityKind::Ellipse { mi, .. } => Some(mi),
                        _ => None,
                    },
                    PointRole::Node => match &e.kind {
                        EntityKind::Spline { nodes, .. } => r.node_index.and_then(|i| nodes.get(i).copied()),
                        _ => None,
                    },
                };
                // A closed spline has nodes but no start/end roles.
                let p = if matches!((&e.kind, r.role), (EntityKind::Spline { ends: Ends::Closed, .. }, PointRole::Start | PointRole::End)) {
                    None
                } else {
                    p
                };
                p.ok_or_else(|| Error::Invalid(format!("role {:?} is unavailable on curve {id}", r.role)))
            }
        }
    }
}
