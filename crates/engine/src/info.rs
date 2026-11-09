//! `DocInfo`: a compact picture of the whole document for the agent.

use crate::params::ParamInfo;
use crate::session::{BodyInfo, NodeIssue, Session};
use crate::sketch::SketchInfo;
use qymcad_core::model::Id;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct NodeInfo {
    pub id: Id,
    pub name: String,
    /// Native owning Part (assemblies can own datum/skeleton nodes in imported files). Omitted, like body
    /// placement fields, in documents without assembly structure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_part: Option<Id>,
    /// Feature kind; sketch extrusion booleans expose their operation (`extrude (cut/add/intersect)`).
    pub kind: String,
    pub suppressed: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ComponentInfo {
    pub id: Id,
    pub name: String,
    pub parent: Option<Id>,
    pub kind: &'static str,
    pub local_transform: [f64; 12],
    pub world_transform: [f64; 12],
    pub grounded: bool,
    /// Current body ids owned directly by this component.
    pub bodies: Vec<Id>,
    /// The source Part this clone repeats (native PartInstance, ADR 0053).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_of: Option<Id>,
    /// The clone Parts that repeat this source Part.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<Id>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DocInfo {
    pub qymcad_version: &'static str,
    pub path: Option<String>,
    pub components: Vec<ComponentInfo>,
    pub active_component: Option<Id>,
    /// Native mechanism and its diagnostics, separate from feature regeneration errors.
    pub assembly: serde_json::Value,
    pub params: Vec<ParamInfo>,
    /// Named native sketch, feature and sketch-item drivers with editable selectors and formulas.
    pub named_dimensions: Vec<crate::dimensions::DimensionInfo>,
    pub sketches: Vec<SketchInfo>,
    /// The timeline (the recipe), in build order.
    pub timeline: Vec<NodeInfo>,
    /// Persisted native active-prefix boundary; the inactive timeline remains in the recipe.
    pub rollback: crate::RollbackInfo,
    pub bodies: Vec<BodyInfo>,
    pub errors: Vec<NodeIssue>,
    pub warnings: Vec<String>,
}

impl Session {
    /// Snapshot the current document for reporting.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()`;
    /// result-body bbox reporting acquires that non-reentrant mutex.
    pub fn info(&self) -> DocInfo {
        let p = self.project();
        let bodies = self.result_bodies();
        let assembly_view = self.has_assembly_structure();
        let mut warnings: Vec<String> =
            p.regen_warnings.iter().map(|(id, e)| format!("{} ({id}): {}", self.node_name(*id), crate::localization::error(e))).collect();
        warnings.extend(self.advisory_warnings.iter().map(|w| {
            let w = w.issue();
            format!("{} ({}): {}", w.name, w.node, w.message)
        }));
        warnings.extend(self.cross_part_projection_warnings().iter().map(|w| format!("{} ({}): {}", w.name, w.node, w.message)));
        warnings.extend(
            bodies.iter().filter_map(|b| self.ambiguous_face_warning(b.id).map(|message| format!("{} ({}): {message}", b.name, b.id))),
        );
        warnings.extend(self.param_formula_problems());
        for twins in self.case_twin_params() {
            warnings.push(format!(
                "names {} differ only by letter case; QymCAD keeps them apart, so check each formula names the one it means",
                twins.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
            ));
        }
        let links = crate::instances::instance_links(p);
        DocInfo {
            qymcad_version: crate::QYMCAD_VERSION,
            path: self.path().map(|p| p.display().to_string()),
            components: p
                .components
                .iter()
                .map(|c| ComponentInfo {
                    id: c.id,
                    name: crate::localization::name(&c.name),
                    parent: c.parent,
                    kind: match c.kind {
                        qymcad_core::feature::ComponentKind::Part => "part",
                        qymcad_core::feature::ComponentKind::Assembly => "assembly",
                    },
                    local_transform: c.transform,
                    world_transform: p.world_transform(c.id),
                    grounded: c.grounded,
                    bodies: bodies.iter().filter(|b| b.owner_part == Some(c.id)).map(|b| b.id).collect(),
                    instance_of: links.get(&c.id).copied(),
                    instances: crate::instances::instances_of(&links, c.id),
                })
                .collect(),
            active_component: p.active_component,
            assembly: self.assembly_info(),
            params: self.params(),
            named_dimensions: self.named_dimensions(),
            sketches: self.sketches(),
            timeline: p
                .timeline
                .iter()
                .map(|n| NodeInfo {
                    id: n.id,
                    name: crate::localization::name(&n.name),
                    owner_part: n.parent.filter(|_| assembly_view),
                    kind: kind_name(&n.kind),
                    suppressed: n.suppressed,
                })
                .collect(),
            rollback: self.rollback_info(),
            bodies,
            errors: p
                .regen_errors
                .iter()
                .map(|(id, e)| NodeIssue { node: *id, name: self.node_name(*id), message: crate::localization::error(e) })
                .collect(),
            warnings,
        }
    }
}

fn kind_name(k: &qymcad_core::feature::FeatureKind) -> String {
    use qymcad_core::feature::FeatureKind;
    if let FeatureKind::Combine { op, .. } = k {
        let op = match op {
            0 => "cut",
            1 => "add",
            2 => "intersect",
            _ => "unknown",
        };
        return format!("extrude ({op})");
    }
    let s = format!("{k:?}");
    s.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_lowercase()
}
