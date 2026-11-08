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
#[derive(Clone, Debug)]
pub struct JointAnchor {
    pub body: Id,
    pub face: Sel,
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
}
use qymcad_core::{
    feature::{rot12_axis, AnchorRef, ComponentKind, FaceKey, JointKind, PLACE_IDENTITY},
    model::Project,
};
use std::collections::HashSet;

fn refusal(message: impl Into<String>) -> Error {
    Error::Assembly(message.into())
}
fn kind_name(k: JointKind) -> String {
    format!("{k:?}").to_lowercase()
}
fn object_name(name: &str) -> Result<&str> {
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
fn inspect(p: &mut Project, requested: Option<(Id, f64)>) -> Value {
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
        for key in ["angle", "offset", "offset2"] {
            if let Some(expr) = p.feat_dim(j.id, key) {
                let result = Num::Expr(expr.into()).eval(&p.param_map()).and_then(|value| {
                    p.check_dim_target_text(&qymcad_core::model::DimTarget::Feature { node: j.id, key: key.into() }, expr)
                        .map_err(|e| Error::Expr(e.to_string()))?;
                    if key == "offset" && (j.limit_min[1].is_some_and(|min| value < min) || j.limit_max[1].is_some_and(|max| value > max)) {
                        return Err(refusal("formula offset is outside existing limits"));
                    }
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
            if let AnchorRef::FaceCenter(body, key) = &c.anchor {
                let alive = p.timeline.iter().take(p.rollback_place()).any(|n| !n.suppressed && n.kind.bodies().contains(body));
                let count = p.regen_faces.get(body).map_or(0, |faces| faces.iter().filter(|f| f.id == key.id).count());
                if !alive || p.mesh_index(*body).is_none() || count != 1 {
                    witnesses.push(j.id);
                }
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
            for (slot, read) in [j.angle, j.offset, j.offset2].into_iter().enumerate() {
                if let Some(want) = j.drive[slot] {
                    if !read.is_finite() || !want.is_finite() || (read - want).abs() > 1e-5 {
                        requests.push(json!({"joint":j.id,"slot":slot,"requested":want,"read":read}));
                    }
                }
            }
        }
        if let Some((id, want)) = requested {
            if let Some(j) = p.joints.iter().find(|j| j.id == id) {
                if !j.offset.is_finite() || (j.offset - want).abs() > 1e-5 {
                    requests.push(json!({"joint":id,"slot":1,"requested":want,"read":j.offset}));
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
    json!({"healthy":healthy,"feature_errors":p.regen_errors.keys().collect::<Vec<_>>(),"joint_faults":faults,"faulty_joints":faulty,"anchor_faults":witnesses,"solve_errors":errors,"violated_joints":violated,"conflict":p.mates_conflict,"invalid_transforms":invalid,"request_mismatches":requests,"constraint_faults":constraint_faults,"relation_faults":relation_faults,"formula_errors":formula_errors,"invalid_slots":invalid_slots})
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
    fn assembly_edit(&mut self, tool: &str, args: Value, edit: impl FnOnce(&mut Self) -> Result<Value>) -> Result<Value> {
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
        let before = self.grounding_violations();
        self.assembly_edit("ground", json!({"part":part,"grounded":grounded}), |s| {
            s.p.set_grounded(id, grounded);
            let new: Vec<_> = s.grounding_violations().into_iter().filter(|v| !before.contains(v)).collect();
            if !new.is_empty() {
                return Err(refusal(format!("{}; nothing was changed", new.join("; "))));
            }
            s.assembly_accept(None)?;
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
        }
        out
    }
    fn part_result(&self, id: Id, changed: bool) -> Value {
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
        let key = self.one_face(a.body, &a.face)?;
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
    pub fn joint_create(&mut self, o: &JointCreate) -> Result<Value> {
        if let Some(name) = &o.name {
            object_name(name)?;
        }
        match (o.kind, o.axis) {
            (AssemblyJointKind::Slider, None) => return Err(refusal("slider joint_create requires axis.base_plane (XY, XZ or YZ)")),
            (AssemblyJointKind::Rigid, Some(_)) => return Err(refusal("rigid joint_create does not accept axis")),
            _ => {}
        }
        if o.kind == AssemblyJointKind::Rigid && o.offset.is_some() {
            return Err(refusal("rigid joint_create does not accept offset"));
        }
        self.assembly_edit("joint_create", json!({"name":o.name,"kind":o.kind,"mode":o.mode}), |s| {
            let (a, fa) = s.resolve_anchor(&o.a)?;
            let (b, fb) = s.resolve_anchor(&o.b)?;
            if a == b {
                return Err(refusal(format!("joint_create requires anchors in two different parts; both bodies belong to part {a}")));
            }
            if !s.p.is_effectively_grounded(a) {
                let moving = s.p.joints.iter().find(|j| j.kind == JointKind::Slider && s.p.connector(j.b).is_some_and(|c| c.owner == a));
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
                AssemblyJointKind::Slider => JointKind::Slider,
            };
            let j = s.p.add_joint(ca, cb, kind);
            if let Some(name) = &o.name {
                s.p.joints.iter_mut().find(|v| v.id == j).unwrap().name = name.trim().into();
            }
            if o.mode == JointMode::AsBuilt && !s.p.set_joint_as_built(j) {
                return Err(refusal("joint_create cannot capture as_built frames"));
            }
            let wanted = if let Some(value) = &o.offset { Some(s.write_offset(j, value)?) } else { None };
            if o.offset.as_ref().is_some_and(|v| v.expr().is_some()) {
                s.rebuild_joint_formulas()?;
            }
            s.assembly_accept(wanted.map(|value| (j, value)))?;
            Ok(s.joint_result(j, true))
        })
    }
    fn write_offset(&mut self, id: Id, value: &Num) -> Result<f64> {
        let v = value.eval(&self.p.param_map())?;
        self.check_offset_limits(id, v)?;
        if let Some(expr) = value.expr() {
            self.p
                .check_dim_target_text(&qymcad_core::model::DimTarget::Feature { node: id, key: "offset".into() }, &expr)
                .map_err(|e| Error::Expr(e.to_string()))?;
        }
        let expression = value.expr().unwrap_or_default();
        self.p.set_feat_dim(id, "offset", expression);
        self.p.set_joint_drive(id, 1, Some(v));
        Ok(v)
    }
    fn check_offset_limits(&self, id: Id, value: f64) -> Result<()> {
        let j = self.p.joints.iter().find(|j| j.id == id).unwrap();
        let min = j.limit_min[1].unwrap_or(f64::NEG_INFINITY);
        let max = j.limit_max[1].unwrap_or(f64::INFINITY);
        if min.is_nan() || max.is_nan() || min > max || value < min || value > max {
            return Err(refusal(format!("joint {id} offset {value} mm is outside [{min}, {max}] mm; nothing was changed")));
        }
        Ok(())
    }
    fn rebuild_joint_formulas(&mut self) -> Result<()> {
        self.validate_joint_formulas()?;
        let r = self.rebuild();
        if !r.errors.is_empty() {
            return Err(Error::Rebuild(r.errors.iter().map(|e| e.message.clone()).collect()));
        }
        Ok(())
    }
    pub fn joint_set(&mut self, joint: &JointRef, offset: Option<&Num>, release: Option<&str>) -> Result<Value> {
        if offset.is_some() == release.is_some() || release.is_some_and(|v| v != "offset") {
            return Err(refusal("joint_set requires exactly one of offset or release"));
        }
        let id = self.resolve_joint(joint)?;
        let j = self.p.joints.iter().find(|j| j.id == id).unwrap();
        if j.kind != JointKind::Slider {
            return Err(refusal(format!("joint {id} has no editable offset slot for kind {}", kind_name(j.kind))));
        }
        let owners: HashSet<_> = [j.a, j.b].iter().filter_map(|id| self.p.connector(*id).map(|c| c.owner)).collect();
        self.check_supported_parts(&owners)?;
        for owner in owners {
            self.root_part(owner)?;
        }
        let previous = j.offset;
        let old_expression = self.p.feat_dim(id, "offset").map(str::to_string);
        let wanted = offset.map(|value| value.eval(&self.p.param_map())).transpose()?;
        if let Some(value) = wanted {
            self.check_offset_limits(id, value)?;
        }
        let same = match offset {
            Some(value) => old_expression == value.expr() && j.drive[1] == wanted,
            None => old_expression.is_none() && j.drive[1].is_none(),
        };
        if same {
            let mut r = self.joint_result(id, false);
            r["previous"] = json!(previous);
            r["requested"] = json!(offset);
            r["evaluated"] = json!(wanted);
            return Ok(r);
        }
        self.assembly_edit("joint_set", json!({"joint":joint,"offset":offset,"release":release}), |s| {
            if let Some(value) = offset {
                s.write_offset(id, value)?;
            } else {
                s.p.set_feat_dim(id, "offset", String::new());
                s.p.set_joint_drive(id, 1, None);
            }
            if offset.is_some_and(|v| v.expr().is_some()) {
                s.rebuild_joint_formulas()?;
            }
            s.assembly_accept(wanted.map(|value| (id, value)))?;
            let mut r = s.joint_result(id, true);
            r["previous"] = json!(previous);
            r["requested"] = json!(offset);
            r["evaluated"] = json!(wanted);
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
            s.assembly_accept(None)?;Ok(json!({"removed_joint":id,"removed_connectors":removed,"placements":s.placements(),"diagnostics":s.assembly_diagnostics(),"changed":true}))
        })
    }
    pub(crate) fn component_info(&self) -> Vec<crate::ComponentInfo> {
        let bodies = self.current_body_ids();
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
            })
            .collect()
    }
    fn placements(&self) -> Vec<Value> {
        self.p
            .components
            .iter()
            .map(|c| json!({"id":c.id,"local_transform":c.transform,"world_transform":self.p.world_transform(c.id)}))
            .collect()
    }
    fn joint_result(&self, id: Id, changed: bool) -> Value {
        let j = self.p.joints.iter().find(|j| j.id == id).unwrap();
        let anchor = |cid| {
            self.p.connector(cid).map(|c|json!({"connector":cid,"owner_part":c.owner,"anchor":c.anchor,"body":match &c.anchor {AnchorRef::FaceCenter(b,_)=>Some(*b),_=>None},"face":match &c.anchor {AnchorRef::FaceCenter(_,f)=>Some(f.id),_=>None},"axis_ref":c.axis_ref,"frame_local":self.p.connector_matrix(cid),"frame_for_kind_local":self.p.connector_frame_for_kind(c,j.kind).map(|f|f.matrix12())}))
        };
        json!({"joint":{"id":id,"name":crate::localization::name(&j.name),"kind":kind_name(j.kind)},"connector_ids":[j.a,j.b],"a":anchor(j.a),"b":anchor(j.b),"mode":if j.as_built.is_some(){"as_built"}else{"align"},"as_built":j.as_built,"flip":j.flip,"roll_flip":j.roll_flip,"flip_decided":j.flip_decided,"offset":j.offset,"read":j.offset,"unit":"mm","free_slots":j.kind.free_slots(),"drive":j.drive,"expression":self.p.feat_dim(id,"offset"),"expressions":self.p.feat_dims.get(&id),"driven":j.drive[1].is_some(),"state":if self.p.feat_dim(id,"offset").is_some(){"formula"}else if j.drive[1].is_some(){"numeric"}else{"free"},"signed_world_axis":self.p.joint_slot_axis(id,1,self.p.root),"limit_min":j.limit_min,"limit_max":j.limit_max,"placements":self.placements(),"diagnostics":self.assembly_diagnostics(),"changed":changed})
    }
    pub fn assembly_info(&self) -> Value {
        let diagnostics = self.assembly_diagnostics();
        let mut value = json!({"root":self.p.root,"active_component":self.p.active_component,"components":self.component_info(),"parts":self.component_info().into_iter().filter(|c|c.kind=="part").collect::<Vec<_>>(),"connectors":self.p.connectors,"joints":self.p.joints.iter().map(|j|json!({"id":j.id,"name":crate::localization::name(&j.name),"kind":kind_name(j.kind),"free_slots":j.kind.free_slots(),"a":j.a,"b":j.b,"as_built":j.as_built,"flip":j.flip,"roll_flip":j.roll_flip,"drive":j.drive,"read":[j.angle,j.offset,j.offset2],"slots":[{"name":"angle","unit":"deg","read":j.angle,"drive":j.drive[0],"formula":self.p.feat_dim(j.id,"angle")},{"name":"offset","unit":if j.kind==JointKind::Ball {"deg"}else{"mm"},"read":j.offset,"drive":j.drive[1],"formula":self.p.feat_dim(j.id,"offset")},{"name":"offset2","unit":if j.kind==JointKind::Ball {"deg"}else{"mm"},"read":j.offset2,"drive":j.drive[2],"formula":self.p.feat_dim(j.id,"offset2")}],"expressions":self.p.feat_dims.get(&j.id),"limit_min":j.limit_min,"limit_max":j.limit_max,"signed_world_axis":self.p.joint_slot_axis(j.id,1,self.p.root)})).collect::<Vec<_>>(),"constraints":self.p.mate_constraints,"relations":self.p.relations,"diagnostics":diagnostics,"healthy":diagnostics["healthy"]});
        for key in ["joint_faults", "faulty_joints", "violated_joints", "conflict", "solve_errors"] {
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
    fn assembly_diagnostics(&self) -> Value {
        let mut p = self.p.clone_without_source_data();
        inspect(&mut p, None)
    }
    pub(crate) fn assembly_accept(&mut self, requested: Option<(Id, f64)>) -> Result<()> {
        self.validate_joint_formulas()?;
        let d = inspect(&mut self.p, requested);
        if d["healthy"] == true {
            Ok(())
        } else {
            Err(solve_refusal(&d))
        }
    }
    pub(crate) fn validate_joint_formulas(&self) -> Result<()> {
        let vars = self.p.param_map();
        for j in &self.p.joints {
            for key in ["angle", "offset", "offset2"] {
                if let Some(expr) = self.p.feat_dim(j.id, key) {
                    let value = Num::Expr(expr.into()).eval(&vars)?;
                    self.p
                        .check_dim_target_text(&qymcad_core::model::DimTarget::Feature { node: j.id, key: key.into() }, expr)
                        .map_err(|e| Error::Expr(e.to_string()))?;
                    if key == "offset" {
                        self.check_offset_limits(j.id, value)?;
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
        self.validate_joint_formulas()?;
        let d = inspect(&mut self.p, None);
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
    fn check_supported_parts(&self, parts: &HashSet<Id>) -> Result<()> {
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
                && !matches!(j.kind, JointKind::Rigid | JointKind::Slider)
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
                    let count = p.component_bodies(src_comp).into_iter().filter(|b| !p.consumed_bodies().contains(b)).count();
                    if count > 1 {
                        return Err(refusal("part instances are not supported for multi-body source parts in package A"));
                    }
                    return Err(refusal(format!(
                        "component {} is a part instance; instance editing is not supported in package A",
                        n.parent.unwrap_or(n.id)
                    )));
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        if !ids.is_empty() {
            return Err(refusal(format!(
                "assembly edit touches unsupported constraints or relations {ids:?}; package A supports root-level Rigid and Slider joints"
            )));
        }
        Ok(())
    }
}
