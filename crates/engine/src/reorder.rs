//! Checked native feature menu reorder.
use crate::{Error, Id, LostBodyMetadata, Rebuild, Result, RollbackInfo, Session, SuppressedFeature};

use qymcad_core::{
    feature::{FeatureKind, SketchPlane},
    model::{AxisDef, DimTarget, EntityKind, PlaneDef, PointDef, Project},
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct RewiredFeature {
    pub feature: Id,
    pub before_inputs: Vec<Id>,
    pub after_inputs: Vec<Id>,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct RehostedSketch {
    pub sketch: Id,
    pub before_body: Id,
    pub after_body: Id,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct ReorderResult {
    pub feature: SuppressedFeature,
    pub direction: String,
    pub changed: bool,
    pub from_index: usize,
    pub to_index: usize,
    pub order: Vec<Id>,
    pub rewired: Vec<RewiredFeature>,
    pub rehosted_sketches: Vec<RehostedSketch>,
    pub rollback: RollbackInfo,
    pub lost_body_metadata: Vec<LostBodyMetadata>,
    pub rebuild: Rebuild,
}

impl Session {
    /// Move a body feature across its same-parent GUI neighbour using native history rewriting.
    pub fn feature_reorder(&mut self, feature: Id, direction: &str) -> Result<ReorderResult> {
        if !matches!(direction, "up" | "down") {
            return Err(Error::Invalid(format!("unknown reorder direction `{direction}`; use up or down")));
        }
        let ti = self.p.timeline.iter().position(|n| n.id == feature).ok_or_else(|| {
            Error::Invalid(format!("feature_reorder needs a timeline feature id; object {feature} is not a timeline node"))
        })?;
        check_mover(&self.p, ti)?;
        let matches = |i: usize| self.p.timeline[i].parent == self.p.timeline[ti].parent && self.p.timeline[i].kind.body().is_some();
        let neighbor =
            if direction == "up" { (0..ti).rev().find(|i| matches(*i)) } else { (ti + 1..self.p.timeline.len()).find(|i| matches(*i)) };
        let mut result = ReorderResult {
            feature: SuppressedFeature { id: feature, name: self.node_name(feature) },
            direction: direction.into(),
            changed: false,
            from_index: ti,
            to_index: ti,
            order: self.p.timeline.iter().map(|n| n.id).collect(),
            rewired: Vec::new(),
            rehosted_sketches: Vec::new(),
            rollback: self.rollback_info(),
            lost_body_metadata: Vec::new(),
            rebuild: self.current_rebuild(),
        };
        let Some(neighbor) = neighbor else { return Ok(result) };
        check_mover(&self.p, neighbor)?;
        // Down raises the next node, exactly as the app does; (selected,next) is an insertion no-op.
        let (from, to) = if direction == "up" { (ti, neighbor) } else { (neighbor, ti) };
        let refused = || {
            Error::Invalid(format!(
                "cannot move feature {feature} {direction} across feature {}: native QymCAD refuses this order or chain rewrite",
                self.p.timeline[neighbor].id
            ))
        };
        let mut candidate = self.p.clone_without_source_data();
        if !candidate.reorder_feature(from, to) {
            return Err(refused());
        }
        result.order = candidate.timeline.iter().map(|n| n.id).collect();
        for node in &candidate.timeline {
            let before = self.p.timeline.iter().find(|n| n.id == node.id).unwrap().kind.inputs();
            let after = node.kind.inputs();
            if before != after {
                result.rewired.push(RewiredFeature { feature: node.id, before_inputs: before, after_inputs: after });
            }
        }
        for sk in &candidate.sketches {
            if let (Some(SketchPlane::Face(before, _)), SketchPlane::Face(after, _)) =
                (self.p.sketches.iter().find(|s| s.id == sk.id).map(|s| s.plane), sk.plane)
            {
                if before != after {
                    result.rehosted_sketches.push(RehostedSketch { sketch: sk.id, before_body: before, after_body: after });
                }
            }
        }
        result.changed = result.order != self.p.timeline.iter().map(|n| n.id).collect::<Vec<_>>()
            || !result.rewired.is_empty()
            || !result.rehosted_sketches.is_empty();
        if !result.changed {
            return Ok(result);
        }
        check_candidate(&self.p, &candidate)?;
        let splines = projected_splines(&self.p);
        let projections = self.projection_provenance();
        result.to_index = candidate.timeline.iter().position(|n| n.id == feature).unwrap();
        let metadata = self.body_metadata_at_risk();
        // Native dirties every node. Preserve every output's live handle and its original absence.
        let nodes: HashSet<Id> = self.p.timeline.iter().flat_map(|n| std::iter::once(n.id).chain(n.kind.bodies())).collect();
        let snapshot = if self.tool_edit_active {
            None
        } else {
            let mut snapshot = self.begin_tool_edit()?;
            snapshot.record_call("feature_reorder", serde_json::json!({"feature":feature,"direction":direction}));
            Some(snapshot)
        };
        let before = self.p.clone();
        let outcome = self.with_rebuild_copies(before, &nodes, "feature reorder", |s| {
            // Apply only the sanctioned native rewrite; retain live embedded source bytes.
            if !s.p.reorder_feature(from, to) {
                return Err(Error::Invalid("native reorder changed after candidate validation".into()));
            }
            s.reconcile_datum_dependencies();
            s.reconcile_hole_dependencies();
            let report = s.rebuild_for_reorder(Some(&nodes));
            let buildable = Self::geometry_check_nodes(&s.p);
            for spline in &splines {
                if !buildable.contains(&spline.sketch) {
                    continue;
                }
                let changed = s.p.sketches.iter().find(|sk| sk.id == spline.sketch).is_none_or(|sk| {
                    sk.projections.iter().find(|p| p.id == spline.projection).is_none_or(|p| {
                        p.entities != spline.entities
                            || !crate::sketch::patterns::same_geometry(
                                &spline.geometry,
                                &crate::sketch::patterns::curve_geometry(sk, &p.entities),
                            )
                    })
                });
                if changed {
                    return Err(spline_refusal(spline.sketch));
                }
            }
            let report = report?;
            if let Some(error) = s.released_projection(&projections) {
                return Err(error);
            }
            let mut errors: HashMap<Id, Vec<String>> = HashMap::new();
            for issue in &report.errors {
                errors.entry(issue.node).or_default().push(issue.message.clone());
            }
            for (&id, error) in &s.p.regen_errors {
                let message = crate::localization::error(error);
                let messages = errors.entry(id).or_default();
                if !messages.contains(&message) {
                    messages.push(message);
                }
            }
            if !errors.is_empty() {
                return Err(Error::Rebuild(
                    s.p.timeline
                        .iter()
                        .flat_map(|n| {
                            errors.remove(&n.id).unwrap_or_default().into_iter().map(|e| format!("{} ({}): {e}", s.node_name(n.id), n.id))
                        })
                        .collect(),
                ));
            }
            result.rollback = s.rollback_info();
            result.lost_body_metadata = s.removed_body_metadata(metadata);
            result.rebuild = report;
            Ok(result)
        });
        if let Some(snapshot) = snapshot {
            self.finish_tool_edit(snapshot, outcome.is_ok());
        }
        outcome
    }

    /// Native reprojection releases a curve whose source edge is gone and keeps it as free geometry; a move must not.
    fn released_projection(&self, before: &HashMap<(Id, Id), crate::session::DetachedProjection>) -> Option<Error> {
        let after = self.projection_provenance();
        let buildable = Self::geometry_check_nodes(&self.p);
        let mut lost: Vec<_> = before.iter().filter(|(key, _)| buildable.contains(&key.0) && !after.contains_key(*key)).collect();
        lost.sort_by_key(|((sketch, entity), _)| (*sketch, *entity));
        lost.first().map(|((sketch, entity), projection)| {
            Error::Invalid(format!(
                "cannot reorder: projection {} in sketch {sketch} would be released: this move removes the source of its curve {entity} on body {}; delete that sketch with cascade, reorder, then recreate it",
                projection.id, projection.body
            ))
        })
    }
}

fn check_mover(p: &Project, index: usize) -> Result<()> {
    let node = &p.timeline[index];
    let id = node.id;
    let outputs = node.kind.bodies();
    if outputs.is_empty() {
        return Err(Error::Invalid(format!("feature {id} is not a body-producing feature; feature_reorder moves body features only")));
    }
    if outputs.iter().any(|body| p.mesh_born(*body)) {
        return Err(Error::Invalid(format!("feature {id} is mesh-born; feature_reorder currently requires B-rep features")));
    }
    if outputs.len() != 1 {
        return Err(Error::Invalid(format!(
            "feature {id} has multiple output bodies; moving multi-output features is not supported under the pinned app"
        )));
    }
    Ok(())
}

/// Check the rewritten graph, including suppressed and rollback-tail recipes. This validates dependencies;
/// it never edits them or interprets every driving formula owner as a geometry producer. A forward relation the
/// file already had is not caused by the move, and the file rebuilds with it, so only new ones refuse.
fn check_candidate(before: &Project, p: &Project) -> Result<()> {
    let existing = forward_references(before);
    if let Some((node, producer)) = forward_references(p).into_iter().find(|pair| !existing.contains(pair)) {
        return Err(Error::Invalid(format!("cannot reorder: feature {node} would precede required object {producer}")));
    }
    for node in &p.timeline {
        // Body/sketch isolation is native policy, including its intentional Piece exception. A reorder
        // cannot introduce a cross-parent operand through the adjacent suffix rewrite.
        if let Some(owner) = node.parent {
            if !matches!(node.kind, FeatureKind::Piece { .. }) {
                if let Some(input) = node.kind.inputs().into_iter().find(|id| p.ref_owner(*id).is_some_and(|o| o != owner)) {
                    return Err(Error::Rebuild(vec![format!(
                        "{} ({}): {}",
                        crate::localization::name(&node.name),
                        node.id,
                        crate::localization::error(&qymcad_core::errors::CoreError::CrossComponentInput { input })
                    )]));
                }
            }
        }
    }
    Ok(())
}

/// Every (node, required object) pair where the object is declared at or after the node.
fn forward_references(p: &Project) -> HashSet<(Id, Id)> {
    let mut forward = HashSet::new();
    let producers: HashMap<Id, usize> =
        p.timeline.iter().enumerate().flat_map(|(i, n)| n.kind.declares().into_iter().map(move |id| (id, i))).collect();
    for (index, node) in p.timeline.iter().enumerate() {
        let mut refs = node.kind.inputs();
        let mut expressions: Vec<String> = p.feat_dims.get(&node.id).into_iter().flat_map(|d| d.values().cloned()).collect();
        match node.kind {
            FeatureKind::Sketch { sketch } => {
                if let Some(sk) = p.sketches.iter().find(|s| s.id == sketch) {
                    match sk.plane {
                        SketchPlane::Datum(id) | SketchPlane::Face(id, _) => refs.push(id),
                        _ => {}
                    }
                    for pr in &sk.projections {
                        refs.extend([pr.body, crate::dependencies::projection_source(p, sketch, pr.body)]);
                    }
                    expressions.extend(sk.constraints.iter().filter_map(|c| c.expr().map(str::to_string)));
                    for id in sk
                        .points
                        .iter()
                        .map(|p| p.id)
                        .chain(sk.entities.iter().map(|e| e.id))
                        .chain(sk.patterns.iter().map(|p| p.id))
                        .chain(sk.texts.iter().map(|t| t.id))
                    {
                        expressions.extend(p.feat_dims.get(&id).into_iter().flat_map(|d| d.values().cloned()));
                    }
                }
            }
            FeatureKind::Plane { plane } => {
                if let Some(pl) = p.planes.iter().find(|pl| pl.id == plane) {
                    match pl.def {
                        PlaneDef::OffsetPlane { plane, .. } => refs.push(plane),
                        PlaneDef::OffsetFace { body, .. } => refs.push(body),
                        _ => {}
                    }
                }
            }
            FeatureKind::DatumAxis { axis } => {
                if let Some(ax) = p.datum_axes.iter().find(|a| a.id == axis) {
                    match ax.def {
                        AxisDef::TwoPoints { a, b } => refs.extend([a, b]),
                        AxisDef::FromEdge { body, .. } | AxisDef::FromFace { body, .. } => refs.push(body),
                        _ => {}
                    }
                }
            }
            FeatureKind::DatumPoint { point } => {
                if let Some(pt) = p.datum_points.iter().find(|pt| pt.id == point) {
                    if let PointDef::AtVertex { body, .. } = pt.def {
                        refs.push(body);
                    }
                }
            }
            FeatureKind::Hole { sketch, .. } if sketch != 0 => refs.push(sketch),
            FeatureKind::Mirror { datum, .. } if datum != 0 => refs.push(datum),
            FeatureKind::Revolve { axis_datum, .. } if axis_datum != 0 => refs.push(axis_datum),
            FeatureKind::CircularArray { axis, .. } if axis != 0 => refs.push(axis),
            FeatureKind::Thicken { join, .. } if join != 0 => refs.push(join),
            _ => {}
        }
        refs.extend(measured_sources(p, expressions).into_iter().filter(|id| !node.kind.declares().contains(id)));
        forward.extend(refs.into_iter().filter(|id| producers.get(id).is_some_and(|i| *i >= index)).map(|id| (node.id, id)));
    }
    forward
}

/// Only values measured from geometry need timeline scheduling. Driving values are scope inputs.
fn measured_sources(p: &Project, expressions: Vec<String>) -> Vec<Id> {
    let names = crate::dependencies::expression_names(p, expressions);
    p.named_dims
        .iter()
        .filter(|n| names.contains(&n.name))
        .filter_map(|named| match &named.target {
            DimTarget::Sketch { sketch, refs } => p
                .sketch_index(*sketch)
                .filter(|si| {
                    p.sketches[*si]
                        .constraints
                        .iter()
                        .any(|c| c.is_driven() && Project::dim_refs(c).is_some_and(|r| Project::dim_key_pub(&r) == *refs))
                })
                .map(|_| *sketch),
            DimTarget::Feature { node, key }
                if p.feat_dim(*node, key).is_none_or(|e| e.trim().is_empty()) && p.kept_number(*node, key).is_some() =>
            {
                p.sketches
                    .iter()
                    .find(|sk| sk.points.iter().any(|pt| pt.id == *node))
                    .map(|sk| sk.id)
                    .or_else(|| p.datum_points.iter().find(|pt| pt.id == *node && !matches!(pt.def, PointDef::Manual)).map(|pt| pt.id))
            }
            _ => None,
        })
        .collect()
}

struct ProjectedSpline {
    sketch: Id,
    projection: Id,
    entities: Vec<Id>,
    geometry: Vec<f64>,
}
fn projected_splines(p: &Project) -> Vec<ProjectedSpline> {
    let active = Session::geometry_check_nodes(p);
    p.sketches
        .iter()
        .filter(|sk| active.contains(&sk.id))
        .flat_map(|sk| {
            sk.projections
                .iter()
                .filter(|pr| pr.entities.iter().any(|id| sk.entity(*id).is_some_and(|e| matches!(e.kind, EntityKind::Spline { .. }))))
                .map(|pr| ProjectedSpline {
                    sketch: sk.id,
                    projection: pr.id,
                    entities: pr.entities.clone(),
                    geometry: crate::sketch::patterns::curve_geometry(sk, &pr.entities),
                })
        })
        .collect()
}
fn spline_refusal(sketch: Id) -> Error {
    Error::Invalid(format!("cannot reorder: projected spline in sketch {sketch} would change its source geometry; delete that sketch with cascade, reorder, then recreate it (F-141)"))
}
