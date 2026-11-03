//! Read-only native ruler over current topology. See ADR 0043 for support-geometry semantics.
use crate::{EdgeKind, Element, Error, FaceKind, Id, Result, Sel, Session};
use qymcad_core::feature::{apply12, apply12_dir};
use qymcad_core::measure::{measure_one, measure_pair, MeasureItem};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MeasureAt {
    Start,
    End,
    Mid,
    Center,
}

#[derive(Clone, Debug)]
pub enum MeasureOperand {
    Point([f64; 3]),
    Face { body: Id, selection: Sel },
    Edge { body: Id, selection: Sel, at: Option<MeasureAt> },
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct ResolvedMeasure {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<u32>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct MeasureResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle_deg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_mm: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diameter_mm: Option<f64>,
    pub resolved: Vec<ResolvedMeasure>,
    pub coordinate_frame: &'static str,
}

struct Pick {
    item: MeasureItem,
    resolved: ResolvedMeasure,
    length: Option<f64>,
    approximate: bool,
}

fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}

fn finite(p: [f64; 3]) -> Result<()> {
    if p.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(invalid("measurement coordinates and directions must be finite"))
    }
}

fn direction(d: [f64; 3]) -> Result<()> {
    finite(d)?;
    // Native normalization squares components; a finite vector can otherwise normalize to zero.
    if !d.iter().map(|v| v * v).sum::<f64>().is_finite() {
        return Err(invalid("measurement direction exceeds finite numeric range"));
    }
    if d[0].hypot(d[1]).hypot(d[2]) < 1e-12 {
        return Err(invalid("measurement direction is zero or degenerate"));
    }
    Ok(())
}

fn validate(item: &MeasureItem) -> Result<()> {
    use MeasureItem::*;
    match *item {
        Point(p) => finite(p),
        Line { origin, dir, len } => {
            finite(origin)?;
            direction(dir)?;
            if !len.is_finite() || len <= 0.0 {
                return Err(invalid("measurement edge length must be finite and positive"));
            }
            Ok(())
        }
        Plane { origin, normal } => {
            finite(origin)?;
            direction(normal)
        }
        Circle { center: origin, axis, r } | Cylinder { origin, axis, r } => {
            finite(origin)?;
            direction(axis)?;
            if !r.is_finite() || r <= 0.0 {
                return Err(invalid("measurement radius must be finite and positive"));
            }
            Ok(())
        }
    }
}

/// Classify supported native branches before calling the ruler, so its anchor fallback cannot escape.
fn pair_kind(a: &MeasureItem, b: &MeasureItem) -> Result<&'static str> {
    use MeasureItem::*;
    Ok(match (a, b) {
        (Point(_), Point(_)) => "point_point",
        (Point(_), Line { .. }) | (Line { .. }, Point(_)) => "point_support_line",
        (Line { .. }, Line { .. }) => "support_lines",
        (Point(_), Plane { .. }) | (Plane { .. }, Point(_)) => "point_support_plane",
        (Plane { .. }, Plane { .. }) | (Line { .. }, Plane { .. }) | (Plane { .. }, Line { .. }) => "support_plane_gap",
        (Point(_), Cylinder { .. }) | (Cylinder { .. }, Point(_)) => "point_cylinder_surface",
        (Point(_), Circle { .. }) | (Circle { .. }, Point(_)) => "point_full_circle",
        (Circle { .. }, Circle { .. }) => "circle_centre_distance",
        (Cylinder { .. }, Cylinder { .. }) => "signed_axis_clearance",
        _ => {
            return Err(invalid(
                "unsupported measurement pair: native fallback measures anchor distance; use edge `at` to measure from a point",
            ))
        }
    })
}

