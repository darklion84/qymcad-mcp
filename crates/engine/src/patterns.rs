//! Copies of a whole body: linear and circular arrays, mirror. QymCAD patterns the source BODY (not a single
//! feature): the result is one body holding every copy. Atomic like every feature.

use crate::error::{Error, Result};
use crate::modifiers::dim;
use crate::session::{Rebuild, Session};
use crate::sketch::{BaseName, PlaneRef};
use crate::topology::{face_key, Axis};
use crate::value::Num;
use qymcad_core::feature::FeatureKind;
use qymcad_core::model::{ArrayAxis, AxisDef, DatumAxis, Id};

/// Most copies one array may make, all directions multiplied. An array copies the whole body and every later
/// rebuild repeats it: 1000 copies of a trivial Ø2 cylinder took 1.2 s (debug and release alike, linear in the
/// count) and real bodies cost more each. Upstream has no cap and allocates for the product of the counts, so
/// 10000 × 10000 would try to build 10⁸ copies. 1000 is more than any sensible pattern on a 270 mm print bed
/// (≈ 32 × 32 at an 8 mm pitch).
pub(crate) const MAX_ARRAY_COPIES: u64 = 1000;

/// One direction of a linear array: the step between copies and how many copies (the original included).
#[derive(Clone, Debug, PartialEq)]
pub struct ArrayDir {
    /// Step along X between copies, mm (number or expression).
    pub dx: Num,
    /// Step along Y between copies, mm (number or expression).
    pub dy: Num,
    /// Step along Z between copies, mm (number or expression).
    pub dz: Num,
    /// Copies along this direction, the original included (number or expression); at most 1000 per array.
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
    /// The axis of a cylindrical or conical face (creates a datum axis that follows the face). `body` defaults
    /// to the current body of the active part.
    FaceAxis { body: Option<Id>, face: u32 },
}

impl Session {
    /// A datum axis id for `axis` (0 = world Z), creating a datum axis when needed. Sketch-relative forms are
    /// refused here. Call inside an atomic edit.
    pub(crate) fn datum_axis_for(&mut self, axis: &AxisRef) -> Result<Id> {
        Ok(match axis {
            AxisRef::World(Axis::Z) => 0,
            AxisRef::World(a) => {
                let dir = if *a == Axis::X { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
                self.fixed_axis(&format!("axis {a:?}"), [0.0; 3], dir)?
            }
            AxisRef::Through { origin, dir } => self.fixed_axis("axis", *origin, *dir)?,
            AxisRef::FaceAxis { body, face } => {
                let body = &match body {
                    Some(b) => *b,
                    None => self.default_body("the part has no body yet")?,
                };
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

    /// A fixed (manual) datum axis through `origin` along `dir`: an existing one on the same line with the same
    /// sense is reused, so repeated features about one axis share one datum.
    fn fixed_axis(&mut self, name: &str, origin: [f64; 3], dir: [f64; 3]) -> Result<Id> {
        let unit = |d: [f64; 3]| {
            let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            (l.is_finite() && l > 1e-12).then(|| [d[0] / l, d[1] / l, d[2] / l])
        };
        let u = unit(dir).ok_or_else(|| Error::Invalid("axis direction has no length".into()))?;
        let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let small = |v: [f64; 3]| v.iter().map(|x| x * x).sum::<f64>().sqrt() < 1e-9;
        let same = self.p.datum_axes.iter().find(|a| match a.def {
            AxisDef::Manual { origin: o, dir: d } => unit(d).is_some_and(|w| {
                let off = [o[0] - origin[0], o[1] - origin[1], o[2] - origin[2]];
                w[0] * u[0] + w[1] * u[1] + w[2] * u[2] > 0.0 && small(cross(w, u)) && small(cross(off, u))
            }),
            _ => false,
        });
        Ok(match same {
            Some(a) => a.id,
            None => self.p.add_datum_axis(DatumAxis::manual(name, origin, dir)),
        })
    }

    /// `count` copies along `d1` (and `d2`, if given) of the whole body, as one body.
    pub fn linear_array(&mut self, body: Option<Id>, d1: &ArrayDir, d2: Option<&ArrayDir>, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let a = s.array_axis(d1)?;
            let b = match d2 {
                Some(d) => s.array_axis(d)?,
                None => ArrayAxis::none(),
            };
            let total = u64::from(a.count) * u64::from(b.count);
            if total < 2 {
                return Err(Error::Invalid("an array needs at least 2 copies".into()));
            }
            check_total(total)?;
            let id = s.p.add_linear_array_grid(src, a, b);
            for (sfx, d) in [("", Some(d1)), ("2", d2)] {
                if let Some(d) = d {
                    dim(s, id, &format!("dx{sfx}"), &d.dx);
                    dim(s, id, &format!("dy{sfx}"), &d.dy);
                    dim(s, id, &format!("dz{sfx}"), &d.dz);
                    dim(s, id, &format!("count{sfx}"), &d.count);
                }
            }
            s.set_node_name(id, name)?;
            Ok(id)
        })
    }

    /// `count` copies of the whole body turned about `axis` (default world Z). QymCAD's step is `360/count` when
    /// `angle` ≥ 359.9°, otherwise `angle/count` (so the last copy stands at `angle·(count−1)/count`, F-027).
    pub fn circular_array(
        &mut self,
        body: Option<Id>,
        count: &Num,
        angle: &Num,
        axis: Option<&AxisRef>,
        name: Option<&str>,
    ) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let n = s.count(count)?;
            if n < 2 {
                return Err(Error::Invalid("an array needs at least 2 copies".into()));
            }
            check_total(u64::from(n))?;
            let ang = angle.eval(&s.p.param_map())?;
            if ang.is_nan() || ang <= 0.0 || ang > 360.0 {
                return Err(Error::Invalid(format!("angle must be in (0, 360], got {ang}")));
            }
            let ax = match axis {
                Some(a) => s.datum_axis_for(a)?,
                None => 0,
            };
            let id = s.p.add_circular_array_axis(src, n, ang, ax);
            dim(s, id, "count", count);
            dim(s, id, "angle", angle);
            s.set_node_name(id, name)?;
            Ok(id)
        })
    }

