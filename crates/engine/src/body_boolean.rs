use crate::{BodyInfo, Error, Id, Rebuild, Result, Session};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooleanOp {
    Cut,
    Union,
    Intersect,
}

impl BooleanOp {
    pub fn native(self) -> u8 {
        match self {
            Self::Cut => 0,
            Self::Union => 1,
            Self::Intersect => 2,
        }
    }
}

impl Serialize for BooleanOp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Self::Cut => "cut",
            Self::Union => "union",
            Self::Intersect => "intersect",
        })
    }
}

#[derive(Clone, Debug)]
pub struct BodyBooleanOptions {
    pub a: Id,
    pub b: Id,
    pub op: BooleanOp,
    pub name: Option<String>,
    pub piece_names: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct BodyBooleanResult {
    pub feature: Id,
    pub body: Id,
    pub name: String,
    pub op: BooleanOp,
    pub consumed: [Id; 2],
    pub pieces: Vec<BodyInfo>,
    pub rebuild: Rebuild,
}

#[derive(Clone)]
pub(crate) struct BooleanIdentity {
    node: Id,
    outputs: Vec<(Id, [f64; 6])>,
    solids: u32,
}

impl Session {
    pub fn body_boolean(&mut self, options: &BodyBooleanOptions) -> Result<BodyBooleanResult> {
        self.ensure_topology()?;
        let (result, _) = self.atomic_with_post(
            |s| {
                if options.a == options.b {
                    return Err(Error::Invalid("body_boolean requires two distinct bodies".into()));
                }
                let a = s.source_body(Some(options.a))?;
                let b = s.source_body(Some(options.b))?;
                if s.p.body_owner(a) != s.p.body_owner(b) {
                    return Err(Error::Invalid("body_boolean operands must belong to the same part".into()));
                }
                let owner = s.p.body_owner(a);
                let feature = s.in_component(owner, |p| p.add_body_boolean(a, b, options.op.native()));
                s.set_node_name(feature, options.name.as_deref())?;
                Ok((feature, a, b))
            },
            |s, (feature, a, b), rebuild| {
                let outputs = s.node_outputs(feature);
                let names = s.piece_names(feature, outputs.len(), options.piece_names.as_deref())?;
                for (id, name) in outputs.iter().zip(names) {
                    Self::check_object_name(&name)?;
                    if let Some(index) = s.p.mesh_index(*id) {
                        s.p.set_mesh_name(index, name);
                    }
                }
                rebuild.bodies = s.result_bodies();
                let pieces = rebuild.bodies.iter().filter(|body| outputs.contains(&body.id)).cloned().collect();
                Ok(BodyBooleanResult {
                    feature,
                    body: feature,
                    name: s.node_name(feature),
                    op: options.op,
                    consumed: [a, b],
                    pieces,
                    rebuild: rebuild.clone(),
                })
            },
        )?;
        Ok(result)
    }

    fn node_outputs(&self, feature: Id) -> Vec<Id> {
        self.p.timeline.iter().find(|node| node.id == feature).map(|node| node.kind.bodies()).unwrap_or_default()
    }

    pub(crate) fn boolean_identities(&self) -> Vec<BooleanIdentity> {
        let _gate = qymcad_kernel::kernel_gate();
        self.p
            .timeline
            .iter()
            .filter_map(|node| {
                if !matches!(node.kind, qymcad_core::feature::FeatureKind::BodyBoolean { op: 0..=2, .. }) {
                    return None;
                }
                let outputs =
                    node.kind.bodies().into_iter().map(|id| Some((id, self.shapes.get(&id)?.bbox()?))).collect::<Option<Vec<_>>>()?;

                let solids = self.shapes.get(&node.id)?.solid_count();
                Some(BooleanIdentity { node: node.id, outputs, solids })
            })
            .collect()
    }

    pub(crate) fn boolean_identity_refusals(&self, before: &[BooleanIdentity], rebuilt: &HashSet<Id>) -> Vec<(Id, String)> {
        let mut errors = Vec::new();
        for identity in
            before.iter().filter(|identity| identity.outputs.iter().any(|(id, _)| rebuilt.contains(id)) || rebuilt.contains(&identity.node))
        {
            let outputs = self.node_outputs(identity.node);
            if outputs.len() != identity.outputs.len() {
                errors.push((identity.node, format!("body_boolean output count changed from {} to {}; edit refused atomically because piece identities or downstream references would change; recreate the boolean", identity.outputs.len(), outputs.len())));
                continue;
            }
            let (after, solids) = {
                let _gate = qymcad_kernel::kernel_gate();
                let bounds = outputs.iter().map(|id| self.shapes.get(id).and_then(|shape| shape.bbox())).collect::<Option<Vec<_>>>();
                let solids = self.shapes.get(&identity.node).map(|shape| shape.solid_count());
                (bounds, solids)
            };
            let Some(after) = after else {
                errors.push((identity.node, "body_boolean piece identity cannot be verified; edit refused atomically".into()));
                continue;
            };
            let Some(solids) = solids else {
                errors.push((identity.node, "body_boolean solid count cannot be verified; edit refused atomically".into()));
                continue;
            };
            if solids != identity.solids {
                errors.push((
                    identity.node,
                    format!(
                        "body_boolean solid count changed from {} to {solids}; edit refused atomically because the result topology changed",
                        identity.solids
                    ),
                ));
                continue;
            }
            let scores: Vec<Vec<f64>> =
                identity.outputs.iter().map(|(_, old)| after.iter().map(|new| boolean_overlap(old, new)).collect()).collect();
            let tolerances: Vec<f64> = identity
                .outputs
                .iter()
                .zip(&after)
                .map(|((_, old), new)| (boolean_overlap(old, old).max(boolean_overlap(new, new)) * 1e-8).max(1e-9))
                .collect();
            let changed = crate::split::uncertain_piece_assignment(&scores, &tolerances);
            if !changed.is_empty() {
                let names = changed
                    .iter()
                    .map(|&i| format!("`{}` ({})", self.body_name(identity.outputs[i].0), identity.outputs[i].0))
                    .collect::<Vec<_>>()
                    .join(", ");
                errors.push((identity.node, format!("body_boolean piece identity would change or is ambiguous for {names}; edit refused atomically, recreate the boolean")));
            }
        }
        errors
    }
}

fn boolean_overlap(a: &[f64; 6], b: &[f64; 6]) -> f64 {
    (0..3).map(|axis| (a[axis + 3].min(b[axis + 3]) - a[axis].max(b[axis])).max(0.0)).product()
}
