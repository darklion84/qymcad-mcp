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

    /// Create or change a parameter and rebuild everything that depends on it. Names are case-sensitive, as in
    /// QymCAD 0.1.1; a new name that differs from an existing one only by case is refused (ADR 0021). The change
    /// is rolled back if the model no longer rebuilds.
    pub fn param_set(&mut self, name: &str, value: &Num) -> Result<Rebuild> {
        let name = normalize_name(name)?;
        // QymCAD resolves these constants before parameters, so a parameter with such a name is never read
        // (FINDINGS F-039). Only creation/assignment is refused: an old document may hold one, and it must stay
        // deletable.
        if qymcad_core::expr::is_constant(&name) {
            return Err(Error::Invalid(format!("`{name}` is a built-in constant in expressions; choose another parameter name")));
        }
        let expr = match value {
            Num::Value(v) if !v.is_finite() => return Err(Error::Expr(format!("number {v} is not finite"))),
            Num::Value(v) => format!("{v}"),
            Num::Expr(_) => value.expr().unwrap_or_default(),
        };
        // QymCAD evaluates it (eval_parameters) with a recursive parser that has no depth cap.
        crate::value::check_expr(&expr)?;
        if !self.p.parameters.iter().any(|q| q.name == name) {
            if let Some(twin) = self.case_twin(&name) {
                return Err(Error::Invalid(format!(
                    "parameter `{twin}` already exists and `{name}` differs from it only by letter case; names are case-sensitive, \
                     so use `{twin}` to change it, or choose a clearly different name"
                )));
            }
        }
        let before = self.p.clone();
        let broken_before = self.unsolved_sketches();
        match self.p.parameters.iter_mut().find(|q| q.name == name) {
            Some(q) => q.expr = expr.clone(),
            None => self.p.parameters.push(Param { name: name.clone(), expr: expr.clone(), value: 0.0 }),
        }
        if let Some((what, err)) = self.p.eval_parameters().into_iter().next() {
            let message = format!("`{what}`: {}", crate::value::explain(&err, self.p.param_map().keys()));
            self.p = before;
            return Err(Error::Expr(message));
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
        let idx = self.p.parameters.iter().position(|q| q.name == name).ok_or_else(|| {
            let hint = self.case_twin(&name).map(|twin| format!("; names are case-sensitive: did you mean `{twin}`?")).unwrap_or_default();
            Error::NotFound(format!("parameter `{name}`{hint}"))
        })?;
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
            if q.name != name && mentions(&q.expr, name) {
                out.push(format!("parameter `{}`", q.name));
            }
        }
        for (node, dims) in &self.p.feat_dims {
            let is_sketch = self.p.sketch_index(*node).is_some();
            if is_sketch && dims.iter().any(|(k, e)| k.starts_with("datum_dist_") && mentions(e, name)) {
                out.push(format!("datum dependency of sketch {} `{}`", node, self.node_name(*node)));
            }
            if dims.iter().any(|(k, e)| (!is_sketch || !k.starts_with("datum_dist_")) && mentions(e, name)) {
                out.push(format!("feature {} `{}`", node, self.node_name(*node)));
            }
        }
        for s in &self.p.sketches {
            if s.constraints.iter().any(|c| c.expr().is_some_and(|e| mentions(e, name))) {
                out.push(format!("sketch {} `{}`", s.id, crate::localization::name(&s.name)));
            }
        }
        out
    }

    /// Solve only reached sketch expressions, including names refreshed by driven dimensions after a solve.
    /// Rebuild scheduling retains blanket native dirtying and face-derived datum barriers (F-002, F-017).
    /// Names match exactly, as QymCAD evaluates them. Returns reached sketches left unsolved.
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
                return Err(Error::Expr(format!("`{what}`: {}", crate::value::explain(&err, self.p.param_map().keys()))));
            }
            let values = self.p.param_map();
            let changed: Vec<&str> =
                values.iter().filter(|(name, value)| previous.get(*name) != Some(*value)).map(|(name, _)| name.as_str()).collect();
            if changed.is_empty() {
                break;
            }
            let reached = |expr: &str| changed.iter().any(|name| qymcad_core::expr::mentions(expr, name));
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

    /// Names in the expression scope: parameters, then named driving dimensions (`param_map` holds both).
    fn scope_names(&self) -> impl Iterator<Item = &String> {
        self.p.parameters.iter().map(|q| &q.name).chain(self.p.named_dims.iter().map(|d| &d.name)).filter(|n| !n.is_empty())
    }

    /// Groups of names in the expression scope (parameters and named dimensions) that differ only by letter case.
    /// QymCAD 0.1.1 keeps them apart, but they are easy to confuse; a document made elsewhere may hold them, and
    /// doc_info warns.
    pub fn case_twin_params(&self) -> Vec<Vec<String>> {
        let mut groups: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for name in self.scope_names() {
            let group = groups.entry(name.to_lowercase()).or_default();
            if !group.contains(name) {
                group.push(name.clone());
            }
        }
        groups.into_values().filter(|g| g.len() > 1).collect()
    }

    /// An existing name in the expression scope that equals `name` ignoring case but not exactly.
    fn case_twin(&self, name: &str) -> Option<String> {
        self.scope_names().find(|n| *n != name && crate::value::same_ignoring_case(n, name)).cloned()
    }

    /// Parameters whose formula does not evaluate now, explained (an unknown name in the wrong case gets a
    /// did-you-mean). A document can arrive so, e.g. from QymCAD 0.1.0, whose names were not case-sensitive.
    pub fn param_formula_problems(&self) -> Vec<String> {
        let vars = self.p.param_map();
        self.p
            .parameters
            .iter()
            .filter_map(|q| {
                let err = qymcad_core::expr::eval(&q.expr, &vars).err()?;
                Some(format!("parameter `{}` = `{}` does not evaluate: {}", q.name, q.expr, crate::value::explain(&err, vars.keys())))
            })
            .collect()
    }
}

