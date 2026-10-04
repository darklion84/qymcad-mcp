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
    /// Feature kind as QymCAD names it (`Extrude`, `Combine`, `Plane`, `Sketch`, ...).
    pub kind: String,
    pub suppressed: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DocInfo {
    pub qymcad_version: &'static str,
    pub path: Option<String>,
    pub params: Vec<ParamInfo>,
    pub sketches: Vec<SketchInfo>,
    /// The timeline (the recipe), in build order.
    pub timeline: Vec<NodeInfo>,
    pub bodies: Vec<BodyInfo>,
    pub errors: Vec<NodeIssue>,
    pub warnings: Vec<String>,
}

impl Session {
    pub fn info(&self) -> DocInfo {
        let p = self.project();
        let mut warnings: Vec<String> = p.regen_warnings.iter().map(|(id, e)| format!("{} ({id}): {e}", self.node_name(*id))).collect();
        let upper = self.uppercase_params();
        if !upper.is_empty() {
            warnings.push(format!(
                "parameters {upper:?} have uppercase letters: QymCAD {} does not rebuild features from them in the GUI; rename them lowercase",
                crate::QYMCAD_VERSION
            ));
        }
        DocInfo {
            qymcad_version: crate::QYMCAD_VERSION,
            path: self.path().map(|p| p.display().to_string()),
            params: self.params(),
            sketches: self.sketches(),
            timeline: p
                .timeline
                .iter()
                .map(|n| NodeInfo { id: n.id, name: n.name.clone(), kind: kind_name(&n.kind), suppressed: n.suppressed })
                .collect(),
            bodies: self.result_bodies(),
            errors: p
                .regen_errors
                .iter()
                .map(|(id, e)| NodeIssue { node: *id, name: self.node_name(*id), message: e.to_string() })
                .collect(),
            warnings,
        }
    }
}

fn kind_name(k: &qymcad_core::feature::FeatureKind) -> String {
    let s = format!("{k:?}");
    s.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_string()
}
