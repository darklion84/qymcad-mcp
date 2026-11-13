//! Primitives: the app's six commands `part.box|cylinder|sphere|cone|torus|prism` (ADR 0056).
//!
//! One action in the app: the primitive node, the sizes' formulas, an optional `Move` to the clicked vertex or datum point,
//! and a union with the single body of the Part (`apply_prim_cmd`). The GUI's field limits hold at creation and on every
//! later edit; the core itself checks only `> 1e-9` (probes j0).

use crate::datum_point::VertexEnd;
use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::value::Num;
use qymcad_core::feature::{FeatureKind, PLACE_IDENTITY};
use qymcad_core::model::{FeatNumber, Id, PlainNumber};
use serde::Serialize;
use std::collections::BTreeMap;

/// The field's upper bound for every length (`start.rs`).
pub(crate) const PRIMITIVE_MAX: f64 = 1.0e5;
/// The torus's tube may not come closer to the ring's radius than this (`sync_torus_params`).
const TORUS_GAP: f64 = 0.001;
/// Largest radius difference at which the app's cone fails with `OpFailed(Cone)`: `10` against `10 - d` fails at d = 1e-7 and builds at
/// d = 1.01e-7 (probes `p5_cone_radii`, `p5b_cone_equal_radii_threshold`), so everything the core refuses is refused here first, and
/// nothing it builds is.
const CONE_EQUAL: f64 = 1.0e-7;
/// The app's number field accepts a value within this of its bounds and a whole number within this of an integer
/// (`tool_frame/mod.rs`, `field_value`).
const FIELD_TOL: f64 = 1.0e-9;

/// A primitive's sizes: each a number or an expression, stored on the node's `feat_dims` keys (the field names).
#[derive(Clone, Debug, PartialEq)]
pub enum PrimitiveSpec {
    Box { dx: Num, dy: Num, dz: Num },
    Cylinder { r: Num, h: Num },
    Sphere { r: Num },
    Cone { r1: Num, r2: Num, h: Num },
    Torus { major: Num, minor: Num },
    Prism { r: Num, h: Num, n: Num },
}

impl PrimitiveSpec {
    fn kind(&self) -> &'static str {
        match self {
            PrimitiveSpec::Box { .. } => "box",
            PrimitiveSpec::Cylinder { .. } => "cylinder",
            PrimitiveSpec::Sphere { .. } => "sphere",
            PrimitiveSpec::Cone { .. } => "cone",
            PrimitiveSpec::Torus { .. } => "torus",
            PrimitiveSpec::Prism { .. } => "prism",
        }
    }

    /// The sizes by key, in the order of `FeatureKind::dims`.
    fn fields(&self) -> Vec<(&'static str, &Num)> {
        match self {
            PrimitiveSpec::Box { dx, dy, dz } => vec![("dx", dx), ("dy", dy), ("dz", dz)],
            PrimitiveSpec::Cylinder { r, h } => vec![("r", r), ("h", h)],
            PrimitiveSpec::Sphere { r } => vec![("r", r)],
            PrimitiveSpec::Cone { r1, r2, h } => vec![("r1", r1), ("r2", r2), ("h", h)],
            PrimitiveSpec::Torus { major, minor } => vec![("major", major), ("minor", minor)],
            PrimitiveSpec::Prism { r, h, n } => vec![("r", r), ("n", n), ("h", h)],
        }
    }
}

/// Where the primitive stands. As in the app a click gives a pure translation and no association: the point or vertex does not
/// lead the primitive afterwards.
#[derive(Clone, Debug, PartialEq)]
pub enum Placement {
    /// On the Part's origin: box, cylinder, cone and prism on z = 0 centred on XY, sphere and torus centred on the origin.
    Origin,
    /// At a datum point of the active Part.
    Point(Id),
    /// At an end of a persistent edge of a current body of the active Part.
    Vertex { edge: u32, end: VertexEnd, body: Option<Id> },
}

/// What the union with the Part's body did.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Joined {
    /// The Part's body the primitive was united with.
    pub prior: Id,
    /// The union node, which is the resulting body.
    pub union: Id,
}

/// A primitive as it stands after the rebuild.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PrimitiveInfo {
    /// The primitive's node: the target of `dimension_set` (keys are the size names).
    pub feature: Id,
    /// The Part's body after the action: the primitive itself (first, unplaced), its placement `Move` (first, placed), or the union
    /// node (the Part already had a body).
    pub body: Id,
    pub kind: &'static str,
    pub name: String,
    /// Evaluated sizes after the rebuild.
    pub sizes: BTreeMap<&'static str, f64>,
    /// The translation applied (omitted for the origin).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placed_at: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joined: Option<Joined>,
}

