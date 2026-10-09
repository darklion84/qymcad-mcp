//! Document parameters (`name = expression`), the global drivers of a parametric part.

use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::value::Num;
use qymcad_core::model::{Id, Param};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ParamInfo {
    pub name: String,
    pub expr: String,
    pub value: f64,
}

impl Session {
    pub fn params(&self) -> Vec<ParamInfo> {
        self.p.parameters.iter().map(|q| ParamInfo { name: q.name.clone(), expr: q.expr.clone(), value: q.value }).collect()
    }

    /// Create or change a parameter and rebuild everything that depends on it. Names are stored lowercase
    /// (FINDINGS F-001). The change is rolled back if the model no longer rebuilds.
    pub fn param_set(&mut self, name: &str, value: &Num) -> Result<Rebuild> {
        let name = normalize_name(name)?;
        // QymCAD resolves these constants before parameters, so a parameter with such a name is never read
        // (FINDINGS F-039). Only creation/assignment is refused: an old document may hold one, and it must stay
        // deletable.
        if ["pi", "tau", "e"].contains(&name.as_str()) {
            return Err(Error::Invalid(format!("`{name}` is a built-in constant in expressions; choose another parameter name")));
        }
        let expr = match value {
            Num::Value(v) if !v.is_finite() => return Err(Error::Expr(format!("number {v} is not finite"))),
            Num::Value(v) => format!("{v}"),
            Num::Expr(_) => value.expr().unwrap_or_default(),
        };
        // QymCAD evaluates it (eval_parameters) with a recursive parser that has no depth cap.
        crate::value::check_expr(&expr)?;
        let before = self.p.clone();
        let broken_before = self.unsolved_sketches();
        match self.p.parameters.iter_mut().find(|q| q.name.eq_ignore_ascii_case(&name)) {
            Some(q) => {
                q.name = name.clone();
                q.expr = expr.clone();
            }
            None => self.p.parameters.push(Param { name: name.clone(), expr: expr.clone(), value: 0.0 }),
        }
        if let Some((what, err)) = self.p.eval_parameters().into_iter().next() {
            self.p = before;
            return Err(Error::Expr(format!("`{what}`: {err:?}")));
        }
        // Array counts may follow parameters; refuse an edit that would make an array too large (F-033).
        if let Err(e) = self.check_array_limits() {
            self.p = before;
            return Err(e);
        }
        let unsolved: Vec<String> = self
            .propagate_params(&before.param_map())
            .into_iter()
            .filter(|(sid, _)| !broken_before.contains(sid))
            .map(|(sid, res)| format!("sketch {sid} `{}` (residual {res:.3e})", self.node_name(sid)))
            .collect();
        if !unsolved.is_empty() {
            self.p = before;
            return Err(Error::Invalid(format!(
                "with `{name}` = {expr} {} does not solve, so the change was rolled back. A dimension cannot take this value \
                 (e.g. an angle outside 0..180, or a coordinate/offset expression that would change sign: QymCAD dimensions \
                 keep the side they were created on, FINDINGS F-045).",
                unsolved.join(", ")
            )));
        }
        // Propagation has dirtied every affected node. Preserve their original handles and rebuild on
        // independent B-rep copies, like sketch_edit: rollback must retain the original representation (F-034).
        let mut nodes: HashSet<Id> = self.p.regen_plan().nodes.into_iter().collect();
        for sketch in &self.p.sketches {
            if nodes.contains(&sketch.id) {
                nodes.extend(self.p.dependents(sketch.id));
            }
        }
        self.with_rebuild_copies(before, &nodes, "parameter rebuild", |s| {
            let r = s.rebuild_retrying(Some(&nodes));
            if !r.errors.is_empty() {
                let lines = r.errors.iter().map(|i| format!("{} ({}): {}", i.name, i.node, i.message)).collect();
                return Err(Error::Rebuild(lines));
            }
            Ok(r)
        })
    }

