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
                    s.rebind_pattern_profiles(sketch, &source, &profile_picks)?;
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

    pub(crate) fn rebind_pattern_profiles(
        &mut self,
        sketch: Id,
        _source: &[Id],
        old_picks: &[(Id, std::collections::HashSet<Id>)],
    ) -> Result<()> {
        let si = self.sketch_si(sketch)?;
        let contours = self.p.sketches[si].contour_ids.clone();
        let mut by_entities = Vec::new();
        for &cid in &contours {
            if let Some(entities) = self.p.contours.ents_of(cid) {
                by_entities.push((entities.iter().copied().collect::<std::collections::HashSet<_>>(), cid));
            }
        }
        let explicit_nodes: std::collections::HashSet<Id> =
            self.p.timeline.iter().filter(|node| self.p.feat_dim(node.id, "__profile_pick").is_some()).map(|node| node.id).collect();
        for node in &mut self.p.timeline {
            let replace = match &mut node.kind {
                FeatureKind::Extrude { sketch: sid, profiles, .. } | FeatureKind::Combine { sketch: sid, profiles, .. }
                    if *sid == sketch =>
                {
                    Some(profiles)
                }
                _ => None,
            };
            let Some(profiles) = replace else { continue };
            let explicit = explicit_nodes.contains(&node.id);
            if !explicit {
                *profiles =
                    contours.iter().copied().filter(|cid| self.p.contours.parent_of(*cid).is_none_or(|parent| parent == 0)).collect();
            } else {
                let mut rebound = Vec::with_capacity(profiles.len());
                for old in profiles.iter().copied() {
                    let entities = old_picks
                        .iter()
                        .find(|(id, _)| *id == old)
                        .map(|(_, entities)| entities)
                        .ok_or_else(|| Error::Invalid(format!("pattern edit removed selected profile contour {old}")))?;
                    if let Some((_, new)) = by_entities.iter().find(|(candidate, _)| candidate == entities) {
                        rebound.push(*new);
                    } else {
                        return Err(Error::Invalid(format!("pattern edit removed selected profile contour {old}")));
                    }
                }
                *profiles = rebound;
            }
        }
        Ok(())
    }

    pub(crate) fn refresh_sketch_patterns(&mut self, sketch: Id) -> Result<()> {
        let si = self.sketch_si(sketch)?;
        let patterns: Vec<(Id, PatternKind)> = self.p.sketches[si].patterns.iter().map(|p| (p.id, p.kind)).collect();
        for (id, kind) in patterns {
            let picks = self.pattern_profile_picks(sketch)?;
            let pi =
                self.p.sketches[si].patterns.iter().position(|p| p.id == id).ok_or_else(|| Error::NotFound(format!("pattern {id}")))?;
            let source = self.p.sketches[si].patterns[pi].source.clone();
            self.p.update_pattern(si, pi, kind);
            self.rebind_pattern_profiles(sketch, &source, &picks)?;
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