/// The kind's name and its own `FeatureKind` keys, for the six primitive kinds.
fn primitive_kind(kind: &FeatureKind) -> Option<&'static str> {
    Some(match kind {
        FeatureKind::Box3 { .. } => "box",
        FeatureKind::Cylinder { .. } => "cylinder",
        FeatureKind::Sphere { .. } => "sphere",
        FeatureKind::Cone { .. } => "cone",
        FeatureKind::Torus { .. } => "torus",
        FeatureKind::Prism { .. } => "prism",
        _ => return None,
    })
}

/// The field's lower bound. The app's creation field and its edit field differ for the cone's first radius only.
fn lower(kind: &str, key: &str, creating: bool) -> f64 {
    match (kind, key) {
        ("box", _) | ("cylinder", "h") | ("cone", "h") | ("prism", "h") | ("torus", _) | ("prism", "r") => 0.1,
        ("cylinder", "r") | ("sphere", "r") => 0.05,
        ("cone", "r1") if creating => 0.01,
        _ => 0.0,
    }
}

/// One way the evaluated sizes leave the app's fields. `keys` are the sizes it concerns: an exemption of a file that was already
/// outside the fields holds only when every one of them still has the value it had then.
pub(crate) struct Fault {
    pub keys: Vec<&'static str>,
    pub error: Error,
}

