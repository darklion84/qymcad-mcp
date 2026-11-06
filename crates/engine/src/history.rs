//! Session history and guarded native timeline deletion.
use crate::session::AdvisoryWarning;
use crate::{Error, Id, NodeIssue, Rebuild, Result, Session};
use qymcad_core::{
    feature::{FeatureKind, SketchPlane},
    model::{AxisDef, DimTarget, PlaneDef, PointDef, Project},
};
use qymcad_kernel::Shape;
use std::collections::{HashMap, HashSet};

/// Retain at most this many successful modelling tool calls per open document.
pub const UNDO_LIMIT: usize = 16;

/// A recipe and its original live handles. Edits run on independent B-rep copies so these handles
/// preserve the exact geometry representation, including kernel tolerance padding.
pub struct Snapshot {
    project: Project,
    shapes: HashMap<Id, Shape>,
    warnings: Vec<AdvisoryWarning>,
    detached_projections: HashMap<(Id, Id), crate::session::DetachedProjection>,
    call: Option<ToolCall>,
    created_node: Option<Id>,
}

/// The successful modelling call represented by a history entry.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ToolCall {
    pub tool: String,
    /// Localized created feature name, or a human action label when no feature was created.
    pub label: String,
    pub arguments: serde_json::Value,
}

/// Restored geometry and diagnostics, without regenerating the exact retained B-reps.
pub struct Undone {
    pub call: Option<ToolCall>,
    pub rebuild: Rebuild,
}

/// The existing timeline feature whose suppression state was requested.
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct SuppressedFeature {
    pub id: Id,
    pub name: String,
}

