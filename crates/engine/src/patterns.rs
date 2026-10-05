//! Copies of a whole body: linear and circular arrays, mirror. QymCAD patterns the source BODY (not a single
//! feature): the result is one body holding every copy. Atomic like every feature.

use crate::error::{Error, Result};
use crate::modifiers::dim;
use crate::session::{Rebuild, Session};
use crate::sketch::{BaseName, PlaneRef};
use crate::topology::{face_key, Axis};
use crate::value::Num;
use qymcad_core::model::{ArrayAxis, DatumAxis, Id};

/// One direction of a linear array: the step between copies and how many copies (the original included).
#[derive(Clone, Debug, PartialEq)]
pub struct ArrayDir {
    pub dx: Num,
    pub dy: Num,
    pub dz: Num,
    pub count: Num,
}

/// An axis to turn about.
#[derive(Clone, Debug, PartialEq)]
pub enum AxisRef {
    /// The sketch's x axis (revolve only).
    SketchX,
    /// The sketch's y axis (revolve only).
    SketchY,
    /// A line entity of the revolved sketch, usually a construction line (revolve only).
    Line(Id),
    /// A world axis through the origin.
    World(Axis),
    /// An existing datum axis.
    Datum(Id),
    /// A fixed axis through `origin` along `dir` (creates a datum axis).
    Through { origin: [f64; 3], dir: [f64; 3] },
    /// The axis of a cylindrical or conical face (creates a datum axis that follows the face).
    FaceAxis { body: Id, face: u32 },
}

