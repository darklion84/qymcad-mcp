//! Solid features. Each call is atomic: if the new feature does not build, the document is left unchanged and
//! the error is returned (see `Session::atomic`).

use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::sketch::PlaneRef;
use crate::value::Num;
use qymcad_core::feature::{Extent, Reach};
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
    #[serde(default)]
    pub op: Op,
    #[serde(default)]
    pub direction: Direction,
    /// Cut/add through the whole body (only for `cut`, `add` onto an existing body, `intersect`).
    #[serde(default)]
    pub through: bool,
    /// Body to modify. Default: the current body of the part.
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
            s.set_node_name(id, name);
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
            if !a.through && h <= 0.0 {
                return Err(Error::Invalid(format!("height must be positive, got {h}")));
            }
            let reach: Reach = a.direction.into();
            let src = match a.target {
                Some(t) => Some(t),
                None => s.tip_body(),
            };
            let id = match (a.op, src) {
                (Op::NewBody, _) | (Op::Add, None) => {
                    if a.through {
                        return Err(Error::Invalid("`through` needs a body to go through; give a height".into()));
                    }
                    s.p.add_extrude_multi(a.sketch, profiles, h, reach, 0.0, vec![])
                }
                (Op::Cut | Op::Intersect, None) => return Err(Error::Invalid("there is no body to cut or intersect yet".into())),
                (op, Some(src)) => {
                    let code = match op {
                        Op::Cut => 0,
                        Op::Add => 1,
                        _ => 2,
                    };
                    let span = CombineSpan { height: h, down: 0.0, extent: Extent { through: a.through, reach }, fill: &[] };
                    s.p.add_combine_multi_op(src, a.sketch, profiles, span, code)
                }
            };
            if let (Some(e), false) = (a.height.expr(), a.through) {
                s.p.set_feat_dim(id, "height", e);
            }
            s.set_node_name(id, a.name.as_deref());
            Ok(id)
        })
    }
}
