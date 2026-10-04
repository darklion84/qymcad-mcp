//! Sketches: planar profiles. Every entity added here is fully dimensioned (size and position from the sketch
//! origin), with the given expressions as the driving values, so the sketch stays fully defined and editable
//! in the QymCAD GUI.

use crate::error::{Error, Result};
use crate::session::Session;
use crate::value::Num;
use qymcad_core::feature::{BasePlane, FaceKey, Purpose, SketchPlane};
use qymcad_core::model::{Constraint, EntityKind, Id, SketchPoint};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where a sketch (or an offset plane) sits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaneRef {
    /// A base plane: `XY` (normal +Z), `XZ` (normal −Y), `YZ` (normal +X).
    Base(BaseName),
    /// A datum plane by id (from `plane_offset`).
    Plane(Id),
    /// A planar face of a body (face id from `topology`).
    Face { body: Id, face: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum BaseName {
    XY,
    XZ,
    YZ,
}

impl From<BaseName> for BasePlane {
    fn from(b: BaseName) -> Self {
        match b {
            BaseName::XY => BasePlane::XY,
            BaseName::XZ => BasePlane::XZ,
            BaseName::YZ => BasePlane::YZ,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ContourInfo {
    pub id: Id,
    /// The enclosing contour, if any. Extruding a contour extrudes it minus its direct children (holes).
    pub parent: Option<Id>,
    /// mm²
    pub area: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SketchInfo {
    pub id: Id,
    pub name: String,
    pub plane: String,
    /// Remaining degrees of freedom (0 = fully defined) and redundant constraints.
    pub dof: (i32, i32),
    pub contours: Vec<ContourInfo>,
}

impl Session {
    /// Create an empty sketch on `plane`. Returns the sketch id (also its timeline node id).
    pub fn sketch_create(&mut self, plane: &PlaneRef, name: Option<&str>) -> Result<Id> {
        let sp = self.sketch_plane(plane)?;
        let n = self.p.sketches.len() + 1;
        let name = name.map(str::to_string).unwrap_or_else(|| format!("Sketch {n}"));
        let sid = self.p.add_sketch(name.clone(), vec![], None);
        self.p.add_sketch_node(sid, name);
        let si = self.sketch_si(sid)?;
        self.p.sketches[si].plane = sp;
        Ok(sid)
    }

    /// Add a rectangle centred at (`cx`, `cy`) of size `w` × `h` in sketch coordinates. Returns its four line
    /// ids (bottom, right, top, left).
    pub fn sketch_rect(&mut self, sketch: Id, cx: &Num, cy: &Num, w: &Num, h: &Num, construction: bool) -> Result<Vec<Id>> {
        self.transact(|s| s.sketch_rect_inner(sketch, cx, cy, w, h, construction))
    }

    fn sketch_rect_inner(&mut self, sketch: Id, cx: &Num, cy: &Num, w: &Num, h: &Num, construction: bool) -> Result<Vec<Id>> {
        let si = self.sketch_si(sketch)?;
        let vars = self.p.param_map();
        let (vx, vy, vw, vh) = (cx.eval(&vars)?, cy.eval(&vars)?, w.eval(&vars)?, h.eval(&vars)?);
        if vw <= 0.0 || vh <= 0.0 {
            return Err(Error::Invalid(format!("rectangle size must be positive, got {vw} × {vh}")));
        }
        let lines = self.p.add_rect_entity(si, vx - vw / 2.0, vy - vh / 2.0, vx + vw / 2.0, vy + vh / 2.0, purpose(construction));
        let [bl, br, tr, tl] = self.rect_corners(si, &lines)?;
        let s = &mut self.p.sketches[si];
        s.constraints.push(dist(br, bl, 1, vw, w.magnitude_expr(vw)));
        s.constraints.push(dist(tl, bl, 2, vh, h.magnitude_expr(vh)));
        let c = self.p.alloc_id();
        let s = &mut self.p.sketches[si];
        s.points.push(SketchPoint { id: c, x: vx, y: vy });
        s.constraints.push(Constraint::Midpoint { p: c, a: bl, b: tr });
        self.pin_point(si, c, cx, vx, cy, vy);
        self.finish_sketch_edit(si)?;
        Ok(lines)
    }

    /// Add a circle centred at (`cx`, `cy`) with diameter `d`. Returns the circle entity id.
    pub fn sketch_circle(&mut self, sketch: Id, cx: &Num, cy: &Num, d: &Num, construction: bool) -> Result<Id> {
        self.transact(|s| s.sketch_circle_inner(sketch, cx, cy, d, construction))
    }

    fn sketch_circle_inner(&mut self, sketch: Id, cx: &Num, cy: &Num, d: &Num, construction: bool) -> Result<Id> {
        let si = self.sketch_si(sketch)?;
        let vars = self.p.param_map();
        let (vx, vy, vd) = (cx.eval(&vars)?, cy.eval(&vars)?, d.eval(&vars)?);
        if vd <= 0.0 {
            return Err(Error::Invalid(format!("circle diameter must be positive, got {vd}")));
        }
        let eid = self.p.add_circle_entity(si, vx, vy, vd / 2.0, purpose(construction));
        let center = self.p.sketches[si]
            .entities
            .iter()
            .find_map(|e| match e.kind {
                EntityKind::Circle { center, .. } if e.id == eid => Some(center),
                _ => None,
            })
            .ok_or_else(|| Error::Invalid("circle was not created".into()))?;
        let idx = self.p.ensure_diameter(si, center, true).ok_or_else(|| Error::Invalid("cannot dimension the circle".into()))?;
        if let Constraint::Diameter { expr, d: dd, .. } = &mut self.p.sketches[si].constraints[idx] {
            *expr = d.magnitude_expr(vd);
            *dd = vd;
        }
        self.pin_point(si, center, cx, vx, cy, vy);
        self.finish_sketch_edit(si)?;
        Ok(eid)
    }

    pub fn sketch_info(&self, sketch: Id) -> Result<SketchInfo> {
        let si = self.sketch_si(sketch)?;
        let s = &self.p.sketches[si];
        let contours = s
            .contour_ids
            .iter()
            .map(|&cid| ContourInfo {
                id: cid,
                parent: self.p.contours.parent_of(cid).filter(|&x| x != 0),
                area: self.p.contours.index_of(cid).map(|i| self.p.contours[i].signed_area().abs()).unwrap_or(0.0),
            })
            .collect();
        Ok(SketchInfo { id: s.id, name: s.name.clone(), plane: plane_desc(&s.plane), dof: self.p.sketch_dof(si), contours })
    }

    pub fn sketches(&self) -> Vec<SketchInfo> {
        self.p.sketches.iter().filter_map(|s| self.sketch_info(s.id).ok()).collect()
    }

    /// Contours that are not inside another: extruding them gives each region minus its holes.
    pub(crate) fn root_contours(&self, sketch: Id) -> Result<Vec<Id>> {
        let si = self.sketch_si(sketch)?;
        Ok(self.p.sketches[si].contour_ids.iter().copied().filter(|&c| self.p.contours.parent_of(c).is_none_or(|x| x == 0)).collect())
    }

    pub(crate) fn sketch_si(&self, sketch: Id) -> Result<usize> {
        self.p.sketch_index(sketch).ok_or_else(|| Error::NotFound(format!("sketch {sketch}")))
    }

    fn sketch_plane(&self, plane: &PlaneRef) -> Result<SketchPlane> {
        Ok(match plane {
            PlaneRef::Base(b) => SketchPlane::World((*b).into()),
            PlaneRef::Plane(id) => {
                if !self.p.planes.iter().any(|w| w.id == *id) {
                    return Err(Error::NotFound(format!("datum plane {id}")));
                }
                SketchPlane::Datum(*id)
            }
            PlaneRef::Face { body, face } => {
                let f = self
                    .p
                    .regen_faces
                    .get(body)
                    .and_then(|fs| fs.iter().find(|f| f.id == *face))
                    .ok_or_else(|| Error::NotFound(format!("face {face} of body {body} (re-read topology after each rebuild)")))?;
                SketchPlane::Face(
                    *body,
                    FaceKey { index: 0, centroid: [f.centroid.x, f.centroid.y, f.centroid.z], normal: f.normal, id: f.id },
                )
            }
        })
    }

    /// The four distinct corner points of a rectangle, classified by position: [bl, br, tr, tl].
    fn rect_corners(&self, si: usize, lines: &[Id]) -> Result<[Id; 4]> {
        let s = &self.p.sketches[si];
        let mut ids: Vec<Id> = Vec::new();
        for e in s.entities.iter().filter(|e| lines.contains(&e.id)) {
            if let EntityKind::Line { a, b } = e.kind {
                for p in [a, b] {
                    if !ids.contains(&p) {
                        ids.push(p);
                    }
                }
            }
        }
        let pts: Vec<(Id, f64, f64)> =
            ids.iter().filter_map(|id| s.points.iter().find(|p| p.id == *id).map(|p| (p.id, p.x, p.y))).collect();
        if pts.len() != 4 {
            return Err(Error::Invalid(format!("rectangle has {} corners", pts.len())));
        }
        let (mx, my) = (pts.iter().map(|p| p.1).sum::<f64>() / 4.0, pts.iter().map(|p| p.2).sum::<f64>() / 4.0);
        let pick = |left: bool, low: bool| pts.iter().find(|p| (p.1 < mx) == left && (p.2 < my) == low).map(|p| p.0);
        match (pick(true, true), pick(false, true), pick(false, false), pick(true, false)) {
            (Some(bl), Some(br), Some(tr), Some(tl)) => Ok([bl, br, tr, tl]),
            _ => Err(Error::Invalid("cannot classify rectangle corners".into())),
        }
    }

    /// Fix point `p` at (`x`, `y`) from the sketch origin with driving dimensions. A plain zero puts the point
    /// on the axis instead (a zero distance has no side). Distances are magnitudes `|Δ|`: the side comes from
    /// the initial geometry and the expression is negated when its value is negative (see `Num::magnitude_expr`).
    fn pin_point(&mut self, si: usize, p: Id, x: &Num, vx: f64, y: &Num, vy: f64) {
        let origin = self.p.ensure_origin(si);
        let x_on_axis = x.expr().is_none() && vx == 0.0;
        let y_on_axis = y.expr().is_none() && vy == 0.0;
        let yaxis = if x_on_axis { Some(self.p.ensure_axis(si, 1)) } else { None };
        let xaxis = if y_on_axis { Some(self.p.ensure_axis(si, 0)) } else { None };
        let s = &mut self.p.sketches[si];
        match yaxis {
            Some((a, b)) => s.constraints.push(Constraint::PointOnLine { p, a, b }),
            None => s.constraints.push(dist(p, origin, 1, vx.abs(), x.magnitude_expr(vx))),
        }
        match xaxis {
            Some((a, b)) => s.constraints.push(Constraint::PointOnLine { p, a, b }),
            None => s.constraints.push(dist(p, origin, 2, vy.abs(), y.magnitude_expr(vy))),
        }
    }

    fn finish_sketch_edit(&mut self, si: usize) -> Result<()> {
        self.p.eval_parameters();
        let residual = self.p.solve_sketch(si);
        let sid = self.p.sketches[si].id;
        self.p.mark_sketch_dirty(sid);
        if residual > 1e-6 {
            return Err(Error::Invalid(format!("sketch {sid} does not solve (residual {residual:.3e}): conflicting dimensions")));
        }
        Ok(())
    }
}

fn dist(a: Id, b: Id, axis: u8, d: f64, expr: String) -> Constraint {
    Constraint::Distance { a, b, d, off: 0.0, expr, driven: false, axis, at: None }
}

fn purpose(construction: bool) -> Purpose {
    if construction {
        Purpose::Construction
    } else {
        Purpose::Real
    }
}

fn plane_desc(p: &SketchPlane) -> String {
    match p {
        SketchPlane::World(b) => format!("{b:?}"),
        SketchPlane::Datum(id) => format!("plane {id}"),
        SketchPlane::Face(body, k) => format!("face {} of body {body}", k.id),
    }
}
