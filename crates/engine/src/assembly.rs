//! Native root-child Parts and planar Rigid/Slider joints (ADR 0051).
use crate::{BaseName, Error, Id, Num, Result, Sel, Session};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum PartRef {
    Id(Id),
    Name(String),
    Root {
        /// Select the root navigation context; must be true.
        root: bool,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum JointRef {
    Id(Id),
    Name(String),
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssemblyJointKind {
    Rigid,
    Slider,
    Revolute,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JointMode {
    Align,
    AsBuilt,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PartRotation {
    /// Finite nonzero parent-local rotation axis.
    pub axis: [f64; 3],
    /// Finite right-handed rotation in degrees.
    pub angle_deg: f64,
}
/// What is picked on the anchor body: a planar face (Rigid, Slider) or a straight edge (Revolute hinge axis).
#[derive(Clone, Debug)]
pub enum AnchorPick {
    Face(Sel),
    Edge(Sel),
}
#[derive(Clone, Debug)]
pub struct JointAnchor {
    pub body: Id,
    pub pick: AnchorPick,
}
impl JointAnchor {
    pub fn face(body: Id, sel: Sel) -> Self {
        Self { body, pick: AnchorPick::Face(sel) }
    }
    pub fn edge(body: Id, sel: Sel) -> Self {
        Self { body, pick: AnchorPick::Edge(sel) }
    }
}
/// A joint's free slot, as the app names it: Revolute turns `angle` (degrees), Slider travels `offset` (mm).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JointSlot {
    Angle,
    Offset,
}
/// One bound of the app's Limits section: keep it, tick it off (value and formula), or set a number or formula.
#[derive(Clone, Debug)]
pub enum LimitEdit {
    Keep,
    Clear,
    Set(Num),
}
/// Limits given at joint creation, applied before the first drive.
#[derive(Clone, Debug, Default)]
pub struct JointLimits {
    pub min: Option<Num>,
    pub max: Option<Num>,
}
#[derive(Clone, Debug)]
pub struct JointCreate {
    pub name: Option<String>,
    pub kind: AssemblyJointKind,
    pub a: JointAnchor,
    pub b: JointAnchor,
    pub mode: JointMode,
    pub axis: Option<BaseName>,
    pub offset: Option<Num>,
    /// Revolute angle in degrees: number or native formula.
    pub angle: Option<Num>,
    pub limits: Option<JointLimits>,
}
use qymcad_core::{
    feature::{rot12_axis, AnchorRef, ComponentKind, FaceKey, JointKind, PLACE_IDENTITY},
    model::Project,
};
use std::collections::HashSet;

pub(crate) fn refusal(message: impl Into<String>) -> Error {
    Error::Assembly(message.into())
}
fn kind_name(k: JointKind) -> String {
    format!("{k:?}").to_lowercase()
}
/// The one free slot of a supported kind: Slider travel (1), Revolute angle (0).
pub(crate) fn free_slot(k: JointKind) -> Option<usize> {
    match k {
        JointKind::Slider => Some(1),
        JointKind::Revolute => Some(0),
        _ => None,
    }
}
const SLOT_KEYS: [&str; 3] = ["angle", "offset", "offset2"];
/// The limit fields' range in the app's joint box (`popup.rs:113-118`).
/// The text of the F-166 note.
pub const EDGE_ANCHOR_NOTE: &str = "QymCAD.app reports this joint's anchor as lost after opening the file until Edit → Rebuild all (F-166)";
const LIMIT_RANGE: f64 = 100_000.0;
fn limit_key(min: bool, slot: usize) -> String {
    format!("{}{slot}", if min { "min" } else { "max" })
}
/// A formula kept under `min<slot>`/`max<slot>` of a joint whose bound is ticked off. The app never reads it:
/// `item_value` answers `None` for a bound that is off and `impose_item_values` skips it. Ticking the bound back on
/// re-imposes it at the next evaluation, and a driver rename rewrites it meanwhile (F-164). Nothing here evaluates,
/// validates or counts a dormant formula either.
pub(crate) fn dormant_limit_formula(p: &Project, joint: Id, key: &str) -> bool {
    let Some(j) = p.joints.iter().find(|j| j.id == joint) else { return false };
    let Some(slot) = key.get(3..).and_then(|s| s.parse::<usize>().ok()).filter(|s| *s < 3) else { return false };
    match key.get(..3) {
        Some("min") => j.limit_min[slot].is_none(),
        Some("max") => j.limit_max[slot].is_none(),
        _ => false,
    }
}
/// A joint's formula under `key` that something reads: a slot's drive formula, or a limit formula of a bound that is on.
fn live_formula<'a>(p: &'a Project, joint: Id, key: &str) -> Option<&'a str> {
    p.feat_dim(joint, key).filter(|e| !e.trim().is_empty() && !dormant_limit_formula(p, joint, key))
}
/// Every key a joint can keep a formula under: the three slots' drives, then the six limits.
fn joint_keys() -> impl Iterator<Item = String> {
    SLOT_KEYS.into_iter().map(String::from).chain((0..3).flat_map(|s| [limit_key(true, s), limit_key(false, s)]))
}
/// The numeric drives (those without a drive formula) as they stand: what a native solve may clamp without a word.
pub(crate) fn numeric_drives(p: &Project) -> Vec<(Id, usize, f64)> {
    let mut out = Vec::new();
    for j in &p.joints {
        for (slot, key) in SLOT_KEYS.iter().enumerate() {
            if let (Some(v), None) = (j.drive[slot], live_formula(p, j.id, key)) {
                out.push((j.id, slot, v));
            }
        }
    }
    out
}
/// Evaluated bounds of a joint's free slot: a formula's value where one is read, else the stored value.
fn evaluated_limits(p: &Project, id: Id, slot: usize) -> Result<(Option<f64>, Option<f64>)> {
    let Some(j) = p.joints.iter().find(|j| j.id == id) else { return Ok((None, None)) };
    let vars = p.param_map();
    let bound = |min: bool, stored: Option<f64>| -> Result<Option<f64>> {
        match live_formula(p, id, &limit_key(min, slot)) {
            Some(expr) if stored.is_some() => Ok(Some(Num::Expr(expr.into()).eval(&vars)?)),
            _ => Ok(stored),
        }
    };
    Ok((bound(true, j.limit_min[slot])?, bound(false, j.limit_max[slot])?))
}
/// What is wrong with the bounds of a joint's free slot, without a refusal's closing words; `None` when they are fine.
/// Only a typed number is held to the app's field range: the GUI bounds the field, and a formula that comes to a big
/// number breaks nothing natively.
fn limit_problem(p: &Project, id: Id) -> Result<Option<String>> {
    let Some(j) = p.joints.iter().find(|j| j.id == id) else { return Ok(None) };
    let Some(slot) = free_slot(j.kind) else { return Ok(None) };
    let name = SLOT_KEYS[slot];
    let (min, max) = evaluated_limits(p, id, slot)?;
    for (v, is_min) in [(min, true), (max, false)] {
        let Some(v) = v else { continue };
        let typed = live_formula(p, id, &limit_key(is_min, slot)).is_none();
        if !v.is_finite() || (typed && v.abs() > LIMIT_RANGE) {
            return Ok(Some(format!("joint {id} {name} limit {v} is outside the app's range [-{LIMIT_RANGE}, {LIMIT_RANGE}]")));
        }
    }
    if j.kind == JointKind::Revolute {
        for v in [min, max].into_iter().flatten() {
            if !(v > -180.0 && v <= 180.0) {
                return Ok(Some(format!(
                    "joint {id} angle limit {v} is outside (-180, 180] degrees: QymCAD reads angles wrapped to (-180, 180], so -180 reads as 180 and a limit outside the range snaps a released part to the other bound (F-158)"
                )));
            }
        }
    }
    if let (Some(a), Some(b)) = (min, max) {
        if a > b {
            return Ok(Some(format!(
                "joint {id} {name} limit min {a} is greater than max {b}; QymCAD would send every value to max (F-157)"
            )));
        }
    }
    Ok(None)
}
/// Faults the app accepts in a joint that the server refuses to build on: a limit it would refuse, a Revolute drive
/// outside the wrapped reading range. They are reported; only an edit that touches the joint is refused.
fn joint_fault_messages(p: &Project, j: &qymcad_core::feature::Joint) -> Vec<String> {
    let mut out: Vec<String> = limit_problem(p, j.id).ok().flatten().into_iter().collect();
    if let (JointKind::Revolute, Some(v)) = (j.kind, j.drive[0]) {
        if !(v > -180.0 && v <= 180.0) {
            out.push(format!(
                "joint {} angle drive {v}° is outside (-180, 180]: QymCAD reads angles wrapped to (-180, 180], so the reading disagrees with the drive (F-158)",
                j.id
            ));
        }
    }
    out
}
fn limit_faults(p: &Project) -> Vec<Value> {
    p.joints.iter().flat_map(|j| joint_fault_messages(p, j).into_iter().map(|message| json!({"joint":j.id,"message":message}))).collect()
}
/// `{joint, slot}` for every free slot standing on a bound (native `joints_at_limit`, the app's at-limit mark): the one
/// format of `at_limit` in diagnostics, joint results and `assembly_info`.
fn at_limit_list(p: &Project) -> Vec<Value> {
    p.joints_at_limit().into_iter().map(|(joint, slot)| json!({"joint":joint,"slot":SLOT_KEYS[slot]})).collect()
}
/// The joints an edit touched: created or changed by it, or with a drive or limit formula that reads a parameter it
/// changed. Only these are validated; the others are reported as diagnostics (`limit_faults`).
fn touched_joints(before: &Project, after: &Project) -> HashSet<Id> {
    let (old_vars, new_vars) = (before.param_map(), after.param_map());
    let changed: Vec<&String> = new_vars
        .iter()
        .filter(|(k, v)| old_vars.get(*k) != Some(*v))
        .map(|(k, _)| k)
        .chain(old_vars.keys().filter(|k| !new_vars.contains_key(*k)))
        .collect();
    after
        .joints
        .iter()
        .filter(|j| {
            let moved = before.joints.iter().find(|o| o.id == j.id).is_none_or(|o| {
                o.kind != j.kind
                    || o.a != j.a
                    || o.b != j.b
                    || o.limit_min != j.limit_min
                    || o.limit_max != j.limit_max
                    || o.drive != j.drive
                    || before.feat_dims.get(&j.id) != after.feat_dims.get(&j.id)
            });
            moved
                || joint_keys()
                    .filter_map(|key| live_formula(after, j.id, &key))
                    .any(|expr| changed.iter().any(|name| qymcad_core::expr::mentions(expr, name)))
        })
        .map(|j| j.id)
        .collect()
}
/// Drive formulas that a limit clamps, after a solve: what the formula asks and what the drive holds.
fn formula_clamps(p: &Project) -> Vec<Value> {
    let vars = p.param_map();
    let mut out = Vec::new();
    for j in &p.joints {
        for (slot, key) in SLOT_KEYS.iter().enumerate() {
            let (Some(expr), Some(applied)) = (p.feat_dim(j.id, key), j.drive[slot]) else { continue };
            if let Ok(requested) = Num::Expr(expr.into()).eval(&vars) {
                if (requested - applied).abs() > 1e-9 {
                    out.push(json!({"joint":j.id,"slot":slot,"requested":requested,"applied":applied}));
                }
            }
        }
    }
    out
}
pub(crate) fn object_name(name: &str) -> Result<&str> {
    if name.chars().any(char::is_control) {
        return Err(Error::Invalid("object name must not contain Unicode control characters".into()));
    }
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Invalid("object name must not be empty after trimming".into()));
    }
    Session::check_object_name(name)?;
    Ok(name)
}

