//! Native rigid body Move: formula translations and literal rotations in the source part frame.
use crate::{Axis, Error, Id, Num, Rebuild, Result, Session};
use qymcad_core::{
    feature::{rot12_axis, FeatureKind, PLACE_IDENTITY},
    model::{FeatNumber, PlainNumber},
};
use serde::Serialize;

/// One literal, right-handed rotation, like a single app gizmo step.
#[derive(Clone, Debug, PartialEq)]
pub struct Rotation {
    /// Degrees; formulas are not supported by the native recipe.
    pub angle: f64,
    /// Axis in the source part's local frame.
    pub axis: Axis,
    /// Fixed local pivot; defaults to the current display mesh's bounding-box centre.
    pub pivot: Option<[f64; 3]>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BaseName, PlaneRef};

    #[test]
    fn reused_move_native_error_restores_recipe_and_exact_breps() {
        let mut s = Session::new_part();
        let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &10.0.into(), &20.0.into(), false).unwrap();
        s.extrude(&serde_json::from_value(serde_json::json!({"sketch":sketch,"height":30})).unwrap()).unwrap();
        let options = Move { body: None, translation: Some([1.0.into(), 0.0.into(), 0.0.into()]), rotation: None, name: None };
        let (moved, _) = s.move_body(&options).unwrap();
        // A clean cached legacy recipe can carry a bad reference. Reusing its old id must still
        // check native errors, rather than trusting atomic's check of newly allocated node ids.
        let node = s.p.timeline.iter_mut().find(|n| n.id == moved.body).unwrap();
        let FeatureKind::Move { src, .. } = &mut node.kind else { unreachable!() };
        *src = u64::MAX;
        let before = serde_json::to_value(&s.p).unwrap();
        let breps = |s: &Session| {
            let _gate = qymcad_kernel::kernel_gate();
            s.shapes.iter().map(|(id, shape)| (*id, shape.to_brep_bytes().unwrap())).collect::<std::collections::BTreeMap<_, _>>()
        };
        let before_breps = breps(&s);
        assert!(s.move_body(&options).is_err(), "native errors on a reused Move must refuse the edit");
        assert_eq!(serde_json::to_value(&s.p).unwrap(), before);
        assert_eq!(breps(&s), before_breps);
    }
}

/// Move the current body, reusing its native Move node when possible.
#[derive(Clone, Debug, PartialEq)]
pub struct Move {
    /// Source body, defaulting to the active part's current body.
    pub body: Option<Id>,
    /// Incremental X/Y/Z translations, exclusive with rotation.
    pub translation: Option<[Num; 3]>,
    /// Literal rotation, exclusive with translation.
    pub rotation: Option<Rotation>,
    /// Native feature name, also applied when the node is reused.
    pub name: Option<String>,
}

/// Identity and evaluated total affine transform of the resulting native Move.
#[derive(Clone, Debug, Serialize)]
pub struct MoveResult {
    /// Output body and native feature id.
    pub body: Id,
    /// Original native source body (also retained on node reuse).
    pub source_body: Id,
    /// Whether native accumulation reused the existing node.
    pub reused: bool,
    /// Total row-major 3x4 affine transform; dx/dy/dz are slots 3/7/11.
    pub transform: [f64; 12],
}

const KEYS: [&str; 3] = ["dx", "dy", "dz"];
const SLOTS: [usize; 3] = [3, 7, 11];

/// Numeric strings are literals too; retain round-trippable decimal precision.
fn text(input: &Num, value: f64) -> String {
    input.expr().filter(|e| e.parse::<f64>().is_err()).unwrap_or_else(|| value.to_string())
}

impl Session {
    /// Apply the app's native add/accumulate path. No kernel gate may be held by the caller.
    pub fn move_body(&mut self, options: &Move) -> Result<(MoveResult, Rebuild)> {
        if options.translation.is_some() == options.rotation.is_some() {
            return Err(Error::Invalid("move needs translation OR rotation, mutually exclusive".into()));
        }
        self.ensure_topology()?;
        // As with atomic(), establish a clean baseline before taking rollback copies.
        if self.p.timeline.iter().any(|n| n.dirty) {
            let baseline = self.rebuild();
            if !baseline.errors.is_empty() {
                return Err(Error::Rebuild(
                    baseline.errors.iter().map(|i| format!("baseline: {} ({}): {}", i.name, i.node, i.message)).collect(),
                ));
            }
        }
        let src = self.source_body(options.body)?;
        let reused = self.p.timeline.iter().any(|n| n.id == src && matches!(n.kind, FeatureKind::Move { .. }));
        let before = self.p.clone();
        let before_scope = before.param_map();
        // Recipe changes and name propagation do not touch live shapes. Plan AFTER propagation:
        // an independent sketch or solid may read a named Move shift without consuming this body.
        let mut result = self.transaction(|s| {
            let result = s.apply_move(src, reused, options)?;
            if s.p.param_map() != before_scope {
                let unsolved = s.propagate_params(&before_scope)?;
                if !unsolved.is_empty() {
                    return Err(Error::Invalid("Move dimension propagation left a sketch unsolved; edit rolled back".into()));
                }
                s.check_array_limits()?;
            }
            Ok(result)
        })?;
        let nodes = crate::dependencies::rebuild_nodes(&self.p);
        self.with_rebuild_copies(before, &nodes, "Move", |s| {
            let report = s.rebuild_for_edit(Some(&nodes))?;
            // Accumulation edits an old node; checking only new node ids would miss its errors.
            if !report.errors.is_empty() {
                return Err(Error::Rebuild(report.errors.iter().map(|i| format!("{} ({}): {}", i.name, i.node, i.message)).collect()));
            }
            result.transform = s.move_transform(result.body)?;
            Ok((result, report))
        })
    }

