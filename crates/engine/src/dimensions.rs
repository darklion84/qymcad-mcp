//! Edit and discover native sketch, feature and sketch-item driving dimensions.
use crate::{Error, Id, Num, Rebuild, Result, Session};
use qymcad_core::model::{DimTarget, Project};
use serde::Serialize;
use std::collections::HashSet;

/// A dimension selected by its current sketch constraint index or native feature/item key.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(untagged)]
pub enum DimensionTarget {
    /// A dimensional constraint from `sketch_detail` (zero-based index).
    Sketch {
        /// Sketch id.
        sketch: Id,
        /// Current constraint index; persistence uses the native reference ids.
        constraint: usize,
    },
    /// A native numeric field of a feature or sketch item.
    Feature {
        /// Timeline node id, or the centre/item id for sketch item values.
        feature: Id,
        /// Native key such as `height`, `sides`, `w`, `h` or `turn` (ellipse axes are full sizes).
        key: String,
    },
}

/// A discoverable native named dimension, including its current value and sketch freedom.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DimensionInfo {
    /// Current selector, absent only for a stale or ambiguous imported sketch target.
    pub target: Option<DimensionTarget>,
    /// Stable native references, retained even for a stale imported dimension.
    pub references: Vec<Id>,
    /// Its case-sensitive name, empty for an unnamed dimension.
    pub name: String,
    /// Evaluated numeric value, absent when an imported target is gone.
    pub value: Option<f64>,
    /// Retained expression, absent for a literal sketch dimension.
    pub expression: Option<String>,
    /// Remaining degrees of freedom and redundant constraints of the owning sketch.
    pub dof: Option<(i32, i32)>,
}

/// The edited dimension and the dependent geometry rebuilt atomically with it.
#[derive(Clone, Debug, Serialize)]
pub struct DimensionEdit {
    /// Current dimension state.
    pub dimension: DimensionInfo,
    /// Dependent bodies and diagnostics.
    pub rebuild: Rebuild,
}

impl Session {
    /// Resolve a feature or sketch-item reference for `dimension_set`; sketch items have ids but no names.
    pub fn dimension_feature_resolve(&self, reference: &str) -> Result<Id> {
        if let Ok(id) = reference.trim().parse::<Id>() {
            if self
                .p
                .sketches
                .iter()
                .any(|s| s.points.iter().any(|p| p.id == id) || s.patterns.iter().any(|p| p.id == id) || s.texts.iter().any(|t| t.id == id))
            {
                return Ok(id);
            }
        }
        self.resolve(reference)
    }

    /// Expose every named dimension in the document, including imported stale targets.
    pub fn named_dimensions(&self) -> Vec<DimensionInfo> {
        self.p.named_dims.iter().map(|n| self.dimension_info(&n.target)).collect()
    }

