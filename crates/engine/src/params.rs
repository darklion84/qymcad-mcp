//! Document parameters (`name = expression`), the global drivers of a parametric part.

use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::value::Num;
use qymcad_core::model::{Id, Param};
use serde::Serialize;

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
        // (FINDINGS F-3A-1). Only creation/assignment is refused: an old document may hold one, and it must stay
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
        // Array counts may follow parameters; refuse an edit that would make an array too large (F-3B-11).
        if let Err(e) = self.check_array_limits() {
            self.p = before;
            return Err(e);
        }
        let unsolved: Vec<String> = self
            .propagate_params()
            .into_iter()
            .filter(|(sid, _)| !broken_before.contains(sid))
            .map(|(sid, res)| format!("sketch {sid} `{}` (residual {res:.3e})", self.node_name(sid)))
            .collect();
        if !unsolved.is_empty() {
            self.p = before;
            self.propagate_params();
            self.rebuild();
            return Err(Error::Invalid(format!(
                "with `{name}` = {expr} {} does not solve, so the change was rolled back. A dimension cannot take this value \
                 (e.g. an angle outside 0..180, or a coordinate/offset expression that would change sign: QymCAD dimensions \
                 keep the side they were created on, FINDINGS F-3A-7).",
                unsolved.join(", ")
            )));
        }
        let r = self.rebuild();
        if !r.errors.is_empty() {
            let lines = r.errors.iter().map(|i| format!("{} ({}): {}", i.name, i.node, i.message)).collect();
            self.p = before;
            self.propagate_params();
            self.rebuild();
            return Err(Error::Rebuild(lines));
        }
        Ok(r)
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
            if dims.values().any(|e| mentions(&e.to_lowercase(), name)) {
                out.push(format!("feature {} `{}`", node, self.node_name(*node)));
            }
        }
        for s in &self.p.sketches {
            if s.constraints.iter().any(|c| c.expr().is_some_and(|e| mentions(&e.to_lowercase(), name))) {
                out.push(format!("sketch {} `{}`", s.id, s.name));
            }
        }
        out
    }

    /// After parameter values changed: re-solve sketches whose dimensions carry expressions and mark every
    /// node with a feature expression dirty (FINDINGS F-002; `mark_param_dependents_dirty` is case-blind,
    /// unlike the GUI's per-name path, F-001). Returns the sketches left unsolved, with their residuals.
    pub(crate) fn propagate_params(&mut self) -> Vec<(Id, f64)> {
        self.p.eval_parameters();
        let mut unsolved = Vec::new();
        for si in 0..self.p.sketches.len() {
            if self.p.sketches[si].constraints.iter().any(|c| c.expr().is_some()) {
                let residual = self.solve_settled(si);
                let sid = self.p.sketches[si].id;
                self.p.mark_sketch_dirty(sid);
                if residual.is_nan() || residual > 1e-6 {
                    unsolved.push((sid, residual));
                }
            }
        }
        self.p.mark_param_dependents_dirty();
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
