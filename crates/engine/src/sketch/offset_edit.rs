//! Native associative sketch offsets and their persisted formula-bearing distances.
use super::{EditedSketch, SketchCurveSel};
use crate::{Error, Id, Num, Result, Session};
use qymcad_core::geom::Point2;
use qymcad_core::model::{Constraint, CurveRef, DimTarget, EntityKind, Project, Sketch};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Offset native sketch curves to the side selected by a one-time geometric hint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SketchOffsetSpec {
    /// Source chain, loop or individual native curves; construction chains are unsupported upstream.
    pub curves: SketchCurveSel,
    /// Nonzero distance in mm or a driving formula; a negative value reverses the selected side.
    pub distance: Num,
    /// One-time side-selection point in sketch coordinates, not a replayed driver.
    pub toward: [Num; 2],
    /// Name the created native distance only when the offset has exactly one driving dimension.
    #[serde(default)]
    pub dimension_name: Option<String>,
}

impl Session {
    /// Copy native curves at a formula-driven offset, retaining native associations and refusing tied sides.
    pub fn sketch_offset(&mut self, sketch: Id, spec: &SketchOffsetSpec) -> Result<EditedSketch> {
        let si = self.sketch_si(sketch)?;
        let ids = self.sketch_curve_ids(sketch, &spec.curves)?;
        let vars = self.p.param_map();
        let d = spec.distance.eval(&vars)?;
        if d.abs() <= 1e-9 {
            return Err(Error::Invalid("offset distance must have magnitude greater than 1e-9 mm".into()));
        }
        let expression = spec.distance.magnitude_expr(d)?;
        let toward = Point2::new(spec.toward[0].eval(&vars)?, spec.toward[1].eval(&vars)?);
        let s = &self.p.sketches[si];
        for id in &ids {
            let entity = s.entity(*id).ok_or_else(|| Error::NotFound(format!("curve {id}")))?;
            if entity.construction && !matches!(entity.kind, EntityKind::Circle { .. }) {
                return Err(Error::Invalid(format!("native offset excludes construction curve {id}; select profile curves")));
            }
        }
        if let Some(name) = &spec.dimension_name {
            qymcad_core::drivers::check_ident(name).map_err(|e| Error::Invalid(format!("invalid dimension name `{name}`: {e:?}")))?;
            if self.p.parameters.iter().any(|p| crate::value::same_ignoring_case(&p.name, name))
                || self.p.named_dims.iter().any(|p| crate::value::same_ignoring_case(&p.name, name))
            {
                return Err(Error::Invalid(format!("dimension name `{name}` collides with an existing parameter or dimension")));
            }
        }
        // The app previews both sides before choosing the nearer one. Refuse a tied hint instead of inheriting
        // its arbitrary positive-side fallback. A circle has an exact radial comparison, avoiding preview chords.
        let side_distance = |distance: f64| {
            if ids.len() == 1 {
                if let Some(c) = s.entity(ids[0]).and_then(|e| e.kind.as_circle()) {
                    if let Some(p) = s.point(c.center) {
                        let radius = c.r + distance;
                        if radius > 0.05 {
                            return ((toward.x - p.x).hypot(toward.y - p.y) - radius).abs();
                        }
                    }
                    return f64::MAX;
                }
            }
            self.p
                .offset_preview(si, &ids, distance)
                .iter()
                .flat_map(|p| p.windows(2))
                .map(|p| segment_distance(toward, p[0], p[1]))
                .fold(f64::MAX, f64::min)
        };
        let (plus, minus) = (side_distance(d.abs()), side_distance(-d.abs()));
        if plus == f64::MAX && minus == f64::MAX {
            return Err(Error::Invalid("native offset has no surviving curves at this distance".into()));
        }
        if (plus - minus).abs() <= 1e-7 * (1.0 + d.abs()) {
            return Err(Error::Invalid("offset side is ambiguous; move `toward` towards the desired copy".into()));
        }
        // The side the comparison above found, then a negative distance turned to the other side, as the app's
        // `offset_toward` does with its own (chord) comparison.
        let near = if minus < plus { -d.abs() } else { d.abs() };
        let signed = if d < 0.0 { -near } else { near };
        if self.p.offset_preview(si, &ids, signed).is_empty() {
            return Err(Error::Invalid("native offset vanishes on the selected side at this distance".into()));
        }
        let old_entities: HashSet<_> = self.p.sketches[si].entities.iter().map(|e| e.id).collect();
        self.sketch_edit_report(sketch, |session| {
            let first = session.p.sketches[si].constraints.len();
            let groups = session.p.offset_entities(si, &ids, signed);
            if groups == 0 {
                return Err(Error::Invalid("native offset could not copy the selected curves".into()));
            }
            let s = &mut session.p.sketches[si];
            // F-076: EdgeDistance expressions never evaluate in the app. Use CurveDistance for circular
            // radii. Arc offsets use endpoint Distance upstream; replace that radial row as well.
            for ci in first..s.constraints.len() {
                if let Some((source, copy)) = radial_centres(s, &old_entities, first, &s.constraints[ci]) {
                    s.constraints[ci] = Constraint::CurveDistance {
                        // These are radial dimensions of the parent circles. The native endpoint rays below
                        // retain the source/copy arc spans independently of the radial size.
                        from: CurveRef::Round { c: source },
                        to: CurveRef::Round { c: copy },
                        d: d.abs(), off: 0.0, expr: String::new(), driven: false, at: None,
                    };
                }
            }
            let mut drivers = Vec::new();
            for (ci, c) in s.constraints.iter_mut().enumerate().skip(first) {
                if c.dim_value().is_some() {
                    if let Some(expr) = c.expr_mut() {
                        *expr = expression.clone();
                        drivers.push(ci);
                    }
                }
            }
            if drivers.is_empty() {
                return Err(Error::Invalid("native offset made no associative driving distance; this topology is unsupported".into()));
            }
            if let Some(name) = &spec.dimension_name {
                if drivers.len() != 1 {
                    return Err(Error::Invalid(format!(
                        "dimension_name requires exactly one native offset driver; this offset has {} (use dimension_set on the reported indices)",
                        drivers.len()
                    )));
                }
                let refs = Project::dim_refs(&s.constraints[drivers[0]])
                    .ok_or_else(|| Error::Invalid("offset dimension has no native references".into()))?;
                let target = DimTarget::Sketch { sketch, refs };
                if session.p.naming_closes_circle(&target, name).is_some() || !session.p.name_dim(name.clone(), target) {
                    return Err(Error::Invalid("offset dimension name would collide or create a formula cycle".into()));
                }
            }
            session.finish_sketch_edit(si)?;
            Ok(drivers)
        })
    }
}