    /// Set or name a dimension using the app's SetDriver write, target solve and parameter-edit path.
    /// Errors restore the project and original live geometry; an empty name explicitly removes its name.
    pub fn dimension_set(&mut self, target: &DimensionTarget, value: Option<&Num>, name: Option<&str>) -> Result<DimensionEdit> {
        if value.is_none() && name.is_none() {
            return Err(Error::Invalid("dimension_set needs value or name (an empty name clears the name)".into()));
        }
        let native = self.native_dimension_target(target)?;
        let before = self.p.clone();
        let before_scope = before.param_map();
        let had_unsolved: HashSet<Id> = (0..before.sketches.len())
            .filter(|&si| before.sketch_residuals(si).iter().any(|r| !r.is_finite() || *r > 1e-6))
            .map(|si| before.sketches[si].id)
            .collect();
        // Validate selectors and native reference ambiguity before any mutation. All following edits are reversible.
        self.transaction(|s| {
            if let Some(name) = name {
                let name = name.trim();
                if !name.is_empty() {
                    if matches!(&native, DimTarget::Feature { node, .. } if !s.p.timeline.iter().any(|n| n.id == *node))
                        && s.p.name_of_target(&native) != name
                    {
                        return Err(Error::Invalid(
                            "naming a sketch-item dimension is unsupported: QymCAD 0.1.1 SetDriver does not dirty its owning sketch; name a sketch constraint dimension instead".into(),
                        ));
                    }
                    qymcad_core::drivers::check_ident(name)
                        .map_err(|e| Error::Invalid(format!("invalid dimension name `{name}`: {e:?}")))?;
                    for existing in
                        s.p.parameters.iter().map(|p| (&p.name, None)).chain(s.p.named_dims.iter().map(|n| (&n.name, Some(&n.target))))
                    {
                        if existing.1 == Some(&native) {
                            continue;
                        }
                        if crate::value::same_ignoring_case(existing.0, name) {
                            return Err(Error::Invalid(format!(
                                "dimension name `{name}` collides with existing {} `{}` (names differing only by letter case are refused)",
                                if existing.1.is_some() { "named dimension" } else { "parameter" },
                                existing.0
                            )));
                        }
                    }
                    if let Some(chain) = s.p.naming_closes_circle(&native, name) {
                        return Err(Error::Invalid(format!("dimension name would create a formula cycle: {chain}")));
                    }
                }
                let named = s.p.name_dim(name.to_string(), native.clone());
                if !name.is_empty() && !named {
                    return Err(Error::Invalid(format!("cannot name dimension `{name}`: invalid name, collision or formula cycle")));
                }
            }
            if let Some(value) = value {
                let text = match value {
                    Num::Value(n) => n.to_string(),
                    Num::Expr(e) => e.trim().to_string(),
                };
                crate::value::check_expr(&text)?;
                // Num validation also rejects NaN/Inf strings before the native numeric fast path.
                let evaluated = value.eval(&s.p.param_map())?;
                // Native item evaluation rounds and clamps polygon counts; preserve the server's strict policy
                // before imposing a value that could also allocate arbitrarily many vertices.
                if matches!(&native, DimTarget::Feature { key, .. } if key == "sides")
                    && (evaluated.fract() != 0.0 || !(3.0..=64.0).contains(&evaluated))
                {
                    return Err(Error::Invalid(format!("a polygon needs an integral side count in 3..=64, got {evaluated}")));
                }
                s.p.check_dim_target_text(&native, &text).map_err(|e| Error::Expr(crate::value::explain(&e, s.p.param_map().keys())))?;
                if !s.p.set_dim_target_text(&native, &text).map_err(|e| Error::Expr(crate::value::explain(&e, s.p.param_map().keys())))? {
                    return Err(Error::NotFound("dimension target no longer exists".into()));
                }
            }
            // A renamed/edited driver can feed the count of a different polygon, too.
            s.check_polygon_sides()?;
            // Act::SetDriver explicitly solves and dirties its target, even for a literal with no names.
            if let DimTarget::Sketch { sketch, .. } = &native {
                let si = s.p.sketch_index(*sketch).ok_or_else(|| Error::NotFound(format!("sketch {sketch}")))?;
                let residual = s.solve_settled(si);
                if !residual.is_finite() || residual > 1e-6 {
                    return Err(Error::Invalid(format!(
                        "dimension target sketch {sketch} does not solve (residual {residual:.3e}); edit rolled back"
                    )));
                }
                s.p.mark_sketch_dirty(*sketch);
            }
            // apply_param_edit: item-value formulas count as named sketch inputs, too.
            if let Some((what, err)) = s.p.eval_parameters().into_iter().next() {
                return Err(Error::Expr(format!("`{what}`: {}", crate::value::explain(&err, s.p.param_map().keys()))));
            }
            for si in 0..s.p.sketches.len() {
                if s.p.sketch_reads_names(si) {
                    let residual = s.solve_settled(si);
                    if (!residual.is_finite() || residual > 1e-6) && !had_unsolved.contains(&s.p.sketches[si].id) {
                        return Err(Error::Invalid(format!(
                            "sketch {} does not solve (residual {residual:.3e}); dimension edit rolled back",
                            s.p.sketches[si].id
                        )));
                    }
                }
            }
            let unsolved = s.propagate_params(&before_scope)?;
            if unsolved.iter().any(|(id, _)| !had_unsolved.contains(id)) {
                return Err(Error::Invalid("dimension propagation left a sketch unsolved; edit rolled back".into()));
            }
            // Propagation evaluates item formulas again even if the named scope is unchanged. As in the
            // app, solve after that imposition; otherwise an item edit could leave old driving rows unsatisfied.
            for si in 0..s.p.sketches.len() {
                if s.p.sketch_reads_names(si) {
                    let residual = s.solve_settled(si);
                    if (!residual.is_finite() || residual > 1e-6) && !had_unsolved.contains(&s.p.sketches[si].id) {
                        return Err(Error::Invalid(format!(
                            "sketch {} does not solve after dimension propagation; edit rolled back", s.p.sketches[si].id
                        )));
                    }
                }
            }
            s.check_array_limits()?;
            if let DimTarget::Feature { node, key } = &native {
                let item = !s.p.timeline.iter().any(|n| n.id == *node);
                if item && value.is_some() {
                    if let Some((actual, expected)) = s.p.kept_number(*node, key).zip(s.p.dim_target_value(&native)) {
                        let difference = if key == "turn" {
                            (actual - expected + 180.0).rem_euclid(360.0) - 180.0
                        } else {
                            actual - expected
                        };
                        if difference.abs() > 1e-7 * expected.abs().max(1.0) {
                            return Err(Error::Invalid(format!(
                                "item `{key}` cannot take {expected}: its driving constraints retain {actual}; edit the sketch dimension instead (edit rolled back)"
                            )));
                        }
                    }
                    let owner = s.p.sketches.iter().find(|sk| sk.points.iter().any(|p| p.id == *node)
                        || sk.patterns.iter().any(|p| p.id == *node) || sk.texts.iter().any(|t| t.id == *node)).map(|sk| sk.id);
                    if let Some(sketch) = owner {
                        s.p.mark_sketch_dirty(sketch);
                    }
                }
            }
            Ok(())
        })?;
        let mut nodes: HashSet<Id> = self.p.regen_plan().nodes.into_iter().collect();
        for sketch in &self.p.sketches {
            if nodes.contains(&sketch.id) {
                nodes.extend(self.p.dependents(sketch.id));
            }
        }
        let had: HashSet<Id> = before.regen_errors.keys().copied().collect();
        self.with_rebuild_copies(before, &nodes, "dimension edit", |s| {
            let rebuild = s.rebuild_retrying(Some(&nodes));
            let broken = rebuild
                .errors
                .iter()
                .filter(|e| !had.contains(&e.node))
                .map(|e| format!("{} ({}): {}", e.name, e.node, e.message))
                .collect::<Vec<_>>();
            if !broken.is_empty() {
                return Err(Error::Rebuild(broken));
            }
            Ok(DimensionEdit { dimension: s.dimension_info(&native), rebuild })
        })
    }