fn compute(a: Pick, b: Option<Pick>) -> Result<MeasureResult> {
    validate(&a.item)?;
    if let Some(b) = &b {
        validate(&b.item)?;
    }
    let mut out = MeasureResult {
        distance_mm: None,
        distance_kind: None,
        angle_deg: None,
        angle_kind: None,
        delta_mm: None,
        length_mm: None,
        length_kind: None,
        radius_mm: None,
        diameter_mm: None,
        resolved: vec![a.resolved],
        coordinate_frame: "world",
    };
    if let Some(b) = b {
        if a.approximate || b.approximate {
            return Err(invalid("spline/other edges cannot be paired; use edge `at` to measure from a point"));
        }
        let kind = pair_kind(&a.item, &b.item)?;
        let native = measure_pair(&a.item, &b.item);
        out.distance_mm = native.distance;
        out.distance_kind = native.distance.map(|_| kind);
        out.angle_deg = native.angle_deg;
        out.angle_kind = native.angle_deg.map(|_| "acute_unsigned");
        out.delta_mm = native.delta;
        out.resolved.push(b.resolved);
    } else {
        let native = measure_one(&a.item);
        match a.item {
            MeasureItem::Line { .. } => {
                out.length_mm = native.value.map(|(_, value)| value);
                out.length_kind = a.approximate.then_some("polyline_approximation");
            }
            MeasureItem::Circle { r, .. } | MeasureItem::Cylinder { r, .. } => {
                out.radius_mm = Some(r);
                out.diameter_mm = native.value.map(|(_, value)| value);
                out.length_mm = a.length;
            }
            _ => return Err(invalid("a point or planar face alone has no size; give a second operand")),
        }
    }
    if out
        .distance_mm
        .into_iter()
        .chain(out.angle_deg)
        .chain(out.delta_mm.into_iter().flatten())
        .chain(out.length_mm)
        .chain(out.radius_mm)
        .chain(out.diameter_mm)
        .any(|v| !v.is_finite())
    {
        return Err(invalid("measurement exceeds finite numeric range"));
    }
    Ok(out)
}

impl Session {
    /// Measure current analytic support geometry with the native ruler, in world coordinates.
    /// Caller must not hold a kernel gate. Does not create history or modify the modelling recipe.
    pub fn measure(&mut self, a: MeasureOperand, b: Option<MeasureOperand>) -> Result<MeasureResult> {
        // Reject malformed literal points before any topology restoration/native reads.
        for operand in std::iter::once(&a).chain(b.iter()) {
            if let MeasureOperand::Point(p) = operand {
                finite(*p)?;
            }
        }
        let a = self.measure_pick(a)?;
        let b = b.map(|b| self.measure_pick(b)).transpose()?;
        compute(a, b)
    }