/// Non-default native metadata discarded when a body record is removed by history.
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct LostBodyMetadata {
    pub body: Id,
    pub body_name: String,
    pub visible: bool,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct SuppressResult {
    pub feature: SuppressedFeature,
    pub suppressed: bool,
    pub changed: bool,
    /// Active-prefix outputs deliberately absent after native suppression/cascading.
    pub unbuilt: Vec<Id>,
    pub lost_body_metadata: Vec<LostBodyMetadata>,
    pub rebuild: Rebuild,
}

/// A timeline boundary, expressed without unstable numeric indexes.
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct RollbackInfo {
    pub position: &'static str,
    pub after: Option<SuppressedFeature>,
    pub next_feature: Option<SuppressedFeature>,
    pub active_nodes: usize,
    pub total_nodes: usize,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct RollbackResult {
    #[serde(flatten)]
    pub marker: RollbackInfo,
    pub changed: bool,
    pub lost_body_metadata: Vec<LostBodyMetadata>,
    pub rebuild: Rebuild,
}

impl Snapshot {
    /// Attach the caller's tool name and arguments for undo reporting.
    pub fn record_call(&mut self, tool: &str, arguments: serde_json::Value) {
        let label = match tool {
            "batch" => format!("Batch ({} steps)", arguments["steps"].as_array().map_or(0, Vec::len)),
            "param_set" => "Set parameter".into(),
            "param_delete" => "Delete parameter".into(),
            "sketch_create" => "Create sketch".into(),
            "sketch_add" => "Add sketch geometry".into(),
            "sketch_project" => "Project sketch geometry".into(),
            "sketch_constrain" => "Constrain sketch".into(),
            "sketch_remove" => "Remove sketch geometry".into(),
            "sketch_edit" => "Edit sketch curves".into(),
            "feature_delete" => "Delete feature".into(),
            _ => {
                let action = tool.replace('_', " ");
                let mut chars = action.chars();
                chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
            }
        };
        self.call = Some(ToolCall { tool: tool.into(), label, arguments });
    }

    /// Identify the feature returned by the tool, excluding auxiliary nodes created in the same call.
    pub fn record_created_node(&mut self, node: Id) {
        self.created_node = Some(node);
    }
}

impl Session {
    /// Start a modelling tool call on independent shapes, retaining the original handles for rollback/undo.
    pub fn begin_tool_edit(&mut self) -> Result<Snapshot> {
        let copies = crate::session::copy_shapes(self.shapes.iter().map(|(&id, sh)| (id, sh)))
            .map_err(|id| Error::Invalid(format!("cannot snapshot body {id} for undo")))?;
        self.tool_edit_active = true;
        Ok(Snapshot {
            project: self.p.clone_without_source_data(),
            shapes: std::mem::replace(&mut self.shapes, copies),
            warnings: self.advisory_warnings.clone(),
            detached_projections: self.detached_projections.clone(),
            call: None,
            created_node: None,
        })
    }

    /// End a tool call. Failures restore exact state and never consume an undo entry.
    pub fn finish_tool_edit(&mut self, mut snapshot: Snapshot, success: bool) {
        self.tool_edit_active = false;
        if success {
            if let Some(call) = &mut snapshot.call {
                if let Some(node) = snapshot.created_node.and_then(|id| self.p.timeline.iter().find(|node| node.id == id)) {
                    let blank_name = call.arguments["name"].as_str().is_some_and(|name| Self::optional_object_name(Some(name)).is_none());
                    if !blank_name && !node.name.trim().is_empty() {
                        call.label = crate::localization::name(&node.name);
                    }
                }
            }
            if self.undo.len() == UNDO_LIMIT {
                self.undo.pop_front();
                self.undo_limit_reached = true;
            }
            self.undo.push_back(snapshot);
        } else {
            self.restore_snapshot(snapshot);
        }
        self.prune_retired_sources();
    }

    fn prune_retired_sources(&mut self) {
        let needed: HashSet<_> = self.undo.iter().flat_map(|snapshot| snapshot.project.sources.iter().map(|src| src.id)).collect();
        self.retired_sources.retain(|id, _| needed.contains(id));
    }

    fn restore_snapshot(&mut self, mut snapshot: Snapshot) {
        snapshot.project.keep_ids_past(&self.p);
        // Native remove_sketch also drops its embedded original. Recover that single retained payload
        // before the native source-data transfer; ordinary live sources need no archive or duplication.
        for src in &snapshot.project.sources {
            if !self.p.sources.iter().any(|live| live.id == src.id) {
                if let Some(original) = self.retired_sources.remove(&src.id) {
                    self.p.sources.push(original);
                }
            }
        }
        snapshot.project.take_source_data_from(&mut self.p);
        self.p = snapshot.project;
        self.shapes = snapshot.shapes;
        self.advisory_warnings = snapshot.warnings;
        self.detached_projections = snapshot.detached_projections;
    }

    /// Restore the last successful modelling tool call, without regenerating the retained geometry.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()`;
    /// restored body metrics acquire the gate internally.
    pub fn undo(&mut self) -> Result<Undone> {
        let snapshot = self.undo.pop_back().ok_or_else(|| {
            Error::Invalid(if self.undo_limit_reached {
                format!("history limit reached ({UNDO_LIMIT}): older calls cannot be undone")
            } else {
                "nothing to undo in this document".into()
            })
        })?;
        let call = snapshot.call.clone();
        self.restore_snapshot(snapshot);
        self.prune_retired_sources();
        let errors = self
            .p
            .regen_errors
            .iter()
            .map(|(&node, e)| NodeIssue { node, name: self.node_name(node), message: crate::localization::error(e) })
            .collect();
        let mut warnings: Vec<_> = self
            .p
            .regen_warnings
            .iter()
            .map(|(&node, e)| NodeIssue { node, name: self.node_name(node), message: crate::localization::error(e) })
            .collect();
        warnings.extend(self.advisory_warnings.iter().map(|w| w.issue().clone()));
        warnings.extend(self.cross_part_projection_warnings());
        Ok(Undone { call, rebuild: Rebuild { errors, warnings, bodies: self.result_bodies(), ..Default::default() } })
    }

    /// Inspect the persisted native rollback boundary. All timeline nodes, including sketches and
    /// datums, count. The end is canonical even if an external archive stored Some(len).
    pub fn rollback_info(&self) -> RollbackInfo {
        let total_nodes = self.p.timeline.len();
        let active_nodes = self.p.rollback_place().min(total_nodes);
        let node = |i: usize| self.p.timeline.get(i).map(|n| SuppressedFeature { id: n.id, name: crate::localization::name(&n.name) });
        let position = if active_nodes == total_nodes {
            "end"
        } else if active_nodes == 0 {
            "start"
        } else {
            "after"
        };
        RollbackInfo {
            position,
            after: if position == "after" { node(active_nodes - 1) } else { None },
            next_feature: node(active_nodes),
            active_nodes,
            total_nodes,
        }
    }

    /// Move the native history marker after a timeline node, or to start/end. A changed marker
    /// owns one undo boundary unless called inside a tool/batch. Every rebuild error refuses the
    /// move, restoring the original recipe, live B-reps and diagnostics without a repair rebuild.
    pub fn history_rollback(&mut self, after: Option<Id>, position: Option<&str>) -> Result<RollbackResult> {
        let target = match (after, position) {
            (Some(id), None) => {
                let ti = self
                    .p
                    .timeline
                    .iter()
                    .position(|n| n.id == id)
                    .ok_or_else(|| Error::NotFound(format!("no timeline node with id {id}")))?;
                Some(ti + 1).filter(|&n| n < self.p.timeline.len())
            }
            (None, Some("start")) => Some(0).filter(|&n| n < self.p.timeline.len()),
            (None, Some("end")) => None,
            (None, Some(value)) => return Err(Error::Invalid(format!("unknown rollback position `{value}`; use start or end"))),
            _ => return Err(Error::Invalid("history_rollback needs exactly one of after or position (start/end)".into())),
        };
        if self.p.rollback == target {
            return Ok(RollbackResult {
                marker: self.rollback_info(),
                changed: false,
                lost_body_metadata: Vec::new(),
                rebuild: self.current_rebuild(),
            });
        }
        let snapshot = if self.tool_edit_active {
            None
        } else {
            let mut snapshot = self.begin_tool_edit()?;
            snapshot.record_call("history_rollback", serde_json::json!({"after":after,"position":position}));
            Some(snapshot)
        };
        let metadata = self.body_metadata_at_risk();
        let before = self.p.clone();
        // Retry/settling can reach beyond the dirty suffix through named drivers and projections.
        let nodes: HashSet<Id> = self.p.timeline.iter().map(|n| n.id).collect();
        let result = self.with_rebuild_copies(before, &nodes, "history rollback", |s| {
            s.p.set_rollback(target);
            let report = s.rebuild_for_edit(Some(&nodes))?;
            if !report.errors.is_empty() || !s.p.regen_errors.is_empty() {
                let mut errors: Vec<String> = report.errors.iter().map(|e| format!("{} ({}): {}", e.name, e.node, e.message)).collect();
                errors.extend(
                    s.p.regen_errors
                        .iter()
                        .filter(|(id, _)| !report.errors.iter().any(|e| e.node == **id))
                        .map(|(id, e)| format!("{} ({id}): {}", s.node_name(*id), crate::localization::error(e))),
                );
                return Err(Error::Rebuild(errors));
            }
            Ok(RollbackResult {
                marker: s.rollback_info(),
                changed: true,
                lost_body_metadata: s.removed_body_metadata(metadata),
                rebuild: report,
            })
        });
        if let Some(snapshot) = snapshot {
            self.finish_tool_edit(snapshot, result.is_ok());
        }
        result
    }

    /// Set a body-producing feature's native suppression flag. A successful modifier bypass keeps
    /// its output id; base suppression deliberately leaves its dependent bodies unbuilt.
    pub fn feature_suppress(&mut self, feature: Id, suppressed: bool) -> Result<SuppressResult> {
        let ti =
            self.p.timeline.iter().position(|n| n.id == feature).ok_or_else(|| Error::NotFound(format!("no feature with id {feature}")))?;
        let node = &self.p.timeline[ti];
        if node.kind.bodies().is_empty() {
            return Err(Error::Invalid(format!(
                "feature {feature} is not a body-producing feature; sketches, datums and parameters cannot be suppressed"
            )));
        }
        if matches!(node.kind, FeatureKind::Move { body, .. } if self.p.mesh_born(body)) {
            return Err(Error::Invalid(format!("feature {feature} is a mesh-born move; native suppression requires a B-rep (F-128)")));
        }
        let changed = node.suppressed != suppressed;
        if !changed {
            return Ok(self.suppression_result(feature, suppressed, false));
        }
        self.check_suppression_outputs(feature, suppressed)?;
        let metadata = self.body_metadata_at_risk();
        // Direct engine calls own one history boundary; MCP and batch already supply theirs.
        let snapshot = if self.tool_edit_active {
            None
        } else {
            let mut snapshot = self.begin_tool_edit()?;
            snapshot.record_call("feature_suppress", serde_json::json!({"feature": feature, "suppressed": suppressed}));
            Some(snapshot)
        };
        let before = self.p.clone();
        // Native retry/settling can reach beyond the suffix through projections and named drivers.
        // Retain every timeline body's original handle so any such failure restores exactly.
        let nodes: HashSet<Id> = self.p.timeline.iter().map(|n| n.id).collect();
        let result = self.with_rebuild_copies(before, &nodes, "feature suppression", |s| {
            s.p.set_feature_suppressed(ti, suppressed);
            let report = s.rebuild_for_edit(Some(&nodes))?;
            if !report.errors.is_empty() || !s.p.regen_errors.is_empty() {
                let mut errors: Vec<String> = report.errors.iter().map(|e| format!("{} ({}): {}", e.name, e.node, e.message)).collect();
                errors.extend(
                    s.p.regen_errors
                        .iter()
                        .filter(|(id, _)| !report.errors.iter().any(|e| e.node == **id))
                        .map(|(id, e)| format!("{} ({id}): {}", s.node_name(*id), crate::localization::error(e))),
                );
                return Err(Error::Rebuild(errors));
            }
            let mut result = s.suppression_result(feature, suppressed, true);
            result.lost_body_metadata = s.removed_body_metadata(metadata);
            Ok(result)
        });
        if let Some(snapshot) = snapshot {
            self.finish_tool_edit(snapshot, result.is_ok());
        }
        result
    }

    fn suppression_result(&self, feature: Id, suppressed: bool, changed: bool) -> SuppressResult {
        let cascade = crate::session::suppression_unbuilt(&self.p, None);
        // Preserve timeline/output order and report only genuinely absent document records.
        let unbuilt = self
            .p
            .timeline
            .iter()
            .take(self.p.rollback_place())
            .flat_map(|n| n.kind.bodies())
            .filter(|id| cascade.contains(id) && self.p.mesh_index(*id).is_none())
            .collect();
        SuppressResult {
            feature: SuppressedFeature { id: feature, name: self.node_name(feature) },
            suppressed,
            changed,
            unbuilt,
            lost_body_metadata: Vec::new(),
            rebuild: self.current_rebuild(),
        }
    }

    /// Reject native multi-piece suppression before any setter, snapshot or rebuild.
    fn check_suppression_outputs(&self, feature: Id, suppressed: bool) -> Result<()> {
        let before = crate::session::suppression_unbuilt(&self.p, None);
        let after = crate::session::suppression_unbuilt(&self.p, Some((feature, suppressed)));
        for node in &self.p.timeline {
            let outputs = node.kind.bodies();
            if outputs.len() > 1 && (node.id == feature || outputs.iter().any(|id| before.contains(id) != after.contains(id))) {
                return Err(Error::Invalid(format!(
                    "feature `{}` ({}): multi-output suppression is refused under the pinned app: it leaves stale or overlapping pieces / the identity guard cannot follow their removal or recreation",
                    self.node_name(node.id), node.id
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn body_metadata_at_risk(&self) -> Vec<LostBodyMetadata> {
        self.p
            .bodies
            .iter()
            .filter(|body| {
                let default_name = body.name.is_empty()
                    || body
                        .name
                        .strip_prefix("name-body#")
                        .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_digit()));
                !default_name || !body.visible
            })
            .map(|body| LostBodyMetadata { body: body.id, body_name: self.native_body_name(body.id), visible: body.visible })
            .collect()
    }

    pub(crate) fn removed_body_metadata(&self, mut metadata: Vec<LostBodyMetadata>) -> Vec<LostBodyMetadata> {
        metadata.retain(|body| self.p.mesh_index(body.body).is_none());
        metadata.sort_by_key(|body| body.body);
        metadata
    }

    // Native inputs omit datum placement dependencies (F-017). Extend native graph traversal with
    // sketch hosts and datum definitions, including literal offsets without persisted parameter guards.
    fn deletion_closure(&self, root: Id) -> HashSet<Id> {
        let mut gone = HashSet::from([root]);
        loop {
            let before = gone.len();
            gone = crate::dependencies::closure(&self.p, gone);
            for sketch in &self.p.sketches {
                if matches!(sketch.plane, SketchPlane::Datum(id) | SketchPlane::Face(id, _) if gone.contains(&id)) {
                    gone.insert(sketch.id);
                }
            }
            for plane in &self.p.planes {
                if matches!(plane.def, PlaneDef::OffsetPlane { plane: id, .. } | PlaneDef::OffsetFace { body: id, .. } if gone.contains(&id))
                {
                    gone.insert(plane.id);
                }
            }
            for axis in &self.p.datum_axes {
                if matches!(axis.def, AxisDef::FromEdge { body, .. } | AxisDef::FromFace { body, .. } if gone.contains(&body)) {
                    gone.insert(axis.id);
                }
            }
            for point in &self.p.datum_points {
                if matches!(point.def, PointDef::AtVertex { body, .. } if gone.contains(&body)) {
                    gone.insert(point.id);
                }
            }
            for axis in &self.p.datum_axes {
                if matches!(axis.def, AxisDef::TwoPoints { a, b } if gone.contains(&a) || gone.contains(&b)) {
                    gone.insert(axis.id);
                }
            }
            for node in &self.p.timeline {
                let references = match node.kind {
                    FeatureKind::Revolve { axis_datum, .. } => vec![axis_datum],
                    FeatureKind::Hole { sketch, .. } => vec![sketch],
                    FeatureKind::Thicken { join, .. } => vec![join],
                    _ => Vec::new(),
                };
                let copy_body = node.kind.copy_source().and_then(|component| self.p.active_body(component));
                if references.iter().any(|id| *id != 0 && gone.contains(id)) || copy_body.is_some_and(|id| gone.contains(&id)) {
                    gone.insert(node.id);
                }
            }
            if before == gone.len() {
                return gone;
            }
        }
    }

    /// Delete a timeline node; cascade explicitly deletes its transitive dependents first.
    /// The native operation cleanup restores the consumed source body as the current result.
    /// Projection sources refuse in either mode; explicitly delete the projecting sketches first.
    pub fn feature_delete(&mut self, feature: Id, cascade: bool) -> Result<Rebuild> {
        if !self.p.timeline.iter().any(|n| n.id == feature) {
            return Err(Error::NotFound(format!("no feature {feature}")));
        }
        let gone = self.deletion_closure(feature);
        // Native deletion cannot reliably distinguish a detached source from an unbuilt one. Refuse
        // before touching state, including cascades: callers must explicitly delete projecting sketches.
        let projections: Vec<_> = self
            .p
            .sketches
            .iter()
            .filter(|sketch| {
                sketch.projections.iter().any(|pr| {
                    gone.contains(&pr.body) || gone.contains(&crate::dependencies::projection_source(&self.p, sketch.id, pr.body))
                })
            })
            .map(|sketch| format!("{} ({})", self.node_name(sketch.id), sketch.id))
            .collect();
        if !projections.is_empty() {
            return Err(Error::Invalid(format!(
                "feature {} is a source of sketch projections in {}; delete those sketches first (also with cascade=true)",
                self.node_name(feature),
                projections.join(", ")
            )));
        }
        let dependents: Vec<_> = self
            .p
            .timeline
            .iter()
            .filter(|n| n.id != feature && gone.contains(&n.id))
            .map(|n| format!("{} ({})", crate::localization::name(&n.name), n.id))
            .collect();
        if !cascade && !dependents.is_empty() {
            return Err(Error::Invalid(format!(
                "feature {} has dependents: {}; use cascade=true to delete them too",
                self.node_name(feature),
                dependents.join(", ")
            )));
        }
        // Sketch-owned records (points, entities, patterns, texts) are not timeline nodes, yet their
        // formulas and names live in separate maps that native deletion leaves behind (F-138).
        let deleted_sketches: HashSet<Id> = self
            .p
            .timeline
            .iter()
            .filter(|n| gone.contains(&n.id))
            .filter_map(|n| match n.kind {
                FeatureKind::Sketch { sketch } => Some(sketch),
                _ => None,
            })
            .collect();
        let mut owned: HashSet<Id> = gone.clone();
        for sketch in self.p.sketches.iter().filter(|sk| deleted_sketches.contains(&sk.id)) {
            owned.insert(sketch.id);
            owned.extend(sketch.points.iter().map(|x| x.id));
            owned.extend(sketch.entities.iter().map(|x| x.id));
            owned.extend(sketch.patterns.iter().map(|x| x.id));
            owned.extend(sketch.texts.iter().map(|x| x.id));
        }
        // A name a surviving global still reads stays, as before: the user deletes that alias explicitly
        // (golden_medium_r1 named_deletion). Every other name owned by deleted records goes with them.
        let kept_names: HashSet<String> = self
            .p
            .named_dims
            .iter()
            .filter(|d| self.p.parameters.iter().any(|q| qymcad_core::expr::mentions(&q.expr, &d.name)))
            .map(|d| d.name.clone())
            .collect();
        let doomed = |d: &qymcad_core::model::NamedDim| {
            !kept_names.contains(&d.name)
                && match &d.target {
                    DimTarget::Sketch { sketch, .. } => deleted_sketches.contains(sketch),
                    DimTarget::Feature { node, .. } => owned.contains(node),
                }
        };
        // MCP already isolates shapes and retains rollback state for the whole tool call.
        // Direct engine callers still need their own atomic deletion boundary.
        let snapshot = if self.tool_edit_active { None } else { Some(self.begin_tool_edit()?) };
        let rollback_prefix = self.p.rollback.map(|bar| self.p.timeline.iter().take(bar).map(|n| n.id).collect::<HashSet<_>>());
        let nodes: Vec<_> = self.p.timeline.iter().filter(|n| gone.contains(&n.id)).map(|n| (n.id, n.kind.clone())).collect();
        for (id, kind) in nodes.into_iter().rev() {
            match kind {
                FeatureKind::Sketch { sketch } => {
                    if let Some(source) = self.p.sketches.iter().find(|sk| sk.id == sketch).and_then(|sk| sk.source) {
                        if let Some(index) = self.p.sources.iter().position(|src| src.id == source) {
                            let original = self.p.sources.remove(index);
                            self.retired_sources.insert(source, original);
                        }
                    }
                    self.p.delete_sketch(sketch);
                }
                FeatureKind::Plane { plane } => {
                    self.p.planes.retain(|p| p.id != plane);
                    self.p.delete_feature_op(id);
                }
                FeatureKind::DatumPoint { point } => {
                    self.p.datum_points.retain(|p| p.id != point);
                    self.p.delete_feature_op(id);
                }
                FeatureKind::DatumAxis { axis } => {
                    self.p.datum_axes.retain(|a| a.id != axis);
                    self.p.delete_feature_op(id);
                }
                _ => {
                    self.p.delete_feature_op(id);
                }
            }
            self.p.feat_dims.remove(&id);
            self.p.regen_errors.remove(&id);
            self.p.regen_warnings.remove(&id);
        }
        self.p.feat_dims.retain(|id, _| !owned.contains(id));
        self.p.named_dims.retain(|d| !doomed(d));
        if let Some(prefix) = rollback_prefix {
            self.p.set_rollback(Some(self.p.timeline.iter().filter(|n| prefix.contains(&n.id)).count()));
        }
        let live: HashSet<Id> = self.p.timeline.iter().flat_map(|n| n.kind.bodies()).collect();
        self.shapes.retain(|id, _| live.contains(id));
        let report = self.rebuild();
        if !report.errors.is_empty() {
            if let Some(snapshot) = snapshot {
                self.finish_tool_edit(snapshot, false);
            }
            return Err(Error::Rebuild(report.errors.iter().map(|e| format!("{} ({}): {}", e.name, e.node, e.message)).collect()));
        }
        if snapshot.is_some() {
            self.tool_edit_active = false;
            self.prune_retired_sources();
        }
        Ok(report)
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use qymcad_core::model::SourceFile;

    #[test]
    fn rollback_native_tail_error_restores_private_depth_and_live_shapes() {
        use crate::{BaseName, PlaneRef};
        let mut s = Session::new_part();
        let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &10.0.into(), &20.0.into(), false).unwrap();
        let (base, report) = s.extrude(&serde_json::from_value(serde_json::json!({"sketch":sketch,"height":20})).unwrap()).unwrap();
        assert!(report.errors.is_empty());
        assert!((report.bodies[0].volume - 10.0 * 20.0 * 20.0).abs() < 1e-6); // V=area*height=4000.
        let mut matrix = qymcad_core::feature::PLACE_IDENTITY;
        matrix[3] = 5.0;
        let tail = s.p.add_move(base, matrix);
        let report = s.rebuild();
        assert!(report.errors.is_empty());
        assert_eq!(report.bodies[0].id, tail);
        assert!((report.bodies[0].volume - 10.0 * 20.0 * 20.0).abs() < 1e-6);
        let rolled = s.history_rollback(Some(base), None).unwrap();
        assert!(rolled.changed);
        assert_eq!(rolled.rebuild.bodies[0].id, base);
        assert_eq!(s.undo.len(), 1);
        // A native document can carry an invalid inactive tail. The marker hides that recipe
        // until resume; no injected engine error or mocked rebuild is involved.
        let FeatureKind::Move { src, .. } = &mut s.p.timeline.iter_mut().find(|n| n.id == tail).unwrap().kind else { unreachable!() };
        *src = u64::MAX;
        let state = |s: &Session| {
            let breps: std::collections::BTreeMap<_, _> = {
                let _gate = qymcad_kernel::kernel_gate();
                s.shapes.iter().map(|(&id, sh)| (id, sh.to_brep_bytes().unwrap())).collect()
            };
            (ron::to_string(&s.p).unwrap(), breps, s.info(), s.advisory_warnings.clone(), s.undo.len())
        };
        let topology = s.topology(None, true).unwrap();
        let before = state(&s);
        assert!(s.history_rollback(None, Some("end")).is_err());
        assert_eq!(state(&s), before);
        assert_eq!(s.topology(None, true).unwrap(), topology);
        assert_eq!(s.undo.len(), 1);
        assert_eq!(s.undo().unwrap().call.unwrap().tool, "history_rollback");
        assert!(s.undo.is_empty());
    }

    #[test]
    fn suppression_lost_pick_restores_private_undo_depth_and_exact_state() {
        use crate::{BaseName, PlaneRef, Sel};
        let mut s = Session::new_part();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("stock profile")).unwrap();
        s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &20.0.into(), false).unwrap();
        let (base, r) = s.extrude(&serde_json::from_value(serde_json::json!({"sketch": sk, "height": 20})).unwrap()).unwrap();
        assert!(r.errors.is_empty());
        assert!((r.bodies[0].volume - 10.0 * 20.0 * 20.0).abs() < 1e-6); // 4000 mm³.
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("bore profile")).unwrap();
        s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
        let (cut, r) =
            s.extrude(&serde_json::from_value(serde_json::json!({"sketch":sk,"height":20,"op":"cut","target":base})).unwrap()).unwrap();
        assert!(r.errors.is_empty());
        assert!((r.bodies[0].volume - (10.0 * 20.0 * 20.0 - std::f64::consts::PI * 2_f64.powi(2) * 20.0)).abs() < 1e-6); // Prism minus cylinder.
        let rim = s.p.regen_edges[&cut].iter().find(|e| (e.radius - 2.0).abs() < 1e-7 && (e.center[2] - 20.0).abs() < 1e-7).unwrap().id;
        let (fillet, r) = s.fillet(Some(cut), &Sel::Ids(vec![rim]), &0.5.into(), Some("rim")).unwrap();
        assert!(r.errors.is_empty());
        // Nonempty history makes a lost or extra entry observable, in addition to the golden's empty-history control.
        assert!(s.rename(crate::RenameTarget::Feature(base), "stock").unwrap().changed);
        assert_eq!(s.undo.len(), 1);
        let state = |s: &Session| {
            let breps: std::collections::BTreeMap<_, _> = {
                let _gate = qymcad_kernel::kernel_gate();
                s.shapes.iter().map(|(&id, sh)| (id, sh.to_brep_bytes().unwrap())).collect()
            };
            (ron::to_string(&s.p).unwrap(), breps, s.info(), s.undo.len())
        };
        let topology = s.topology(Some(fillet), true).unwrap();
        let before = state(&s);
        assert!(s.feature_suppress(cut, true).is_err());
        assert_eq!(state(&s), before);
        assert_eq!(s.topology(Some(fillet), true).unwrap(), topology);
        assert_eq!(s.undo().unwrap().call.unwrap().tool, "rename");
        assert!(s.undo.is_empty());
    }

    #[test]
    fn snapshots_omit_source_bytes_and_restore_them_on_undo_and_failure() {
        let mut s = Session::new_part();
        let data = vec![42; 1024];
        s.p.sources.push(SourceFile { id: 77, name: "original.stl".into(), ext: "stl".into(), data: data.clone() });
        for success in [true, false] {
            let snapshot = s.begin_tool_edit().unwrap();
            assert!(snapshot.project.sources[0].data.is_empty(), "undo snapshot must not retain embedded source bytes");
            assert_eq!(s.p.sources[0].data, data, "live source stays intact");
            s.param_set("n", &1.0.into()).unwrap();
            s.finish_tool_edit(snapshot, success);
            if success {
                s.undo().unwrap();
            }
            assert_eq!(s.p.sources[0].data, data, "restore must move original source bytes into the snapshot");
        }
    }

    #[test]
    fn undo_restores_source_bytes_removed_by_native_sketch_deletion() {
        let mut s = Session::new_part();
        let sketch = s.sketch_create(&crate::PlaneRef::Base(crate::BaseName::XY), None).unwrap();
        let data = vec![17; 2048];
        s.p.sketches.iter_mut().find(|sk| sk.id == sketch).unwrap().source = Some(77);
        s.p.sources.push(SourceFile { id: 77, name: "original.dxf".into(), ext: "dxf".into(), data: data.clone() });
        let original_allocation = s.p.sources[0].data.as_ptr();
        for success in [true, false] {
            let snapshot = s.begin_tool_edit().unwrap();
            s.feature_delete(sketch, false).unwrap();
            assert!(s.tool_edit_active, "deletion must reuse and retain the enclosing tool boundary");
            assert!(s.p.sources.is_empty(), "native deletion removes the source record");
            assert_eq!(s.retired_sources.len(), 1, "one original payload is retained outside snapshots");
            s.finish_tool_edit(snapshot, success);
            if success {
                s.undo().unwrap();
            }
            assert!(s.p.sources[0].data == data, "undo must recover a deleted sketch's embedded original");
            assert_eq!(s.p.sources[0].data.as_ptr(), original_allocation, "payload is moved, never duplicated");
            assert!(s.retired_sources.is_empty());
        }
        let snapshot = s.begin_tool_edit().unwrap();
        s.feature_delete(sketch, false).unwrap();
        s.finish_tool_edit(snapshot, true);
        for n in 0..UNDO_LIMIT {
            let snapshot = s.begin_tool_edit().unwrap();
            s.param_set("n", &(n as f64).into()).unwrap();
            s.finish_tool_edit(snapshot, true);
        }
        // 1 deletion + 16 edits evicts the only snapshot containing the source record.
        assert!(s.retired_sources.is_empty(), "evicted history releases removed originals");
    }
}