    fn native_dimension_target(&self, target: &DimensionTarget) -> Result<DimTarget> {
        match target {
            DimensionTarget::Sketch { sketch, constraint } => {
                let si = self.p.sketch_index(*sketch).ok_or_else(|| Error::NotFound(format!("sketch {sketch}")))?;
                let c = self.p.sketches[si]
                    .constraints
                    .get(*constraint)
                    .ok_or_else(|| Error::NotFound(format!("constraint {constraint} in sketch {sketch}")))?;
                if c.is_driven() {
                    return Err(Error::Invalid("a reference measurement is not a driving dimension".into()));
                }
                let refs = Project::dim_refs(c)
                    .ok_or_else(|| Error::Invalid(format!("constraint {constraint} is not a dimensional constraint")))?;
                let refs = Project::dim_key_pub(&refs);
                let target = DimTarget::Sketch { sketch: *sketch, refs };
                if self.dimension_selector(&target).is_none() {
                    return Err(Error::Invalid(format!("constraint {constraint} in sketch {sketch} has ambiguous native dimension references; another dimension uses the same entities")));
                }
                Ok(target)
            }
            DimensionTarget::Feature { feature, key } => {
                if self.p.kept_number(*feature, key).is_none() {
                    return Err(Error::NotFound(format!("no numeric dimension `{key}` on feature or sketch item {feature}")));
                }
                Ok(DimTarget::Feature { node: *feature, key: key.clone() })
            }
        }
    }

    fn dimension_selector(&self, target: &DimTarget) -> Option<DimensionTarget> {
        match target {
            DimTarget::Feature { node, key } => Some(DimensionTarget::Feature { feature: *node, key: key.clone() }),
            DimTarget::Sketch { sketch, refs } => {
                let si = self.p.sketch_index(*sketch)?;
                let want: HashSet<_> = refs.iter().copied().collect();
                let hits = self.p.sketches[si]
                    .constraints
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| Project::dim_refs(c).filter(|r| r.iter().copied().collect::<HashSet<_>>() == want).map(|_| i))
                    .collect::<Vec<_>>();
                match hits.as_slice() {
                    [constraint] => Some(DimensionTarget::Sketch { sketch: *sketch, constraint: *constraint }),
                    _ => None,
                }
            }
        }
    }

    fn dimension_info(&self, target: &DimTarget) -> DimensionInfo {
        let owner = match target {
            DimTarget::Sketch { sketch, .. } => self.p.sketch_index(*sketch),
            DimTarget::Feature { node, .. } => self.p.sketches.iter().position(|s| {
                s.points.iter().any(|p| p.id == *node) || s.patterns.iter().any(|p| p.id == *node) || s.texts.iter().any(|p| p.id == *node)
            }),
        };
        DimensionInfo {
            target: self.dimension_selector(target),
            references: match target {
                DimTarget::Sketch { refs, .. } => refs.clone(),
                _ => Vec::new(),
            },
            name: self.p.name_of_target(target),
            value: self.p.dim_target_value(target),
            expression: self.p.dim_target_expr(target),
            dof: owner.map(|si| self.p.sketch_dof(si)),
        }
    }
}
