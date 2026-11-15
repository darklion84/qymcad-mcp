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

/// Datum points and axes with their resolved geometry, in the part frames they belong to.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct DatumsInfo {
    pub points: Vec<crate::DatumPointInfo>,
    pub axes: Vec<crate::DatumAxisInfo>,
    /// Rebuild errors of datum nodes.
    pub errors: Vec<NodeIssue>,
}

impl DatumsInfo {
    fn is_empty(&self) -> bool {
        self.points.is_empty() && self.axes.is_empty() && self.errors.is_empty()
    }
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
    /// Datum points and axes (omitted when there are none).
    #[serde(skip_serializing_if = "DatumsInfo::is_empty")]
    pub datums: DatumsInfo,
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
            timeline: self.timeline(),
            datums: DatumsInfo {
                points: p.datum_points.iter().filter_map(|d| self.datum_point_info(d.id).ok()).collect(),
                axes: p.datum_axes.iter().filter_map(|d| self.datum_axis_info(d.id).ok()).collect(),
                errors: p
                    .regen_errors
                    .iter()
                    .filter(|(id, _)| p.datum_points.iter().any(|d| d.id == **id) || p.datum_axes.iter().any(|d| d.id == **id))
                    .map(|(id, e)| NodeIssue { node: *id, name: self.node_name(*id), message: crate::localization::error(e) })
                    .collect(),
            },
            rollback: self.rollback_info(),
            bodies,
            errors: p
                .regen_errors
                .iter()
                .map(|(id, e)| NodeIssue { node: *id, name: self.node_name(*id), message: crate::localization::error(e) })
                .collect(),
            warnings: self.warnings(),
        }
    }

    /// The document warnings `info()` reports, without the body report (no tessellation).
    pub fn warnings(&self) -> Vec<String> {
        let p = self.project();
        let mut warnings: Vec<String> = p.regen_warnings.iter().map(|(id, e)| self.node_text(*id, crate::localization::error(e))).collect();
        warnings.extend(self.advisory_warnings.iter().map(|w| w.issue().to_string()));
        warnings.extend(self.cross_part_projection_warnings().iter().map(|w| w.to_string()));
        warnings.extend(
            self.current_body_ids()
                .into_iter()
                .filter_map(|b| self.ambiguous_face_warning(b).map(|message| format!("{} ({b}): {message}", self.body_name(b)))),
        );
        warnings.extend(self.param_formula_problems());
        for twins in self.case_twin_params() {
            warnings.push(format!(
                "names {} differ only by letter case; QymCAD keeps them apart, so check each formula names the one it means",
                twins.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
            ));
        }
        warnings
    }

    /// The timeline (the recipe) in build order, as `info()` reports it, without the body report.
    pub fn timeline(&self) -> Vec<NodeInfo> {
        let assembly_view = self.has_assembly_structure();
        self.project()
            .timeline
            .iter()
            .map(|n| NodeInfo {
                id: n.id,
                name: crate::localization::name(&n.name),
                owner_part: n.parent.filter(|_| assembly_view),
                kind: kind_name(&n.kind),
                suppressed: n.suppressed,
            })
            .collect()
    }
}

