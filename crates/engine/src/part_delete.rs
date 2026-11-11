//! The app's tree Delete of a Part (ADR 0054 §1): `execute_delete` on a component runs `set_context_to(root)` when
//! the Part is on the active path, then `Project::delete_component`, then a dirty-only resync. One undo step.
use crate::assembly::{refusal, PartRef};
use crate::{Error, Id, Result, Session};
use qymcad_core::{feature::ComponentKind, model::Project};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashSet};

/// The keys a joint or a connector keeps formulas under (slots, limits, anchor turn and offsets).
const JOINT_KEYS: [&str; 13] = ["angle", "offset", "offset2", "min0", "min1", "min2", "max0", "max1", "max2", "turn", "x", "y", "z"];

/// `feat_dims` entries left behind by joints and connectors that no longer exist (F-155): native `delete_component`
/// removes joints but not their formulas. Nothing reads them natively; the server must not count them as users.
pub(crate) fn orphan_joint_formulas(p: &Project) -> BTreeSet<Id> {
    let mut live: HashSet<Id> = HashSet::new();
    live.extend(p.joints.iter().map(|j| j.id));
    live.extend(p.connectors.iter().map(|c| c.id));
    live.extend(p.relations.iter().map(|r| r.id));
    live.extend(p.mate_constraints.iter().map(|c| c.id));
    live.extend(p.components.iter().map(|c| c.id));
    live.extend(p.timeline.iter().flat_map(|n| std::iter::once(n.id).chain(n.kind.bodies())));
    live.extend(p.bodies.iter().map(|b| b.id));
    live.extend(p.planes.iter().map(|x| x.id));
    live.extend(p.datum_points.iter().map(|x| x.id));
    live.extend(p.datum_axes.iter().map(|x| x.id));
    for s in &p.sketches {
        live.insert(s.id);
        live.extend(s.points.iter().map(|x| x.id));
        live.extend(s.patterns.iter().map(|x| x.id));
        live.extend(s.texts.iter().map(|x| x.id));
    }
    p.feat_dims
        .iter()
        .filter(|(id, dims)| !live.contains(id) && dims.keys().all(|k| JOINT_KEYS.contains(&k.as_str())))
        .map(|(id, _)| *id)
        .collect()
}

impl Session {
    /// The app's tree Delete of a root-child Part with everything it holds. Joints touching it go with it, as
    /// natively; the surviving connector and the deleted joints' formulas stay, as natively (F-155), and are
    /// reported. Refused: the root, a nested Part, a cloned source (F-154), an active rollback (F-156) and
    /// unsupported derived/constraint records (ADR 0051).
    pub fn part_delete(&mut self, part: &PartRef) -> Result<Value> {
        let root_refusal = || refusal("the root assembly cannot be deleted");
        let root = self.p.root;
        let names_root = match part {
            PartRef::Root { root } => *root,
            PartRef::Id(id) => *id == root,
            PartRef::Name(name) => name.trim().parse::<Id>().ok() == Some(root),
        };
        if names_root {
            return Err(root_refusal());
        }
        let id = self.resolve_part(part)?;
        if id == self.p.root {
            return Err(root_refusal());
        }
        let c = self.p.components.iter().find(|c| c.id == id).unwrap();
        if c.kind != ComponentKind::Part || c.parent != Some(self.p.root) {
            return Err(refusal(format!("part_delete requires a root-child part; part {id} is nested")));
        }
        let clones = crate::instances::instances_of(&crate::instances::instance_links(&self.p), id);
        if !clones.is_empty() {
            return Err(refusal(format!(
                "part {id} is the source of instances {clones:?}; delete the instances first (QymCAD leaves them pointing at a missing part with their old body, F-154); nothing was changed"
            )));
        }
        if self.p.rollback_place() < self.p.timeline.len() {
            return Err(refusal(
                "part_delete is refused while a history rollback is active: QymCAD keeps the bar's index, so nodes past the bar would be built (F-156); roll the history to the end first; nothing was changed",
            ));
        }
        self.check_supported_parts(&HashSet::from([id]))?;
        let joints_before = self.p.joints.clone();
        let connectors_before: Vec<Id> = self.p.connectors.iter().map(|c| c.id).collect();
        let orphans_before = orphan_joint_formulas(&self.p);
        self.assembly_edit("part_delete", json!({"part":part}), |s| {
            let (bodies, _) = s.atomic(|s| {
                let root = s.p.root;
                let mut subtree = s.p.descendants(id);
                subtree.push(id);
                if s.p.active_component.is_some_and(|a| subtree.contains(&a)) {
                    s.p.set_active_component(Some(root));
                }
                let bodies = s.p.delete_component(id);
                if !s.p.components.iter().all(|c| c.id != id) {
                    return Err(Error::Invalid(format!("QymCAD did not delete part {id}")));
                }
                let live: HashSet<Id> = s.p.timeline.iter().flat_map(|n| n.kind.bodies()).collect();
                s.shapes.retain(|b, _| live.contains(b));
                Ok(bodies)
            })?;
            let mut joints: Vec<Id> = joints_before.iter().map(|j| j.id).filter(|j| !s.p.joints.iter().any(|x| x.id == *j)).collect();
            joints.sort_unstable();
            let mut connectors: Vec<Id> = connectors_before.iter().copied().filter(|c| s.p.connector(*c).is_none()).collect();
            connectors.sort_unstable();
            let mut kept: Vec<Id> = joints_before
                .iter()
                .filter(|j| joints.contains(&j.id))
                .flat_map(|j| [j.a, j.b])
                .filter(|c| s.p.connector(*c).is_some_and(|c| !c.standalone) && s.p.connector_users(*c).is_empty())
                .collect();
            kept.sort_unstable();
            kept.dedup();
            let formulas: Vec<Id> = orphan_joint_formulas(&s.p).difference(&orphans_before).copied().collect();
            let mut bodies = bodies;
            bodies.sort_unstable();
            Ok(json!({"removed":{"part":id,"bodies":bodies,"joints":joints,"connectors":connectors},"native_leftovers":{"connectors":kept,"formulas":formulas},"active_component":s.p.active_component,"placements":s.placements(),"diagnostics":s.assembly_diagnostics(),"changed":true}))
        })
    }
}
