//! Engine errors. Messages are written for the agent calling the tool: say what was wrong and what to do.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// A referenced object (sketch, feature, body, parameter, file) does not exist.
    NotFound(String),
    /// The request itself is invalid (bad name, wrong kind of object, missing body to cut, ...).
    Invalid(String),
    /// An expression did not evaluate.
    Expr(String),
    /// Reading or writing a file failed.
    Io(String),
    /// The model did not rebuild; the operation was rolled back. One line per failing node.
    Rebuild(Vec<String>),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound(s) => write!(f, "not found: {s}"),
            Error::Invalid(s) => write!(f, "invalid request: {s}"),
            Error::Expr(s) => write!(f, "expression error: {s}"),
            Error::Io(s) => write!(f, "i/o error: {s}"),
            Error::Rebuild(lines) => write!(f, "the model did not rebuild (operation rolled back): {}", lines.join("; ")),
        }
    }
}

impl std::error::Error for Error {}