    /// Delete a parameter. Refused while an expression still uses it.
    pub fn param_delete(&mut self, name: &str) -> Result<()> {
        let name = normalize_name(name)?;
        let idx = self
            .p
            .parameters
            .iter()
            .position(|q| q.name.eq_ignore_ascii_case(&name))
            .ok_or_else(|| Error::NotFound(format!("parameter `{name}`")))?;
        let users = self.param_users(&name);
        if !users.is_empty() {
            return Err(Error::Invalid(format!("parameter `{name}` is used by {}; change those first", users.join(", "))));
        }
        self.p.parameters.remove(idx);
        Ok(())
    }

    /// Where a parameter is used: other parameters, feature dimensions and sketch dimensions.
    pub fn param_users(&self, name: &str) -> Vec<String> {
        use qymcad_core::expr::mentions;
        let mut out = Vec::new();
        for q in &self.p.parameters {
            if !q.name.eq_ignore_ascii_case(name) && mentions(&q.expr.to_lowercase(), name) {
                out.push(format!("parameter `{}`", q.name));
            }
        }
        for (node, dims) in &self.p.feat_dims {
            let is_sketch = self.p.sketch_index(*node).is_some();
            if is_sketch && dims.iter().any(|(k, e)| k.starts_with("datum_dist_") && mentions(&e.to_lowercase(), name)) {
                out.push(format!("datum dependency of sketch {} `{}`", node, self.node_name(*node)));
            }
            if dims.iter().any(|(k, e)| (!is_sketch || !k.starts_with("datum_dist_")) && mentions(&e.to_lowercase(), name)) {
                out.push(format!("feature {} `{}`", node, self.node_name(*node)));
            }
        }
        for s in &self.p.sketches {
            if s.constraints.iter().any(|c| c.expr().is_some_and(|e| mentions(&e.to_lowercase(), name))) {
                out.push(format!("sketch {} `{}`", s.id, crate::localization::name(&s.name)));
            }
        }
        out
    }

    /// Evaluate first (F-002), then solve/dirty only expressions reached by changed values, including
    /// transitive parameters and named dimensions in param_map. Match lowercase expressions for imported
    /// documents too: native per-name dirty matching is case-sensitive (F-001).
    /// Returns the reached sketches left unsolved, with their residuals.
    pub(crate) fn propagate_params(&mut self, before: &std::collections::HashMap<String, f64>) -> Vec<(Id, f64)> {
        // Native evaluation fixed-points global parameters, but seeds named dimensions only once before
        // applying sketch expressions. Settle those drivers too before deciding which sketches to solve:
        // a chain w -> named width a -> named width b must not hide later dependents from the snapshot.
        // At most one extra evaluation per named driver; unchanged scopes stop early, and cyclic imported
        // formulas retain bounded evaluation rather than introducing an unbounded fixed-point loop.
        let mut previous_values = self.p.param_map();
        for _ in 0..=self.p.named_dims.len() {
            self.p.eval_parameters();
            let values = self.p.param_map();
            if values == previous_values {
                break;
            }
            previous_values = values;
        }
        let changed: Vec<String> =
            self.p.param_map().into_iter().filter(|(name, value)| before.get(name) != Some(value)).map(|(name, _)| name).collect();
        let reached = |expr: &str| {
            let expr = expr.to_lowercase();
            changed.iter().any(|name| qymcad_core::expr::mentions(&expr, name))
        };
        let mut unsolved = Vec::new();
        for si in 0..self.p.sketches.len() {
            if self.p.sketches[si].constraints.iter().any(|c| c.expr().is_some_and(reached)) {
                #[cfg(test)]
                PARAM_SOLVES.with(|solves| solves.borrow_mut().push(self.p.sketches[si].id));
                let residual = self.solve_settled(si);
                let sid = self.p.sketches[si].id;
                self.p.mark_sketch_dirty(sid);
                if residual.is_nan() || residual > 1e-6 {
                    unsolved.push((sid, residual));
                }
            }
        }
        let nodes: Vec<Id> =
            self.p.feat_dims.iter().filter(|(_, dims)| dims.values().any(|expr| reached(expr))).map(|(id, _)| *id).collect();
        for node in nodes {
            self.p.mark_node_dirty(node);
        }
        unsolved
    }

