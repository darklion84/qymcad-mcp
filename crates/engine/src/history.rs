//! Session history and guarded native timeline deletion.
use crate::{Error, Id, NodeIssue, Rebuild, Result, Session};
use qymcad_core::{
    feature::{FeatureKind, SketchPlane},
    model::{AxisDef, PlaneDef, PointDef, Project},
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
    warnings: Vec<NodeIssue>,
}

impl Session {
    /// Start a modelling tool call on independent shapes, retaining the original handles for rollback/undo.
    pub fn begin_tool_edit(&mut self) -> Result<Snapshot> {
        let copies = {
            let _gate = qymcad_kernel::kernel_gate();
            self.shapes
                .iter()
                .map(|(&id, sh)| {
                    sh.to_brep_bytes()
                        .and_then(|bytes| Shape::from_brep_bytes(&bytes))
                        .map(|copy| (id, copy))
                        .ok_or_else(|| Error::Invalid(format!("cannot snapshot body {id} for undo")))
                })
                .collect::<Result<HashMap<Id, Shape>>>()?
        };
        Ok(Snapshot {
            project: self.p.clone(),
            shapes: std::mem::replace(&mut self.shapes, copies),
            warnings: self.advisory_warnings.clone(),
        })
    }

    /// End a tool call. Failures restore exact state and never consume an undo entry.
    pub fn finish_tool_edit(&mut self, snapshot: Snapshot, success: bool) {
        if success {
            if self.undo.len() == UNDO_LIMIT {
                self.undo.pop_front();
            }
            self.undo.push_back(snapshot);
        } else {
            self.restore_snapshot(snapshot);
        }
    }

    fn restore_snapshot(&mut self, snapshot: Snapshot) {
        self.p = snapshot.project;
        self.shapes = snapshot.shapes;
        self.advisory_warnings = snapshot.warnings;
    }

    /// Restore the last successful modelling tool call, without regenerating the retained geometry.
    pub fn undo(&mut self) -> Result<()> {
        let snapshot = self.undo.pop_back().ok_or_else(|| Error::Invalid("nothing to undo in this document".into()))?;
        self.restore_snapshot(snapshot);
        Ok(())
    }

    // Native inputs omit datum placement dependencies (F-017). Extend native graph traversal with
    // sketch hosts and datum definitions, including literal offsets without persisted parameter guards.
    fn deletion_closure(&self, root: Id) -> HashSet<Id> {
        let mut gone = HashSet::from([root]);
        loop {
            let before = gone.len();
            for id in gone.clone() {
                gone.extend(self.p.dependents(id));
            }
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
    pub fn feature_delete(&mut self, feature: Id, cascade: bool) -> Result<Rebuild> {
        if !self.p.timeline.iter().any(|n| n.id == feature) {
            return Err(Error::NotFound(format!("no feature {feature}")));
        }
        let gone = self.deletion_closure(feature);
        let dependents: Vec<_> =
            self.p.timeline.iter().filter(|n| n.id != feature && gone.contains(&n.id)).map(|n| format!("{} ({})", n.name, n.id)).collect();
        if !cascade && !dependents.is_empty() {
            return Err(Error::Invalid(format!(
                "feature {} has dependents: {}; use cascade=true to delete them too",
                self.node_name(feature),
                dependents.join(", ")
            )));
        }
        let snapshot = self.begin_tool_edit()?;
        let nodes: Vec<_> = self.p.timeline.iter().filter(|n| gone.contains(&n.id)).map(|n| (n.id, n.kind.clone())).collect();
        for (id, kind) in nodes.into_iter().rev() {
            match kind {
                FeatureKind::Sketch { sketch } => self.p.delete_sketch(sketch),
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
        if let Some(bar) = snapshot.project.rollback {
            let prefix: HashSet<Id> = snapshot.project.timeline.iter().take(bar).map(|n| n.id).collect();
            self.p.set_rollback(Some(self.p.timeline.iter().filter(|n| prefix.contains(&n.id)).count()));
        }
        let live: HashSet<Id> = self.p.timeline.iter().flat_map(|n| n.kind.bodies()).collect();
        self.shapes.retain(|id, _| live.contains(id));
        let report = self.rebuild();
        if !report.errors.is_empty() {
            self.restore_snapshot(snapshot);
            return Err(Error::Rebuild(report.errors.iter().map(|e| format!("{} ({}): {}", e.name, e.node, e.message)).collect()));
        }
        Ok(report)
    }
}
