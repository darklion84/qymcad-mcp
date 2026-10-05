//! Document parameters (`name = expression`), the global drivers of a parametric part.

use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::value::Num;
use qymcad_core::model::Param;
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
        let expr = match value {
            Num::Value(v) => format!("{v}"),
            Num::Expr(_) => value.expr().unwrap_or_default(),
        };
        let before = self.p.clone();
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
        self.propagate_params();
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
    /// unlike the GUI's per-name path, F-001).
    pub(crate) fn propagate_params(&mut self) {
        self.p.eval_parameters();
        for si in 0..self.p.sketches.len() {
            if self.p.sketches[si].constraints.iter().any(|c| c.expr().is_some()) {
                self.solve_settled(si);
                let sid = self.p.sketches[si].id;
                self.p.mark_sketch_dirty(sid);
            }
        }
        self.p.mark_param_dependents_dirty();
    }

    /// Parameter names that break GUI rebuilds (uppercase, FINDINGS F-001) — reported as warnings for files
    /// created elsewhere.
    pub fn uppercase_params(&self) -> Vec<String> {
        self.p.parameters.iter().filter(|q| q.name.chars().any(|c| c.is_uppercase())).map(|q| q.name.clone()).collect()
    }
}

fn normalize_name(name: &str) -> Result<String> {
    let n = name.trim().to_lowercase();
    // QymCAD resolves these constants before parameters, so a parameter with such a name is never read
    // (FINDINGS F-3A-1).
    if ["pi", "tau", "e"].contains(&n.as_str()) {
        return Err(Error::Invalid(format!("`{name}` is a built-in constant in expressions; choose another parameter name")));
    }
    qymcad_core::drivers::check_ident(&n).map_err(|e| {
        Error::Invalid(format!("`{name}` is not a valid parameter name ({e:?}); use letters, digits and _ starting with a letter"))
    })?;
    Ok(n)
}
