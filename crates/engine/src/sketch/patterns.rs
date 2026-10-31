use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::value::Num;
use qymcad_core::feature::FeatureKind;
use qymcad_core::model::{FeatNumber, Id, PatternKind, PlainNumber};
use serde::Serialize;
use std::collections::HashMap;

type PatternValues = (PatternKind, HashMap<String, String>, HashMap<String, f64>);

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SketchPatternResult {
    pub pattern: Id,
    pub sketch: Id,
    pub source: Vec<Id>,
    pub created_instances: Vec<Id>,
    pub removed_instances: Vec<Id>,
    pub evaluated: HashMap<String, f64>,
    pub formulas: HashMap<String, String>,
    pub names: HashMap<String, String>,
    pub contours: Vec<crate::ContourInfo>,
    pub rebuild: Option<Rebuild>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SketchPatternKind {
    Linear { count: Num, dx: Num, dy: Num, count2: Option<Num>, dx2: Option<Num>, dy2: Option<Num> },
    Circular { count: Num, centre: [Num; 2], angle: Num },
}

impl Session {
    pub fn sketch_pattern_create(&mut self, sketch: Id, entities: &[Id], kind: &SketchPatternKind) -> Result<SketchPatternResult> {
        self.sketch_pattern_edit(sketch, entities, None, kind)
    }

    pub fn sketch_pattern_update(
        &mut self,
        sketch: Id,
        pattern: Id,
        entities: Option<&[Id]>,
        kind: &SketchPatternKind,
    ) -> Result<SketchPatternResult> {
        self.sketch_pattern_edit(sketch, entities.unwrap_or(&[]), Some(pattern), kind)
    }

    fn sketch_pattern_edit(
        &mut self,
        sketch: Id,
        entities: &[Id],
        pattern: Option<Id>,
        kind: &SketchPatternKind,
    ) -> Result<SketchPatternResult> {
        let si = self.sketch_si(sketch)?;
        let source = match pattern {
            Some(pid) => {
                let p = self.p.sketches[si]
                    .patterns
                    .iter()
                    .find(|p| p.id == pid)
                    .ok_or_else(|| Error::NotFound(format!("pattern {pid} in sketch {sketch}")))?;
                if !entities.is_empty() && entities != p.source.as_slice() {
                    return Err(Error::Invalid(format!("pattern update entities must equal the stored source {:?}", p.source)));
                }
                p.source.clone()
            }
            None => {
                if entities.is_empty() {
                    return Err(Error::Invalid("entities must be non-empty".into()));
                }
                self.sketch_curve_ids(sketch, &crate::SketchCurveSel::Ids(entities.to_vec()))?
            }
        };
        let vars = self.p.param_map();
        let (native, formulas, evaluated) = native_kind(kind, &vars)?;
        let total = match native {
            PatternKind::Linear { count, count2, .. } => u64::from(count) * u64::from(count2.max(1)),
            PatternKind::Circular { count, .. } => u64::from(count),
        };
        check_count(total)?;
        let old_instances = pattern
            .and_then(|pid| self.p.sketches[si].patterns.iter().find(|p| p.id == pid).map(|p| p.instances.clone()))
            .unwrap_or_default();
        let profile_picks = self.pattern_profile_picks(sketch)?;
        let (pid, rebuild) = self.sketch_edit(sketch, |s| {
            let pid = match pattern {
                Some(pid) => {
                    let pi = s.p.sketches[si]
                        .patterns
                        .iter()
                        .position(|p| p.id == pid)
                        .ok_or_else(|| Error::NotFound(format!("pattern {pid}")))?;
                    s.p.update_pattern(si, pi, native);
                    pid
                }
                None => s.p.add_pattern(si, &source, native),
            };
            for (key, text) in &formulas {
                let plain = if text.parse::<f64>().is_ok() { PlainNumber::ClearsFormula } else { PlainNumber::Stands };
                s.p.write_feat_formula(&FeatNumber { node: pid, key }, text, plain)
                    .map_err(|e| Error::Expr(format!("pattern formula `{key}`: {e:?}")))?;
            }
            s.finish_sketch_edit(si)?;
            let new_instances = s.p.sketches[si].patterns.iter().find(|p| p.id == pid).map(|p| p.instances.clone()).unwrap_or_default();
            s.rebind_pattern_profiles(sketch, &source, &old_instances, &new_instances, &profile_picks)?;
            Ok(pid)
        })?;
        let new_instances = self.p.sketches[si].patterns.iter().find(|p| p.id == pid).map(|p| p.instances.clone()).unwrap_or_default();
        let mut names = HashMap::new();
        for key in formulas.keys() {
            let target = qymcad_core::model::DimTarget::Feature { node: pid, key: key.clone() };
            let name = self.p.name_of_target(&target);
            if !name.is_empty() {
                names.insert(key.clone(), name);
            }
        }
        let mut formula_map = HashMap::new();
        for key in formulas.keys() {
            if let Some(value) = self.p.feat_dim(pid, key).filter(|v| !v.is_empty()) {
                formula_map.insert(key.clone(), value.to_string());
            }
        }
        Ok(SketchPatternResult {
            pattern: pid,
            sketch,
            source,
            created_instances: new_instances.iter().filter(|id| !old_instances.contains(id)).copied().collect(),
            removed_instances: old_instances.iter().filter(|id| !new_instances.contains(id)).copied().collect(),
            evaluated,
            formulas: formula_map,
            names,
            contours: self.sketch_info(sketch)?.contours,
            rebuild,
        })
    }

    fn pattern_profile_picks(&self, sketch: Id) -> Result<Vec<(Id, std::collections::HashSet<Id>)>> {
        let si = self.sketch_si(sketch)?;
        Ok(self.p.sketches[si]
            .contour_ids
            .iter()
            .filter_map(|&cid| self.p.contours.ents_of(cid).map(|entities| (cid, entities.iter().copied().collect())))
            .collect())
    }

    pub(crate) fn record_pattern_profile_indices(&mut self, node: Id, sketch: Id, profiles: &[Id]) -> Result<()> {
        let si = self.sketch_si(sketch)?;
        let pattern = self.p.sketches[si]
            .patterns
            .iter()
            .find(|p| profiles.iter().any(|cid| self.p.contours.ents_of(*cid).is_some_and(|e| e.iter().any(|id| p.instances.contains(id)))))
            .cloned();
        let Some(pattern) = pattern else { return Ok(()) };
        let source_len = pattern.source.len();
        let indices = profiles
            .iter()
            .map(|cid| {
                let entities =
                    self.p.contours.ents_of(*cid).ok_or_else(|| Error::Invalid(format!("profile contour {cid} has no boundary")))?;
                if entities.iter().any(|id| pattern.source.contains(id)) {
                    return Ok(0usize);
                }
                pattern
                    .instances
                    .chunks(source_len)
                    .position(|chunk| entities.iter().any(|id| chunk.contains(id)))
                    .map(|i| i + 1)
                    .ok_or_else(|| Error::Invalid(format!("profile contour {cid} is not a pattern instance")))
            })
            .collect::<Result<Vec<_>>>()?;
        for (position, index) in indices.iter().enumerate() {
            let key = format!("__pattern_profile_{position}");
            self.p.set_feat_dim(node, &key, index.to_string());
        }
        Ok(())
    }

    pub(crate) fn rebind_pattern_profiles(
        &mut self,
        sketch: Id,
        source: &[Id],
        old_instances: &[Id],
        _new_instances: &[Id],
        old_picks: &[(Id, std::collections::HashSet<Id>)],
    ) -> Result<()> {
        let si = self.sketch_si(sketch)?;
        let contours = self.p.sketches[si].contour_ids.clone();
        let source_set: std::collections::HashSet<Id> = source.iter().copied().collect();
        let old_index = |entities: &std::collections::HashSet<Id>| {
            if *entities == source_set {
                Some(0)
            } else {
                old_instances
                    .chunks(source.len())
                    .position(|chunk| chunk.iter().copied().collect::<std::collections::HashSet<_>>() == *entities)
                    .map(|index| index + 1)
            }
        };
        let explicit_nodes: std::collections::HashSet<Id> =
            self.p.timeline.iter().filter(|node| self.p.feat_dim(node.id, "__profile_pick").is_some()).map(|node| node.id).collect();
        let stored_indices: std::collections::HashMap<Id, Vec<usize>> = self
            .p
            .timeline
            .iter()
            .filter_map(|node| {
                let indices = (0..)
                    .map_while(|position| self.p.feat_dim(node.id, &format!("__pattern_profile_{position}")).and_then(|v| v.parse().ok()))
                    .collect::<Vec<_>>();
                (!indices.is_empty()).then_some((node.id, indices))
            })
            .collect();
        let root_contours: Vec<Id> =
            contours.iter().copied().filter(|cid| self.p.contours.parent_of(*cid).is_none_or(|parent| parent == 0)).collect();
        let contour_for = |index: usize, _entities: &std::collections::HashSet<Id>| root_contours.get(index).copied();
        for node in &mut self.p.timeline {
            let profiles = match &mut node.kind {
                FeatureKind::Extrude { sketch: sid, profiles, .. }
                | FeatureKind::Combine { sketch: sid, profiles, .. }
                | FeatureKind::Revolve { sketch: sid, profiles, .. }
                | FeatureKind::Sweep { sketch: sid, profiles, .. }
                    if *sid == sketch =>
                {
                    profiles
                }
                _ => continue,
            };
            if !explicit_nodes.contains(&node.id) {
                *profiles =
                    contours.iter().copied().filter(|cid| self.p.contours.parent_of(*cid).is_none_or(|parent| parent == 0)).collect();
                continue;
            }
            let stored_indices = stored_indices.get(&node.id);
            if stored_indices.is_some_and(|indices| indices.iter().copied().all(|index| index < profiles.len())) {
                *profiles = root_contours.iter().copied().take(_new_instances.len() / source.len() + 1).collect();
                continue;
            }
            let mut rebound = Vec::with_capacity(profiles.len());
            for (position, old) in profiles.iter().copied().enumerate() {
                let owned_entities = old_picks
                    .iter()
                    .find(|(id, _)| *id == old)
                    .map(|(_, e)| e.clone())
                    .or_else(|| self.p.contours.ents_of(old).map(|e| e.iter().copied().collect::<std::collections::HashSet<_>>()))
                    .unwrap_or_default();
                let index = stored_indices
                    .as_ref()
                    .and_then(|indices| indices.get(position).copied())
                    .or_else(|| old_index(&owned_entities))
                    .ok_or_else(|| {
                        Error::Invalid(format!("pattern edit removed selected profile contour {old}; instance index is unavailable"))
                    })?;
                let source_entities = if index == 0 { source.iter().copied().collect() } else { owned_entities };
                rebound.push(contour_for(index, &source_entities).ok_or_else(|| {
                    Error::Invalid(format!("pattern edit removed selected profile contour {old}; instance index {index} no longer exists"))
                })?);
            }
            *profiles = rebound;
        }
        Ok(())
    }

    pub(crate) fn refresh_sketch_patterns(&mut self, sketch: Id) -> Result<()> {
        let si = self.sketch_si(sketch)?;
        // Counts may read values that changed after the caller's own cap check (reference measurements
        // settling): refuse before any instance is generated.
        self.check_sketch_pattern_values().map_err(|(id, e)| Error::Invalid(format!("sketch pattern {id}: {e}")))?;
        let patterns: Vec<Id> = self.p.sketches[si].patterns.iter().map(|p| p.id).collect();
        for id in patterns {
            let picks = self.pattern_profile_picks(sketch)?;
            let pi =
                self.p.sketches[si].patterns.iter().position(|p| p.id == id).ok_or_else(|| Error::NotFound(format!("pattern {id}")))?;
            let source = self.p.sketches[si].patterns[pi].source.clone();
            let old_instances = self.p.sketches[si].patterns[pi].instances.clone();
            let kind = self.p.sketches[si].patterns[pi].kind;
            let kind = match kind {
                PatternKind::Linear { dx, dy, count, dx2, dy2, count2 } => PatternKind::Linear {
                    dx: self.p.feat_dim(id, "dx").and_then(|v| Num::Expr(v.into()).eval(&self.p.param_map()).ok()).unwrap_or(dx),
                    dy: self.p.feat_dim(id, "dy").and_then(|v| Num::Expr(v.into()).eval(&self.p.param_map()).ok()).unwrap_or(dy),
                    count: self
                        .p
                        .feat_dim(id, "count")
                        .and_then(|v| Num::Expr(v.into()).eval(&self.p.param_map()).ok())
                        .map(|v| v.round() as u32)
                        .unwrap_or(count),
                    dx2: self.p.feat_dim(id, "dx2").and_then(|v| Num::Expr(v.into()).eval(&self.p.param_map()).ok()).unwrap_or(dx2),
                    dy2: self.p.feat_dim(id, "dy2").and_then(|v| Num::Expr(v.into()).eval(&self.p.param_map()).ok()).unwrap_or(dy2),
                    count2: self
                        .p
                        .feat_dim(id, "count2")
                        .and_then(|v| Num::Expr(v.into()).eval(&self.p.param_map()).ok())
                        .map(|v| v.round() as u32)
                        .unwrap_or(count2),
                },
                other => other,
            };
            self.p.update_pattern(si, pi, kind);
            self.finish_sketch_edit(si)?;
            let new_instances = self.p.sketches[si].patterns[pi].instances.clone();
            self.rebind_pattern_profiles(sketch, &source, &old_instances, &new_instances, &picks)?;
        }
        Ok(())
    }

    pub(crate) fn refuse_pattern_instance_edit(&self, sketch: Id, entities: &[Id]) -> Result<()> {
        let si = self.sketch_si(sketch)?;
        if let Some((pattern, entity)) =
            entities.iter().find_map(|id| self.p.sketches[si].patterns.iter().find(|p| p.instances.contains(id)).map(|p| (p.id, *id)))
        {
            return Err(Error::Invalid(format!("entity {entity} is an instance of sketch pattern {pattern}; edit the pattern instead")));
        }
        Ok(())
    }
}

fn native_kind(kind: &SketchPatternKind, vars: &HashMap<String, f64>) -> Result<PatternValues> {
    let mut formulas = HashMap::new();
    let mut evaluated = HashMap::new();
    let num = |key: &str, n: &Num, formulas: &mut HashMap<String, String>, evaluated: &mut HashMap<String, f64>| -> Result<f64> {
        let value = n.eval(vars)?;
        if !value.is_finite() {
            return Err(Error::Invalid(format!("pattern `{key}` is not finite")));
        }
        formulas.insert(key.into(), n.expr().unwrap_or_else(|| value.to_string()));
        evaluated.insert(key.into(), value);
        Ok(value)
    };
    let count = |key: &str, n: &Num, formulas: &mut HashMap<String, String>, evaluated: &mut HashMap<String, f64>| -> Result<u32> {
        let value = num(key, n, formulas, evaluated)?;
        if value < 2.0 || value > 1000.0 || (value - value.round()).abs() > 1e-9 {
            return Err(Error::Invalid(format!("{key} must be an integral count from 2 to 1000, got {value}")));
        }
        Ok(value as u32)
    };
    let native = match kind {
        SketchPatternKind::Linear { count: c, dx, dy, count2, dx2, dy2 } => {
            let count_value = count("count", c, &mut formulas, &mut evaluated)?;
            let dx = num("dx", dx, &mut formulas, &mut evaluated)?;
            let dy = num("dy", dy, &mut formulas, &mut evaluated)?;
            let count2 = match count2 {
                Some(n) => count("count2", n, &mut formulas, &mut evaluated)?,
                None => 1,
            };
            let dx2 = match dx2 {
                Some(n) => num("dx2", n, &mut formulas, &mut evaluated)?,
                None => 0.0,
            };
            let dy2 = match dy2 {
                Some(n) => num("dy2", n, &mut formulas, &mut evaluated)?,
                None => 0.0,
            };
            PatternKind::Linear { dx, dy, count: count_value, dx2, dy2, count2 }
        }
        SketchPatternKind::Circular { count: c, centre, angle } => {
            let count_value = count("count", c, &mut formulas, &mut evaluated)?;
            let cx = centre[0].eval(vars)?;
            let cy = centre[1].eval(vars)?;
            if !cx.is_finite() || !cy.is_finite() {
                return Err(Error::Invalid("circular pattern centre must be finite".into()));
            }
            let angle = num("angle", angle, &mut formulas, &mut evaluated)?;
            if !(angle > 0.0 && angle <= 360.0) {
                return Err(Error::Invalid(format!("angle must be in (0, 360], got {angle}")));
            }
            PatternKind::Circular { cx, cy, count: count_value, total_deg: angle }
        }
    };
    Ok((native, formulas, evaluated))
}

fn check_count(total: u64) -> Result<()> {
    if total > 1000 {
        return Err(Error::Invalid(format!("{total} copies; a sketch pattern makes at most 1000")));
    }
    Ok(())
}
