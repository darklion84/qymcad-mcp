//! Native Part clones and copies (ADR 0053): the app's tree actions «Сделать клон» (`Project::clone_part`) and
//! Copy/Paste of a Part (`Project::clone_component`).
use crate::assembly::{object_name, refusal, PartRef, PartRotation};
use crate::{Id, Result, Session};
use qymcad_core::{
    feature::{rot12_axis, ComponentKind, FeatureKind, PLACE_IDENTITY},
    model::Project,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// A Part's current solid bodies: built, unconsumed and not sheets, inside the active timeline prefix. These are
/// the bodies native `active_body_before` chooses the clone's one body from (the last one, F-146).
pub(crate) fn solid_bodies(p: &Project, part: Id) -> Vec<Id> {
    let consumed = p.consumed_bodies();
    let mut out = Vec::new();
    for b in p.timeline.iter().take(p.rollback_place()).flat_map(|n| n.kind.bodies()) {
        if p.mesh_index(b).is_some()
            && !consumed.contains(&b)
            && p.body_owner(b) == Some(part)
            && !p.bodies.iter().any(|x| x.id == b && x.sheet)
            && !out.contains(&b)
        {
            out.push(b);
        }
    }
    out
}

/// Every clone Part with the source Part its PartInstance node repeats, sorted by clone id.
pub(crate) fn instance_links(p: &Project) -> BTreeMap<Id, Id> {
    p.timeline
        .iter()
        .filter_map(|n| match n.kind {
            FeatureKind::PartInstance { src_comp, .. } => n.parent.map(|clone| (clone, src_comp)),
            _ => None,
        })
        .collect()
}

/// Clones of `source`, sorted.
pub(crate) fn instances_of(links: &BTreeMap<Id, Id>, source: Id) -> Vec<Id> {
    links.iter().filter(|(_, s)| **s == source).map(|(c, _)| *c).collect()
}

/// part_place's placement convention; `None` keeps the app's own placement.
fn placement(tool: &str, translation_mm: Option<[f64; 3]>, rotation: Option<&PartRotation>) -> Result<Option<[f64; 12]>> {
    let Some(translation) = translation_mm else {
        return match rotation {
            Some(_) => Err(refusal(format!("{tool} rotation requires translation_mm"))),
            None => Ok(None),
        };
    };
    if !translation.iter().all(|v| v.is_finite()) {
        return Err(refusal(format!("{tool} translation must be finite")));
    }
    let mut t = PLACE_IDENTITY;
    if let Some(r) = rotation {
        let length = r.axis.iter().map(|v| v * v).sum::<f64>().sqrt();
        if !r.angle_deg.is_finite() || !length.is_finite() || length < 1e-12 {
            return Err(refusal(format!("{tool} rotation axis must be finite and nonzero")));
        }
        t = rot12_axis([0.; 3], r.axis.map(|v| v / length), r.angle_deg);
    }
    t[3] = translation[0];
    t[7] = translation[1];
    t[11] = translation[2];
    Ok(Some(t))
}

/// Was anything inside `part` added, changed or removed between the two recipes?
fn part_modelled(before: &Project, after: &Project, part: Id) -> bool {
    let differs = |a: &qymcad_core::feature::FeatureNode, b: &qymcad_core::feature::FeatureNode| {
        a.parent != b.parent || a.suppressed != b.suppressed || serde_json::to_value(&a.kind).ok() != serde_json::to_value(&b.kind).ok()
    };
    let nodes = after
        .timeline
        .iter()
        .filter(|n| n.parent == Some(part))
        .any(|n| before.timeline.iter().find(|o| o.id == n.id).is_none_or(|o| differs(o, n)))
        || before
            .timeline
            .iter()
            .filter(|n| n.parent == Some(part))
            .any(|n| after.timeline.iter().find(|o| o.id == n.id).is_none_or(|o| differs(o, n)));
    nodes
        || after.sketches.iter().filter(|sk| after.sketch_owner(sk.id) == Some(part)).any(|sk| {
            before.sketches.iter().find(|o| o.id == sk.id).is_none_or(|o| serde_json::to_value(o).ok() != serde_json::to_value(sk).ok())
        })
}

impl Session {
    /// Server policy for clones (ADR 0053), between the recipe before an edit and after it:
    /// - nothing is modelled inside an existing clone Part (the GUI redirects such edits into the original);
    /// - when `built`, a source with one solid body keeps exactly one, because a native clone silently copies
    ///   only the last body (F-146). Sources that were already multi-body keep the package A refusal.
    pub(crate) fn check_instance_policy(before: &Project, after: &Project, built: bool) -> Result<()> {
        let old = instance_links(before);
        let new = instance_links(after);
        // A clone Part created by this edit is the creation itself, not modelling inside a clone.
        let mut clones = old.clone();
        for (clone, source) in &new {
            if before.components.iter().any(|c| c.id == *clone) {
                clones.entry(*clone).or_insert(*source);
            }
        }
        for (clone, source) in &clones {
            // part_delete removes a whole clone Part, as the app's tree Delete does (ADR 0054).
            if !after.components.iter().any(|c| c.id == *clone) {
                continue;
            }
            // Deleting or suppressing the clone's own node alone would leave an empty Part; the app's tree Delete
            // removes the whole clone Part (`delete_component`), which is part_delete.
            let alone = |p: &Project| {
                p.timeline
                    .iter()
                    .find(|n| n.parent == Some(*clone) && matches!(n.kind, FeatureKind::PartInstance { .. }))
                    .map(|n| (n.id, n.suppressed))
            };
            let (was, now) = (alone(before), alone(after));
            if was.is_some() && (now.is_none() || now.map(|n| n.1) != was.map(|n| n.1)) {
                return Err(refusal(format!(
                    "part {clone} is an instance of {source}; its instance node cannot be deleted or suppressed alone: the app removes a clone as a whole Part; use part_delete (or undo the call that made it)"
                )));
            }
            if part_modelled(before, after, *clone) {
                return Err(refusal(format!("part {clone} is an instance of {source}; edit the source part")));
            }
        }
        if !built {
            return Ok(());
        }
        let sources: std::collections::BTreeSet<Id> = new.values().copied().collect();
        for source in sources {
            let kept: Vec<Id> = instances_of(&new, source).into_iter().filter(|c| old.get(c) == Some(&source)).collect();
            if kept.is_empty() || solid_bodies(before, source).len() != 1 {
                continue;
            }
            // A clone past the rollback bar is not built, so rolling a bodiless source back is not silent.
            let now = solid_bodies(after, source).len();
            // A suppressed base leaves the source bodiless while every rebuild keeps the clone's old body (F-153).
            let stale: Vec<Id> = kept.iter().copied().filter(|c| !solid_bodies(after, *c).is_empty()).collect();
            if now == 0 && !stale.is_empty() {
                return Err(refusal(format!(
                    "edit leaves part {source} without a solid body, but QymCAD keeps the old body in its instances {stale:?} without an error (F-153); nothing was changed"
                )));
            }
            if now > 1 {
                return Err(refusal(format!(
                    "edit makes part {source} multi-body while instances {kept:?} copy it (native clones copy only the last body, F-146); nothing was changed"
                )));
            }
        }
        Ok(())
    }

    /// A root-child source Part and a fresh unique name for a clone or copy.
    fn derived_part_source(&self, tool: &str, source: &PartRef, name: &str) -> Result<(Id, String)> {
        let name = object_name(name)?.to_string();
        let src = self.resolve_part(source)?;
        if src == self.p.root {
            return Err(refusal(format!("{tool} requires a part; the root assembly cannot be cloned or copied")));
        }
        let c = self.p.components.iter().find(|c| c.id == src).unwrap();
        if c.kind != ComponentKind::Part || c.parent != Some(self.p.root) {
            return Err(refusal(format!("{tool} requires a root-child part; part {src} is nested")));
        }
        if self.p.components.iter().any(|c| c.name == name || crate::localization::name(&c.name) == name) {
            return Err(refusal(format!("a part named '{name}' already exists; {tool} requires a unique name")));
        }
        Ok((src, name))
    }

    /// The app's «Сделать клон» (`act-clone-part`): a new root-child Part whose one PartInstance node repeats the
    /// source's body. A clone of a clone repeats the original. Without a placement the app's place beside the
    /// source is kept. Refused for a source with other than one solid body (F-146).
    pub fn part_instance(
        &mut self,
        source: &PartRef,
        name: &str,
        translation_mm: Option<[f64; 3]>,
        rotation: Option<&PartRotation>,
    ) -> Result<Value> {
        let (src, name) = self.derived_part_source("part_instance", source, name)?;
        let place = placement("part_instance", translation_mm, rotation)?;
        if self.p.rollback_place() < self.p.timeline.len() {
            return Err(refusal(
                "part_instance is refused while a history rollback is active: QymCAD inserts the clone at the rollback bar, where it misses the source's later features and a later second body passes unseen (F-152); roll the history to the end first",
            ));
        }
        let args = json!({"source":source,"name":name,"translation_mm":translation_mm,"rotation":rotation});
        self.assembly_edit("part_instance", args, |s| {
            let ((clone, origin), _) = s.atomic(|s| {
                let origin = s.p.instance_origin(src);
                let bodies = solid_bodies(&s.p, origin);
                if bodies.is_empty() {
                    return Err(refusal(format!("part_instance requires a single-body source; part {origin} has no solid body yet; model it first")));
                }
                if bodies.len() != 1 {
                    return Err(refusal(format!(
                        "part_instance requires a single-body source; part {origin} has {} solid bodies {bodies:?} and a native clone copies only the last one (F-146); part_copy copies all bodies",
                        bodies.len()
                    )));
                }
                let root = s.p.root;
                let clone = s.p.clone_part(src, root).ok_or_else(|| refusal(format!("QymCAD cannot clone part {src}")))?;
                s.p.rename_component(clone, name.clone());
                if let Some(t) = place {
                    s.p.set_component_transform(clone, t);
                }
                Ok((clone, origin))
            })?;
            let mut r = s.part_result(clone, true);
            r["part"]["instance_of"] = json!(origin);
            Ok(r)
        })
    }

    /// The app's tree Copy then «Вставить сюда» on the Part's row (`clone_component` into its parent assembly): a deep
    /// copy of every body with an independent recipe. Formulas are copied verbatim, so global parameters still
    /// drive the copy. Joints and connectors are not copied. Without a placement the copy stands where the source
    /// stands, as natively; grounding is copied too.
    pub fn part_copy(
        &mut self,
        source: &PartRef,
        name: &str,
        translation_mm: Option<[f64; 3]>,
        rotation: Option<&PartRotation>,
    ) -> Result<Value> {
        let (src, name) = self.derived_part_source("part_copy", source, name)?;
        let place = placement("part_copy", translation_mm, rotation)?;
        if place.is_some() && self.p.components.iter().any(|c| c.id == src && c.grounded) {
            return Err(refusal(format!(
                "part_copy cannot place a copy of grounded part {src}: the copy keeps its grounding, as in the app, and part_place does not move grounded parts; copy without translation_mm, then unground the copy (ground grounded:false) and part_place it"
            )));
        }
        // `clone_subtree_into` remaps a point's id but not its body and edge: a vertex point of the copy would stand on the
        // SOURCE body, and deleting the source would leave it dangling (F-175). Edge, face and two-point axes are remapped.
        let mut subtree = self.p.descendants(src);
        subtree.push(src);
        let standing: Vec<Id> = self
            .p
            .timeline
            .iter()
            .filter(|n| n.parent.is_some_and(|p| subtree.contains(&p)))
            .filter_map(|n| match n.kind {
                FeatureKind::DatumPoint { point } => self.p.datum_points.iter().find(|p| p.id == point),
                _ => None,
            })
            .filter(|p| matches!(p.def, qymcad_core::model::PointDef::AtVertex { .. }))
            .map(|p| p.id)
            .collect();
        if !standing.is_empty() {
            return Err(refusal(format!(
                "part_copy would give the copy datum points {standing:?} that stand on a vertex of the SOURCE's body: the app's copy does not remap a vertex point's body and edge (F-175). Delete them first (or make coordinate points with `at`) and copy; nothing was changed"
            )));
        }
        let args = json!({"source":source,"name":name,"translation_mm":translation_mm,"rotation":rotation});
        self.assembly_edit("part_copy", args, |s| {
            let ((copy, pairs, nodes), _) = s.atomic(|s| {
                let target = s.p.components.iter().find(|c| c.id == src).and_then(|c| c.parent).unwrap_or(s.p.root);
                let mut subtree = s.p.descendants(src);
                subtree.push(src);
                let sources: Vec<_> = s.p.timeline.iter().filter(|n| n.parent.is_some_and(|p| subtree.contains(&p))).cloned().collect();
                let start = s.p.timeline.len();
                let copy = s.p.clone_component(src, target).ok_or_else(|| refusal(format!("QymCAD cannot copy part {src}")))?;
                // clone_subtree_into appends the copied nodes in the source's order.
                let pairs: Vec<(Id, Id)> = sources
                    .iter()
                    .zip(&s.p.timeline[start..])
                    .flat_map(|(a, b)| a.kind.bodies().into_iter().zip(b.kind.bodies()))
                    .collect();
                let nodes: Vec<(Id, Id)> = sources.iter().zip(&s.p.timeline[start..]).map(|(a, b)| (a.id, b.id)).collect();
                s.p.rename_component(copy, name.clone());
                if let Some(t) = place {
                    s.p.set_component_transform(copy, t);
                }
                Ok((copy, pairs, nodes))
            })?;
            // The copy's nodes are new ids: a file's tolerated faults go with them (the copy has the same sizes).
            s.copy_open_faults(&nodes);
            let current = s.current_body_ids();
            let mut r = s.part_result(copy, true);
            r["copied_from"] = json!(src);
            r["body_map"] = json!(pairs.into_iter().filter(|(_, b)| current.contains(b)).map(|(a, b)| [a, b]).collect::<Vec<_>>());
            if let Some(origin) = instance_links(&s.p).get(&copy) {
                r["part"]["instance_of"] = json!(origin);
            }
            Ok(r)
        })
    }
}