    /// Sketches whose constraints do not hold as they stand (a document that came in broken): a parameter edit
    /// is not blamed for them.
    fn unsolved_sketches(&self) -> Vec<Id> {
        (0..self.p.sketches.len())
            .filter(|&si| self.p.sketch_residuals(si).iter().any(|r| r.is_nan() || *r > 1e-6))
            .map(|si| self.p.sketches[si].id)
            .collect()
    }

    /// Parameter names that break GUI rebuilds (uppercase, FINDINGS F-001) — reported as warnings for files
    /// created elsewhere.
    pub fn uppercase_params(&self) -> Vec<String> {
        self.p.parameters.iter().filter(|q| q.name.chars().any(|c| c.is_uppercase())).map(|q| q.name.clone()).collect()
    }
}

fn normalize_name(name: &str) -> Result<String> {
    let n = name.trim().to_lowercase();
    qymcad_core::drivers::check_ident(&n).map_err(|e| {
        Error::Invalid(format!("`{name}` is not a valid parameter name ({e:?}); use letters, digits and _ starting with a letter"))
    })?;
    Ok(n)
}

#[cfg(test)]
thread_local! {
    static PARAM_SOLVES: std::cell::RefCell<Vec<Id>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BaseName, Direction, Extrude, Op, PlaneRef};

    fn two_sketches() -> (Session, Id, Id) {
        let mut s = Session::new_part();
        for (name, value) in [("w", Num::Value(10.0)), ("other", Num::Value(8.0)), ("twice", Num::Expr("w*2".into()))] {
            s.param_set(name, &value).unwrap();
        }
        let mut sketches = Vec::new();
        for expr in ["twice", "other"] {
            let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
            s.sketch_rect(sk, &0.0.into(), &0.0.into(), &Num::Expr(expr.into()), &3.0.into(), false).unwrap();
            s.extrude(&Extrude {
                sketch: sk,
                profiles: None,
                height: 2.0.into(),
                op: Op::NewBody,
                direction: Direction::Normal,
                through: false,
                target: None,
                name: None,
            })
            .unwrap();
            sketches.push(sk);
        }
        PARAM_SOLVES.with(|solves| solves.borrow_mut().clear());
        (s, sketches[0], sketches[1])
    }

    #[test]
    fn parameter_edit_solves_only_reached_sketch_including_transitive_uppercase_expressions() {
        let (mut s, reached, untouched) = two_sketches();
        // Existing files may carry uppercase expressions; matching must respect F-001.
        for c in &mut s.p.sketches.iter_mut().find(|sk| sk.id == reached).unwrap().constraints {
            if let qymcad_core::model::Constraint::Distance { expr, .. } = c {
                *expr = expr.to_uppercase();
            }
        }
        let untouched_before = format!("{:?}", s.p.sketches.iter().find(|sk| sk.id == untouched).unwrap());
        let r = s.param_set("W", &12.0.into()).unwrap();
        PARAM_SOLVES.with(|solves| assert_eq!(*solves.borrow(), vec![reached]));
        assert_eq!(format!("{:?}", s.p.sketches.iter().find(|sk| sk.id == untouched).unwrap()), untouched_before);
        // First width follows twice=2*w; second width follows other=8. Both have height 3 and depth 2.
        for (body, expected) in r.bodies.iter().zip([2.0 * 12.0 * 3.0 * 2.0, 8.0 * 3.0 * 2.0]) {
            assert!((body.volume - expected).abs() < 1e-6, "{} != {expected}", body.volume);
        }
    }

    #[test]
    fn parameter_edit_reaches_a_chain_of_imported_named_sketch_dimensions() {
        let mut s = Session::new_part();
        s.param_set("w", &10.0.into()).unwrap();
        let mut sketches = Vec::new();
        for (expr, driver) in [("w", Some("a")), ("a", Some("b")), ("b", Some("c")), ("c", None)] {
            let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
            s.sketch_rect(sk, &0.0.into(), &0.0.into(), &Num::Expr(expr.into()), &3.0.into(), false).unwrap();
            if let Some(driver) = driver {
                // Reproduce a GUI-authored document: expose each width as a named driving dimension.
                let si = s.p.sketch_index(sk).unwrap();
                let c = s.p.sketches[si].constraints.iter().find(|c| c.expr() == Some(expr)).unwrap();
                let refs = qymcad_core::model::Project::dim_refs(c).unwrap();
                assert!(s.p.add_named_dim(driver.into(), sk, refs));
            }
            s.extrude(&Extrude {
                sketch: sk,
                profiles: None,
                height: 2.0.into(),
                op: Op::NewBody,
                direction: Direction::Normal,
                through: false,
                target: None,
                name: None,
            })
            .unwrap();
            sketches.push(sk);
        }
        PARAM_SOLVES.with(|solves| solves.borrow_mut().clear());
        let report = s.param_set("w", &12.0.into()).unwrap();
        PARAM_SOLVES.with(|solves| assert_eq!(*solves.borrow(), sketches, "all four widths follow w through named dimensions"));
        // Each prism has width w=12, height 3, and extrusion depth 2, regardless of dependency depth.
        for body in report.bodies {
            assert!((body.volume - 12.0 * 3.0 * 2.0).abs() < 1e-6, "named-dimension chain left stale body volume {}", body.volume);
        }
    }

    #[test]
    fn parameter_edit_does_not_blame_a_preexisting_unsolved_unrelated_sketch() {
        let (mut s, reached, untouched) = two_sketches();
        let si = s.p.sketch_index(untouched).unwrap();
        // Simulate an already-unsolved imported sketch without dirtying its current body.
        s.p.sketches[si].points[0].x += 1.0;
        assert!(s.unsolved_sketches().contains(&untouched));
        let report = s.param_set("w", &12.0.into()).unwrap();
        PARAM_SOLVES.with(|solves| assert_eq!(*solves.borrow(), vec![reached]));
        // Native regenerate settles any preexisting unsolved sketch independently of propagation.
        // The edit still succeeds, and the unrelated body retains width=8, height=3, depth=2.
        assert!(report.errors.is_empty());
        assert!((report.bodies[1].volume - 8.0 * 3.0 * 2.0).abs() < 1e-6);
    }

    #[test]
    fn unused_parameter_edit_does_not_solve_any_sketch() {
        let (mut s, _, _) = two_sketches();
        s.param_set("unused", &1.0.into()).unwrap();
        PARAM_SOLVES.with(|solves| assert!(solves.borrow().is_empty(), "unexpected solves: {:?}", solves.borrow()));
    }
    #[test]
    #[ignore = "manual shelf timing; run with --ignored --nocapture"]
    fn parameter_and_project_clone_shelf_timings() {
        use std::{hint::black_box, path::Path, time::Instant};
        let (mut s, r) = Session::open(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hanging_shelf.qcad"))).unwrap();
        assert!(r.errors.is_empty());
        for value in [3.5, 4.0, 3.5] {
            PARAM_SOLVES.with(|solves| solves.borrow_mut().clear());
            let start = Instant::now();
            s.param_set("rim_floor_fillet", &value.into()).unwrap();
            eprintln!(
                "shelf param edit {value}: {:?}, {} sketches solved",
                start.elapsed(),
                PARAM_SOLVES.with(|solves| solves.borrow().len())
            );
        }
        let mut scaled = s.p.clone();
        // Stress clone-owned mesh/recipe vectors, not the kernel: eight shelf-sized payloads.
        // IDs need not be remapped because this synthetic Project is never regenerated.
        for _ in 1..8 {
            scaled.bodies.extend(s.p.bodies.clone());
            scaled.sketches.extend(s.p.sketches.clone());
            scaled.timeline.extend(s.p.timeline.clone());
            scaled.parameters.extend(s.p.parameters.clone());
            scaled.sources.extend(s.p.sources.clone());
        }
        for (label, p) in [("shelf", &s.p), ("8x shelf vectors", &scaled)] {
            let start = Instant::now();
            for _ in 0..50 {
                black_box(p.clone());
            }
            eprintln!(
                "Project::clone {label}: {:?} per clone, {} bodies / {} sketches / {} nodes",
                start.elapsed() / 50,
                p.bodies.len(),
                p.sketches.len(),
                p.timeline.len()
            );
        }
    }
}