    /// Mirror the whole body about a base plane, a datum plane or a planar face. `keep` keeps the original too
    /// (one body holding both halves); otherwise only the image remains.
    pub fn mirror(&mut self, body: Option<Id>, plane: &PlaneRef, keep: bool, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
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
            s.set_node_name(id, name)?;
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

    /// `Project::eval_parameters`, refusing a sketch-pattern count over the cap before the native pass imposes it.
    /// The native evaluation imposes item values (pattern counts) and regenerates every instance at once, so a
    /// count raised to 100000 by a parameter edit would build 100000 copies before any later check could run.
    /// First pass: pattern count formulas are pinned to their current counts, the parameters are worked out,
    /// the formulas restored and checked against the new values; only then the real native pass runs.
    pub(crate) fn eval_parameters_capped(&mut self) -> Result<Vec<(String, qymcad_core::errors::ExprError)>> {
        let mut pinned = Vec::new();
        let mut mesh_pinned = Vec::new();
        for node in &self.p.timeline {
            let (source, keys): (Id, &[&str]) = match node.kind {
                qymcad_core::feature::FeatureKind::MeshSolid { src, .. } => (src, &["simplify"]),
                qymcad_core::feature::FeatureKind::MeshRecognised { src, .. } => (src, &["tol", "sharp", "simplify"]),
                _ => continue,
            };
            for key in keys {
                if let Some(formula) = self.p.feat_dim(node.id, key) {
                    let value = match qymcad_core::expr::eval(formula, &self.p.param_map()) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    mesh_pinned.push((node.id, source, *key, formula.to_string(), value.to_string()));
                }
            }
        }
        let set_dim = |p: &mut qymcad_core::model::Project, id: Id, key: &str, value: &str| {
            if let Some(dims) = p.feat_dims.get_mut(&id) {
                dims.insert(key.to_string(), value.to_string());
            }
        };
        for (id, _, key, _, value) in &mesh_pinned {
            set_dim(&mut self.p, *id, key, value);
        }
        for (id, _, key, formula, _) in &mesh_pinned {
            set_dim(&mut self.p, *id, key, formula);
        }
        for sketch in &self.p.sketches {
            for pattern in &sketch.patterns {
                let (count, count2) = match pattern.kind {
                    qymcad_core::model::PatternKind::Linear { count, count2, .. } => (count, Some(count2)),
                    qymcad_core::model::PatternKind::Circular { count, .. } => (count, None),
                };
                for (key, current) in [("count", Some(count)), ("count2", count2)] {
                    if let (Some(current), Some(formula)) = (current, self.p.feat_dim(pattern.id, key)) {
                        pinned.push((pattern.id, key, formula.to_string(), current.to_string()));
                    }
                }
            }
        }
        if !pinned.is_empty() {
            let set = |p: &mut qymcad_core::model::Project, id: Id, key: &str, value: &str| {
                if let Some(dims) = p.feat_dims.get_mut(&id) {
                    dims.insert(key.to_string(), value.to_string());
                }
            };
            for (id, key, _, current) in &pinned {
                set(&mut self.p, *id, key, current);
            }
            self.p.eval_parameters();
            for (id, key, formula, _) in &pinned {
                set(&mut self.p, *id, key, formula);
            }
            self.check_sketch_pattern_values().map_err(|(id, e)| Error::Invalid(format!("sketch pattern {id}: {e}")))?;
        }
        let errors = self.p.eval_parameters();
        Ok(errors)
    }

    pub(crate) fn check_sketch_pattern_values(&self) -> std::result::Result<(), (Id, Error)> {
        let vars = self.p.param_map();
        for sketch in &self.p.sketches {
            for pattern in &sketch.patterns {
                let eval_count = |key: &str, fallback: f64| -> Result<u64> {
                    let value = self
                        .p
                        .feat_dim(pattern.id, key)
                        .filter(|e| !e.trim().is_empty())
                        .map_or(Ok(fallback), |e| Num::Expr(e.to_string()).eval(&vars))?;
                    if value < if key == "count2" && fallback == 1.0 { 1.0 } else { 2.0 }
                        || value > 1000.0
                        || (value - value.round()).abs() > 1e-9
                    {
                        return Err(Error::Invalid(format!("{key} must be an integral count from 2 to 1000, got {value}")));
                    }
                    Ok(value as u64)
                };
                let total = match pattern.kind {
                    qymcad_core::model::PatternKind::Linear { count, count2, .. } => {
                        let first = eval_count("count", f64::from(count)).map_err(|e| (pattern.id, e))?;
                        let second = eval_count("count2", f64::from(count2.max(1))).map_err(|e| (pattern.id, e))?;
                        first.saturating_mul(second)
                    }
                    qymcad_core::model::PatternKind::Circular { count, .. } => {
                        eval_count("count", f64::from(count)).map_err(|e| (pattern.id, e))?
                    }
                };
                if total > MAX_ARRAY_COPIES {
                    return Err((pattern.id, Error::Invalid(format!("{total} copies; a sketch pattern makes at most {MAX_ARRAY_COPIES}"))));
                }
            }
        }
        Ok(())
    }

    fn count(&self, n: &Num) -> Result<u32> {
        let v = n.eval(&self.p.param_map())?;
        if !(1.0..=MAX_ARRAY_COPIES as f64).contains(&v) || (v - v.round()).abs() > 1e-9 {
            return Err(Error::Invalid(format!("count must be a whole number from 1 to {MAX_ARRAY_COPIES}, got {v}")));
        }
        Ok(v.round() as u32)
    }

    /// Every array's total copy count, with the current parameter values, within `MAX_ARRAY_COPIES`. Counts can
    /// be expressions (`count`, `count2`, `count3` feature dimensions), so a parameter edit can grow them;
    /// Check known values before kernel work and recheck the settled reference scope on every rebuild.
    pub(crate) fn check_array_limits(&self) -> Result<()> {
        self.check_array_values().map_err(|(id, e)| Error::Invalid(format!("array {id} `{}`: {e}", self.node_name(id))))?;
        self.check_sketch_pattern_values().map_err(|(id, e)| Error::Invalid(format!("sketch pattern {id}: {e}")))
    }

    pub(crate) fn check_array_values(&self) -> std::result::Result<(), (Id, Error)> {
        let vars = self.p.param_map();
        let dim = |node: Id, key: &str, stored: u32| -> Result<u64> {
            let v = match self.p.feat_dims.get(&node).and_then(|d| d.get(key)) {
                Some(e) if !e.trim().is_empty() => Num::Expr(e.clone()).eval(&vars)?,
                _ => f64::from(stored),
            };
            Ok(u64::from(self.count(&Num::Value(v))?))
        };
        for n in self.p.timeline.iter().filter(|n| !n.suppressed) {
            let total = match n.kind {
                FeatureKind::LinearArray { count, count2, count3, .. } => {
                    let (a, b, c) = (
                        dim(n.id, "count", count).map_err(|e| (n.id, e))?,
                        dim(n.id, "count2", count2).map_err(|e| (n.id, e))?,
                        dim(n.id, "count3", count3).map_err(|e| (n.id, e))?,
                    );
                    a.checked_mul(b).and_then(|x| x.checked_mul(c)).unwrap_or(u64::MAX)
                }
                FeatureKind::CircularArray { count, .. } => dim(n.id, "count", count).map_err(|e| (n.id, e))?,
                _ => continue,
            };
            check_total(total).map_err(|e| (n.id, e))?;
        }
        Ok(())
    }
}

fn check_total(total: u64) -> Result<()> {
    if total > MAX_ARRAY_COPIES {
        return Err(Error::Invalid(format!("{total} copies; an array makes at most {MAX_ARRAY_COPIES}")));
    }
    Ok(())
}
