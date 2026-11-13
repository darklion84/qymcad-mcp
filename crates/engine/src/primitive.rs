//! Primitives: the app's six commands `part.box|cylinder|sphere|cone|torus|prism` (ADR 0056).
//!
//! One action in the app: the primitive node, the sizes' formulas, an optional `Move` to the clicked vertex or datum point,
//! and a union with the single body of the Part (`apply_prim_cmd`). The GUI's field limits hold at creation and on every
//! later edit; the core itself checks only `> 1e-9` (probes j0).

use crate::datum_point::VertexEnd;
use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::value::Num;
use qymcad_core::feature::{FeatureKind, PLACE_IDENTITY};
use qymcad_core::model::{FeatNumber, Id, PlainNumber};
use serde::Serialize;
use std::collections::BTreeMap;

/// The field's upper bound for every length (`start.rs`).
pub(crate) const PRIMITIVE_MAX: f64 = 1.0e5;
/// The torus's tube may not come closer to the ring's radius than this (`sync_torus_params`).
const TORUS_GAP: f64 = 0.001;
/// Largest radius difference at which the app's cone fails with `OpFailed(Cone)` (probe: fails at 1e-7, builds at 1e-5).
const CONE_EQUAL: f64 = 1.0e-7;

/// A primitive's sizes: each a number or an expression, stored on the node's `feat_dims` keys (the field names).
#[derive(Clone, Debug, PartialEq)]
pub enum PrimitiveSpec {
    Box { dx: Num, dy: Num, dz: Num },
    Cylinder { r: Num, h: Num },
    Sphere { r: Num },
    Cone { r1: Num, r2: Num, h: Num },
    Torus { major: Num, minor: Num },
    Prism { r: Num, h: Num, n: Num },
}

impl PrimitiveSpec {
    fn kind(&self) -> &'static str {
        match self {
            PrimitiveSpec::Box { .. } => "box",
            PrimitiveSpec::Cylinder { .. } => "cylinder",
            PrimitiveSpec::Sphere { .. } => "sphere",
            PrimitiveSpec::Cone { .. } => "cone",
            PrimitiveSpec::Torus { .. } => "torus",
            PrimitiveSpec::Prism { .. } => "prism",
        }
    }

    /// The sizes by key, in the order of `FeatureKind::dims`.
    fn fields(&self) -> Vec<(&'static str, &Num)> {
        match self {
            PrimitiveSpec::Box { dx, dy, dz } => vec![("dx", dx), ("dy", dy), ("dz", dz)],
            PrimitiveSpec::Cylinder { r, h } => vec![("r", r), ("h", h)],
            PrimitiveSpec::Sphere { r } => vec![("r", r)],
            PrimitiveSpec::Cone { r1, r2, h } => vec![("r1", r1), ("r2", r2), ("h", h)],
            PrimitiveSpec::Torus { major, minor } => vec![("major", major), ("minor", minor)],
            PrimitiveSpec::Prism { r, h, n } => vec![("r", r), ("n", n), ("h", h)],
        }
    }
}

/// Where the primitive stands. As in the app a click gives a pure translation and no association: the point or vertex does not
/// lead the primitive afterwards.
#[derive(Clone, Debug, PartialEq)]
pub enum Placement {
    /// On the Part's origin: box, cylinder, cone and prism on z = 0 centred on XY, sphere and torus centred on the origin.
    Origin,
    /// At a datum point of the active Part.
    Point(Id),
    /// At an end of a persistent edge of a current body of the active Part.
    Vertex { edge: u32, end: VertexEnd, body: Option<Id> },
}

/// What the union with the Part's body did.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Joined {
    /// The Part's body the primitive was united with.
    pub prior: Id,
    /// The union node, which is the resulting body.
    pub union: Id,
}

/// A primitive as it stands after the rebuild.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PrimitiveInfo {
    /// The primitive's node: the target of `dimension_set` (keys are the size names).
    pub feature: Id,
    /// The Part's body after the action: the primitive itself for the first one, the union otherwise.
    pub body: Id,
    pub kind: &'static str,
    pub name: String,
    /// Evaluated sizes after the rebuild.
    pub sizes: BTreeMap<&'static str, f64>,
    /// The translation applied (omitted for the origin).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placed_at: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joined: Option<Joined>,
}