fn segment_distance(p: Point2, a: Point2, b: Point2) -> f64 {
    let (x, y) = (b.x - a.x, b.y - a.y);
    let den = x * x + y * y;
    let t = if den > 0.0 { ((p.x - a.x) * x + (p.y - a.y) * y) / den } else { 0.0 }.clamp(0.0, 1.0);
    (p.x - a.x - t * x).hypot(p.y - a.y - t * y)
}

fn radial_centres(s: &Sketch, original: &HashSet<Id>, first: usize, c: &Constraint) -> Option<(Id, Id)> {
    match *c {
        Constraint::EdgeDistance { c1, c2, .. } => Some((c1, c2)),
        Constraint::Distance { a, b, .. } => s.constraints[first..].iter().find_map(|c| {
            let Constraint::Concentric { c1, c2 } = *c else { return None };
            let source =
                s.entities.iter().filter(|e| original.contains(&e.id)).filter_map(|e| e.kind.as_arc()).find(|arc| arc.center == c1)?;
            let copy =
                s.entities.iter().filter(|e| !original.contains(&e.id)).filter_map(|e| e.kind.as_arc()).find(|arc| arc.center == c2)?;
            let matches = [source.a, source.b].contains(&a) && [copy.a, copy.b].contains(&b)
                || [source.a, source.b].contains(&b) && [copy.a, copy.b].contains(&a);
            matches.then_some((c1, c2))
        }),
        _ => None,
    }
}
