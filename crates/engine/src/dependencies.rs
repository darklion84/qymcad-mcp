//! Native reachability extended with the projection edges followed during regeneration.
use qymcad_core::model::{Id, Project};
use std::collections::HashSet;

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

/// Include roots, native outputs/consumers, and consumers reached only through sketch projections.
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
        for sketch in &p.sketches {
            if sketch.projections.iter().any(|pr| reached.contains(&pr.body) || reached.contains(&projection_source(p, sketch.id, pr.body)))
            {
                reached.insert(sketch.id);
            }
        }
    }
}

pub(crate) fn rebuild_nodes(p: &Project) -> HashSet<Id> {
    closure(p, p.regen_plan().nodes.into_iter().collect())
}
