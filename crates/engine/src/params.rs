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
        let propagated = match self.propagate_params(&before.param_map()) {
            Ok(unsolved) => unsolved,
            Err(e) => {
                self.p = before;
                return Err(e);
            }
        };
        // Named reference measurements can change count expressions after the initial limit check.
        if let Err(e) = self.check_array_limits() {
            self.p = before;
            return Err(e);
        }
        let unsolved: Vec<String> = propagated
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

    /// Solve only reached sketch expressions, including names refreshed by driven dimensions after a solve.
    /// Rebuild scheduling retains blanket native dirtying and face-derived datum barriers (F-002, F-017).
    /// Lowercase imported expressions too (F-001). Returns reached sketches left unsolved.
    pub(crate) fn propagate_params(&mut self, before: &std::collections::HashMap<String, f64>) -> Result<Vec<(Id, f64)>> {
        let mut previous = before.clone();
        let mut unsolved = Vec::new();
        // Each solve round can expose the next named reference dimension. Bound imported cycles, and
        // reserve one extra round to observe stabilization after the longest acyclic sketch chain.
        for round in 0..=self.p.sketches.len() {
            // Native evaluation seeds named driving dimensions once. Settle their expression chains
            // before each reachability snapshot, including chains fed by newly measured references.
            let mut values = self.p.param_map();
            let mut errors = Vec::new();
            for _ in 0..=self.p.named_dims.len() {
                errors = self.p.eval_parameters();
                let next = self.p.param_map();
                if next == values {
                    break;
                }
                values = next;
            }
            if let Some((what, err)) = errors.into_iter().next() {
                return Err(Error::Expr(format!("`{what}`: {err:?}")));
            }
            let values = self.p.param_map();
            let changed: Vec<&str> =
                values.iter().filter(|(name, value)| previous.get(*name) != Some(*value)).map(|(name, _)| name.as_str()).collect();
            if changed.is_empty() {
                break;
            }
            let reached = |expr: &str| {
                let expr = expr.to_lowercase();
                changed.iter().any(|name| qymcad_core::expr::mentions(&expr, name))
            };
            for si in 0..self.p.sketches.len() {
                if self.p.sketches[si].constraints.iter().any(|c| c.expr().is_some_and(reached)) {
                    #[cfg(test)]
                    PARAM_SOLVES.with(|solves| solves.borrow_mut().push(self.p.sketches[si].id));
                    let residual = self.solve_settled(si);
                    let sid = self.p.sketches[si].id;
                    self.p.mark_sketch_dirty(sid);
                    unsolved.retain(|(id, _)| *id != sid);
                    if residual.is_nan() || residual > 1e-6 {
                        unsolved.push((sid, residual));
                    }
                }
            }
            // update_driven_dims rewrites measurements during solving, after the snapshot above.
            if self.p.param_map() == values {
                break;
            }
            if round == self.p.sketches.len() {
                return Err(Error::Invalid(
                    "named sketch dimensions did not stabilize after parameter propagation; \
                    check for cyclic reference-dimension expressions. The change was rolled back."
                        .into(),
                ));
            }
            previous = values;
        }
        // Native inputs() omits datum-hosted sketches. Blanket dirtying alone can still prepare a solid
        // on the old frame, so dirty face-derived datum sketches as barriers without solving them.
        self.p.mark_param_dependents_dirty();
        let mut barriers = Vec::new();
        for sketch in &self.p.sketches {
            if let qymcad_core::feature::SketchPlane::Datum(mut plane) = sketch.plane {
                for _ in 0..self.p.planes.len() {
                    match self.p.planes.iter().find(|p| p.id == plane).map(|p| &p.def) {
                        Some(qymcad_core::model::PlaneDef::OffsetPlane { plane: parent, .. }) => plane = *parent,
                        Some(qymcad_core::model::PlaneDef::OffsetFace { .. }) => {
                            barriers.push(sketch.id);
                            break;
                        }
                        _ => break,
                    }
                }
            }
        }
        for sid in barriers {
            self.p.mark_sketch_dirty(sid);
        }
        Ok(unsolved)
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

    fn reference_chain() -> (Session, Vec<Id>) {
        use qymcad_core::model::{Constraint, EntityKind, Project};
        let mut s = Session::new_part();
        s.param_set("w", &10.0.into()).unwrap();
        let mut sketches = Vec::new();
        for (expr, name) in [("w", "ra"), ("ra", "rb"), ("rb", "rc")] {
            let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
            let lines = s.sketch_rect(sk, &0.0.into(), &0.0.into(), &Num::Expr(expr.into()), &3.0.into(), false).unwrap();
            let si = s.p.sketch_index(sk).unwrap();
            let point = |line| match s.p.sketches[si].entities.iter().find(|e| e.id == line).unwrap().kind {
                EntityKind::Line { a, .. } => a,
                _ => unreachable!(),
            };
            let c = Constraint::Distance {
                a: point(lines[0]),
                b: point(lines[2]),
                d: 0.0,
                axis: 0,
                expr: String::new(),
                driven: true,
                off: 0.0,
                at: None,
            };
            let refs = Project::dim_refs(&c).unwrap();
            s.p.sketches[si].constraints.push(c);
            s.solve_settled(si);
            assert!(s.p.add_named_dim(name.into(), sk, refs));
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
        (s, sketches)
    }

    #[test]
    fn named_reference_chain_solves_in_rounds_independent_of_sketch_storage_order() {
        let (mut s, sketches) = reference_chain();
        // Imported sketch pool order need not be dependency order. A one-pass extra solve is insufficient.
        s.p.sketches.reverse();
        let r = s.param_set("w", &12.0.into()).unwrap();
        PARAM_SOLVES.with(|solves| assert_eq!(*solves.borrow(), sketches));
        let mut width: f64 = 12.0;
        for (body, name) in r.bodies.iter().zip(["ra", "rb", "rc"]) {
            assert!((body.volume - width * 3.0 * 2.0).abs() < 1e-6);
            let diagonal = (width * width + 3.0 * 3.0).sqrt();
            assert!((s.p.param_map()[name] - diagonal).abs() < 1e-6);
            width = diagonal;
        }
    }

    #[test]
    fn cyclic_named_references_are_bounded_and_restore_the_project() {
        let (mut s, sketches) = reference_chain();
        let si = s.p.sketch_index(sketches[0]).unwrap();
        for c in &mut s.p.sketches[si].constraints {
            if let qymcad_core::model::Constraint::Distance { expr, .. } = c {
                if expr == "w" {
                    *expr = "w+rc".into();
                }
            }
        }
        let before = serde_json::to_vec(&s.p).unwrap();
        let shape_bytes = |s: &Session| {
            let _gate = qymcad_kernel::kernel_gate();
            s.shapes.iter().map(|(id, shape)| (*id, shape.to_brep_bytes().unwrap())).collect::<std::collections::BTreeMap<_, _>>()
        };
        let breps = shape_bytes(&s);
        let error = s.param_set("w", &12.0.into()).unwrap_err().to_string();
        assert!(error.contains("did not stabilize") && error.contains("rolled back"), "{error}");
        assert_eq!(serde_json::to_vec(&s.p).unwrap(), before);
        assert_eq!(shape_bytes(&s), breps);
        PARAM_SOLVES.with(|solves| assert!(solves.borrow().len() <= (sketches.len() + 1) * sketches.len()));
    }

    #[test]
    fn updated_reference_revalidates_global_expressions_before_rebuild() {
        let (mut s, _) = reference_chain();
        s.param_set("q", &Num::Expr("sqrt(11-ra)".into())).unwrap();
        let before = serde_json::to_vec(&s.p).unwrap();
        let error = s.param_set("w", &12.0.into()).unwrap_err();
        // ra grows from sqrt(109)<11 to sqrt(153)>11, so q has no real finite value.
        assert!(matches!(error, Error::Expr(_)), "{error}");
        assert_eq!(serde_json::to_vec(&s.p).unwrap(), before);
    }

    #[test]
    fn updated_reference_revalidates_array_total_before_rebuild() {
        use qymcad_core::model::ArrayAxis;
        let (mut s, _) = reference_chain();
        // Imported pending array with a missing source: param_set may repair failing baselines, but
        // must refuse oversized counts before native preparation, even if regeneration would fail too.
        let array = s.p.add_linear_array_grid3(
            0,
            [ArrayAxis { count: 10, d: [20.0, 0.0, 0.0] }, ArrayAxis { count: 100, d: [0.0, 20.0, 0.0] }, ArrayAxis::none()],
        );
        s.p.set_feat_dim(array, "count", "round(ra)".into());
        s.check_array_limits().unwrap(); // round(sqrt(109))*100 = 1000, at the cap.
        let before = serde_json::to_vec(&s.p).unwrap();
        let error = s.param_set("w", &12.0.into()).unwrap_err();
        // round(sqrt(153))*100 = 1200 exceeds the cap after reference propagation.
        assert!(matches!(&error, Error::Invalid(m) if m.contains("1200") && m.contains("at most 1000")), "{error}");
        assert_eq!(serde_json::to_vec(&s.p).unwrap(), before);
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