impl Session {
    /// A datum axis id for `axis` (0 = world Z), creating a datum axis when needed. Sketch-relative forms are
    /// refused here. Call inside an atomic edit.
    pub(crate) fn datum_axis_for(&mut self, axis: &AxisRef) -> Result<Id> {
        Ok(match axis {
            AxisRef::World(Axis::Z) => 0,
            AxisRef::World(a) => {
                let dir = if *a == Axis::X { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
                self.p.add_datum_axis(DatumAxis::manual(format!("axis {a:?}"), [0.0; 3], dir))
            }
            AxisRef::Through { origin, dir } => {
                if dir.iter().map(|v| v * v).sum::<f64>().sqrt() < 1e-12 {
                    return Err(Error::Invalid("axis direction has no length".into()));
                }
                self.p.add_datum_axis(DatumAxis::manual("axis", *origin, *dir))
            }
            AxisRef::FaceAxis { body, face } => {
                let ok = self.p.regen_faces.get(body).is_some_and(|fs| fs.iter().any(|f| f.id == *face));
                if !ok {
                    return Err(Error::NotFound(format!("face {face} of body {body} (re-read topology after each feature)")));
                }
                let round = self.shapes.get(body).is_some_and(|sh| {
                    let _g = qymcad_kernel::kernel_gate();
                    sh.face_axis(*face).is_some()
                });
                if !round {
                    return Err(Error::Invalid(format!("face {face} of body {body} is not a cylinder or cone")));
                }
                self.p.add_axis_from_face(*body, *face)
            }
            AxisRef::Datum(id) => {
                if !self.p.datum_axes.iter().any(|a| a.id == *id) {
                    return Err(Error::NotFound(format!("datum axis {id}")));
                }
                *id
            }
            AxisRef::SketchX | AxisRef::SketchY | AxisRef::Line(_) => {
                return Err(Error::Invalid(
                    "a sketch axis or line only works for revolve; give a world axis, a face or a datum axis".into(),
                ))
            }
        })
    }

    /// `count` copies along `d1` (and `d2`, if given) of the whole body, as one body.
    pub fn linear_array(&mut self, body: Option<Id>, d1: &ArrayDir, d2: Option<&ArrayDir>, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology();
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let a = s.array_axis(d1)?;
            let b = match d2 {
                Some(d) => s.array_axis(d)?,
                None => ArrayAxis::none(),
            };
            if a.count * b.count < 2 {
                return Err(Error::Invalid("an array needs at least 2 copies".into()));
            }
            let id = s.p.add_linear_array_grid(src, a, b);
            for (sfx, d) in [("", Some(d1)), ("2", d2)] {
                if let Some(d) = d {
                    dim(s, id, &format!("dx{sfx}"), &d.dx);
                    dim(s, id, &format!("dy{sfx}"), &d.dy);
                    dim(s, id, &format!("dz{sfx}"), &d.dz);
                    dim(s, id, &format!("count{sfx}"), &d.count);
                }
            }
            s.set_node_name(id, name);
            Ok(id)
        })
    }

    /// `count` copies of the whole body turned about `axis` (default world Z). QymCAD's step is `360/count` when
    /// `angle` ≥ 359.9°, otherwise `angle/count` (so the last copy stands at `angle·(count−1)/count`, F-3B-5).
    pub fn circular_array(
        &mut self,
        body: Option<Id>,
        count: &Num,
        angle: &Num,
        axis: Option<&AxisRef>,
        name: Option<&str>,
    ) -> Result<(Id, Rebuild)> {
        self.ensure_topology();
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let n = s.count(count)?;
            if n < 2 {
                return Err(Error::Invalid("an array needs at least 2 copies".into()));
            }
            let ang = angle.eval(&s.p.param_map())?;
            if ang <= 0.0 || ang > 360.0 {
                return Err(Error::Invalid(format!("angle must be in (0, 360], got {ang}")));
            }
            let ax = match axis {
                Some(a) => s.datum_axis_for(a)?,
                None => 0,
            };
            let id = s.p.add_circular_array_axis(src, n, ang, ax);
            dim(s, id, "count", count);
            dim(s, id, "angle", angle);
            s.set_node_name(id, name);
            Ok(id)
        })
    }

    /// Mirror the whole body about a base plane, a datum plane or a planar face. `keep` keeps the original too
    /// (one body holding both halves); otherwise only the image remains.
    pub fn mirror(&mut self, body: Option<Id>, plane: &PlaneRef, keep: bool, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology();
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let id = match plane {
                PlaneRef::Base(b) => s.p.add_mirror(
                    src,
                    match b {
                        BaseName::XY => 0,
                        BaseName::XZ => 1,
                        BaseName::YZ => 2,
                    },
                    keep,
                    0,
                ),
                PlaneRef::Plane(d) => {
                    if !s.p.planes.iter().any(|w| w.id == *d) {
                        return Err(Error::NotFound(format!("datum plane {d}")));
                    }
                    s.p.add_mirror(src, 0, keep, *d)
                }
                PlaneRef::Face { body: fb, face } => {
                    let f =
                        s.p.regen_faces
                            .get(fb)
                            .and_then(|fs| fs.iter().find(|f| f.id == *face))
                            .ok_or_else(|| Error::NotFound(format!("face {face} of body {fb} (re-read topology after each feature)")))?;
                    let key = face_key(f);
                    if !s.face_is_planar(*fb, *face) {
                        return Err(Error::Invalid(format!("face {face} of body {fb} is not planar")));
                    }
                    let id = s.p.add_mirror(src, 0, keep, 0);
                    s.p.set_op_face(id, *fb, key);
                    id
                }
            };
            s.set_node_name(id, name);
            Ok(id)
        })
    }

    fn array_axis(&self, d: &ArrayDir) -> Result<ArrayAxis> {
        let vars = self.p.param_map();
        let v = [d.dx.eval(&vars)?, d.dy.eval(&vars)?, d.dz.eval(&vars)?];
        let count = self.count(&d.count)?;
        if count >= 2 && v.iter().all(|x| x.abs() < 1e-9) {
            return Err(Error::Invalid("array step (dx, dy, dz) is zero".into()));
        }
        Ok(ArrayAxis { d: v, count })
    }

    fn count(&self, n: &Num) -> Result<u32> {
        let v = n.eval(&self.p.param_map())?;
        if v < 1.0 || v > 10000.0 || (v - v.round()).abs() > 1e-9 {
            return Err(Error::Invalid(format!("count must be a whole number from 1, got {v}")));
        }
        Ok(v.round() as u32)
    }
}
