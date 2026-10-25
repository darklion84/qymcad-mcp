//! Native sketch corner cuts with the app's six driving size modes.
use super::{EditedSketch, EntityRef, PointSel};
use crate::{
    error::{Error, Result},
    Id, Num, Session,
};
use qymcad_core::{
    feature::ChamferMode,
    model::{ChamferLegs, Constraint, CornerAt, CornerCut, FilletBy, FilletSize},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Size of a sketch fillet or chamfer, using the native app's measurement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum CornerSize {
    /// Fillet radius, in mm.
    Radius {
        /// Driving radius or formula.
        value: Num,
    },
    /// Straight chord between the fillet's touching points, in mm.
    Chord {
        /// Driving chord length or formula.
        value: Num,
    },
    /// Length along the fillet arc, in mm.
    ArcLength {
        /// Driving arc length or formula.
        value: Num,
    },
    /// Equal-setback chamfer measured by the cut's length, as in the app.
    Symmetric {
        /// Driving cut length, not setback, or formula.
        value: Num,
    },
    /// Chamfer measured along the two ordered original sides.
    TwoDist {
        /// Setback along the first selected side, or formula.
        first: Num,
        /// Setback along the second selected side, or formula.
        second: Num,
    },
    /// Chamfer measured by its first setback and angle from that side.
    DistAngle {
        /// Setback along the first selected side, or formula.
        first: Num,
        /// Angle in degrees, or formula.
        second: Num,
    },
}

/// One corner identified without an ambiguous cursor-sector guess.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CornerSpec {
    /// Ordered pair of different curves ending at the selected point.
    pub curves: [EntityRef; 2],
    /// Shared endpoint of the two selected curves.
    pub point: PointSel,
    /// Native measurement and its driving sizes.
    pub size: CornerSize,
}