    fn apply_move(&mut self, src: Id, reused: bool, options: &Move) -> Result<MoveResult> {
        let vars = self.p.param_map();
        let mut old = if reused {
            match self.p.timeline.iter().find(|n| n.id == src).map(|n| &n.kind) {
                Some(FeatureKind::Move { mat, .. }) => *mat,
                _ => unreachable!("checked Move node"),
            }
        } else {
            PLACE_IDENTITY
        };
        let previous: [String; 3] = std::array::from_fn(|i| {
            self.p
                .feat_dims
                .get(&src)
                .and_then(|d| d.get(KEYS[i]))
                .filter(|e| !e.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| old[SLOTS[i]].to_string())
        });
        let mut extra = PLACE_IDENTITY;
        let mut writes: [Option<String>; 3] = [None, None, None];
        if let Some(translation) = &options.translation {
            let mut any = false;
            for i in 0..3 {
                let delta = translation[i].eval(&vars)?;
                let delta_text = text(&translation[i], delta);
                let formula = delta_text.parse::<f64>().is_err();
                any |= delta != 0.0 || formula;
                extra[SLOTS[i]] = delta;
                if delta != 0.0 || formula {
                    let expression = if reused {
                        if let (Ok(a), Ok(b)) = (previous[i].parse::<f64>(), delta_text.parse::<f64>()) {
                            (a + b).to_string()
                        } else {
                            format!("({}) + ({delta_text})", previous[i])
                        }
                    } else {
                        delta_text
                    };
                    // Bound the COMPOSED expression too, before the native recursive parser.
                    Num::Expr(expression.clone()).eval(&vars)?;
                    writes[i] = Some(expression);
                }
            }
            if !any {
                return Err(Error::Invalid("move translation must not be all zero".into()));
            }
        } else if let Some(rotation) = &options.rotation {
            if !rotation.angle.is_finite() || rotation.angle == 0.0 {
                return Err(Error::Invalid("move rotation angle must be finite and nonzero".into()));
            }
            if reused && previous.iter().any(|e| e.parse::<f64>().is_err()) {
                return Err(Error::Invalid("rotation accumulation on a formula-driven Move is unsupported: native accumulate_move adds numeric deltas to stored translation formulas instead of rotating their variable coefficients".into()));
            }
            let pivot = match rotation.pivot {
                Some(p) => p,
                None => {
                    let b = self
                        .p
                        .bodies
                        .iter()
                        .find(|b| b.id == src)
                        .and_then(|b| b.mesh.bounds())
                        .ok_or_else(|| Error::Invalid("Move source has no display mesh bounds for the rotation pivot".into()))?;
                    [(b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0, (b.min.z + b.max.z) / 2.0]
                }
            };
            if pivot.iter().any(|n| !n.is_finite()) {
                return Err(Error::Invalid("move rotation pivot must be finite".into()));
            }
            let dir = match rotation.axis {
                Axis::X => [1.0, 0.0, 0.0],
                Axis::Y => [0.0, 1.0, 0.0],
                Axis::Z => [0.0, 0.0, 1.0],
            };
            extra = rot12_axis(pivot, dir, rotation.angle);
            if reused {
                // prep_move evaluates offsets into a temporary matrix. Numeric dimension edits may
                // therefore be newer than the stored matrix even though no variable formula exists.
                for i in 0..3 {
                    old[SLOTS[i]] = Num::Expr(previous[i].clone()).eval(&vars)?;
                }
                if let Some(n) = self.p.timeline.iter_mut().find(|n| n.id == src) {
                    if let FeatureKind::Move { mat, .. } = &mut n.kind {
                        *mat = old;
                    }
                }
            }
        }
        if extra.iter().any(|n| !n.is_finite()) {
            return Err(Error::Invalid("move transform must be finite".into()));
        }
        let id = if reused {
            self.p.accumulate_move(src, &extra);
            src
        } else {
            self.p.add_move(src, extra)
        };
        if reused && options.rotation.is_some() {
            let FeatureKind::Move { mat, .. } = &self.p.timeline.iter().find(|n| n.id == id).unwrap().kind else { unreachable!() };
            for i in 0..3 {
                writes[i] = Some(mat[SLOTS[i]].to_string());
            }
        }
        for (key, expression) in KEYS.into_iter().zip(writes) {
            if let Some(expression) = expression {
                // Replace accumulate_move's rounded deltas from the ORIGINAL expressions (F-117).
                self.p
                    .write_feat_formula(&FeatNumber { node: id, key }, &expression, PlainNumber::Stands)
                    .map_err(|e| Error::Expr(format!("Move {key}: {e:?}")))?;
            }
        }
        self.p.mark_node_dirty(id);
        self.set_node_name(id, options.name.as_deref())?;
        let transform = self.move_transform(id)?;
        let FeatureKind::Move { src: source_body, .. } = self.p.timeline.iter().find(|n| n.id == id).unwrap().kind else { unreachable!() };
        Ok(MoveResult { body: id, source_body, reused, transform })
    }

    fn move_transform(&self, id: Id) -> Result<[f64; 12]> {
        let FeatureKind::Move { mat, .. } = &self.p.timeline.iter().find(|n| n.id == id).unwrap().kind else { unreachable!() };
        let mut transform = *mat;
        let vars = self.p.param_map();
        for i in 0..3 {
            if let Some(e) = self.p.feat_dims.get(&id).and_then(|d| d.get(KEYS[i])).filter(|e| !e.trim().is_empty()) {
                transform[SLOTS[i]] = Num::Expr(e.clone()).eval(&vars)?;
            }
        }
        if transform.iter().any(|n| !n.is_finite()) {
            return Err(Error::Invalid("move accumulated transform must be finite".into()));
        }
        Ok(transform)
    }
}