/// The kind's name and its own `FeatureKind` keys, for the six primitive kinds.
fn primitive_kind(kind: &FeatureKind) -> Option<&'static str> {
    Some(match kind {
        FeatureKind::Box3 { .. } => "box",
        FeatureKind::Cylinder { .. } => "cylinder",
        FeatureKind::Sphere { .. } => "sphere",
        FeatureKind::Cone { .. } => "cone",
        FeatureKind::Torus { .. } => "torus",
        FeatureKind::Prism { .. } => "prism",
        _ => return None,
    })
}

/// The field's lower bound. The app's creation field and its edit field differ for the cone's first radius only.
fn lower(kind: &str, key: &str, creating: bool) -> f64 {
    match (kind, key) {
        ("box", _) | ("cylinder", "h") | ("cone", "h") | ("prism", "h") | ("torus", _) | ("prism", "r") => 0.1,
        ("cylinder", "r") | ("sphere", "r") => 0.05,
        ("cone", "r1") if creating => 0.01,
        _ => 0.0,
    }
}

/// Judge evaluated sizes against the app's fields. `value` gives a size by key.
fn judge(kind: &str, value: &dyn Fn(&str) -> f64, creating: bool) -> Result<()> {
    let keys: &[&str] = match kind {
        "box" => &["dx", "dy", "dz"],
        "cylinder" => &["r", "h"],
        "sphere" => &["r"],
        "cone" => &["r1", "r2", "h"],
        "torus" => &["major", "minor"],
        _ => &["r", "h"],
    };
    for key in keys {
        let v = value(key);
        let lo = lower(kind, key, creating);
        let hi = if kind == "torus" && *key == "minor" { PRIMITIVE_MAX.min(value("major") - TORUS_GAP) } else { PRIMITIVE_MAX };
        if !(lo..=hi).contains(&v) {
            let hint = if kind == "torus" && *key == "minor" { " (the tube stays 0.001 mm inside the ring's radius `major`)" } else { "" };
            return Err(Error::Invalid(format!(
                "{kind} {key} must be within {lo}..={hi} mm, as the app's field requires (below it the core builds silently wrong bodies, F-180), got {v}{hint}"
            )));
        }
    }
    match kind {
        "prism" => {
            let n = value("n");
            if n.fract() != 0.0 || !(3.0..=64.0).contains(&n) {
                return Err(Error::Invalid(format!(
                    "prism n must be a whole number of sides in 3..=64, got {n} (the app silently rounds it and takes at least 3, F-181)"
                )));
            }
        }
        "cone" => {
            let (r1, r2) = (value("r1"), value("r2"));
            if (r1 - r2).abs() <= CONE_EQUAL {
                return Err(Error::Invalid(format!(
                    "cone r1 and r2 are equal ({r1}): the app's cone fails there (OpFailed); use a cylinder (F-182)"
                )));
            }
        }
        _ => {}
    }
    Ok(())
}