    fn measure_pick(&mut self, operand: MeasureOperand) -> Result<Pick> {
        let (body, selection, element, at) = match operand {
            MeasureOperand::Point(p) => {
                return Ok(Pick { item: MeasureItem::Point(p), resolved: ResolvedMeasure::default(), length: None, approximate: false })
            }
            MeasureOperand::Face { body, selection } => (body, selection, Element::Faces, None),
            MeasureOperand::Edge { body, selection, at } => (body, selection, Element::Edges, at),
        };
        // Existing select may restore missing topology after open; it never owns our raw-read gate.
        if self.p.timeline.iter().take(self.p.rollback_place()).any(|n| n.dirty) || !self.p.regen_errors.is_empty() {
            return Err(invalid("measurement needs current, successfully built bodies; rebuild the document first"));
        }
        self.ensure_topology()?;
        self.source_body(Some(body))?;
        let (_, ids) = self.select(Some(body), element, &selection)?;
        let [id] = ids.as_slice() else {
            return Err(Error::Invalid(format!(
                "measurement selection must match exactly one current element of body {body}; matched {}",
                ids.len()
            )));
        };
        let t = self.topology(Some(body), false)?;
        let wt = self.p.body_world_transform(body);
        let point = |p| apply12(&wt, p);
        let dir = |d| apply12_dir(&wt, d);
        let mut pick = Pick {
            item: MeasureItem::Point([0.0; 3]),
            resolved: ResolvedMeasure { body: Some(body), ..Default::default() },
            length: None,
            approximate: false,
        };
        match element {
            Element::Faces => {
                pick.resolved.face = Some(*id);
                let f = t.faces.iter().find(|f| f.id == *id).ok_or_else(|| invalid("selected face is not current"))?;
                pick.item = match f.kind {
                    FaceKind::Plane => {
                        // Match the app's resolve_face path after topology has proved planarity/id validity.
                        let key = qymcad_core::feature::FaceKey { index: 0, centroid: f.centroid, normal: [0.0; 3], id: *id };
                        let (origin, normal) = self.p.resolve_face(body, &key);
                        MeasureItem::Plane { origin: point(origin), normal: dir(normal) }
                    }
                    FaceKind::Cylinder => {
                        let cylinder = {
                            let _gate = qymcad_kernel::kernel_gate();
                            self.shapes.get(&body).and_then(|shape| shape.face_cylinder(*id))
                        };
                        let (o, axis, r) = cylinder.ok_or_else(|| invalid("cylinder has no current native geometry"))?;
                        MeasureItem::Cylinder { origin: point(o), axis: dir(axis), r }
                    }
                    _ => {
                        return Err(invalid(
                            "only planar and cylindrical faces are supported; choose an edge with `at` to measure from a point",
                        ))
                    }
                };
            }
            Element::Edges => {
                pick.resolved.edge = Some(*id);
                let e = t.edges.iter().find(|e| e.id == *id).ok_or_else(|| invalid("selected edge is not current"))?;
                if let Some(at) = at {
                    let p = match at {
                        MeasureAt::Start => e.a,
                        MeasureAt::End => e.b,
                        MeasureAt::Mid => e.mid,
                        MeasureAt::Center => e.center.ok_or_else(|| invalid("at:center requires a circle or arc edge"))?,
                    };
                    pick.item = MeasureItem::Point(point(p));
                } else {
                    pick.length = Some(e.length);
                    pick.item = match e.kind {
                        EdgeKind::Circle | EdgeKind::Arc => {
                            let circle = {
                                let _gate = qymcad_kernel::kernel_gate();
                                self.shapes.get(&body).and_then(|shape| {
                                    let (_, ids, circles) = shape.edges_full();
                                    ids.iter().position(|candidate| candidate == id).and_then(|i| circles[i])
                                })
                            };
                            let (c, axis, r) = circle.ok_or_else(|| invalid("circle has no current native geometry"))?;
                            MeasureItem::Circle { center: point(c), axis: dir(axis), r }
                        }
                        EdgeKind::Line | EdgeKind::Other => {
                            pick.approximate = e.kind == EdgeKind::Other;
                            let (a, b) = (point(e.a), point(e.b));
                            // measure_one only reads len. A closed spline has no chord direction;
                            // retain its native midpoint tangent as a validated placeholder for length
                            // only. This never offers a spline support line: all such pairs are refused.
                            let direction = if pick.approximate {
                                let tangent = self
                                    .p
                                    .regen_edges
                                    .get(&body)
                                    .and_then(|edges| edges.iter().find(|edge| edge.id == *id))
                                    .ok_or_else(|| invalid("selected spline has no current tangent"))?
                                    .dir;
                                dir(tangent)
                            } else {
                                std::array::from_fn(|i| b[i] - a[i])
                            };
                            MeasureItem::Line { origin: a, dir: direction, len: e.length }
                        }
                    };
                }
            }
        }
        Ok(pick)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(item: MeasureItem) -> Pick {
        Pick { item, resolved: ResolvedMeasure::default(), length: None, approximate: false }
    }

    #[test]
    fn zero_directions_refuse_before_native_normalisation() {
        let mut session = Session::new_part();
        let sketch = session.sketch_create(&crate::PlaneRef::Base(crate::BaseName::XY), None).unwrap();
        session.sketch_rect(sketch, &0.0.into(), &0.0.into(), &20.0.into(), &10.0.into(), false).unwrap();
        let (body, report) = session.extrude(&serde_json::from_value(serde_json::json!({"sketch":sketch,"height":5})).unwrap()).unwrap();
        assert!(report.errors.is_empty());
        let snapshot = |s: &mut Session| {
            let topology = s.topology(Some(body), true).unwrap();
            (serde_json::to_value(s.project()).unwrap(), s.info(), topology, s.undo.len())
        };
        for item in [
            MeasureItem::Line { origin: [0.0; 3], dir: [0.0; 3], len: 1.0 },
            MeasureItem::Plane { origin: [0.0; 3], normal: [0.0; 3] },
            MeasureItem::Circle { center: [0.0; 3], axis: [0.0; 3], r: 4.0 },
            MeasureItem::Cylinder { origin: [0.0; 3], axis: [0.0; 3], r: 4.0 },
        ] {
            let before = snapshot(&mut session);
            let result = compute(pick(MeasureItem::Point([100.0, 0.0, 0.0])), Some(pick(item)));
            assert!(
                matches!(result, Err(ref e) if e.to_string().contains("zero or degenerate")),
                "must refuse before native returns a plausible zero: {result:?}"
            );
            assert_eq!(snapshot(&mut session), before);
        }
        // Positive control: a unit support line includes x=100, so perpendicular distance=0.
        let valid = compute(
            pick(MeasureItem::Point([100.0, 0.0, 0.0])),
            Some(pick(MeasureItem::Line { origin: [0.0; 3], dir: [1.0, 0.0, 0.0], len: 1.0 })),
        )
        .unwrap();
        assert_eq!(valid.distance_mm, Some(0.0));
        assert_eq!(valid.distance_kind, Some("point_support_line"));
    }

    #[test]
    fn stale_and_unbuilt_operands_refuse_without_changes() {
        let mut s = Session::new_part();
        let sk = s.sketch_create(&crate::PlaneRef::Base(crate::BaseName::XY), None).unwrap();
        s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &10.0.into(), false).unwrap();
        let (body, report) = s.extrude(&serde_json::from_value(serde_json::json!({"sketch":sk,"height":5})).unwrap()).unwrap();
        assert!(report.errors.is_empty());
        let face = MeasureOperand::Face { body, selection: Sel::Extreme { axis: crate::Axis::Z, max: true } };
        let r = s.measure(face.clone(), Some(MeasureOperand::Point([0.0, 0.0, 12.0]))).unwrap();
        assert_eq!(r.distance_mm, Some(12.0 - 5.0));
        s.p.timeline.iter_mut().find(|n| n.id == body).unwrap().dirty = true;
        let before = (serde_json::to_value(&s.p).unwrap(), s.info(), s.topology(Some(body), true).unwrap(), s.undo.len());
        assert!(s.measure(face.clone(), Some(MeasureOperand::Point([0.0; 3]))).unwrap_err().to_string().contains("current"));
        assert_eq!((serde_json::to_value(&s.p).unwrap(), s.info(), s.topology(Some(body), true).unwrap(), s.undo.len()), before);
        s.p.timeline.iter_mut().find(|n| n.id == body).unwrap().dirty = false;
        let shape = s.shapes.remove(&body).unwrap();
        let before = (serde_json::to_value(&s.p).unwrap(), s.info(), s.topology(Some(body), true), s.undo.len());
        assert!(s.measure(face, Some(MeasureOperand::Point([0.0; 3]))).unwrap_err().to_string().contains("not built"));
        assert_eq!((serde_json::to_value(&s.p).unwrap(), s.info(), s.topology(Some(body), true), s.undo.len()), before);
        s.shapes.insert(body, shape);
    }

