//! Revolve: sketch profiles turned about an axis in the sketch plane, as a new body or added to / cut from /
//! intersected with the part's body. Atomic like every feature.

use crate::error::{Error, Result};
use crate::features::{Direction, Op};
use crate::modifiers::dim;
use crate::patterns::AxisRef;
use crate::session::{Rebuild, Session};
use crate::topology::Axis;
use crate::value::Num;
use qymcad_core::feature::Reach;
use qymcad_core::model::{EntityKind, Id, RevolveAxis, RevolveTurn};

/// Arguments of `Session::revolve`.
#[derive(Clone, Debug, PartialEq)]
pub struct Revolve {
    /// The sketch whose contours are revolved.
    pub sketch: Id,
    /// Contour ids; default: every top-level contour (each minus the contours inside it).
    pub profiles: Option<Vec<Id>>,
    /// The axis; it must lie in the sketch plane and must not cross the profile.
    pub axis: AxisRef,
    /// Degrees, (0, 360].
    pub angle: Num,
    /// `Normal` turns by the right-hand rule about the axis direction (sketch x → +X, sketch y → +Y of the
    /// sketch); `Reverse` the other way; `Symmetric` half each way (F-026).
    pub direction: Direction,
    /// Add (creates the first body), cut, intersect, or a new body.
    pub op: Op,
    /// Body to modify (default: the current body).
    pub target: Option<Id>,
    /// Name for the new feature, usable instead of its id later.
    pub name: Option<String>,
}

impl Session {
    pub fn revolve(&mut self, a: &Revolve) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
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
            let angle = a.angle.eval(&s.p.param_map())?;
            if angle.is_nan() || angle <= 0.0 || angle > 360.0 {
                return Err(Error::Invalid(format!("angle must be in (0, 360], got {angle}")));
            }
            let ax = match &a.axis {
                AxisRef::SketchX => RevolveAxis { axis: 0, datum: 0, line: 0 },
                AxisRef::SketchY => RevolveAxis { axis: 1, datum: 0, line: 0 },
                AxisRef::Line(l) => {
                    let is_line = s.p.sketches[si].entities.iter().any(|e| e.id == *l && matches!(e.kind, EntityKind::Line { .. }));
                    if !is_line {
                        return Err(Error::NotFound(format!("line {l} in sketch {}", a.sketch)));
                    }
                    RevolveAxis { axis: 0, datum: 0, line: *l }
                }
                // `datum_axis_for` answers world Z with 0 (the arrays' default); a revolve needs a real datum.
                AxisRef::World(Axis::Z) => {
                    RevolveAxis { axis: 0, datum: s.datum_axis_for(&AxisRef::Through { origin: [0.0; 3], dir: [0.0, 0.0, 1.0] })?, line: 0 }
                }
                other => RevolveAxis { axis: 0, datum: s.datum_axis_for(other)?, line: 0 },
            };
            let reach: Reach = a.direction.into();
            let turn = RevolveTurn { angle, reach };
            let src = match (a.target, a.op) {
                (Some(_), Op::NewBody) => return Err(Error::Invalid("op new_body makes a separate body; omit `target`".into())),
                (Some(t), _) => Some(s.source_body(Some(t))?),
                (None, _) => s.tip_body(),
            };
            let id = match (a.op, src) {
                (Op::NewBody, _) | (Op::Add, None) => s.p.add_revolve_axis_ex(a.sketch, profiles, ax, turn),
                (Op::Cut | Op::Intersect, None) => return Err(Error::Invalid("there is no body to cut or intersect yet".into())),
                (op, Some(src)) => {
                    let code = match op {
                        Op::Cut => 0,
                        Op::Add => 1,
                        _ => 2,
                    };
                    s.p.add_revolve_multi_op(a.sketch, profiles, ax, turn, src, code)
                }
            };
            dim(s, id, "angle", &a.angle);
            s.set_node_name(id, a.name.as_deref());
            Ok(id)
        })
    }
}
