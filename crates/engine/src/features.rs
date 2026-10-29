//! Solid features. Each call is atomic: if the new feature does not build, the document is left unchanged and
//! the error is returned (see `Session::atomic`).

use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::sketch::PlaneRef;
use crate::value::Num;
use qymcad_core::feature::{Extent, FeatureKind, Reach};
use qymcad_core::model::{CombineSpan, Id};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What a sketch-based feature does to the part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Add material to the part's body (creates the first body if there is none).
    #[default]
    Add,
    /// Remove material from the part's body.
    Cut,
    /// Keep only the common volume.
    Intersect,
    /// Create a separate body.
    NewBody,
}

/// Which way from the sketch plane, relative to its normal (XY: +Z, XZ: −Y, YZ: +X, a face: outward).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Along the normal.
    #[default]
    Normal,
    /// Against the normal (e.g. a pocket cut down from a top plane or face).
    Reverse,
    /// Half to each side.
    Symmetric,
}

impl From<Direction> for Reach {
    fn from(d: Direction) -> Reach {
        match d {
            Direction::Normal => Reach::Forward,
            Direction::Reverse => Reach::Backward,
            Direction::Symmetric => Reach::BothWays,
        }
    }
}

/// Arguments of `extrude`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Extrude {
    /// The sketch to extrude.
    pub sketch: Id,
    /// Contour ids (from `sketch_info`). Default: every top-level contour, each minus the contours inside it
    /// (a rectangle with circles inside gives a plate with holes; circles alone give discs).
    #[serde(default)]
    pub profiles: Option<Vec<Id>>,
    /// Distance, mm (number or expression). Ignored when `through` is set.
    pub height: Num,
    /// Second-side length, mm: normal spans [-down, height], reverse spans [-height, down].
    /// Must evaluate finite and greater than 1e-9; incompatible with symmetric direction or through.
    #[serde(default)]
    pub down: Option<Num>,
    /// What the extrude does to the part: add (creates the first body), cut, intersect, or a new body.
    #[serde(default)]
    pub op: Op,
    /// Which way from the sketch plane, relative to its normal.
    #[serde(default)]
    pub direction: Direction,
    /// Cut/add through the whole body (only for `cut`, `add` onto an existing body, `intersect`).
    #[serde(default)]
    pub through: bool,
    /// Body to modify; must be a current (unconsumed) body. Default: the current body of the part. Not allowed
    /// with `Op::NewBody`.
    #[serde(default)]
    pub target: Option<Id>,
    /// Name for the feature, usable instead of its id later.
    #[serde(default)]
    pub name: Option<String>,
}

impl Session {
    /// A datum plane parallel to a base plane or another datum plane, at distance `dist` along its normal.
    pub fn plane_offset(&mut self, base: &PlaneRef, dist: &Num, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.atomic(|s| {
            let d = dist.eval(&s.p.param_map())?;
            let id = match base {
                PlaneRef::Base(b) => s.p.add_offset_plane((*b).into(), d),
                PlaneRef::Plane(src) => {
                    if !s.p.planes.iter().any(|w| w.id == *src) {
                        return Err(Error::NotFound(format!("datum plane {src}")));
                    }
                    s.p.add_offset_from_plane(*src, d)
                }
                PlaneRef::Face { .. } => {
                    return Err(Error::Invalid("an offset from a face is not supported yet; use a base or datum plane".into()))
                }
            };
            if let Some(e) = dist.expr() {
                s.p.set_feat_dim(id, "dist", e);
            }
            s.set_node_name(id, name)?;
            Ok(id)
        })
    }

