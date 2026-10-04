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
            Num::Value(v) => Err(Error::Invalid(format!("number {v} is not finite"))),
            Num::Expr(s) => {
                let e = normalize_expr(s);
                // A plain number written as a string is still a number.
                if let Ok(v) = e.parse::<f64>() {
                    return Ok(v);
                }
                // QymCAD's own arithmetic parser (numbers, + - * / ^, a fixed set of math functions, parameter
                // names); it does not execute code.
                qymcad_core::expr::eval(&e, vars).map_err(|err| Error::Expr(format!("`{e}`: {err:?}")))
            }
        }
    }

    /// The expression for a dimension that stores a magnitude (`|Δ|`): when the current value is negative, the
    /// side comes from the geometry and the expression is negated so it evaluates positive.
    pub(crate) fn magnitude_expr(&self, value: f64) -> String {
        match self.expr() {
            None => String::new(),
            Some(e) if value < 0.0 => format!("-({e})"),
            Some(e) => e,
        }
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

pub(crate) fn normalize_expr(s: &str) -> String {
    s.trim().to_lowercase()
}