    #[test]
    fn degenerate_current_face_refuses_through_session() {
        let mut s = Session::new_part();
        let sk = s.sketch_create(&crate::PlaneRef::Base(crate::BaseName::XY), None).unwrap();
        s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &10.0.into(), false).unwrap();
        let (body, report) = s.extrude(&serde_json::from_value(serde_json::json!({"sketch":sk,"height":5})).unwrap()).unwrap();
        assert!(report.errors.is_empty());
        let top = s.topology(Some(body), false).unwrap().faces.into_iter().find(|f| f.normal == Some([0.0, 0.0, 1.0])).unwrap().id;
        let operand = || MeasureOperand::Face { body, selection: Sel::Ids(vec![top]) };
        // Positive control: the native plane at z5 is |12-5|=7 away.
        assert_eq!(s.measure(MeasureOperand::Point([0.0, 0.0, 12.0]), Some(operand())).unwrap().distance_mm, Some(7.0));
        // Simulate a malformed restored native face pool; topology still proves its actual mesh planar.
        // The app's resolve_face returns this zero normal, which native normalisation would hide.
        s.p.regen_faces.get_mut(&body).unwrap().iter_mut().find(|f| f.id == top).unwrap().normal = [0.0; 3];
        let snapshot =
            |s: &mut Session| (serde_json::to_value(&s.p).unwrap(), s.info(), s.topology(Some(body), true).unwrap(), s.undo.len());
        let before = snapshot(&mut s);
        let result = s.measure(MeasureOperand::Point([0.0, 0.0, 12.0]), Some(operand()));
        assert!(
            matches!(result, Err(ref e) if e.to_string().contains("zero or degenerate")),
            "must refuse instead of native zero: {result:?}"
        );
        assert_eq!(snapshot(&mut s), before);
    }
}
