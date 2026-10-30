//! Native reachability extended with projection, placement and named coordinate dependencies.
use qymcad_core::model::{DimTarget, Id, Project};
use std::collections::HashSet;

/// Resolve expression reads through global aliases and named dimensions to structural owners.
/// Globals have no timeline node; a named sketch-item dimension belongs to its containing sketch.
pub(crate) fn expression_sources(p: &Project, expressions: impl IntoIterator<Item = String>) -> HashSet<Id> {
    let mut sources = HashSet::new();
    let mut pending: Vec<_> = expressions.into_iter().collect();
    let mut visited = HashSet::new();
    while let Some(expr) = pending.pop() {
        for named in &p.named_dims {
            if qymcad_core::expr::mentions(&expr, &named.name) && visited.insert(named.name.clone()) {
                let owner = match named.target {
                    DimTarget::Sketch { sketch, .. } => sketch,
                    DimTarget::Feature { node, .. } => p
                        .sketches
                        .iter()
                        .find(|sk| {
                            sk.points.iter().any(|pt| pt.id == node)
                                || sk.entities.iter().any(|e| e.id == node)
                                || sk.patterns.iter().any(|item| item.id == node)
                                || sk.texts.iter().any(|item| item.id == node)
                        })
                        .map_or(node, |sk| sk.id),
                };
                sources.insert(owner);
                pending.extend(p.dim_target_expr(&named.target));
            }
        }
        for param in &p.parameters {
            if qymcad_core::expr::mentions(&expr, &param.name) && visited.insert(param.name.clone()) {
                pending.push(param.expr.clone());
            }
        }
    }
    sources
}

/// Match native `body_seen_from`: a projection follows the source's consuming lineage above its sketch.
/// Keep the stored source as a dependency too, since deleting it invalidates the association.
pub(crate) fn projection_source(p: &Project, sketch: Id, body: Id) -> Id {
    let end = p
        .timeline
        .iter()
        .position(|n| matches!(n.kind, qymcad_core::feature::FeatureKind::Sketch { sketch: s } if s == sketch))
        .unwrap_or(p.timeline.len());
    let mut current = body;
    for _ in 0..256 {
        match p.timeline[..end].iter().find(|n| !n.suppressed && n.kind.consumed().contains(&current)).and_then(|n| n.kind.body()) {
            Some(next) if next != current => current = next,
            _ => break,
        }
    }
    current
}

/// Include roots, native outputs/consumers, and consumers reached through sketch sources and expressions.
/// A fixed point is needed for chains of projected sketches and bodies with multiple outputs.
pub(crate) fn closure(p: &Project, mut reached: HashSet<Id>) -> HashSet<Id> {
    let mut visited = HashSet::new();
    loop {
        let frontier: Vec<_> = reached.difference(&visited).copied().collect();
        if frontier.is_empty() {
            return reached;
        }
        for id in frontier {
            visited.insert(id);
            reached.extend(p.dependents(id));
            if let Some(node) = p.timeline.iter().find(|n| n.id == id) {
                reached.extend(node.kind.declares());
            }
        }
        for node in &p.timeline {
            if matches!(node.kind, qymcad_core::feature::FeatureKind::Hole { sketch, .. } if sketch != 0
                && crate::hole_sketch::sketch_sources(p, sketch).iter().any(|id| reached.contains(id)))
                || p.feat_dims.get(&node.id).is_some_and(|dims| {
                    expression_sources(p, dims.iter().filter(|(key, _)| key.starts_with("sketch_dep_")).map(|(_, expr)| expr.clone()))
                        .iter()
                        .any(|id| reached.contains(id))
                })
            {
                reached.insert(node.id);
            }
        }
        for sketch in &p.sketches {
            if crate::hole_sketch::sketch_sources(p, sketch.id).iter().any(|id| reached.contains(id)) {
                reached.insert(sketch.id);
            }
        }
    }
}

pub(crate) fn rebuild_nodes(p: &Project) -> HashSet<Id> {
    closure(p, p.regen_plan().nodes.into_iter().collect())
}

#[cfg(test)]
mod hole_tests {
    use super::*;
    use qymcad_core::model::HoleTool;

    #[test]
    fn hole_sketch_edge_enters_extended_plan() {
        let mut p = Project::default();
        let sketch = p.add_sketch("marks", vec![], None);
        p.add_sketch_node(sketch, "marks");
        let hole = p.add_hole_from_sketch(0, sketch, HoleTool { kind: 0, diameter: 2., depth: 10., dia2: 0., depth2: 0. }, false);
        assert!(closure(&p, HashSet::from([sketch])).contains(&hole), "missing Hole -> sketch dependency");
    }

    #[test]
    fn persisted_coordinate_reads_resolve_aliases_and_native_dimension_owners() {
        use qymcad_core::model::{NamedDim, Param};
        let mut p = Project::default();
        let source = p.add_offset_plane(qymcad_core::feature::BasePlane::XY, 8.);
        let sketch = p.add_sketch("marks", vec![], None);
        p.add_sketch_node(sketch, "marks");
        let hole = p.add_hole_from_sketch(0, sketch, HoleTool { kind: 0, diameter: 2., depth: 10., dia2: 0., depth2: 0. }, false);
        p.named_dims.push(NamedDim { name: "marker_x".into(), target: DimTarget::Feature { node: source, key: "dist".into() } });
        p.parameters.push(Param { name: "alias".into(), expr: "marker_x+1".into(), value: 9. });
        // The stored bridge remains a dependency even if a coordinate formula has been replaced.
        p.set_feat_dim(hole, "sketch_dep_saved_coordinate", "alias".into());
        assert!(closure(&p, HashSet::from([source])).contains(&hole));
        assert!(!closure(&p, HashSet::from([source])).contains(&sketch), "metadata alone does not make the sketch read that source");
        assert!(expression_sources(&p, ["marker_x_extra".into()]).is_empty(), "names match whole identifiers");

        let si = p.sketch_index(sketch).unwrap();
        let point = p.sketch_point_at(si, 8., 6., 1e-6);
        p.named_dims.push(NamedDim { name: "native_item".into(), target: DimTarget::Feature { node: point, key: "radius".into() } });
        p.named_dims.push(NamedDim { name: "sketch_driver".into(), target: DimTarget::Sketch { sketch, refs: vec![point] } });
        assert_eq!(expression_sources(&p, ["native_item+sketch_driver".into()]), HashSet::from([sketch]));
    }
}