fn fault(keys: &[&'static str], message: String) -> Fault {
    Fault { keys: keys.to_vec(), error: Error::Invalid(message) }
}

/// Judge evaluated sizes against the app's fields, with the field's own tolerance (`lo - 1e-9 ..= hi + 1e-9`, `n` within 1e-9 of
/// a whole number, then rounded). `value` gives a size by key, `None` for one that does not evaluate (judged elsewhere; a check that
/// needs it is skipped).
fn judge(kind: &str, value: &dyn Fn(&str) -> Option<f64>, creating: bool) -> Vec<Fault> {
    let keys: &[&'static str] = match kind {
        "box" => &["dx", "dy", "dz"],
        "cylinder" => &["r", "h"],
        "sphere" => &["r"],
        "cone" => &["r1", "r2", "h"],
        "torus" => &["major", "minor"],
        _ => &["r", "h"],
    };
    let mut faults = Vec::new();
    for key in keys {
        let Some(v) = value(key) else { continue };
        let lo = lower(kind, key, creating);
        // The app's torus field: `(major - 0.001).max(minor.lo)` (`sync_torus_params`): the upper bound of `minor` is a pair
        // `(minor, major)`, so its fault concerns both sizes (as the cone's radii).
        let torus_minor = kind == "torus" && *key == "minor";
        let major = if torus_minor { value("major") } else { None };
        let hi = match major {
            Some(major) => (major - TORUS_GAP).max(lo).min(PRIMITIVE_MAX),
            None => PRIMITIVE_MAX,
        };
        if v < lo - FIELD_TOL {
            faults.push(fault(
                &[key],
                format!("{kind} {key} must be within {lo}..={hi} mm, as the app's field requires (below {lo} the core builds silently wrong bodies, F-180), got {v}"),
            ));
        } else if v > hi + FIELD_TOL && (!torus_minor || major.is_some()) {
            let why = if torus_minor {
                "the tube stays 0.001 mm inside the ring's radius `major`: the app's field limit"
            } else {
                "the app's field limit; the core would build it, this server keeps the app's field"
            };
            let keys: &[&'static str] = if torus_minor { &["major", "minor"] } else { &[key] };
            faults.push(fault(keys, format!("{kind} {key} must be within {lo}..={hi} mm, {why}, got {v}")));
        }
    }
    match kind {
        "prism" => {
            if let Some(n) = value("n").filter(|n| (n - n.round()).abs() > FIELD_TOL || !(3.0..=64.0).contains(&n.round())) {
                faults.push(fault(
                    &["n"],
                    format!("prism n must be a whole number of sides in 3..=64, got {n} (the app silently rounds it and takes at least 3, F-181)"),
                ));
            }
        }
        "cone" => {
            if let Some((r1, _)) = value("r1").zip(value("r2")).filter(|(r1, r2)| (r1 - r2).abs() <= CONE_EQUAL) {
                faults.push(fault(
                    &["r1", "r2"],
                    format!("cone r1 and r2 are equal ({r1}): the app's cone fails there (OpFailed); use a cylinder (F-182)"),
                ));
            }
        }
        _ => {}
    }
    faults
}

impl Session {
    /// Create a primitive, as one app action: the node, its formulas, the placement move and the union with the Part's body.
    pub fn primitive(&mut self, spec: &PrimitiveSpec, place: &Placement, name: Option<&str>) -> Result<(PrimitiveInfo, Rebuild)> {
        self.refuse_primitive_outside_a_part()?;
        if let Placement::Vertex { .. } = place {
            self.ensure_topology()?;
        }
        let (result, report) = self.atomic(|s| {
            let vars = s.p.param_map();
            let mut values: BTreeMap<&'static str, f64> = BTreeMap::new();
            for (key, input) in spec.fields() {
                values.insert(key, input.eval(&vars).map_err(|e| Error::Expr(format!("{} {key}: {e}", spec.kind())))?);
            }
            if let Some(f) = judge(spec.kind(), &|key| Some(values[key]), true).into_iter().next() {
                return Err(f.error);
            }
            let at = s.placement_point(place)?;
            let v = |key: &str| values[key];
            let id = match spec {
                PrimitiveSpec::Box { .. } => s.p.add_box(v("dx"), v("dy"), v("dz")),
                PrimitiveSpec::Cylinder { .. } => s.p.add_cylinder(v("r"), v("h")),
                PrimitiveSpec::Sphere { .. } => s.p.add_sphere(v("r")),
                PrimitiveSpec::Cone { .. } => s.p.add_cone(v("r1"), v("r2"), v("h")),
                PrimitiveSpec::Torus { .. } => s.p.add_torus(v("major"), v("minor")),
                PrimitiveSpec::Prism { .. } => s.p.add_prism(v("r"), v("n").round() as u32, v("h")),
            };
            // The app's `store_cmd_exprs`: formulas stand on the node, a plain number clears the key.
            for (key, input) in spec.fields() {
                if let Some(text) = input.expr().filter(|e| e.parse::<f64>().is_err()) {
                    s.p.write_feat_formula(&FeatNumber { node: id, key }, &text, PlainNumber::ClearsFormula)
                        .map_err(|e| Error::Expr(format!("{} {key}: {e:?}", spec.kind())))?;
                }
            }
            s.set_node_name(id, name)?;
            let moved = at.map(|at| {
                let mut m = PLACE_IDENTITY;
                (m[3], m[7], m[11]) = (at[0], at[1], at[2]);
                m
            });
            let placed = match moved {
                Some(m) if m != PLACE_IDENTITY => s.p.add_move(id, m),
                _ => id,
            };
            let body = s.p.finish_base_body(placed, 1);
            // F-185: the app unites with the Part's last body even when it is suppressed or in a suppressed chain, and the result is
            // silently empty. A body that is unbuilt for another reason (a mesh piece without a B-rep) is left to the app, which
            // reports `Body A was not built` itself.
            if body != placed {
                if let Some(FeatureKind::BodyBoolean { a, .. }) = s.p.timeline.iter().find(|n| n.id == body).map(|n| &n.kind) {
                    if crate::session::suppression_unbuilt(&s.p, None).contains(a) {
                        return Err(Error::Invalid(format!(
                            "the Part's last body {a} is not built because it or a feature it depends on is suppressed: the app would unite the primitive with it and give a silently empty result (F-185); unsuppress it first"
                        )));
                    }
                }
            }
            Ok((id, body, moved.filter(|m| *m != PLACE_IDENTITY).map(|m| [m[3], m[7], m[11]])))
        })?;
        let (feature, body, placed_at) = result;
        let joined = self.p.timeline.iter().find(|n| n.id == body).and_then(|n| match n.kind {
            FeatureKind::BodyBoolean { a, .. } if body != feature => Some(Joined { prior: a, union: body }),
            _ => None,
        });
        let info = PrimitiveInfo {
            feature,
            body,
            kind: spec.kind(),
            name: self.node_name(feature),
            sizes: self.primitive_sizes(feature),
            placed_at,
            joined,
        };
        Ok((info, report))
    }

    /// A primitive's numbers as the rebuild takes them: the node's expression when there is one, else the stored number.
    pub(crate) fn primitive_sizes(&self, node: Id) -> BTreeMap<&'static str, f64> {
        let vars = self.p.param_map();
        let Some(kind) = self.p.timeline.iter().find(|n| n.id == node).map(|n| &n.kind) else { return BTreeMap::new() };
        kind.dims()
            .into_iter()
            .map(|(key, seed)| {
                let value = self
                    .p
                    .feat_dim(node, key)
                    .filter(|e| !e.is_empty())
                    .and_then(|e| Num::Expr(e.to_string()).eval(&vars).ok())
                    .unwrap_or(seed);
                (key, value)
            })
            .collect()
    }

    /// Primitives, like datums, are made in a Part: the app would create or reuse a Part when the assembly (root or nested) is
    /// the active context (`body_parent`), which this server does not do silently.
    fn refuse_primitive_outside_a_part(&self) -> Result<()> {
        let ctx = self.p.active_component.unwrap_or(self.p.root);
        if !self.p.ctx_holds_bodies(ctx) {
            return Err(Error::Invalid(
                "the active context is an assembly, not a Part; the app would create a Part for the primitive, which is not covered here (part_select a Part first)".into(),
            ));
        }
        Ok(())
    }

    /// The translation a placement asks for, in the Part's frame.
    fn placement_point(&self, place: &Placement) -> Result<Option<[f64; 3]>> {
        match place {
            Placement::Origin => Ok(None),
            Placement::Point(id) => {
                let point = self
                    .p
                    .datum_points
                    .iter()
                    .find(|p| p.id == *id)
                    .ok_or_else(|| Error::NotFound(format!("datum point {id} (make one with datum_point)")))?;
                let at_index = self.p.timeline.iter().position(|n| n.id == *id);
                if at_index.is_none_or(|i| i >= self.p.rollback_place()) {
                    return Err(Error::Invalid(format!(
                        "datum point {id} is not built: it lies beyond the rollback bar, so the app does not have it in the active history (move the bar past it first)"
                    )));
                }
                let owner = self.p.timeline.iter().find(|n| n.id == *id).and_then(|n| n.parent);
                if self.has_assembly_structure() && owner != self.p.active_component {
                    return Err(Error::Invalid(format!("datum point {id} belongs to another Part; the primitive would take its coordinates in the wrong frame (part_select first)")));
                }
                Ok(Some(point.at))
            }
            Placement::Vertex { edge, end, body } => {
                let body = self.current_part_body(*body, "vertex", true)?;
                let found = self.p.regen_edges.get(&body).and_then(|edges| edges.iter().find(|e| e.id == *edge));
                let found =
                    found.ok_or_else(|| Error::NotFound(format!("edge {edge} of body {body} (re-read topology after each feature)")))?;
                let point = if *end == VertexEnd::End { found.b } else { found.a };
                // The app's vertex pick (`pick_vertex_pos`) skips closed and smooth edges, but it picks a POINT: the end of a smooth
                // edge is clickable when an edge the click does reach (not closed, not smooth) ends at the same point. So the
                // placement is judged by the point, not by the edge it was named by.
                let skipped = self.shapes.get(&body).map(|shape| {
                    let _gate = qymcad_kernel::kernel_gate();
                    // Per edge id: why the click skips it (`None`: it is reachable).
                    let skip: BTreeMap<u32, Option<&'static str>> = shape
                        .edges_info()
                        .into_iter()
                        .map(|e| {
                            let closed = e.poly.first().zip(e.poly.last()).is_none_or(|(f, l)| (0..3).all(|i| (f[i] - l[i]).abs() < 1e-5));
                            let why = if closed {
                                Some("closed (a circle's or a seam's single vertex)")
                            } else if e.smooth {
                                Some("a smooth, tangent junction of faces")
                            } else {
                                None
                            };
                            (e.id, why)
                        })
                        .collect();
                    let near = |q: &[f64; 3]| (0..3).all(|i| (q[i] - point[i]).abs() < 1e-5 * point[i].abs().max(1.0));
                    let reached = self.p.regen_edges.get(&body).is_some_and(|edges| {
                        edges.iter().any(|e| skip.get(&e.id).is_some_and(|why| why.is_none()) && (near(&e.a) || near(&e.b)))
                    });
                    (skip.get(edge).copied().flatten(), reached)
                });
                if let Some((Some(why), false)) = skipped {
                    return Err(Error::Invalid(format!(
                        "edge {edge} of body {body} is {why}: the app's vertex pick skips such edges, and no edge it does reach ends at this point, so no click gives it as a vertex"
                    )));
                }
                Ok(Some(point))
            }
        }
    }

    /// A primitive size as the rebuild takes it: the node's expression when there is one, else the stored number. `None`: the
    /// expression does not evaluate.
    fn primitive_value(&self, node: Id, key: &str, seed: f64) -> Option<f64> {
        match self.p.feat_dim(node, key).filter(|e| !e.is_empty()) {
            Some(text) => Num::Expr(text.to_string()).eval(&self.p.param_map()).ok(),
            None => Some(seed),
        }
    }

    /// The ways the primitives' evaluated sizes are outside the app's fields (edit limits: the cone's first radius may be 0), as
    /// `(node, fault)`. Suppressed and rolled-back primitives are judged too: conservative, they still hold their formulas.
    pub(crate) fn primitive_faults(&self) -> Vec<(Id, Fault)> {
        let vars = self.p.param_map();
        let mut faults = Vec::new();
        for node in &self.p.timeline {
            let Some(kind) = primitive_kind(&node.kind) else { continue };
            // A size that does not evaluate is a fault of its own (an exempt one stays exempt); it neither stops the judging of the
            // node's other sizes nor lets them pass unjudged.
            let mut values: BTreeMap<&'static str, f64> = BTreeMap::new();
            for (key, seed) in node.kind.dims() {
                match self.p.feat_dim(node.id, key).filter(|e| !e.is_empty()) {
                    Some(text) => match Num::Expr(text.to_string()).eval(&vars) {
                        Ok(v) => {
                            values.insert(key, v);
                        }
                        Err(error) => faults.push((node.id, Fault { keys: vec![key], error })),
                    },
                    None => {
                        values.insert(key, seed);
                    }
                }
            }
            faults.extend(judge(kind, &|key| values.get(key).copied(), false).into_iter().map(|f| (node.id, f)));
        }
        faults
    }

    /// A refusal's text for the node it concerns: `name (id): reason`, the reason without its own `invalid request:` prefix.
    pub(crate) fn node_refusal(&self, node: Id, error: &Error) -> String {
        let reason = match error {
            Error::Invalid(message) => message.clone(),
            other => other.to_string(),
        };
        format!("{} ({node}): {reason}", self.node_name(node))
    }

    /// Record the faults of a file as it was opened: each size involved with the value it had. The exemption is per size (a fault
    /// stays tolerated while its sizes are unchanged), never per node.
    pub(crate) fn exempt_primitive_fault(&mut self, node: Id, fault: &Fault) {
        let Some(kind) = self.p.timeline.iter().find(|n| n.id == node).map(|n| n.kind.clone()) else { return };
        for key in &fault.keys {
            if let Some((key, seed)) = kind.dims().into_iter().find(|(k, _)| k == key) {
                let value = self.primitive_value(node, key, seed);
                self.open_primitive_faults.insert((node, key), value);
            }
        }
    }

    /// Is this fault the one the file already had: every size it concerns still has the value it had when the file was opened.
    fn primitive_fault_exempt(&self, node: Id, fault: &Fault) -> bool {
        let Some(kind) = self.p.timeline.iter().find(|n| n.id == node).map(|n| &n.kind) else { return false };
        !fault.keys.is_empty()
            && fault.keys.iter().all(|key| {
                let seed = kind.dims().into_iter().find(|(k, _)| k == key).map(|(_, seed)| seed);
                seed.is_some_and(|seed| self.open_primitive_faults.get(&(node, *key)) == Some(&self.primitive_value(node, key, seed)))
            })
    }

    /// Drop the exemptions that no longer apply: a size changed (an edit that leaves it faulty was refused, so it is back in range)
    /// or its fault is gone, so a later fault on it is a new one.
    pub(crate) fn prune_primitive_exemptions(&mut self) {
        if self.open_primitive_faults.is_empty() {
            return;
        }
        let standing: std::collections::HashSet<(Id, &'static str)> = self
            .primitive_faults()
            .into_iter()
            .filter(|(node, f)| self.primitive_fault_exempt(*node, f))
            .flat_map(|(node, f)| f.keys.into_iter().map(move |k| (node, k)))
            .collect();
        self.open_primitive_faults.retain(|key, _| standing.contains(key));
    }

    /// Carry the exemptions of copied nodes to their copies (`part_copy`): the copy has the same sizes, so the same faults.
    pub(crate) fn copy_open_faults(&mut self, nodes: &[(Id, Id)]) {
        for (from, to) in nodes {
            if self.open_node_faults.contains(from) {
                self.open_node_faults.insert(*to);
            }
            let carried: Vec<_> =
                self.open_primitive_faults.iter().filter(|((node, _), _)| node == from).map(|((_, key), v)| (*key, *v)).collect();
            for (key, value) in carried {
                self.open_primitive_faults.insert((*to, key), value);
            }
        }
    }

    /// The first primitive fault that is new: a size that was already outside the fields when the file was opened (the app's
    /// properties panel has no limits) is reported then and stays tolerated while it keeps its value; every other size of the node,
    /// and a changed one, is judged as usual.
    pub(crate) fn check_primitive_nodes(&self) -> std::result::Result<(), (Id, Error)> {
        match self.primitive_faults().into_iter().find(|(node, f)| !self.primitive_fault_exempt(*node, f)) {
            Some((node, f)) => Err((node, f.error)),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DimensionTarget;

    fn dim(feature: Id, key: &str) -> DimensionTarget {
        DimensionTarget::Feature { feature, key: key.into() }
    }

    /// The app's properties panel has no field limits: write `text` onto a size as the panel does, rebuild, save and open the file.
    fn file_with(s: &mut Session, feature: Id, key: &str, text: &str, tag: &str) -> (Session, crate::Rebuild) {
        s.p.set_feat_dim(feature, key, text.into());
        s.p.mark_all_dirty();
        s.rebuild();
        let dir = std::env::temp_dir().join(format!("qymcad-mcp-primitive-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{tag}.qcad"));
        s.save(Some(&path)).unwrap();
        Session::open(&path).unwrap()
    }

    fn cube() -> (Session, Id) {
        let mut s = Session::new_part();
        let (info, _) =
            s.primitive(&PrimitiveSpec::Box { dx: 20.0.into(), dy: 20.0.into(), dz: 20.0.into() }, &Placement::Origin, None).unwrap();
        (s, info.feature)
    }

    /// A file with a primitive outside the fields opens with a warning (also in `doc_info`) and does not block unrelated edits;
    /// once the size is back inside the fields a new fault is judged again.
    #[test]
    fn a_file_with_a_primitive_outside_the_fields_opens_with_a_warning_and_edits_elsewhere_pass() {
        let (mut s, feature) = cube();
        let (mut loaded, report) = file_with(&mut s, feature, "dx", "0.05", "below-field");
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(
            report.warnings.iter().any(|w| w.node == feature && w.message.contains("box dx must be within 0.1")),
            "{:?}",
            report.warnings
        );
        assert!(loaded.info().warnings.iter().any(|w| w.contains("box dx must be within 0.1")), "{:?}", loaded.info().warnings);
        loaded.param_set("unrelated", &1.0.into()).unwrap();
        // Back inside the field the exemption is gone: a later fault is a new one.
        loaded.dimension_set(&dim(feature, "dx"), Some(&20.0.into()), None).unwrap();
        assert!(loaded.open_primitive_faults.is_empty());
        assert!(loaded.dimension_set(&dim(feature, "dx"), Some(&0.05.into()), None).is_err());
    }

    /// The exemption is for the size that was outside the fields, with the value it had: the node's other sizes are judged as
    /// usual, and moving the exempt size is allowed only into the field.
    #[test]
    fn the_exemption_of_a_file_is_per_size_and_holds_only_for_the_original_value() {
        let (mut s, feature) = cube();
        let (mut loaded, _) = file_with(&mut s, feature, "dx", "0.05", "per-size");
        let before = ron::to_string(loaded.project()).unwrap();
        // Another size of the same node: judged.
        let e = loaded.dimension_set(&dim(feature, "dy"), Some(&0.0001.into()), None).unwrap_err().to_string();
        assert!(e.contains("box dy must be within 0.1"), "{e}");
        // The exempt size itself: only into the field.
        let e = loaded.dimension_set(&dim(feature, "dx"), Some(&0.06.into()), None).unwrap_err().to_string();
        assert!(e.contains("box dx must be within 0.1"), "{e}");
        assert_eq!(ron::to_string(loaded.project()).unwrap(), before, "refusals change nothing");
        loaded.dimension_set(&dim(feature, "dz"), Some(&15.0.into()), None).unwrap();
        assert!(loaded.open_primitive_faults.contains_key(&(feature, "dx")), "an independent edit keeps the exemption");
        loaded.dimension_set(&dim(feature, "dx"), Some(&0.1.into()), None).unwrap();
        assert!(loaded.open_primitive_faults.is_empty(), "into the field: the exemption goes");
        // The prism's side count: the rounded 2.5 stays tolerated while it is untouched, and 3.5 (square) is a new fault.
        let mut p = Session::new_part();
        let (prism, _) =
            p.primitive(&PrimitiveSpec::Prism { r: 10.0.into(), h: 20.0.into(), n: 6.0.into() }, &Placement::Origin, None).unwrap();
        let (mut loaded, report) = file_with(&mut p, prism.feature, "n", "2.5", "prism-n");
        assert!(report.warnings.iter().any(|w| w.message.contains("prism n must be a whole number")), "{:?}", report.warnings);
        loaded.param_set("unrelated", &1.0.into()).unwrap();
        let e = loaded.dimension_set(&dim(prism.feature, "n"), Some(&3.5.into()), None).unwrap_err().to_string();
        assert!(e.contains("prism n must be a whole number"), "{e}");
        let e = loaded.dimension_set(&dim(prism.feature, "h"), Some(&1e-6.into()), None).unwrap_err().to_string();
        assert!(e.contains("prism h must be within 0.1"), "{e}");
        loaded.dimension_set(&dim(prism.feature, "n"), Some(&5.0.into()), None).unwrap();
        assert!(loaded.open_primitive_faults.is_empty());
    }

    /// Undo and the rollback of a failed call restore the exemption together with the recipe it belongs to.
    #[test]
    fn undo_and_a_rolled_back_batch_restore_the_exemption_of_a_file() {
        let (mut s, feature) = cube();
        let (mut loaded, _) = file_with(&mut s, feature, "dx", "0.05", "undo");
        let faults = loaded.open_primitive_faults.clone();
        assert!(!faults.is_empty());
        // The fix is one tool call, then undone.
        let snapshot = loaded.begin_tool_edit().unwrap();
        loaded.dimension_set(&dim(feature, "dx"), Some(&20.0.into()), None).unwrap();
        loaded.finish_tool_edit(snapshot, true);
        assert!(loaded.open_primitive_faults.is_empty());
        loaded.undo().unwrap();
        assert_eq!(loaded.open_primitive_faults, faults, "undo brings the recipe back with its exemption");
        loaded.param_set("unrelated", &1.0.into()).unwrap();
        // A batch whose first step fixes the size and whose later step fails is rolled back as a whole.
        let snapshot = loaded.begin_tool_edit().unwrap();
        loaded.dimension_set(&dim(feature, "dx"), Some(&20.0.into()), None).unwrap();
        loaded.finish_tool_edit(snapshot, false);
        assert_eq!(loaded.open_primitive_faults, faults, "the rolled back batch brings the exemption back");
        loaded.param_set("unrelated", &2.0.into()).unwrap();
    }

    /// `part_copy` gives the copy new node ids: the copy of a tolerated size stays tolerated, a new fault on the copy does not.
    #[test]
    fn a_part_copy_carries_the_exemption_to_the_copied_nodes() {
        let (mut s, feature) = cube();
        let (mut loaded, _) = file_with(&mut s, feature, "dx", "0.05", "copy");
        let part = loaded.project().components.iter().find(|c| c.kind == qymcad_core::feature::ComponentKind::Part).unwrap().id;
        loaded.part_copy(&crate::PartRef::Id(part), "copy", None, None).unwrap();
        let copied = loaded
            .project()
            .timeline
            .iter()
            .filter(|n| matches!(n.kind, FeatureKind::Box3 { .. }) && n.id != feature)
            .map(|n| n.id)
            .next()
            .expect("the copy's box");
        assert!(loaded.open_primitive_faults.contains_key(&(copied, "dx")), "{:?}", loaded.open_primitive_faults);
        loaded.param_set("unrelated", &1.0.into()).unwrap();
        let e = loaded.dimension_set(&dim(copied, "dy"), Some(&0.05.into()), None).unwrap_err().to_string();
        assert!(e.contains("box dy must be within 0.1"), "{e}");
    }

    /// The field's own tolerance: a value within 1e-9 of a bound, and a whole number within 1e-9 of an integer, are inside, so a
    /// file with such values opens without a false warning.
    #[test]
    fn the_fields_tolerance_accepts_float_noise_and_does_not_warn_on_open() {
        let (mut s, feature) = cube();
        let (loaded, report) = file_with(&mut s, feature, "dx", "0.3-0.2", "noise");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(loaded.primitive_faults().is_empty());
        let inside = |kind: &str, values: &[(&str, f64)]| {
            let v = |key: &str| values.iter().find(|(k, _)| *k == key).unwrap().1;
            judge(kind, &|key| Some(v(key)), true).into_iter().map(|f| f.error.to_string()).collect::<Vec<_>>()
        };
        assert!(inside("box", &[("dx", 0.1 - 1e-10), ("dy", 1e5 + 1e-10), ("dz", 5.0)]).is_empty());
        assert!(inside("prism", &[("r", 5.0), ("h", 5.0), ("n", 3.0000000004)]).is_empty());
        assert!(inside("torus", &[("major", 0.1005), ("minor", 0.1)]).is_empty(), "the field's upper bound is max(major - 0.001, 0.1)");
        assert!(!inside("prism", &[("r", 5.0), ("h", 5.0), ("n", 3.00000001)]).is_empty());
        assert!(!inside("box", &[("dx", 0.1 - 1e-8), ("dy", 5.0), ("dz", 5.0)]).is_empty());
    }

    /// A million sides are refused by `param_set` before the rebuild would try to build them.
    #[test]
    fn a_huge_side_count_through_a_parameter_is_refused_before_the_rebuild() {
        let mut s = Session::new_part();
        s.param_set("k", &6.0.into()).unwrap();
        s.primitive(&PrimitiveSpec::Prism { r: 10.0.into(), h: 20.0.into(), n: "k".into() }, &Placement::Origin, None).unwrap();
        let started = std::time::Instant::now();
        let e = s.param_set("k", &1.0e6.into()).unwrap_err().to_string();
        assert!(e.contains("prism n must be a whole number of sides in 3..=64"), "{e}");
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "refused early, not after a rebuild: {:?}", started.elapsed());
    }

    /// J2 #2: a size that does not evaluate and is tolerated (the file had it) neither stops the judging of the node's other sizes
    /// nor lets them pass.
    #[test]
    fn an_exempt_size_that_does_not_evaluate_does_not_hide_the_nodes_other_sizes() {
        let (mut s, feature) = cube();
        let (mut loaded, _) = file_with(&mut s, feature, "dx", "missing_name", "missing");
        assert!(loaded.open_primitive_faults.contains_key(&(feature, "dx")), "{:?}", loaded.open_primitive_faults);
        loaded.param_set("unrelated", &1.0.into()).unwrap();
        let e = loaded.dimension_set(&dim(feature, "dy"), Some(&1e-6.into()), None).unwrap_err().to_string();
        assert!(e.contains("box dy must be within 0.1"), "{e}");
        // The same through a parameter that drives another size of the node.
        loaded.param_set("k", &20.0.into()).unwrap();
        loaded.dimension_set(&dim(feature, "dz"), Some(&Num::Expr("k".into())), None).unwrap();
        let e = loaded.param_set("k", &1e-6.into()).unwrap_err().to_string();
        assert!(e.contains("box dz must be within 0.1"), "{e}");
    }

    /// J2 #3: the torus tube's upper bound is a pair `(minor, major)`: a tolerated gap stays tolerated while both keep their
    /// values, and a change of `major` that leaves the gap faulty is a new fault.
    #[test]
    fn the_torus_exemption_holds_for_the_pair_minor_and_major() {
        let mut s = Session::new_part();
        let (info, _) = s.primitive(&PrimitiveSpec::Torus { major: 10.0.into(), minor: 2.0.into() }, &Placement::Origin, None).unwrap();
        let feature = info.feature;
        // 9.9995 against 10: a gap of 0.0005, under the field's 0.001.
        let (mut loaded, report) = file_with(&mut s, feature, "minor", "9.9995", "torus-gap");
        assert!(report.warnings.iter().any(|w| w.message.contains("torus minor must be within")), "{:?}", report.warnings);
        loaded.param_set("unrelated", &1.0.into()).unwrap();
        let before = ron::to_string(loaded.project()).unwrap();
        // Worse by `major`: 9.9996 leaves a gap of 0.0001.
        let e = loaded.dimension_set(&dim(feature, "major"), Some(&9.9996.into()), None).unwrap_err().to_string();
        assert!(e.contains("torus minor must be within"), "{e}");
        assert_eq!(ron::to_string(loaded.project()).unwrap(), before, "refused, nothing changed");
        // Into the field by `major`: allowed, and the exemption goes.
        loaded.dimension_set(&dim(feature, "major"), Some(&20.0.into()), None).unwrap();
        assert!(loaded.open_primitive_faults.is_empty(), "{:?}", loaded.open_primitive_faults);
    }
}
