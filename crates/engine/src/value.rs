//! `Num`: a number or an expression over the document's parameters.

use crate::error::{Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A dimension: either a plain number (millimetres or degrees) or an expression over parameters, e.g. `"w/2"`,
/// `"t+1"`, `"2*pi*r"`. Expressions keep the model parametric: changing the parameter later rebuilds the part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Num {
    Value(f64),
    Expr(String),
}

impl Num {
    /// The expression as stored in the document: trimmed and lowercased (FINDINGS F-001). `None` for numbers.
    pub fn expr(&self) -> Option<String> {
        match self {
            Num::Value(_) => None,
            Num::Expr(s) => Some(normalize_expr(s)),
        }
    }

    /// The current value against `vars` (from `Project::param_map`, keys lowercase).
    pub fn eval(&self, vars: &HashMap<String, f64>) -> Result<f64> {
        match self {
            Num::Value(v) if v.is_finite() => Ok(*v),
            Num::Value(v) => Err(Error::Expr(format!("number {v} is not finite"))),
            Num::Expr(s) => {
                let e = normalize_expr(s);
                check_expr(&e)?;
                // A plain number written as a string is still a number. Rust's parser also accepts "nan",
                // "inf" and overflowing literals such as "1e400"; those are refused below.
                let v = match e.parse::<f64>() {
                    Ok(v) => v,
                    // QymCAD's own arithmetic parser (numbers, + - * / ^, a fixed set of math functions,
                    // parameter names); it does not execute code.
                    Err(_) => qymcad_core::expr::eval(&e, vars).map_err(|err| Error::Expr(format!("`{e}`: {err:?}")))?,
                };
                if !v.is_finite() {
                    return Err(Error::Expr(format!("`{e}` is not a finite number ({v})")));
                }
                Ok(v)
            }
        }
    }

    /// The expression for a dimension that stores a magnitude (`|Δ|`): when the current value is negative, the
    /// side comes from the geometry and the expression is negated so it evaluates positive.
    pub(crate) fn magnitude_expr(&self, value: f64) -> Result<String> {
        let expr = match self.expr() {
            None => String::new(),
            Some(e) if value < 0.0 => format!("-({e})"),
            Some(e) => e,
        };
        check_expr(&expr)?;
        Ok(expr)
    }
}

impl From<f64> for Num {
    fn from(v: f64) -> Self {
        Num::Value(v)
    }
}

impl From<&str> for Num {
    fn from(s: &str) -> Self {
        Num::Expr(s.to_string())
    }
}

/// Longest expression accepted, in characters. Real dimension formulas are tens of characters.
const MAX_EXPR_LEN: usize = 1000;
/// Deepest nesting accepted, per kind (parentheses, consecutive unary signs, `^`). QymCAD's parser
/// (`qymcad_core::expr`) is recursive descent without a depth cap: each `(` costs five frames, each unary sign
/// and each `^` one or two, so 100 000 of them overflow the stack and abort the process. 64 of each is far
/// beyond any real formula and a few hundred frames deep.
const MAX_EXPR_NESTING: usize = 64;

/// Refuse expressions that could overflow QymCAD's recursive parser. Every string the engine hands to QymCAD's
/// evaluator goes through here: `Num::eval` (all feature and sketch dimensions are evaluated before they are
/// stored), `Session::param_set` (parameters), and generated dimension formulas before storing them.
pub(crate) fn check_expr(e: &str) -> Result<()> {
    if e.chars().count() > MAX_EXPR_LEN {
        return Err(Error::Expr(format!("expression longer than {MAX_EXPR_LEN} characters")));
    }
    let (mut depth, mut max_depth, mut carets, mut run, mut max_run) = (0usize, 0usize, 0usize, 0usize, 0usize);
    for c in e.chars().filter(|c| !c.is_whitespace()) {
        match c {
            '(' => {
                depth += 1;
                max_depth = max_depth.max(depth);
            }
            ')' => depth = depth.saturating_sub(1),
            '^' => carets += 1,
            _ => {}
        }
        run = if c == '-' || c == '+' { run + 1 } else { 0 };
        max_run = max_run.max(run);
    }
    if max_depth > MAX_EXPR_NESTING || carets > MAX_EXPR_NESTING || max_run > MAX_EXPR_NESTING {
        return Err(Error::Expr(format!(
            "expression nested too deeply (at most {MAX_EXPR_NESTING} levels of parentheses, `^` or consecutive signs)"
        )));
    }
    Ok(())
}

pub(crate) fn normalize_expr(s: &str) -> String {
    s.trim().to_lowercase()
}