/// One acceptance implementation for mutations and diagnostic reads. Reads solve an independent Project.
///
/// `scope` is the set of joints the edit touched (`None`: every joint, as a diagnostic read judges them). A joint
/// outside the scope that carries a fault the app accepts (`limit_faults`) is not held to its drive: its mismatch is
/// reported by the read, and does not refuse an edit that has nothing to do with it.
fn inspect(p: &mut Project, requested: Option<(Id, usize, f64)>, scope: Option<&HashSet<Id>>) -> Value {
    let invalid_before: Vec<_> = p.components.iter().filter(|c| !rigid(&c.transform)).map(|c| c.id).collect();
    // Never feed nonfinite matrices to the native nonlinear solver.
    let invalid_slots: Vec<_> = p
        .joints
        .iter()
        .filter(|j| {
            [j.angle, j.offset, j.offset2].iter().chain(j.drive.iter().flatten()).any(|v| !v.is_finite())
                || j.limit_min.iter().chain(&j.limit_max).flatten().any(|v| !v.is_finite())
        })
        .map(|j| j.id)
        .collect();
    let mut formula_errors = Vec::new();
    for j in &p.joints {
        for key in joint_keys() {
            let key = key.as_str();
            // A dormant limit formula is read by nothing, so nothing here judges it.
            if let Some(expr) = p.feat_dim(j.id, key).filter(|_| !dormant_limit_formula(p, j.id, key)) {
                let result = Num::Expr(expr.into()).eval(&p.param_map()).and_then(|value| {
                    p.check_dim_target_text(&qymcad_core::model::DimTarget::Feature { node: j.id, key: key.into() }, expr)
                        .map_err(|e| Error::Expr(e.to_string()))?;
                    Ok(value)
                });
                if let Err(e) = result {
                    formula_errors.push(json!({"joint":j.id,"key":key,"message":e.to_string()}));
                }
            }
        }
    }
    let errors = if invalid_before.is_empty() && invalid_slots.is_empty() { p.solve_joints().errors } else { Vec::new() };
    let faults = p.joint_faults();
    let mut witnesses = Vec::new();
    for j in &p.joints {
        for cid in [j.a, j.b] {
            let Some(c) = p.connector(cid) else { continue };
            let (body, count) = match &c.anchor {
                // A face has the same rule as an edge below: exactly one face with the persistent id on the CURRENT
                // body, with the centroid and normal the consumed body's face has, which is where the connector
                // reads it from (F-160).
                AnchorRef::FaceCenter(body, key) => {
                    let heads = current_heads(p, *body);
                    let found: Vec<_> = heads
                        .iter()
                        .filter_map(|h| p.regen_faces.get(h))
                        .flat_map(|faces| faces.iter().filter(|f| f.id == key.id))
                        .collect();
                    let stale = p.regen_faces.get(body).and_then(|faces| faces.iter().find(|f| f.id == key.id));
                    let in_place = heads == [*body] || matches!((stale, found.as_slice()), (Some(old), [new]) if same_face(old, new));
                    (body, if in_place { found.len() } else { 0 })
                }
                // A hinge edge: exactly one edge with its persistent id on the CURRENT body, where the connector's
                // pivot is. Natively the connector stays on the body it names; after a feature consumed that body
                // the pivot is still read from its last-built, stale edge (F-160), so an edge that moved or went
                // away on the current body is a lost anchor.
                AnchorRef::EdgeMid(body, edge) => {
                    let heads = current_heads(p, *body);
                    let found: Vec<_> = heads
                        .iter()
                        .filter_map(|h| p.regen_edges.get(h))
                        .flat_map(|edges| edges.iter().filter(|e| e.id == *edge))
                        .collect();
                    let stale = p.regen_edges.get(body).and_then(|edges| edges.iter().find(|e| e.id == *edge));
                    let in_place = heads == [*body]
                        || matches!((stale, found.as_slice()), (Some(old), [new]) if same_edge(old.mid, old.dir, new.mid, new.dir));
                    (body, if in_place { found.len() } else { 0 })
                }
                _ => continue,
            };
            let alive = p.timeline.iter().take(p.rollback_place()).any(|n| !n.suppressed && n.kind.bodies().contains(body));
            if !alive || p.mesh_index(*body).is_none() || count != 1 {
                witnesses.push(j.id);
            }
        }
    }
    witnesses.sort_unstable();
    witnesses.dedup();
    let mut faulty: Vec<_> = faults.iter().map(|(id, _)| *id).chain(witnesses.iter().copied()).collect();
    faulty.sort_unstable();
    faulty.dedup();
    let mut violated = p.mates_violated.clone();
    violated.sort_unstable();
    let invalid: Vec<_> = p.components.iter().filter(|c| !rigid(&c.transform)).map(|c| c.id).collect();
    let mut requests = Vec::new();
    if p.drag_pull.is_none() {
        for j in &p.joints {
            if scope.is_some_and(|s| !s.contains(&j.id)) && !joint_fault_messages(p, j).is_empty() {
                continue;
            }
            for (slot, read) in [j.angle, j.offset, j.offset2].into_iter().enumerate() {
                if let Some(want) = j.drive[slot] {
                    if !read.is_finite() || !want.is_finite() || (read - want).abs() > 1e-5 {
                        requests.push(json!({"joint":j.id,"slot":slot,"requested":want,"read":read}));
                    }
                }
            }
        }
        if let Some((id, slot, want)) = requested {
            if let Some(j) = p.joints.iter().find(|j| j.id == id) {
                let read = [j.angle, j.offset, j.offset2][slot];
                if !read.is_finite() || (read - want).abs() > 1e-5 {
                    requests.push(json!({"joint":id,"slot":slot,"requested":want,"read":read}));
                }
            }
        }
    }
    let constraint_faults: Vec<_> = p
        .mate_constraints
        .iter()
        .filter(|c| {
            c.members.iter().chain(c.faces.iter().map(|(id, _)| id)).any(|id| !p.components.iter().any(|p| p.id == *id))
                || c.anchors.iter().any(|id| p.connector(*id).is_none())
        })
        .map(|c| c.id)
        .collect();
    let relation_faults = p.relation_faults();
    let healthy = p.regen_errors.is_empty()
        && faulty.is_empty()
        && errors.is_empty()
        && violated.is_empty()
        && !p.mates_conflict
        && invalid.is_empty()
        && invalid_before.is_empty()
        && requests.is_empty()
        && constraint_faults.is_empty()
        && relation_faults.is_empty()
        && formula_errors.is_empty()
        && invalid_slots.is_empty();
    json!({"healthy":healthy,"feature_errors":p.regen_errors.keys().collect::<Vec<_>>(),"joint_faults":faults,"faulty_joints":faulty,"anchor_faults":witnesses,"solve_errors":errors,"violated_joints":violated,"conflict":p.mates_conflict,"invalid_transforms":invalid,"request_mismatches":requests,"constraint_faults":constraint_faults,"relation_faults":relation_faults,"formula_errors":formula_errors,"invalid_slots":invalid_slots,"at_limit":at_limit_list(p),"clamped":formula_clamps(p)})
}
/// The bodies that now carry what `body` built: itself when no feature consumes it, else the results of the features
/// that do, followed to the end (a split or a pattern has several).
fn current_heads(p: &Project, body: Id) -> Vec<Id> {
    let consumed = p.consumed_bodies();
    let (mut heads, mut todo, mut seen) = (Vec::new(), vec![body], HashSet::new());
    while let Some(b) = todo.pop() {
        if !seen.insert(b) {
            continue;
        }
        if !consumed.contains(&b) {
            heads.push(b);
            continue;
        }
        todo.extend(
            p.timeline
                .iter()
                .take(p.rollback_place())
                .filter(|n| !n.suppressed && n.kind.consumed().contains(&b))
                .flat_map(|n| n.kind.bodies()),
        );
    }
    heads.sort_unstable();
    heads
}
/// The same face place: centroid and outward normal within the kernel's noise.
fn same_face(a: &qymcad_core::geom::MeshFace, b: &qymcad_core::geom::MeshFace) -> bool {
    let (ca, cb) = ([a.centroid.x, a.centroid.y, a.centroid.z], [b.centroid.x, b.centroid.y, b.centroid.z]);
    (0..3).all(|k| (ca[k] - cb[k]).abs() < 1e-5 && (a.normal[k] - b.normal[k]).abs() < 1e-6)
}
/// The same straight edge: midpoint and direction (either sense) within the kernel's noise.
fn same_edge(mid_a: [f64; 3], dir_a: [f64; 3], mid_b: [f64; 3], dir_b: [f64; 3]) -> bool {
    let near = |a: [f64; 3], b: [f64; 3], sign: f64| (0..3).all(|k| (a[k] - sign * b[k]).abs() < 1e-5);
    near(mid_a, mid_b, 1.0) && (near(dir_a, dir_b, 1.0) || near(dir_a, dir_b, -1.0))
}
/// `clamped:{joint,slot,requested,applied}` when a limit moved the request, else null.
fn clamp_report(id: Id, slot: usize, requested: Option<f64>, applied: Option<f64>) -> Value {
    match (requested, applied) {
        (Some(r), Some(a)) if (r - a).abs() > 1e-9 => json!({"joint":id,"slot":slot,"requested":r,"applied":a}),
        _ => Value::Null,
    }
}
fn limit_arg(edit: &LimitEdit) -> Value {
    match edit {
        LimitEdit::Keep => json!("keep"),
        LimitEdit::Clear => Value::Null,
        LimitEdit::Set(v) => json!(v),
    }
}
fn rigid(t: &[f64; 12]) -> bool {
    if !t.iter().all(|x| x.is_finite()) {
        return false;
    }
    let row = |i: usize| [t[i * 4], t[i * 4 + 1], t[i * 4 + 2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let a = row(0);
    let b = row(1);
    let c = row(2);
    (0..3).all(|i| (dot(row(i), row(i)) - 1.).abs() < 1e-5)
        && dot(a, b).abs() < 1e-5
        && dot(a, c).abs() < 1e-5
        && dot(b, c).abs() < 1e-5
        && (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]) - 1.).abs() < 1e-5
}
fn solve_refusal(d: &Value) -> Error {
    // Name only the causes that are present: an all-empty list reads as "nothing failed".
    let mut causes = Vec::new();
    for (key, label) in [
        ("faulty_joints", "faulty joints"),
        ("violated_joints", "violated joints"),
        ("solve_errors", "solver errors"),
        ("request_mismatches", "requested joint values not reached"),
        ("invalid_slots", "non-finite joint values"),
        ("invalid_transforms", "non-rigid part transforms"),
        ("formula_errors", "formula errors"),
        ("constraint_faults", "constraint faults"),
        ("relation_faults", "relation faults"),
        ("feature_errors", "feature errors"),
    ] {
        if d[key].as_array().is_some_and(|a| !a.is_empty()) {
            causes.push(format!("{label} {}", d[key]));
        }
    }
    if d["conflict"] == true {
        causes.push("conflicting joints or drives".into());
    }
    if causes.is_empty() {
        causes.push("unhealthy mechanism".into());
    }
    refusal(format!("assembly solve failed: {}; nothing was changed", causes.join("; ")))
}
impl Session {
    pub fn resolve_part(&self, part: &PartRef) -> Result<Id> {
        let id = match part {
            PartRef::Root { root: true } => return Ok(self.p.root),
            PartRef::Root { root: false } => return Err(refusal("part root selector must be true")),
            PartRef::Id(id) => *id,
            PartRef::Name(name) => {
                if let Ok(id) = name.trim().parse::<Id>() {
                    return self.resolve_part(&PartRef::Id(id));
                }
                let mut found: Vec<_> = self
                    .p
                    .components
                    .iter()
                    .filter(|c| {
                        c.kind == ComponentKind::Part && (c.name == name.trim() || crate::localization::name(&c.name) == name.trim())
                    })
                    .map(|c| c.id)
                    .collect();
                found.sort_unstable();
                match found.as_slice() {
                    [id] => *id,
                    [] => return Err(refusal(format!("no part named '{name}'"))),
                    _ => return Err(refusal(format!("several parts are named '{name}' ({found:?}); use the part id"))),
                }
            }
        };
        let c = self.p.components.iter().find(|c| c.id == id).ok_or_else(|| refusal(format!("no part with id {id}")))?;
        if c.kind == ComponentKind::Assembly {
            return Err(refusal(format!("component {id} is a nested assembly; nested assembly editing is not supported in package A")));
        }
        Ok(id)
    }
    pub fn resolve_joint(&self, joint: &JointRef) -> Result<Id> {
        match joint {
            JointRef::Id(id) => {
                self.p.joints.iter().any(|j| j.id == *id).then_some(*id).ok_or_else(|| refusal(format!("no joint with id {id}")))
            }
            JointRef::Name(name) => {
                if let Ok(id) = name.trim().parse::<Id>() {
                    return self.resolve_joint(&JointRef::Id(id));
                }
                let mut ids: Vec<_> = self
                    .p
                    .joints
                    .iter()
                    .filter(|j| j.name == name.trim() || crate::localization::name(&j.name) == name.trim())
                    .map(|j| j.id)
                    .collect();
                ids.sort_unstable();
                match ids.as_slice() {
                    [id] => Ok(*id),
                    [] => Err(refusal(format!("no joint named '{name}'"))),
                    _ => Err(refusal(format!("several joints are named '{name}' ({ids:?}); use the joint id"))),
                }
            }
        }
    }
    fn root_part(&self, id: Id) -> Result<()> {
        let c = self.p.components.iter().find(|c| c.id == id).ok_or_else(|| refusal(format!("no part with id {id}")))?;
        if c.kind != ComponentKind::Part || c.parent != Some(self.p.root) {
            return Err(refusal(format!("joint_create requires root-child parts; part {id} is nested")));
        }
        Ok(())
    }
    /// Retain exact original handles; an enclosing MCP/batch boundary is reused.
    pub(crate) fn assembly_edit(&mut self, tool: &str, args: Value, edit: impl FnOnce(&mut Self) -> Result<Value>) -> Result<Value> {
        let snapshot = if self.tool_edit_active {
            None
        } else {
            let mut snap = self.begin_tool_edit()?;
            snap.record_call(tool, args);
            Some(snap)
        };
        let before = self.p.clone();
        let warnings = self.advisory_warnings.clone();
        let detached = self.detached_projections.clone();
        // Anchor resolution can rebuild even a placement-only call when cached topology is missing.
        // Retain original handles inside an enclosing boundary too; a caller may inspect a refused step.
        let nested_originals = if snapshot.is_none() {
            let copies = crate::session::copy_shapes(self.shapes.iter().map(|(&id, shape)| (id, shape)))
                .map_err(|id| Error::Io(format!("cannot snapshot body {id} before {tool}")))?;
            Some(std::mem::replace(&mut self.shapes, copies))
        } else {
            None
        };
        let result = edit(self).and_then(|v| {
            self.assembly_accept_edit(&before)?;
            Ok(v)
        });
        if result.is_err() && snapshot.is_none() {
            self.p = before;
            self.advisory_warnings = warnings;
            self.detached_projections = detached;
            if let Some(shapes) = nested_originals {
                self.shapes = shapes;
            }
        }
        if let Some(mut snap) = snapshot {
            if let Ok(v) = &result {
                if let Some(id) = v["part"]["id"].as_u64().or(v["joint"]["id"].as_u64()) {
                    snap.record_created_node(id);
                }
            }
            self.finish_tool_edit(snap, result.is_ok());
        }
        result
    }
    pub fn part_new(&mut self, name: &str) -> Result<Value> {
        let name = object_name(name)?.to_string();
        self.assembly_edit("part_new", json!({"name":name}), |s| {
            s.p.set_active_component(Some(s.p.root));
            let id = s.p.add_part(&name);
            s.p.set_active_component(Some(id));
            Ok(json!({"part":{"id":id,"name":name,"parent":s.p.root},"active_component":id,"changed":true}))
        })
    }
    pub fn part_select(&mut self, part: &PartRef) -> Result<Value> {
        let id = self.resolve_part(part)?;
        let previous = self.p.active_component;
        let kind = if id == self.p.root { "assembly" } else { "part" };
        let result = |changed| json!({"previous_component":previous,"active_component":id,"context_kind":kind,"changed":changed});
        if previous == Some(id) {
            return Ok(result(false));
        }
        self.assembly_edit("part_select", json!({"part":part}), |s| {
            s.p.set_active_component(Some(id));
            Ok(result(true))
        })
    }
    pub fn part_place(&mut self, part: &PartRef, translation_mm: [f64; 3], rotation: Option<&PartRotation>) -> Result<Value> {
        let id = self.resolve_part(part)?;
        self.root_part(id)?;
        let c = self.p.components.iter().find(|c| c.id == id).unwrap();
        if c.grounded || self.part_constrained(id) {
            return Err(refusal(format!(
                "part_place requires a free, ungrounded part; part {id} is grounded or constrained; use joint_set"
            )));
        }
        if !translation_mm.iter().all(|v| v.is_finite()) {
            return Err(refusal("part_place translation must be finite"));
        }
        let mut t = PLACE_IDENTITY;
        if let Some(r) = rotation {
            let length = r.axis.iter().map(|v| v * v).sum::<f64>().sqrt();
            if !r.angle_deg.is_finite() || !length.is_finite() || length < 1e-12 {
                return Err(refusal("part_place rotation axis must be finite and nonzero"));
            }
            t = rot12_axis([0.; 3], r.axis.map(|v| v / length), r.angle_deg);
        }
        t[3] = translation_mm[0];
        t[7] = translation_mm[1];
        t[11] = translation_mm[2];
        if c.transform == t {
            return Ok(self.part_result(id, false));
        }
        self.assembly_edit("part_place", json!({"part":part,"translation_mm":translation_mm,"rotation":rotation}), |s| {
            s.p.set_component_transform(id, t);
            Ok(s.part_result(id, true))
        })
    }
    pub fn ground(&mut self, part: &PartRef, grounded: bool) -> Result<Value> {
        let id = self.resolve_part(part)?;
        self.root_part(id)?;
        if self.p.components.iter().find(|c| c.id == id).unwrap().grounded == grounded {
            return Ok(self.part_result(id, false));
        }
        // Name an unsupported mechanism before solving it: its solve can fail for reasons this server does not model.
        self.check_supported_parts(&HashSet::from([id]))?;
        let before = self.grounding_violations();
        self.assembly_edit("ground", json!({"part":part,"grounded":grounded}), |s| {
            s.p.set_grounded(id, grounded);
            let new: Vec<_> = s.grounding_violations().into_iter().filter(|v| !before.contains(v)).collect();
            if !new.is_empty() {
                return Err(refusal(format!("{}; nothing was changed", new.join("; "))));
            }
            // Grounding touches the joints of this Part; the faults of other joints are not its business.
            let joints: HashSet<Id> =
                s.p.joints
                    .iter()
                    .filter(|j| [j.a, j.b].iter().any(|c| s.p.connector(*c).is_some_and(|c| c.owner == id)))
                    .map(|j| j.id)
                    .collect();
            s.assembly_accept_scoped(None, &joints)?;
            Ok(s.part_result(id, true))
        })
    }
    /// Package A policy, also enforced by joint_create: anchor A's part grounded, a Slider's B part free.
    fn grounding_violations(&self) -> Vec<String> {
        let owner = |cid: Id| self.p.connector(cid).map(|c| c.owner);
        let mut out = Vec::new();
        for j in &self.p.joints {
            let (Some(a), Some(b)) = (owner(j.a), owner(j.b)) else { continue };
            if !self.p.is_effectively_grounded(a) {
                out.push(format!("joint {} needs its anchor part {a} grounded", j.id));
            }
            if j.kind == JointKind::Slider && self.p.is_effectively_grounded(b) {
                out.push(format!("slider joint {} moves part {b}, which must stay ungrounded", j.id));
            }
            if j.kind == JointKind::Revolute && self.p.is_effectively_grounded(b) {
                out.push(format!(
                    "revolute joint {} moves part {b}, which must stay ungrounded (a grounded part does not turn and QymCAD reports nothing, F-159)",
                    j.id
                ));
            }
        }
        out
    }
    pub(crate) fn part_result(&self, id: Id, changed: bool) -> Value {
        let c = self.p.components.iter().find(|c| c.id == id).unwrap();
        json!({"part":{"id":id,"name":crate::localization::name(&c.name),"parent":c.parent},"grounded":c.grounded,"local_transform":c.transform,"world_transform":self.p.world_transform(id),"local_metrics":self.result_bodies().into_iter().filter(|b|b.owner_part==Some(id)).map(|b|json!({"body":b.id,"volume_mm3":b.volume,"bbox_local":b.bbox_local})).collect::<Vec<_>>(),"affected_bodies":self.current_body_ids().into_iter().filter(|b|self.p.body_owner(*b)==Some(id)).collect::<Vec<_>>(),"placements":self.placements(),"diagnostics":self.assembly_diagnostics(),"changed":changed})
    }
    fn part_constrained(&self, id: Id) -> bool {
        self.p.joints.iter().any(|j| [j.a, j.b].iter().any(|c| self.p.connector(*c).is_some_and(|c| c.owner == id)))
            || self.p.mate_constraints.iter().any(|c| {
                c.members.contains(&id)
                    || c.faces.iter().any(|(p, _)| *p == id)
                    || c.anchors.iter().any(|c| self.p.connector(*c).is_some_and(|c| c.owner == id))
            })
    }
    fn resolve_anchor(&mut self, a: &JointAnchor) -> Result<(Id, FaceKey)> {
        if !self.current_body_ids().contains(&a.body) {
            return Err(refusal(format!("joint_create requires a current body; body {} is consumed, suppressed or rolled back", a.body)));
        }
        self.ensure_topology()?;
        let AnchorPick::Face(sel) = &a.pick else { unreachable!("edge anchors resolve through resolve_edge_anchor") };
        let key = self.one_face(a.body, sel)?;
        let topology = self.topology(Some(a.body), false)?;
        let faces: Vec<_> = topology.faces.iter().filter(|f| f.id == key.id).collect();
        if faces.len() != 1 || faces[0].ambiguous_id {
            return Err(Error::Invalid(format!("ambiguous face id {} on body {}", key.id, a.body)));
        }
        if faces[0].kind != crate::FaceKind::Plane {
            return Err(refusal(
                format!("joint_create supports planar face anchors in package A; body {} face {} is {:?}", a.body, key.id, faces[0].kind)
                    .to_lowercase(),
            ));
        }
        if self.p.face_principal_dir(a.body, &key).is_none() {
            return Err(refusal(format!(
                "joint_create cannot resolve a fully oriented frame for body {} face {}; rebuild and choose a usable planar face",
                a.body, key.id
            )));
        }
        let owner = self.p.body_owner(a.body).ok_or_else(|| refusal(format!("body {} has no owning part", a.body)))?;
        self.root_part(owner)?;
        Ok((owner, key))
    }
    /// A straight-edge anchor: the persistent edge id with its Part-local midpoint and direction.
    fn resolve_edge_anchor(&mut self, a: &JointAnchor) -> Result<(Id, u32, [f64; 3], [f64; 3])> {
        if !self.current_body_ids().contains(&a.body) {
            return Err(refusal(format!("joint_create requires a current body; body {} is consumed, suppressed or rolled back", a.body)));
        }
        self.ensure_topology()?;
        let AnchorPick::Edge(sel) = &a.pick else { unreachable!("face anchors resolve through resolve_anchor") };
        let ids = self.resolve_sel(a.body, crate::Element::Edges, sel)?;
        let [id] = ids.as_slice() else {
            return Err(Error::Invalid(format!("the edge selection must match exactly one edge of body {}, it matched {ids:?}", a.body)));
        };
        let topology = self.topology(Some(a.body), false)?;
        let edges: Vec<_> = topology.edges.iter().filter(|e| e.id == *id).collect();
        if edges.len() != 1 {
            return Err(Error::Invalid(format!("ambiguous edge id {id} on body {}", a.body)));
        }
        if edges[0].kind != crate::EdgeKind::Line {
            return Err(refusal(
                format!("revolute joint_create needs a straight edge anchor; body {} edge {id} is {:?}", a.body, edges[0].kind)
                    .to_lowercase(),
            ));
        }
        let e = self
            .p
            .regen_edges
            .get(&a.body)
            .and_then(|es| es.iter().find(|e| e.id == *id))
            .ok_or_else(|| Error::NotFound(format!("edge {id}")))?;
        let (mid, dir) = (e.mid, e.dir);
        let owner = self.p.body_owner(a.body).ok_or_else(|| refusal(format!("body {} has no owning part", a.body)))?;
        self.root_part(owner)?;
        Ok((owner, *id, mid, dir))
    }
    pub fn joint_create(&mut self, o: &JointCreate) -> Result<Value> {
        if let Some(name) = &o.name {
            object_name(name)?;
        }
        let edges = [&o.a, &o.b].iter().filter(|a| matches!(a.pick, AnchorPick::Edge(_))).count();
        if o.kind == AssemblyJointKind::Revolute {
            if edges != 2 {
                return Err(refusal(
                    "revolute joint_create needs a straight edge anchor on each part: a planar face would turn the part about the face normal; nothing was changed",
                ));
            }
            if o.axis.is_some() {
                return Err(refusal("revolute joint_create does not accept axis: the hinge axis is the anchor edges"));
            }
            if o.offset.is_some() {
                return Err(refusal("revolute joint_create does not accept offset; use angle"));
            }
            return self.revolute_create(o);
        }
        if edges != 0 {
            return Err(refusal(format!(
                "{} joint_create needs planar face anchors; edge anchors are for revolute",
                if o.kind == AssemblyJointKind::Rigid { "rigid" } else { "slider" }
            )));
        }
        if o.angle.is_some() {
            return Err(refusal("angle is for revolute joints; use offset for a slider"));
        }
        match (o.kind, o.axis) {
            (AssemblyJointKind::Slider, None) => return Err(refusal("slider joint_create requires axis.base_plane (XY, XZ or YZ)")),
            (AssemblyJointKind::Rigid, Some(_)) => return Err(refusal("rigid joint_create does not accept axis")),
            _ => {}
        }
        if o.kind == AssemblyJointKind::Rigid && o.offset.is_some() {
            return Err(refusal("rigid joint_create does not accept offset"));
        }
        if o.kind == AssemblyJointKind::Rigid && o.limits.is_some() {
            return Err(refusal("rigid joint_create does not accept limits"));
        }
        self.assembly_edit("joint_create", json!({"name":o.name,"kind":o.kind,"mode":o.mode}), |s| {
            let (a, fa) = s.resolve_anchor(&o.a)?;
            let (b, fb) = s.resolve_anchor(&o.b)?;
            if a == b {
                return Err(refusal(format!("joint_create requires anchors in two different parts; both bodies belong to part {a}")));
            }
            if !s.p.is_effectively_grounded(a) {
                let moving = s.p.joints.iter().find(|j| matches!(j.kind, JointKind::Slider | JointKind::Revolute) && s.p.connector(j.b).is_some_and(|c| c.owner == a));
                if let Some(j) = moving {
                    return Err(refusal(format!(
                        "joint_create requires grounded anchor A; part {a} is moved by slider joint {}, so chained mechanisms are not supported in package A",
                        j.id
                    )));
                }
                return Err(refusal(format!("joint_create requires grounded anchor A; ground part {a} first")));
            }
            if o.kind == AssemblyJointKind::Slider && s.p.is_effectively_grounded(b) {
                return Err(refusal(format!("slider target part {b} is grounded; unground it first")));
            }
            s.check_supported_parts(&HashSet::from([a, b]))?;
            let ca = s.p.add_connector(a, AnchorRef::FaceCenter(o.a.body, fa));
            let cb = s.p.add_connector(b, AnchorRef::FaceCenter(o.b.body, fb));
            if let Some(axis) = o.axis {
                for cid in [ca, cb] {
                    s.p.set_connector_axis_ref(cid, Some(AnchorRef::BasePlane(axis.into())));
                }
            }
            let kind = match o.kind {
                AssemblyJointKind::Rigid => JointKind::Rigid,
                _ => JointKind::Slider,
            };
            let j = s.p.add_joint(ca, cb, kind);
            if let Some(name) = &o.name {
                s.p.joints.iter_mut().find(|v| v.id == j).unwrap().name = name.trim().into();
            }
            if o.mode == JointMode::AsBuilt && !s.p.set_joint_as_built(j) {
                return Err(refusal("joint_create cannot capture as_built frames"));
            }
            let mut formulas = false;
            if let (Some(limits), Some(slot)) = (&o.limits, free_slot(kind)) {
                for (min, edit) in [(true, &limits.min), (false, &limits.max)] {
                    if let Some(value) = edit {
                        formulas |= s.write_limit(j, slot, min, &LimitEdit::Set(value.clone()))?;
                    }
                }
                s.check_limits(j)?;
            }
            let wanted = if let Some(value) = &o.offset { Some(s.write_drive(j, 1, value)?) } else { None };
            if formulas || o.offset.as_ref().is_some_and(|v| v.expr().is_some()) {
                s.rebuild_joint_formulas(j)?;
            }
            let applied = s.p.joints.iter().find(|x| x.id == j).and_then(|x| x.drive[1]);
            s.assembly_accept_scoped(applied.map(|value| (j, 1, value)), &HashSet::from([j]))?;
            let mut r = s.joint_result(j, true);
            r["clamped"] = clamp_report(j, 1, wanted, applied);
            if let Some(note) = s.edge_anchor_note() {
                r["note"] = json!(note);
            }
            Ok(r)
        })
    }
    /// The note a result carries while the document holds a joint on an edge or a vertex (F-166): QymCAD.app itself loses such
    /// an anchor on opening a saved file, and a workaround in the file is not ours to make.
    pub fn edge_anchor_note(&self) -> Option<&'static str> {
        use qymcad_core::feature::AnchorRef;
        let on_edge =
            |c: Id| self.p.connectors.iter().any(|k| k.id == c && matches!(k.anchor, AnchorRef::EdgeMid(..) | AnchorRef::Vertex(..)));
        self.p.joints.iter().any(|j| on_edge(j.a) || on_edge(j.b)).then_some(EDGE_ANCHOR_NOTE)
    }
    /// A Revolute hinge on two straight edges, as the app's Joint tool makes it with an edge picked on each Part
    /// (`EdgeMid` connectors, `add_joint`, as built captured before the first solve when asked). As built, the axes
    /// must already be collinear: natively the part turns about B's edge and ignores A's (F-162). Align puts B's edge
    /// frame on A's, which is correct as measured (F-163), so it is allowed with any placement.
    fn revolute_create(&mut self, o: &JointCreate) -> Result<Value> {
        self.assembly_edit("joint_create", json!({"name":o.name,"kind":o.kind,"mode":o.mode}), |s| {
            let (a, ea, ma, da) = s.resolve_edge_anchor(&o.a)?;
            let (b, eb, mb, db) = s.resolve_edge_anchor(&o.b)?;
            if a == b {
                return Err(refusal(format!("joint_create requires anchors in two different parts; both bodies belong to part {a}")));
            }
            if !s.p.is_effectively_grounded(a) {
                return Err(refusal(format!("joint_create requires grounded anchor A; ground part {a} first")));
            }
            if s.p.is_effectively_grounded(b) {
                return Err(refusal(format!(
                    "revolute target part {b} is grounded; unground it first (a grounded part does not turn and QymCAD reports nothing, F-159)"
                )));
            }
            s.check_supported_parts(&HashSet::from([a, b]))?;
            let world = |part: Id, m: [f64; 3], d: [f64; 3]| {
                let t = s.p.world_transform(part);
                let point = std::array::from_fn::<f64, 3, _>(|r| t[r * 4] * m[0] + t[r * 4 + 1] * m[1] + t[r * 4 + 2] * m[2] + t[r * 4 + 3]);
                let dir = std::array::from_fn::<f64, 3, _>(|r| t[r * 4] * d[0] + t[r * 4 + 1] * d[1] + t[r * 4 + 2] * d[2]);
                let n = dir.iter().map(|v| v * v).sum::<f64>().sqrt();
                (point, dir.map(|v| v / n))
            };
            let ((pa, ua), (pb, ub)) = (world(a, ma, da), world(b, mb, db));
            let dot = (ua[0] * ub[0] + ua[1] * ub[1] + ua[2] * ub[2]).abs().min(1.0);
            let angle = dot.acos().to_degrees();
            let w = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
            let cross = [w[1] * ua[2] - w[2] * ua[1], w[2] * ua[0] - w[0] * ua[2], w[0] * ua[1] - w[1] * ua[0]];
            let distance = cross.iter().map(|v| v * v).sum::<f64>().sqrt();
            if o.mode == JointMode::AsBuilt && (distance > 1e-3 || angle > 1e-2) {
                let round = |v: f64| (v * 1e3).round() / 1e3;
                return Err(refusal(format!(
                    "revolute hinge axes are not collinear (distance {} mm, angle {}°): QymCAD would turn the part about its own edge and ignore the anchor's; place the part on the hinge first; nothing was changed",
                    round(distance),
                    round(angle)
                )));
            }
            let ca = s.p.add_connector(a, AnchorRef::EdgeMid(o.a.body, ea));
            let cb = s.p.add_connector(b, AnchorRef::EdgeMid(o.b.body, eb));
            let j = s.p.add_joint(ca, cb, JointKind::Revolute);
            if let Some(name) = &o.name {
                s.p.joints.iter_mut().find(|v| v.id == j).unwrap().name = name.trim().into();
            }
            if o.mode == JointMode::AsBuilt && !s.p.set_joint_as_built(j) {
                return Err(refusal("joint_create cannot capture as_built frames"));
            }
            let mut formulas = false;
            if let Some(limits) = &o.limits {
                for (min, edit) in [(true, &limits.min), (false, &limits.max)] {
                    if let Some(value) = edit {
                        formulas |= s.write_limit(j, 0, min, &LimitEdit::Set(value.clone()))?;
                    }
                }
                s.check_limits(j)?;
            }
            let wanted = if let Some(value) = &o.angle { Some(s.write_drive(j, 0, value)?) } else { None };
            s.check_angle_domain(j)?;
            if formulas || o.angle.as_ref().is_some_and(|v| v.expr().is_some()) {
                s.rebuild_joint_formulas(j)?;
            }
            let applied = s.p.joints.iter().find(|x| x.id == j).and_then(|x| x.drive[0]);
            s.assembly_accept_scoped(applied.map(|value| (j, 0, value)), &HashSet::from([j]))?;
            let mut r = s.joint_result(j, true);
            r["clamped"] = clamp_report(j, 0, wanted, applied);
            if let Some(note) = s.edge_anchor_note() {
                r["note"] = json!(note);
            }
            Ok(r)
        })
    }
    /// A Revolute drive must stay in (−180, 180]: QymCAD reads the angle through atan2, wrapped to that range, with
    /// no unwrapping (F-158), so a drive outside it never matches its reading.
    fn check_angle_domain(&self, id: Id) -> Result<()> {
        let j = self.p.joints.iter().find(|j| j.id == id).unwrap();
        if j.kind != JointKind::Revolute {
            return Ok(());
        }
        if let Some(v) = j.drive[0] {
            if !(v > -180.0 && v <= 180.0) {
                return Err(refusal(format!(
                    "joint {id} angle {v}° is outside (-180, 180]: QymCAD reads angles wrapped to (-180, 180], so the reading would disagree with the drive (F-158); nothing was changed"
                )));
            }
        }
        Ok(())
    }
    /// Write a slot drive as the app does (`set_joint_drive` clamps it into the limits); returns the requested value.
    fn write_drive(&mut self, id: Id, slot: usize, value: &Num) -> Result<f64> {
        let v = value.eval(&self.p.param_map())?;
        let key = SLOT_KEYS[slot];
        if let Some(expr) = value.expr() {
            self.p
                .check_dim_target_text(&qymcad_core::model::DimTarget::Feature { node: id, key: key.into() }, &expr)
                .map_err(|e| Error::Expr(e.to_string()))?;
        }
        let expression = value.expr().unwrap_or_default();
        self.p.set_feat_dim(id, key, expression);
        self.p.set_joint_drive(id, slot, Some(v));
        Ok(v)
    }
    /// One bound of the app's Limits section: the value straight into the joint (`popup.rs`), a formula under
    /// `feat_dims[joint]["min<slot>"|"max<slot>"]`, imposed on rebuild. Clear ticks the bound off as the app does: the
    /// value goes, the formula stays dormant. A number typed into the field drops the formula. Returns whether a
    /// formula was written.
    fn write_limit(&mut self, id: Id, slot: usize, min: bool, edit: &LimitEdit) -> Result<bool> {
        let key = limit_key(min, slot);
        let value = match edit {
            LimitEdit::Keep => return Ok(false),
            LimitEdit::Clear => None,
            LimitEdit::Set(value) => {
                let v = value.eval(&self.p.param_map())?;
                if let Some(expr) = value.expr() {
                    self.p
                        .check_dim_target_text(&qymcad_core::model::DimTarget::Feature { node: id, key: key.clone() }, &expr)
                        .map_err(|e| Error::Expr(e.to_string()))?;
                }
                Some(v)
            }
        };
        let mut written = false;
        if let LimitEdit::Set(value) = edit {
            let formula = value.expr().unwrap_or_default();
            written = !formula.is_empty();
            self.p.set_feat_dim(id, &key, formula);
        }
        let j = self.p.joints.iter_mut().find(|j| j.id == id).unwrap();
        if min {
            j.limit_min[slot] = value;
        } else {
            j.limit_max[slot] = value;
        }
        Ok(written)
    }
    /// The limits the server accepts on a joint's free slot (`limit_problem`): a typed number inside the app's field
    /// range, angle bounds inside the wrapped reading range, min not above max (natively every value would go to max,
    /// F-157).
    pub(crate) fn check_limits(&self, id: Id) -> Result<()> {
        match limit_problem(&self.p, id)? {
            Some(message) => Err(refusal(format!("{message}; nothing was changed"))),
            None => Ok(()),
        }
    }
    fn rebuild_joint_formulas(&mut self, joint: Id) -> Result<()> {
        self.validate_joint_formulas(Some(&HashSet::from([joint])))?;
        let r = self.rebuild();
        if !r.errors.is_empty() {
            return Err(Error::Rebuild(r.errors.iter().map(|e| e.message.clone()).collect()));
        }
        Ok(())
    }
    pub fn joint_set(&mut self, joint: &JointRef, offset: Option<&Num>, release: Option<&str>) -> Result<Value> {
        match release {
            Some("angle") if offset.is_none() => self.joint_set_slot(joint, JointSlot::Angle, None, true),
            Some(v) if v != "offset" => Err(refusal("joint_set requires exactly one of offset or release")),
            _ => self.joint_set_slot(joint, JointSlot::Offset, offset, release.is_some()),
        }
    }
    /// The app's value field of a joint's free slot (Slider offset in mm, Revolute angle in degrees), or its
    /// release. A value outside the limits is clamped natively and reported.
    pub fn joint_set_slot(&mut self, joint: &JointRef, slot: JointSlot, value: Option<&Num>, release: bool) -> Result<Value> {
        let name = if slot == JointSlot::Angle { "angle" } else { "offset" };
        if value.is_some() == release {
            return Err(refusal(format!("joint_set requires exactly one of {name} or release")));
        }
        let id = self.resolve_joint(joint)?;
        let j = self.p.joints.iter().find(|j| j.id == id).unwrap().clone();
        let index = if slot == JointSlot::Angle { 0 } else { 1 };
        if free_slot(j.kind) != Some(index) {
            return Err(refusal(format!("joint {id} has no editable {name} slot for kind {}", kind_name(j.kind))));
        }
        let owners: HashSet<_> = [j.a, j.b].iter().filter_map(|id| self.p.connector(*id).map(|c| c.owner)).collect();
        self.check_supported_parts(&owners)?;
        for owner in owners {
            self.root_part(owner)?;
        }
        let previous = [j.angle, j.offset, j.offset2][index];
        let old_expression = self.p.feat_dim(id, name).map(str::to_string);
        let wanted = value.map(|value| value.eval(&self.p.param_map())).transpose()?;
        let clamped = wanted.map(|v| j.clamp_slot(index, v));
        let same = match value {
            Some(value) => old_expression == value.expr() && j.drive[index] == clamped,
            None => old_expression.is_none() && j.drive[index].is_none(),
        };
        let finish = |s: &Self, r: &mut Value, applied: Option<f64>| {
            r["previous"] = json!(previous);
            r["requested"] = json!(value);
            r["evaluated"] = json!(wanted);
            r["clamped"] = clamp_report(id, index, wanted, applied);
            let _ = s;
        };
        if same {
            let mut r = self.joint_result(id, false);
            finish(self, &mut r, clamped);
            return Ok(r);
        }
        let args = if slot == JointSlot::Angle {
            json!({"joint":joint,"angle":value,"release":release.then_some("angle")})
        } else {
            json!({"joint":joint,"offset":value,"release":release.then_some("offset")})
        };
        self.assembly_edit("joint_set", args, |s| {
            if let Some(value) = value {
                s.write_drive(id, index, value)?;
            } else {
                s.p.set_feat_dim(id, name, String::new());
                s.p.set_joint_drive(id, index, None);
            }
            s.check_angle_domain(id)?;
            if value.is_some_and(|v| v.expr().is_some()) {
                s.rebuild_joint_formulas(id)?;
            }
            let applied = s.p.joints.iter().find(|j| j.id == id).and_then(|j| j.drive[index]);
            s.assembly_accept_scoped(wanted.and(applied).map(|value| (id, index, value)), &HashSet::from([id]))?;
            let mut r = s.joint_result(id, true);
            finish(s, &mut r, applied);
            Ok(r)
        })
    }
    pub fn joint_delete(&mut self, joint: &JointRef) -> Result<Value> {
        let id = self.resolve_joint(joint)?;
        let j = self.p.joints.iter().find(|j| j.id == id).unwrap().clone();
        let owners: HashSet<_> = [j.a, j.b].iter().filter_map(|id| self.p.connector(*id).map(|c| c.owner)).collect();
        self.check_supported_parts(&owners)?;
        self.assembly_edit("joint_delete",json!({"joint":joint}),|s|{
            s.p.joints.retain(|j|j.id!=id);s.p.feat_dims.remove(&id);
            let mut removed=Vec::new();
            for cid in [j.a,j.b] {if s.p.connector(cid).is_some_and(|c|!c.standalone) && s.p.connector_users(cid).is_empty() {s.p.connectors.retain(|c|c.id!=cid);s.p.feat_dims.remove(&cid);removed.push(cid);}}
            s.assembly_accept_scoped(None, &HashSet::new())?;Ok(json!({"removed_joint":id,"removed_connectors":removed,"placements":s.placements(),"diagnostics":s.assembly_diagnostics(),"changed":true}))
        })
    }
    /// The app's Limits section of the joint box for the joint's free slot. A bound that excludes the current value
    /// clamps it, as natively; the clamp is reported, never silent.
    pub fn joint_limits(&mut self, joint: &JointRef, slot: Option<JointSlot>, min: &LimitEdit, max: &LimitEdit) -> Result<Value> {
        let id = self.resolve_joint(joint)?;
        let j = self.p.joints.iter().find(|j| j.id == id).unwrap().clone();
        let Some(free) = free_slot(j.kind) else {
            return Err(refusal(format!("joint {id} of kind {} has no free slot to limit", kind_name(j.kind))));
        };
        if let Some(named) = slot {
            let index = if named == JointSlot::Angle { 0 } else { 1 };
            if index != free {
                return Err(refusal(format!(
                    "joint {id} has no free {} slot; {} limits apply to {}",
                    SLOT_KEYS[index],
                    kind_name(j.kind),
                    SLOT_KEYS[free]
                )));
            }
        }
        if matches!((min, max), (LimitEdit::Keep, LimitEdit::Keep)) {
            return Err(refusal("joint_limits requires min or max"));
        }
        let owners: HashSet<_> = [j.a, j.b].iter().filter_map(|id| self.p.connector(*id).map(|c| c.owner)).collect();
        self.check_supported_parts(&owners)?;
        for owner in &owners {
            self.root_part(*owner)?;
        }
        let state = |s: &Self| {
            let j = s.p.joints.iter().find(|j| j.id == id).unwrap();
            (
                j.limit_min,
                j.limit_max,
                s.p.feat_dim(id, &limit_key(true, free)).map(str::to_string),
                s.p.feat_dim(id, &limit_key(false, free)).map(str::to_string),
            )
        };
        let current = state(self);
        let mut probe = self.p.clone_without_source_data();
        std::mem::swap(&mut self.p, &mut probe);
        let planned = (|| {
            self.write_limit(id, free, true, min)?;
            self.write_limit(id, free, false, max)?;
            Ok::<_, Error>(state(self))
        })();
        std::mem::swap(&mut self.p, &mut probe);
        if planned? == current {
            let mut r = self.joint_result(id, false);
            r["clamped"] = Value::Null;
            return Ok(r);
        }
        let args = json!({"joint":joint,"slot":slot,"min":limit_arg(min),"max":limit_arg(max)});
        self.assembly_edit("joint_limits", args, |s| {
            let mut formulas = s.write_limit(id, free, true, min)?;
            formulas |= s.write_limit(id, free, false, max)?;
            s.check_limits(id)?;
            // A drive formula reasserts on regeneration (F-144): rebuild so the result is the settled state.
            if formulas || SLOT_KEYS.iter().any(|k| s.p.feat_dim(id, k).is_some_and(|e| !e.trim().is_empty())) {
                s.rebuild_joint_formulas(id)?;
            }
            // What the slot asks for without the new bounds: its drive formula, else its drive, else its reading.
            let formula = s.p.feat_dim(id, SLOT_KEYS[free]).filter(|e| !e.trim().is_empty()).map(str::to_string);
            let requested = match formula {
                Some(expr) => Num::Expr(expr).eval(&s.p.param_map())?,
                None => j.drive[free].unwrap_or([j.angle, j.offset, j.offset2][free]),
            };
            s.assembly_accept_scoped(None, &HashSet::from([id]))?;
            let after = s.p.joints.iter().find(|j| j.id == id).unwrap();
            let applied = after.drive[free].unwrap_or([after.angle, after.offset, after.offset2][free]);
            let mut r = s.joint_result(id, true);
            r["clamped"] = if (requested - applied).abs() > 1e-6 {
                json!({"joint":id,"slot":free,"requested":requested,"applied":applied})
            } else {
                Value::Null
            };
            Ok(r)
        })
    }
    pub(crate) fn component_info(&self) -> Vec<crate::ComponentInfo> {
        let bodies = self.current_body_ids();
        let links = crate::instances::instance_links(&self.p);
        self.p
            .components
            .iter()
            .map(|c| crate::ComponentInfo {
                id: c.id,
                name: crate::localization::name(&c.name),
                parent: c.parent,
                kind: if c.kind == ComponentKind::Part { "part" } else { "assembly" },
                local_transform: c.transform,
                world_transform: self.p.world_transform(c.id),
                grounded: c.grounded,
                bodies: bodies.iter().copied().filter(|b| self.p.body_owner(*b) == Some(c.id)).collect(),
                instance_of: links.get(&c.id).copied(),
                instances: crate::instances::instances_of(&links, c.id),
            })
            .collect()
    }
    pub(crate) fn placements(&self) -> Vec<Value> {
        self.p
            .components
            .iter()
            .map(|c| json!({"id":c.id,"local_transform":c.transform,"world_transform":self.p.world_transform(c.id)}))
            .collect()
    }
    fn joint_result(&self, id: Id, changed: bool) -> Value {
        let j = self.p.joints.iter().find(|j| j.id == id).unwrap();
        // The reported slot: the kind's free slot (Revolute angle), else the travel slot as ADR 0051 reports it.
        let slot = free_slot(j.kind).unwrap_or(1);
        let key = SLOT_KEYS[slot];
        let anchor = |cid| {
            self.p.connector(cid).map(|c|json!({"connector":cid,"owner_part":c.owner,"anchor":c.anchor,"body":match &c.anchor {AnchorRef::FaceCenter(b,_)|AnchorRef::EdgeMid(b,_)=>Some(*b),_=>None},"face":match &c.anchor {AnchorRef::FaceCenter(_,f)=>Some(f.id),_=>None},"edge":match &c.anchor {AnchorRef::EdgeMid(_,e)=>Some(*e),_=>None},"axis_ref":c.axis_ref,"frame_local":self.p.connector_matrix(cid),"frame_for_kind_local":self.p.connector_frame_for_kind(c,j.kind).map(|f|f.matrix12())}))
        };
        json!({"joint":{"id":id,"name":crate::localization::name(&j.name),"kind":kind_name(j.kind)},"connector_ids":[j.a,j.b],"a":anchor(j.a),"b":anchor(j.b),"mode":if j.as_built.is_some(){"as_built"}else{"align"},"as_built":j.as_built,"flip":j.flip,"roll_flip":j.roll_flip,"flip_decided":j.flip_decided,"offset":j.offset,"angle":j.angle,"read":([j.angle,j.offset,j.offset2][slot]),"unit":(if slot==0{"deg"}else{"mm"}),"free_slots":j.kind.free_slots(),"drive":j.drive,"expression":self.p.feat_dim(id,key),"expressions":self.p.feat_dims.get(&id),"driven":j.drive[slot].is_some(),"state":if self.p.feat_dim(id,key).is_some(){"formula"}else if j.drive[slot].is_some(){"numeric"}else{"free"},"signed_world_axis":self.p.joint_slot_axis(id,slot,self.p.root),"limit_min":j.limit_min,"limit_max":j.limit_max,"limit_formulas":self.limit_formulas(id,false),"dormant_limit_formulas":self.limit_formulas(id,true),"at_limit":self.at_limit(id),"placements":self.placements(),"diagnostics":self.assembly_diagnostics(),"changed":changed})
    }
    /// Formulas of the free slot's bounds, as `{min?, max?}`: those a bound that is on reads, or (`dormant`) those a
    /// ticked-off bound keeps unread (F-164).
    fn limit_formulas(&self, id: Id, dormant: bool) -> Value {
        let mut out = serde_json::Map::new();
        if let Some(slot) = self.p.joints.iter().find(|j| j.id == id).and_then(|j| free_slot(j.kind)) {
            for (name, min) in [("min", true), ("max", false)] {
                let key = limit_key(min, slot);
                let expr = self.p.feat_dim(id, &key).filter(|e| !e.trim().is_empty());
                if let Some(expr) = expr.filter(|_| dormant_limit_formula(&self.p, id, &key) == dormant) {
                    out.insert(name.into(), json!(expr));
                }
            }
        }
        Value::Object(out)
    }
    /// The joint's free slots standing on a bound, in the one `{joint, slot}` format of `at_limit`.
    fn at_limit(&self, id: Id) -> Vec<Value> {
        at_limit_list(&self.p).into_iter().filter(|v| v["joint"] == id).collect()
    }
    pub fn assembly_info(&self) -> Value {
        let diagnostics = self.assembly_diagnostics();
        let mut value = json!({"root":self.p.root,"active_component":self.p.active_component,"components":self.component_info(),"parts":self.component_info().into_iter().filter(|c|c.kind=="part").collect::<Vec<_>>(),"connectors":self.p.connectors,"joints":self.p.joints.iter().map(|j|json!({"id":j.id,"name":crate::localization::name(&j.name),"kind":kind_name(j.kind),"free_slots":j.kind.free_slots(),"a":j.a,"b":j.b,"as_built":j.as_built,"flip":j.flip,"roll_flip":j.roll_flip,"drive":j.drive,"read":[j.angle,j.offset,j.offset2],"slots":[{"name":"angle","unit":"deg","read":j.angle,"drive":j.drive[0],"formula":self.p.feat_dim(j.id,"angle")},{"name":"offset","unit":if j.kind==JointKind::Ball {"deg"}else{"mm"},"read":j.offset,"drive":j.drive[1],"formula":self.p.feat_dim(j.id,"offset")},{"name":"offset2","unit":if j.kind==JointKind::Ball {"deg"}else{"mm"},"read":j.offset2,"drive":j.drive[2],"formula":self.p.feat_dim(j.id,"offset2")}],"expressions":self.p.feat_dims.get(&j.id),"limit_min":j.limit_min,"limit_max":j.limit_max,"limit_formulas":self.limit_formulas(j.id,false),"dormant_limit_formulas":self.limit_formulas(j.id,true),"at_limit":self.at_limit(j.id),"signed_world_axis":self.p.joint_slot_axis(j.id,free_slot(j.kind).unwrap_or(1),self.p.root)})).collect::<Vec<_>>(),"constraints":self.p.mate_constraints,"relations":self.p.relations,"diagnostics":diagnostics,"healthy":diagnostics["healthy"]});
        for key in ["joint_faults", "faulty_joints", "violated_joints", "conflict", "solve_errors", "limit_faults"] {
            value[key] = diagnostics[key].clone();
        }
        value
    }
    /// Mechanism health for rebuild reports: `null` when the document has no
    /// joints, mates or relations, so plain-part results keep their shape.
    pub(crate) fn rebuild_assembly(&self) -> Value {
        if self.p.joints.is_empty() && self.p.mate_constraints.is_empty() && self.p.relations.is_empty() {
            return Value::Null;
        }
        self.assembly_diagnostics()
    }
    /// Mechanism health as a read judges it: every joint, nothing refused. `clamped` lists the drive formulas a limit
    /// holds and the numeric drives a native solve clamped since the call began (`drive_clamps`); `limit_faults`
    /// lists the joints whose limits or drive the app accepted and the server refuses to build on (an edit that
    /// touches them is refused; others are not).
    pub(crate) fn assembly_diagnostics(&self) -> Value {
        let mut p = self.p.clone_without_source_data();
        let mut d = inspect(&mut p, None, None);
        let logged = self.logged_clamps();
        if let Some(list) = d["clamped"].as_array_mut() {
            list.extend(logged);
        }
        d["limit_faults"] = json!(limit_faults(&self.p));
        d
    }
    /// Remember the numeric drives a native solve moved: the request is lost once the drive is overwritten with its
    /// clamped value, and nothing else says it was ever asked (F-165). `before` is `numeric_drives` ahead of the solve.
    pub(crate) fn note_drive_clamps(&mut self, before: &[(Id, usize, f64)]) {
        for &(id, slot, was) in before {
            let Some(now) = self.p.joints.iter().find(|j| j.id == id).and_then(|j| j.drive[slot]) else { continue };
            if (now - was).abs() <= 1e-9 {
                continue;
            }
            match self.drive_clamps.iter_mut().find(|c| c.0 == id && c.1 == slot) {
                Some(c) if (c.3 - was).abs() <= 1e-9 => c.3 = now,
                Some(c) => *c = (id, slot, was, now),
                None => self.drive_clamps.push((id, slot, was, now)),
            }
        }
    }
    /// The recorded clamps that still describe the document: the drive holds the applied value and no formula drives it.
    fn logged_clamps(&self) -> Vec<Value> {
        let numeric = numeric_drives(&self.p);
        self.drive_clamps
            .iter()
            .filter(|c| numeric.iter().any(|n| n.0 == c.0 && n.1 == c.1 && (n.2 - c.3).abs() <= 1e-9))
            .map(|c| json!({"joint":c.0,"slot":c.1,"requested":c.2,"applied":c.3}))
            .collect()
    }
    /// Acceptance of the whole document (every joint is judged); the unit tests of the mechanism use it.
    #[cfg(test)]
    pub(crate) fn assembly_accept(&mut self, requested: Option<(Id, usize, f64)>) -> Result<()> {
        self.accept_in_scope(requested, None)
    }
    /// Acceptance of an edit that touches only `scope`'s joints: other joints are not validated for limits or domain.
    pub(crate) fn assembly_accept_scoped(&mut self, requested: Option<(Id, usize, f64)>, scope: &HashSet<Id>) -> Result<()> {
        self.accept_in_scope(requested, Some(scope))
    }
    fn accept_in_scope(&mut self, requested: Option<(Id, usize, f64)>, scope: Option<&HashSet<Id>>) -> Result<()> {
        self.validate_joint_formulas(scope)?;
        let drives = numeric_drives(&self.p);
        let d = inspect(&mut self.p, requested, scope);
        self.note_drive_clamps(&drives);
        if d["healthy"] == true {
            Ok(())
        } else {
            Err(solve_refusal(&d))
        }
    }
    /// Refuse a formula that cannot be worked out, and limits or an angle formula the server will not build on (F-157,
    /// F-158), for the joints in `scope` (`None`: all). An edit is judged on the joints it touched; the others are
    /// reported by `limit_faults`, so one fault the app accepted never locks the document. Formulas nothing reads (a
    /// ticked-off bound's) are not looked at.
    pub(crate) fn validate_joint_formulas(&self, scope: Option<&HashSet<Id>>) -> Result<()> {
        let vars = self.p.param_map();
        for j in self.p.joints.iter().filter(|j| scope.is_none_or(|s| s.contains(&j.id))) {
            for key in joint_keys() {
                if let Some(expr) = live_formula(&self.p, j.id, &key) {
                    Num::Expr(expr.into()).eval(&vars)?;
                    self.p
                        .check_dim_target_text(&qymcad_core::model::DimTarget::Feature { node: j.id, key }, expr)
                        .map_err(|e| Error::Expr(e.to_string()))?;
                }
            }
            self.check_limits(j.id)?;
            if j.kind == JointKind::Revolute {
                if let Some(expr) = live_formula(&self.p, j.id, "angle") {
                    let v = Num::Expr(expr.into()).eval(&vars)?;
                    let (min, max) = evaluated_limits(&self.p, j.id, 0)?;
                    let held = max.map_or(v, |hi| v.min(hi));
                    let held = min.map_or(held, |lo| held.max(lo));
                    if !(held > -180.0 && held <= 180.0) {
                        return Err(refusal(format!(
                            "joint {} angle {v}° is outside (-180, 180]: QymCAD reads angles wrapped to (-180, 180], so the reading would disagree with the drive (F-158); nothing was changed",
                            j.id
                        )));
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) fn check_joint_anchor_removal(&self, gone: &HashSet<Id>) -> Result<()> {
        let mut ids: Vec<_> = self
            .p
            .joints
            .iter()
            .filter(|j| {
                [j.a, j.b].iter().filter_map(|cid| self.p.connector(*cid)).any(|c| match c.anchor {
                    AnchorRef::FaceCenter(b, _) | AnchorRef::EdgeMid(b, _) | AnchorRef::Vertex(b, _, _) => gone.contains(&b),
                    _ => false,
                })
            })
            .map(|j| j.id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            Ok(())
        } else {
            Err(refusal(format!("edit would invalidate anchors of joints {ids:?}; delete those joints first; nothing was changed")))
        }
    }
    pub(crate) fn assembly_accept_edit(&mut self, before: &Project) -> Result<()> {
        Self::check_instance_policy(before, &self.p, true)?;
        let touched = self.assembly_touched_parts(before);
        // Deleting the last unsupported recipe node must not bypass the affected-state guard.
        Self::check_supported_parts_in(before, &touched)?;
        self.check_supported_parts(&touched)?;
        if self.p.joints.is_empty()
            && self.p.mate_constraints.is_empty()
            && before.joints.is_empty()
            && self.p.components.iter().all(|c| rigid(&c.transform))
        {
            return Ok(());
        }
        let scope = touched_joints(before, &self.p);
        self.validate_joint_formulas(Some(&scope))?;
        let drives = numeric_drives(&self.p);
        let d = inspect(&mut self.p, None, Some(&scope));
        self.note_drive_clamps(&drives);
        if d["healthy"] == true {
            return Ok(());
        }
        let existing = |key: &str| -> Vec<u64> {
            let mut ids: Vec<_> = d[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .filter(|id| before.joints.iter().any(|j| j.id == *id))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        // Lost anchors and unsatisfiable joints need different remedies; do not call a conflict a lost anchor.
        let lost = existing("faulty_joints");
        let violated = existing("violated_joints");
        if !lost.is_empty() {
            Err(refusal(format!("edit would invalidate anchors of joints {lost:?}; delete those joints first; nothing was changed")))
        } else if !violated.is_empty() {
            Err(refusal(format!(
                "edit leaves joints {violated:?} unsatisfied (conflicting joints, drives or grounding); nothing was changed"
            )))
        } else {
            Err(solve_refusal(&d))
        }
    }
    fn assembly_touched_parts(&self, before: &Project) -> HashSet<Id> {
        let mut touched = HashSet::new();
        for c in &self.p.components {
            if before
                .components
                .iter()
                .find(|old| old.id == c.id)
                .is_none_or(|old| old.transform.iter().zip(c.transform).any(|(a, b)| (a - b).abs() > 1e-5) || old.grounded != c.grounded)
            {
                touched.insert(c.id);
            }
        }
        for (p, other) in [(before, &self.p), (&self.p, before)] {
            for n in &p.timeline {
                let changed = other.timeline.iter().find(|old| old.id == n.id).is_none_or(|old| {
                    old.parent != n.parent
                        || old.suppressed != n.suppressed
                        || serde_json::to_value(&old.kind).ok() != serde_json::to_value(&n.kind).ok()
                });
                let crosses = before.timeline.iter().take(before.rollback_place()).any(|old| old.id == n.id)
                    != self.p.timeline.iter().take(self.p.rollback_place()).any(|old| old.id == n.id);
                if changed || crosses {
                    if let Some(part) = n.parent {
                        touched.insert(part);
                    }
                }
            }
            for sk in &p.sketches {
                if other
                    .sketches
                    .iter()
                    .find(|old| old.id == sk.id)
                    .is_none_or(|old| serde_json::to_value(old).ok() != serde_json::to_value(sk).ok())
                {
                    if let Some(part) = p.sketch_owner(sk.id) {
                        touched.insert(part);
                    }
                }
            }
            for (id, exprs) in &p.feat_dims {
                if other.feat_dims.get(id) != Some(exprs) {
                    if let Some(part) = p
                        .timeline
                        .iter()
                        .find(|n| n.id == *id)
                        .and_then(|n| n.parent)
                        .or_else(|| p.ref_owner(*id))
                        .or_else(|| p.plane_owner(*id))
                    {
                        touched.insert(part);
                    }
                    for j in p.joints.iter().filter(|j| j.id == *id) {
                        for cid in [j.a, j.b] {
                            if let Some(c) = p.connector(cid) {
                                touched.insert(c.owner);
                            }
                        }
                    }
                }
            }
        }
        for j in &self.p.joints {
            let changed = before.joints.iter().find(|old| old.id == j.id).is_none_or(|old| {
                old.a != j.a
                    || old.b != j.b
                    || old.kind != j.kind
                    || old.drive.iter().zip(j.drive).any(|(a, b)| match (a, b) {
                        (Some(a), Some(b)) => (a - b).abs() > 1e-5,
                        (None, None) => false,
                        _ => true,
                    })
            });
            if changed {
                for cid in [j.a, j.b] {
                    if let Some(c) = self.p.connector(cid) {
                        touched.insert(c.owner);
                    }
                }
            }
        }
        touched
    }
    pub(crate) fn check_supported_parts(&self, parts: &HashSet<Id>) -> Result<()> {
        Self::check_supported_parts_in(&self.p, parts)
    }
    fn check_supported_parts_in(p: &Project, parts: &HashSet<Id>) -> Result<()> {
        let mut connected = parts.clone();
        loop {
            let old = connected.len();
            for j in &p.joints {
                let owners: Vec<_> = [j.a, j.b].iter().filter_map(|cid| p.connector(*cid).map(|c| c.owner)).collect();
                if owners.iter().any(|id| connected.contains(id)) {
                    connected.extend(owners);
                }
            }
            if old == connected.len() {
                break;
            }
        }
        // Imported geometry trees remain supported. Only affected nested mechanisms are excluded.
        let mut nested: Vec<_> = p
            .components
            .iter()
            .filter(|c| {
                connected.contains(&c.id)
                    && c.id != p.root
                    && c.parent != Some(p.root)
                    && p.joints.iter().any(|j| [j.a, j.b].iter().filter_map(|cid| p.connector(*cid)).any(|anchor| anchor.owner == c.id))
            })
            .map(|c| c.parent.unwrap_or(c.id))
            .collect();
        nested.sort_unstable();
        if let Some(id) = nested.first() {
            return Err(refusal(format!("component {id} is a nested assembly; nested assembly editing is not supported in package A")));
        }
        let mut ids = Vec::new();
        for j in &p.joints {
            if [j.a, j.b].iter().filter_map(|c| p.connector(*c)).any(|c| connected.contains(&c.owner))
                && !matches!(j.kind, JointKind::Rigid | JointKind::Slider | JointKind::Revolute)
            {
                ids.push(j.id);
            }
        }
        for c in &p.mate_constraints {
            if c.members.iter().any(|p| connected.contains(p))
                || c.faces.iter().any(|(p, _)| connected.contains(p))
                || c.anchors.iter().filter_map(|cid| p.connector(*cid)).any(|c| connected.contains(&c.owner))
            {
                ids.push(c.id);
            }
        }
        for r in &p.relations {
            if p.joints
                .iter()
                .filter(|j| j.id == r.a || j.id == r.b)
                .flat_map(|j| [j.a, j.b])
                .filter_map(|cid| p.connector(cid))
                .any(|c| connected.contains(&c.owner))
            {
                ids.push(r.id);
            }
        }
        for n in &p.timeline {
            let affected = match &n.kind {
                qymcad_core::feature::FeatureKind::MirrorPart { src_comp, .. } => {
                    n.parent.is_some_and(|id| connected.contains(&id)) || connected.contains(src_comp)
                }
                qymcad_core::feature::FeatureKind::ComponentPattern { src, copies, .. } => {
                    connected.contains(src) || copies.iter().any(|id| connected.contains(id))
                }
                qymcad_core::feature::FeatureKind::Piece { src, .. } => {
                    n.parent.is_some_and(|id| connected.contains(&id)) || p.body_owner(*src).is_some_and(|id| connected.contains(&id))
                }
                _ => false,
            };
            if affected {
                return Err(refusal(format!(
                    "assembly feature {} uses derived components; derived component editing is not supported in package A",
                    n.id
                )));
            }
            if let qymcad_core::feature::FeatureKind::PartInstance { src_comp, .. } = n.kind {
                if n.parent.is_some_and(|id| connected.contains(&id)) || connected.contains(&src_comp) {
                    // ADR 0053: a clone of a source with at most one solid body is a Part of its own for assembly
                    // edits; a bodiless source is a visible native error or an unbuilt node, not a silent one.
                    if crate::instances::solid_bodies(p, src_comp).len() > 1 {
                        return Err(refusal("part instances are not supported for multi-body source parts in package A"));
                    }
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        if !ids.is_empty() {
            return Err(refusal(format!(
                "assembly edit touches unsupported constraints or relations {ids:?}; package A supports root-level Rigid, Slider and Revolute joints"
            )));
        }
        Ok(())
    }
}