impl Session {
    /// Create a primitive, as one app action: the node, its formulas, the placement move and the union with the Part's body.
    pub fn primitive(&mut self, spec: &PrimitiveSpec, place: &Placement, name: Option<&str>) -> Result<(PrimitiveInfo, Rebuild)> {
        self.refuse_primitive_outside_a_part()?;
        if let Placement::Vertex { .. } = place {
            self.ensure_topology()?;
        }
        let (result, report) = self.atomic(|s| {
            let vars = s.p.param_map();
            let mut values: BTreeMap<&'static str, f64> = BTreeMap::new();
            for (key, input) in spec.fields() {
                values.insert(key, input.eval(&vars).map_err(|e| Error::Expr(format!("{} {key}: {e}", spec.kind())))?);
            }
            judge(spec.kind(), &|key| values[key], true)?;
            let at = s.placement_point(place)?;
            let v = |key: &str| values[key];
            let id = match spec {
                PrimitiveSpec::Box { .. } => s.p.add_box(v("dx"), v("dy"), v("dz")),
                PrimitiveSpec::Cylinder { .. } => s.p.add_cylinder(v("r"), v("h")),
                PrimitiveSpec::Sphere { .. } => s.p.add_sphere(v("r")),
                PrimitiveSpec::Cone { .. } => s.p.add_cone(v("r1"), v("r2"), v("h")),
                PrimitiveSpec::Torus { .. } => s.p.add_torus(v("major"), v("minor")),
                PrimitiveSpec::Prism { .. } => s.p.add_prism(v("r"), v("n") as u32, v("h")),
            };
            // The app's `store_cmd_exprs`: formulas stand on the node, a plain number clears the key.
            for (key, input) in spec.fields() {
                if let Some(text) = input.expr().filter(|e| e.parse::<f64>().is_err()) {
                    s.p.write_feat_formula(&FeatNumber { node: id, key }, &text, PlainNumber::ClearsFormula)
                        .map_err(|e| Error::Expr(format!("{} {key}: {e:?}", spec.kind())))?;
                }
            }
            s.set_node_name(id, name)?;
            let moved = at.map(|at| {
                let mut m = PLACE_IDENTITY;
                (m[3], m[7], m[11]) = (at[0], at[1], at[2]);
                m
            });
            let placed = match moved {
                Some(m) if m != PLACE_IDENTITY => s.p.add_move(id, m),
                _ => id,
            };
            let body = s.p.finish_base_body(placed, 1);
            Ok((id, body, moved.filter(|m| *m != PLACE_IDENTITY).map(|m| [m[3], m[7], m[11]])))
        })?;
        let (feature, body, placed_at) = result;
        let joined = self.p.timeline.iter().find(|n| n.id == body).and_then(|n| match n.kind {
            FeatureKind::BodyBoolean { a, .. } if body != feature => Some(Joined { prior: a, union: body }),
            _ => None,
        });
        let info = PrimitiveInfo {
            feature,
            body,
            kind: spec.kind(),
            name: self.node_name(feature),
            sizes: self.primitive_sizes(feature),
            placed_at,
            joined,
        };
        Ok((info, report))
    }