    /// Extrude sketch profiles into a solid, or cut/add/intersect with the part's body. Returns the new body id.
    pub fn extrude(&mut self, a: &Extrude) -> Result<(Id, Rebuild)> {
        self.atomic(|s| {
            let profiles = match &a.profiles {
                Some(ids) if !ids.is_empty() => ids.clone(),
                _ => s.root_contours(a.sketch)?,
            };
            if profiles.is_empty() {
                return Err(Error::Invalid(format!("sketch {} has no closed contour", a.sketch)));
            }
            let si = s.sketch_si(a.sketch)?;
            if let Some(bad) = profiles.iter().find(|c| !s.p.sketches[si].contour_ids.contains(c)) {
                return Err(Error::NotFound(format!("contour {bad} in sketch {}", a.sketch)));
            }
            let h = a.height.eval(&s.p.param_map())?;
            if !a.through && (h.is_nan() || h <= 0.0) {
                return Err(Error::Invalid(format!("height must be positive, got {h}")));
            }
            let down = match &a.down {
                Some(down) => {
                    if a.through || a.direction == Direction::Symmetric {
                        return Err(Error::Invalid("`down` cannot be combined with `through` or symmetric direction".into()));
                    }
                    let d = down.eval(&s.p.param_map())?;
                    check_down(d)?;
                    d
                }
                None => 0.0,
            };
            let reach: Reach = a.direction.into();
            // A target must be a current body: an extrude into a consumed one branches a ghost chain (F-009). A new
            // body has no target; asking for both is a contradiction, not something to guess about.
            let src = match (a.target, a.op) {
                (Some(_), Op::NewBody) => return Err(Error::Invalid("op new_body makes a separate body; omit `target`".into())),
                (Some(t), _) => Some(s.source_body(Some(t))?),
                (None, _) => s.tip_body(),
            };
            let id = match (a.op, src) {
                (Op::NewBody, _) | (Op::Add, None) => {
                    if a.through {
                        return Err(Error::Invalid("`through` needs a body to go through; give a height".into()));
                    }
                    s.in_component(s.p.sketch_owner(a.sketch), |p| p.add_extrude_multi(a.sketch, profiles, h, reach, down, vec![]))
                }
                (Op::Cut | Op::Intersect, None) => return Err(Error::Invalid("there is no body to cut or intersect yet".into())),
                (op, Some(src)) => {
                    let code = match op {
                        Op::Cut => 0,
                        Op::Add => 1,
                        _ => 2,
                    };
                    let span = CombineSpan { height: h, down, extent: Extent { through: a.through, reach }, fill: &[] };
                    s.in_component(s.p.sketch_owner(a.sketch), |p| p.add_combine_multi_op(src, a.sketch, profiles, span, code))
                }
            };
            if let (Some(e), false) = (a.height.expr(), a.through) {
                s.p.set_feat_dim(id, "height", e);
            }
            if let Some(value) = &a.down {
                // Keep even literals: a formula entry distinguishes explicit Two sides from legacy one-sided zero.
                let expr = value.expr().filter(|e| e.parse::<f64>().is_err()).unwrap_or_else(|| down.to_string());
                s.p.set_feat_dim(id, "down", expr);
            }
            s.set_node_name(id, a.name.as_deref())?;
            Ok(id)
        })
    }

    /// Refuse native abs/threshold/ignored-value behavior on evaluated Two sides fields (F-111).
    pub(crate) fn check_extrude_down(&self) -> std::result::Result<(), (Id, Error)> {
        for node in &self.p.timeline {
            let (down, ignored) = match &node.kind {
                FeatureKind::Extrude { down, reach, .. } => (*down, *reach == Reach::BothWays),
                FeatureKind::Combine { down, extent, .. } => (*down, extent.through || extent.reach == Reach::BothWays),
                _ => continue,
            };
            let formula = self.p.feat_dims.get(&node.id).and_then(|dims| dims.get("down"));
            if down == 0.0 && formula.is_none() {
                continue;
            }
            let checked = (|| {
                if ignored {
                    return Err(Error::Invalid("`down` cannot be combined with `through` or symmetric direction".into()));
                }
                let value = match formula {
                    Some(expr) => Num::Expr(expr.clone()).eval(&self.p.param_map())?,
                    None => down,
                };
                check_down(value)
            })();
            checked.map_err(|e| (node.id, e))?;
        }
        Ok(())
    }
}

/// Native takes abs() and silently drops lengths at or below its threshold; never promise those values.
fn check_down(value: f64) -> Result<()> {
    if !value.is_finite() || value <= 1e-9 {
        return Err(Error::Invalid(format!(
            "down must be finite and greater than 1e-9, got {value}; recreate the feature to return to one side"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod two_sides_tests {
    use super::*;
    use crate::BaseName;

    #[test]
    fn rebuild_refuses_evaluated_down_before_touching_shapes() {
        let mut s = Session::new_part();
        let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
        let a: Extrude = serde_json::from_value(serde_json::json!({"sketch":sketch,"height":7,"down":3})).unwrap();
        let (id, _) = s.extrude(&a).unwrap();
        let volume = s.result_bodies()[0].volume;
        for value in ["-3", "0", "0.000000000001", "0.000000001", "1e400"] {
            s.p.set_feat_dim(id, "down", value.into());
            s.p.mark_all_dirty();
            let report = s.rebuild();
            assert!(!report.errors.is_empty(), "rebuild must refuse down={value}");
            assert_eq!(report.errors[0].node, id);
            assert_eq!(s.result_bodies()[0].volume, volume, "invalid rebuild retains original shape");
        }
    }
    #[test]
    fn prior_native_error_cannot_hide_an_invalid_second_side_parameter_edit() {
        let mut s = Session::new_part();
        s.param_set("back", &3.0.into()).unwrap();
        let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
        let a: Extrude = serde_json::from_value(serde_json::json!({"sketch":sketch,"height":7,"down":"back"})).unwrap();
        let (id, _) = s.extrude(&a).unwrap();
        s.p.regen_errors.insert(id, qymcad_core::errors::CoreError::Kernel("prior native error".into()));
        let before = ron::ser::to_string(&s.p).unwrap();
        let volume = s.result_bodies()[0].volume;
        for value in [-3.0, 0.0, 1e-12, 1e-9] {
            assert!(s.param_set("back", &value.into()).is_err(), "prior error hid down={value}");
            assert_eq!(ron::ser::to_string(&s.p).unwrap(), before);
            assert_eq!(s.result_bodies()[0].volume, volume);
        }
    }
}