fn normalize_name(name: &str) -> Result<String> {
    let n = name.trim().to_string();
    // The release validator also rejects constants. Keep assignment's existing explanation above,
    // while permitting deletion of constant-named parameters from legacy documents (F-039).
    match qymcad_core::drivers::check_ident(&n) {
        Ok(()) | Err(qymcad_core::drivers::IdentError::Constant) => {}
        Err(e) => {
            return Err(Error::Invalid(format!(
                "`{name}` is not a valid parameter name ({e:?}); use letters, digits and _ starting with a letter"
            )))
        }
    }
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
    fn parameter_edit_solves_only_reached_sketch_including_transitive_expressions() {
        let (mut s, reached, untouched) = two_sketches();
        let untouched_before = format!("{:?}", s.p.sketches.iter().find(|sk| sk.id == untouched).unwrap());
        // `w` reaches the first sketch only through `twice` = w*2.
        let r = s.param_set("w", &12.0.into()).unwrap();
        PARAM_SOLVES.with(|solves| assert_eq!(*solves.borrow(), vec![reached]));
        assert_eq!(format!("{:?}", s.p.sketches.iter().find(|sk| sk.id == untouched).unwrap()), untouched_before);
        // First width follows twice=2*w; second width follows other=8. Both have height 3 and depth 2.
        for (body, expected) in r.bodies.iter().zip([2.0 * 12.0 * 3.0 * 2.0, 8.0 * 3.0 * 2.0]) {
            assert!((body.volume - expected).abs() < 1e-6, "{} != {expected}", body.volume);
        }
    }

    /// QymCAD 0.1.1 matches names exactly (5dcaa65); the server follows it and stops lowercasing (ADR 0021).
    #[test]
    fn names_are_case_sensitive_and_case_twins_are_refused() {
        let (mut s, _, _) = two_sketches();
        // A mixed-case name is stored as given and read exactly.
        s.param_set("Wall", &3.0.into()).unwrap();
        assert!(s.params().iter().any(|p| p.name == "Wall" && p.value == 3.0));
        s.param_set("Wall", &4.0.into()).unwrap();
        assert_eq!(s.params().iter().filter(|p| p.name.to_lowercase() == "wall").count(), 1, "same exact name changes it");
        // A twin differing only by case is refused, naming the existing parameter, and nothing changes.
        let before = serde_json::to_vec(&s.p).unwrap();
        for twin in ["W", "WALL", "wall"] {
            let e = s.param_set(twin, &1.0.into()).unwrap_err().to_string();
            assert!(e.contains("differs from it only by letter case"), "{twin}: {e}");
        }
        assert!(s.param_set("W", &1.0.into()).unwrap_err().to_string().contains("`w` already exists"));
        assert_eq!(serde_json::to_vec(&s.p).unwrap(), before);
        // Cyrillic names compare by case too.
        s.param_set("Ширина", &5.0.into()).unwrap();
        assert!(s.param_set("ширина", &5.0.into()).unwrap_err().to_string().contains("`Ширина` already exists"));
        // A formula naming a parameter in the wrong case is an unknown name, explained.
        let e = s.param_set("q", &Num::Expr("TWICE+1".into())).unwrap_err().to_string();
        assert!(e.contains("no parameter `TWICE`") && e.contains("did you mean `twice`?"), "{e}");
        let e = s.param_delete("TWICE").unwrap_err().to_string();
        assert!(e.contains("did you mean `twice`?"), "{e}");
        // A sketch dimension in the wrong case is refused the same way at the sketch tool.
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let e = s.sketch_rect(sk, &0.0.into(), &0.0.into(), &Num::Expr("Other".into()), &3.0.into(), false).unwrap_err().to_string();
        assert!(e.contains("did you mean `other`?"), "{e}");
    }

    /// Named driving dimensions share the expression scope with parameters: a parameter that differs from one only
    /// by case is refused too, and doc_info names such twins.
    #[test]
    fn named_dimensions_count_as_case_twins() {
        let (mut s, sk, _) = two_sketches();
        let si = s.p.sketch_index(sk).unwrap();
        let c = s.p.sketches[si].constraints.iter().find(|c| c.expr() == Some("twice")).unwrap();
        let refs = qymcad_core::model::Project::dim_refs(c).unwrap();
        assert!(s.p.add_named_dim("Span".into(), sk, refs));
        let e = s.param_set("span", &1.0.into()).unwrap_err().to_string();
        assert!(e.contains("`Span` already exists"), "{e}");
        s.p.parameters.push(Param { name: "SPAN".into(), expr: "1".into(), value: 0.0 });
        assert_eq!(s.case_twin_params(), vec![vec!["SPAN".to_string(), "Span".to_string()]]);
    }

    /// A parameter formula that no longer evaluates (a name in the wrong case from an older document) is said in
    /// doc_info with a did-you-mean, rather than leaving a stale value unexplained.
    #[test]
    fn an_unevaluable_parameter_formula_is_explained_in_doc_info() {
        let (mut s, _, _) = two_sketches();
        s.p.parameters.push(Param { name: "q".into(), expr: "W*2".into(), value: 0.0 });
        let warning = s.info().warnings.into_iter().find(|w| w.starts_with("parameter `q`")).expect("doc_info explains");
        assert!(warning.contains("no parameter `W`") && warning.contains("did you mean `w`?"), "{warning}");
        // Several twins: the suggestion is stable (the smallest name).
        s.p.parameters.push(Param { name: "WW".into(), expr: "1".into(), value: 1.0 });
        s.p.parameters.push(Param { name: "Ww".into(), expr: "1".into(), value: 1.0 });
        s.p.parameters.push(Param { name: "z".into(), expr: "ww+1".into(), value: 0.0 });
        for _ in 0..5 {
            let w = s.info().warnings.into_iter().find(|w| w.starts_with("parameter `z`")).unwrap();
            assert!(w.contains("did you mean `WW`?"), "{w}");
        }
    }

    /// A document from the GUI may already hold names differing only by case: it opens, both stay, and edits keep
    /// them apart.
    #[test]
    fn existing_case_twins_stay_apart() {
        let (mut s, _, _) = two_sketches();
        s.p.parameters.push(Param { name: "W".into(), expr: "100".into(), value: 0.0 });
        assert!(s.p.eval_parameters().is_empty());
        assert_eq!(s.case_twin_params(), vec![vec!["w".to_string(), "W".to_string()]]);
        let warning = s.info().warnings.into_iter().find(|w| w.contains("differ only by letter case")).expect("doc_info warns");
        assert!(warning.contains("`w`, `W`"), "{warning}");
        // Editing `W` does not touch `w`, nor the sketch that reads `twice` = w*2.
        PARAM_SOLVES.with(|solves| solves.borrow_mut().clear());
        s.param_set("W", &50.0.into()).unwrap();
        PARAM_SOLVES.with(|solves| assert!(solves.borrow().is_empty()));
        let map = s.p.param_map();
        assert_eq!((map["w"], map["W"], map["twice"]), (10.0, 50.0, 20.0));
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