    /// A primitive's numbers as the rebuild takes them: the node's expression when there is one, else the stored number.
    pub(crate) fn primitive_sizes(&self, node: Id) -> BTreeMap<&'static str, f64> {
        let vars = self.p.param_map();
        let Some(kind) = self.p.timeline.iter().find(|n| n.id == node).map(|n| &n.kind) else { return BTreeMap::new() };
        kind.dims()
            .into_iter()
            .map(|(key, seed)| {
                let value = self
                    .p
                    .feat_dim(node, key)
                    .filter(|e| !e.is_empty())
                    .and_then(|e| Num::Expr(e.to_string()).eval(&vars).ok())
                    .unwrap_or(seed);
                (key, value)
            })
            .collect()
    }

    /// Primitives, like datums, are made in a Part: the app would create or reuse a Part when the assembly (root or nested) is
    /// the active context (`body_parent`), which this server does not do silently.
    fn refuse_primitive_outside_a_part(&self) -> Result<()> {
        let ctx = self.p.active_component.unwrap_or(self.p.root);
        if !self.p.ctx_holds_bodies(ctx) {
            return Err(Error::Invalid(
                "the active context is an assembly, not a Part; the app would create a Part for the primitive, which is not covered here (part_select a Part first)".into(),
            ));
        }
        Ok(())
    }

    /// The translation a placement asks for, in the Part's frame.
    fn placement_point(&self, place: &Placement) -> Result<Option<[f64; 3]>> {
        match place {
            Placement::Origin => Ok(None),
            Placement::Point(id) => {
                let point = self
                    .p
                    .datum_points
                    .iter()
                    .find(|p| p.id == *id)
                    .ok_or_else(|| Error::NotFound(format!("datum point {id} (make one with datum_point)")))?;
                let owner = self.p.timeline.iter().find(|n| n.id == *id).and_then(|n| n.parent);
                if self.has_assembly_structure() && owner != self.p.active_component {
                    return Err(Error::Invalid(format!("datum point {id} belongs to another Part; the primitive would take its coordinates in the wrong frame (part_select first)")));
                }
                Ok(Some(point.at))
            }
            Placement::Vertex { edge, end, body } => {
                let body = self.current_part_body(*body, "vertex", true)?;
                let found = self.p.regen_edges.get(&body).and_then(|edges| edges.iter().find(|e| e.id == *edge));
                let found =
                    found.ok_or_else(|| Error::NotFound(format!("edge {edge} of body {body} (re-read topology after each feature)")))?;
                Ok(Some(if *end == VertexEnd::End { found.b } else { found.a }))
            }
        }
    }

    /// The primitive nodes whose evaluated sizes are outside the app's fields (edit limits: the cone's first radius may be 0).
    pub(crate) fn primitive_faults(&self) -> Vec<(Id, Error)> {
        let vars = self.p.param_map();
        let mut faults = Vec::new();
        for node in &self.p.timeline {
            let Some(kind) = primitive_kind(&node.kind) else { continue };
            let mut values: BTreeMap<&'static str, f64> = BTreeMap::new();
            let mut unreadable = None;
            for (key, seed) in node.kind.dims() {
                match self.p.feat_dim(node.id, key).filter(|e| !e.is_empty()) {
                    Some(text) => match Num::Expr(text.to_string()).eval(&vars) {
                        Ok(v) => values.insert(key, v),
                        Err(e) => {
                            unreadable = Some(e);
                            break;
                        }
                    },
                    None => values.insert(key, seed),
                };
            }
            match unreadable {
                Some(e) => faults.push((node.id, e)),
                None => {
                    if let Err(e) = judge(kind, &|key| values[key], false) {
                        faults.push((node.id, e));
                    }
                }
            }
        }
        faults
    }

    /// The first primitive fault that is new: a primitive that was already outside the fields when the file was opened (the app's
    /// properties panel has no limits) is reported then and does not block other edits.
    pub(crate) fn check_primitive_nodes(&self) -> std::result::Result<(), (Id, Error)> {
        match self.primitive_faults().into_iter().find(|(id, _)| !self.open_node_faults.contains(id)) {
            Some(fault) => Err(fault),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app's properties panel has no field limits: a file can hold a primitive below the fields. It opens with a warning and
    /// does not block unrelated edits; once it is back inside the fields a new fault is judged again.
    #[test]
    fn a_file_with_a_primitive_outside_the_fields_opens_with_a_warning_and_edits_elsewhere_pass() {
        let mut s = Session::new_part();
        let (info, _) =
            s.primitive(&PrimitiveSpec::Box { dx: 20.0.into(), dy: 20.0.into(), dz: 20.0.into() }, &Placement::Origin, None).unwrap();
        // As the panel writes it: the literal stands as text on the node, then the app rebuilds.
        s.p.set_feat_dim(info.feature, "dx", "0.05".into());
        s.p.mark_all_dirty();
        s.rebuild();
        let dir = std::env::temp_dir().join(format!("qymcad-mcp-primitive-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("below-field.qcad");
        s.save(Some(&path)).unwrap();
        let (mut loaded, report) = Session::open(&path).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(
            report.warnings.iter().any(|w| w.node == info.feature && w.message.contains("box dx must be within 0.1")),
            "{:?}",
            report.warnings
        );
        loaded.param_set("unrelated", &1.0.into()).unwrap();
        // Back inside the field the exemption is gone: a later fault is a new one.
        loaded
            .dimension_set(&crate::DimensionTarget::Feature { feature: info.feature, key: "dx".into() }, Some(&20.0.into()), None)
            .unwrap();
        assert!(loaded.open_node_faults.is_empty());
        assert!(loaded
            .dimension_set(&crate::DimensionTarget::Feature { feature: info.feature, key: "dx".into() }, Some(&0.05.into()), None)
            .is_err());
    }
}