/// The name of a node's kind as `timeline[].kind` reports it (a public field, so these words are an interface).
///
/// An explicit `match` with no `_` arm: a variant added by a QymCAD upgrade stops the build here until it is given a
/// word on purpose, instead of leaking its `Debug` spelling into the API. The words are the lowercased variant names
/// (what the `Debug` form used to give), except `Combine`, which reads by its operation.
fn kind_name(k: &qymcad_core::feature::FeatureKind) -> String {
    use qymcad_core::feature::FeatureKind;
    match k {
        FeatureKind::Combine { op, .. } => {
            let op = match op {
                0 => "cut",
                1 => "add",
                2 => "intersect",
                _ => "unknown",
            };
            format!("extrude ({op})")
        }
        FeatureKind::Sketch { .. } => "sketch".into(),
        FeatureKind::Plane { .. } => "plane".into(),
        FeatureKind::DatumPoint { .. } => "datumpoint".into(),
        FeatureKind::DatumAxis { .. } => "datumaxis".into(),
        FeatureKind::Extrude { .. } => "extrude".into(),
        FeatureKind::Revolve { .. } => "revolve".into(),
        FeatureKind::Sweep { .. } => "sweep".into(),
        FeatureKind::Loft { .. } => "loft".into(),
        FeatureKind::Box3 { .. } => "box3".into(),
        FeatureKind::Cylinder { .. } => "cylinder".into(),
        FeatureKind::Sphere { .. } => "sphere".into(),
        FeatureKind::Fillet { .. } => "fillet".into(),
        FeatureKind::Patch { .. } => "patch".into(),
        FeatureKind::SurfaceReplace { .. } => "surfacereplace".into(),
        FeatureKind::Trim { .. } => "trim".into(),
        FeatureKind::Stitch { .. } => "stitch".into(),
        FeatureKind::MeshSolid { .. } => "meshsolid".into(),
        FeatureKind::MeshRecognised { .. } => "meshrecognised".into(),
        FeatureKind::FaceCopy { .. } => "facecopy".into(),
        FeatureKind::OffsetSurface { .. } => "offsetsurface".into(),
        FeatureKind::Chamfer { .. } => "chamfer".into(),
        FeatureKind::Cone { .. } => "cone".into(),
        FeatureKind::Torus { .. } => "torus".into(),
        FeatureKind::Prism { .. } => "prism".into(),
        FeatureKind::Shell { .. } => "shell".into(),
        FeatureKind::Draft { .. } => "draft".into(),
        FeatureKind::LinearArray { .. } => "lineararray".into(),
        FeatureKind::CircularArray { .. } => "circulararray".into(),
        FeatureKind::MirrorPart { .. } => "mirrorpart".into(),
        FeatureKind::Thicken { .. } => "thicken".into(),
        FeatureKind::SplitFace { .. } => "splitface".into(),
        FeatureKind::PartInstance { .. } => "partinstance".into(),
        FeatureKind::ComponentPattern { .. } => "componentpattern".into(),
        FeatureKind::Mirror { .. } => "mirror".into(),
        FeatureKind::Hole { .. } => "hole".into(),
        FeatureKind::Thread { .. } => "thread".into(),
        FeatureKind::Auger { .. } => "auger".into(),
        FeatureKind::RemoveFace { .. } => "removeface".into(),
        FeatureKind::SplitBody { .. } => "splitbody".into(),
        FeatureKind::PushFace { .. } => "pushface".into(),
        FeatureKind::Move { .. } => "move".into(),
        FeatureKind::Piece { .. } => "piece".into(),
        FeatureKind::BodyBoolean { .. } => "bodyboolean".into(),
        FeatureKind::Import { .. } => "import".into(),
        FeatureKind::MeshPiece { .. } => "meshpiece".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::kind_name;
    use qymcad_core::feature::FeatureKind;

    /// What `kind_name` reported before it became a `match`: the first word of `Debug`, lowercased.
    fn debug_word(k: &FeatureKind) -> String {
        format!("{k:?}").split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_lowercase()
    }

    #[test]
    fn combine_reads_by_its_operation() {
        for (op, word) in [(0u8, "extrude (cut)"), (1, "extrude (add)"), (2, "extrude (intersect)"), (9, "extrude (unknown)")] {
            let k: FeatureKind =
                serde_json::from_value(serde_json::json!({"Combine": {"src": 1, "sketch": 2, "height": 1.0, "op": op, "body": 3}}))
                    .unwrap();
            assert_eq!(kind_name(&k), word);
        }
    }

    /// Every kind that has a plain constructor reads as the old `Debug` word did.
    #[test]
    fn simple_kinds_keep_their_debug_word() {
        let kinds = [
            FeatureKind::Sketch { sketch: 1 },
            FeatureKind::Plane { plane: 1 },
            FeatureKind::DatumPoint { point: 1 },
            FeatureKind::DatumAxis { axis: 1 },
            FeatureKind::Cylinder { r: 1.0, h: 1.0, body: 1 },
            FeatureKind::Sphere { r: 1.0, body: 1 },
            FeatureKind::Cone { r1: 1.0, r2: 1.0, h: 1.0, body: 1 },
            FeatureKind::Torus { major: 2.0, minor: 1.0, body: 1 },
            FeatureKind::Prism { r: 1.0, n: 6, h: 1.0, body: 1 },
            FeatureKind::Trim { src: 1, tool: 2, keep: [0.0; 3], body: 3 },
            FeatureKind::Stitch { parts: vec![1, 2], tol: 0.1, body: 3 },
            FeatureKind::Move { src: 1, mat: [0.0; 12], body: 2 },
            FeatureKind::Piece { src: 1, body: 2 },
            FeatureKind::PartInstance { src_comp: 1, body: 2 },
        ];
        for k in &kinds {
            assert_eq!(kind_name(k), debug_word(k), "{k:?}");
        }
    }

    /// Every variant the pinned enum declares (read from its source) has an arm saying its lowercased name;
    /// together with the compiler's exhaustiveness this is "no variant changed its word", also for the variants
    /// no test can build cheaply. `Combine` has its own arm (above).
    #[test]
    fn every_variant_has_its_old_word() {
        let source = include_str!("info.rs");
        let kinds = qymcad_testkit::feature_kinds();
        assert!(kinds.len() >= 40);
        for v in kinds.iter().filter(|v| *v != "Combine") {
            let arm = format!("FeatureKind::{v} {{ .. }} => \"{}\".into(),", v.to_lowercase());
            assert!(source.contains(&arm), "no arm `{arm}`");
        }
    }
}