impl Session {
    /// Round or bevel a shared endpoint through the same native corner path as the app.
    pub fn sketch_corner(&mut self, sketch: Id, spec: &CornerSpec) -> Result<EditedSketch> {
        let si = self.sketch_si(sketch)?;
        let curves = [self.sketch_curve_id(sketch, &spec.curves[0])?, self.sketch_curve_id(sketch, &spec.curves[1])?];
        let point = self.sketch_point_id(sketch, &spec.point)?;
        let corner = CornerAt::of(point, curves);
        if curves[0] == curves[1] || !self.p.corner_at(si, corner) {
            return Err(Error::Invalid("corner requires two different curves ending at the selected shared point".into()));
        }
        let vars = self.p.param_map();
        let (cut, sizes): (CornerCut, Vec<&Num>) = match &spec.size {
            CornerSize::Radius { value } | CornerSize::Chord { value } | CornerSize::ArcLength { value } => {
                let by = match spec.size {
                    CornerSize::Radius { .. } => FilletBy::Radius,
                    CornerSize::Chord { .. } => FilletBy::Chord,
                    _ => FilletBy::ArcLength,
                };
                (CornerCut::Fillet(FilletSize { by, value: value.eval(&vars)? }), vec![value])
            }
            CornerSize::Symmetric { value } => (CornerCut::Chamfer(ChamferLegs::equal(value.eval(&vars)?)), vec![value]),
            CornerSize::TwoDist { first, second } | CornerSize::DistAngle { first, second } => {
                let mode = if matches!(spec.size, CornerSize::TwoDist { .. }) { ChamferMode::TwoDist } else { ChamferMode::DistAngle };
                (CornerCut::Chamfer(ChamferLegs { mode, first: first.eval(&vars)?, second: second.eval(&vars)? }), vec![first, second])
            }
        };
        for size in &sizes {
            if size.eval(&vars)? <= 1e-9 {
                return Err(Error::Invalid("corner sizes must be greater than 1e-9 (lengths in mm, angles in degrees)".into()));
            }
        }
        if self.p.corner_blend(si, point, corner.pair, cut).is_none() {
            return Err(Error::Invalid("native corner cut does not fit these curves or the requested size mode".into()));
        }
        // Freedom left by the cut is reported, not refused (ADR 0022): a chamfer does not hold its cut point on a
        // spline leg, as in the app. Redundant rows would be a defect of the edit, so those are refused.
        let redundant = self.p.sketch_dof(si).1;
        self.sketch_edit_report(sketch, |s| {
            let before = s.p.sketches[si].clone();
            if s.p.cut_corner_set(si, &[corner], cut, None) != 1 {
                return Err(Error::Invalid("native corner edit refused this curve pair or size".into()));
            }
            let after = &s.p.sketches[si];
            let created_entities: Vec<_> = after.entities.iter().filter(|e| before.entity(e.id).is_none()).map(|e| e.id).collect();
            if created_entities.len() != 1 {
                return Err(Error::Invalid("native corner edit did not return exactly one cut curve".into()));
            }
            let new = after.entity(created_entities[0]).ok_or_else(|| Error::Invalid("native cut curve is missing".into()))?;
            let mut dimensions = Vec::new();
            let mut add_unique = |matches: Vec<usize>| -> Result<()> {
                if matches.len() != 1 {
                    return Err(Error::Invalid("native corner driving dimension is missing or ambiguous".into()));
                }
                dimensions.push(matches[0]);
                Ok(())
            };
            let distance = |a, b| {
                after
                    .constraints
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| match c {
                        Constraint::Distance { a: x, b: y, driven: false, .. } if (*x == a && *y == b) || (*x == b && *y == a) => Some(i),
                        _ => None,
                    })
                    .collect()
            };
            match cut {
                CornerCut::Fillet(size) => {
                    let arc = new.kind.as_arc().ok_or_else(|| Error::Invalid("native fillet is not an arc".into()))?;
                    let matches = match size.by {
                        FilletBy::Radius => after
                            .constraints
                            .iter()
                            .enumerate()
                            .filter_map(|(i, c)| {
                                matches!(c,Constraint::Diameter {c,diam:false,driven:false,..} if *c==arc.center).then_some(i)
                            })
                            .collect(),
                        FilletBy::Chord => distance(arc.a, arc.b),
                        FilletBy::ArcLength => after
                            .constraints
                            .iter()
                            .enumerate()
                            .filter_map(|(i, c)| {
                                matches!(c,Constraint::ArcLength {c,a,b,driven:false,..} if *c==arc.center && *a==arc.a && *b==arc.b)
                                    .then_some(i)
                            })
                            .collect(),
                    };
                    add_unique(matches)?;
                }
                CornerCut::Chamfer(legs) => {
                    let line = new.kind.as_line().ok_or_else(|| Error::Invalid("native chamfer is not a line".into()))?;
                    match legs.mode {
                        ChamferMode::Symmetric => add_unique(distance(line.a, line.b))?,
                        ChamferMode::TwoDist => {
                            add_unique(distance(point, line.a))?;
                            add_unique(distance(point, line.b))?;
                        }
                        ChamferMode::DistAngle => {
                            add_unique(distance(point, line.a))?;
                            add_unique(
                                after
                                    .constraints
                                    .iter()
                                    .enumerate()
                                    .filter_map(|(i, c)| {
                                        matches!(c,Constraint::AngleLines {c,d,driven:false,..} if *c==line.a && *d==line.b).then_some(i)
                                    })
                                    .collect(),
                            )?;
                        }
                    }
                }
            }
            for (&ci, size) in dimensions.iter().zip(&sizes) {
                let expression = s.p.sketches[si].constraints[ci]
                    .expr_mut()
                    .ok_or_else(|| Error::Invalid("native corner dimension cannot store a formula".into()))?;
                *expression = size.expr().unwrap_or_default();
            }
            s.finish_sketch_edit(si)?;
            let dof = s.p.sketch_dof(si);
            if dof.1 > redundant {
                return Err(Error::Invalid(format!("the corner cut left redundant constraints in the sketch: DOF {dof:?}")));
            }
            Ok(dimensions)
        })
    }
}
