//! Argument types and result formatting shared by the tool groups.

use qymcad_engine::{BaseName, Id, PlaneRef, Rebuild, Session};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

/// An object reference: its numeric id, or the name given when it was created.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ObjRef {
    Id(Id),
    Name(String),
}

impl ObjRef {
    pub fn resolve(&self, s: &Session) -> Result<Id, String> {
        match self {
            ObjRef::Id(id) => s.resolve(&id.to_string()),
            ObjRef::Name(n) => s.resolve(n),
        }
        .map_err(|e| e.to_string())
    }
}

/// A plane: `"XY"` (normal +Z), `"XZ"` (normal −Y), `"YZ"` (normal +X); `{"plane": <datum plane>}`; or
/// `{"body": <body>, "face": <face id>}` for a planar face (face ids come from `topology`).
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PlaneArg {
    Base(BaseName),
    Datum { plane: ObjRef },
    Face { body: ObjRef, face: u32 },
}

impl PlaneArg {
    pub fn resolve(&self, s: &Session) -> Result<PlaneRef, String> {
        Ok(match self {
            PlaneArg::Base(b) => PlaneRef::Base(*b),
            PlaneArg::Datum { plane } => PlaneRef::Plane(plane.resolve(s)?),
            PlaneArg::Face { body, face } => PlaneRef::Face { body: body.resolve(s)?, face: *face },
        })
    }
}

pub fn round(v: f64, dp: i32) -> f64 {
    let k = 10f64.powi(dp);
    (v * k).round() / k
}

/// A rebuild report, compact: rounded numbers, empty lists omitted.
pub fn rebuild_json(r: &Rebuild) -> Value {
    let bodies: Vec<Value> = r
        .bodies
        .iter()
        .map(|b| {
            json!({
                "id": b.id,
                "name": b.name,
                "volume_mm3": round(b.volume, 4),
                "bbox": b.bbox.iter().map(|v| round(*v, 3)).collect::<Vec<_>>(),
            })
        })
        .collect();
    let mut o = json!({ "bodies": bodies });
    if !r.errors.is_empty() {
        o["errors"] = json!(r.errors);
    }
    if !r.warnings.is_empty() {
        o["warnings"] = json!(r.warnings);
    }
    o
}

pub fn err(e: qymcad_engine::Error) -> String {
    e.to_string()
}

/// Validate a file path an agent asked us to read or write (docs/SECURITY.md):
/// - the extension must be one of `exts` (so a confused or prompt-injected agent cannot overwrite arbitrary files
///   such as shell profiles through this server);
/// - relative paths resolve against the server's working directory;
/// - with `QYMCAD_MCP_ROOT` set, the file must lie inside that directory (symlinks resolved).
pub fn checked_path(path: &str, exts: &[&str]) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    let p = if p.is_absolute() { p } else { std::env::current_dir().map_err(|e| e.to_string())?.join(p) };
    let ext = p.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    if !exts.contains(&ext.as_str()) {
        return Err(format!(
            "`{path}`: only {} files are allowed here",
            exts.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(", ")
        ));
    }
    let name = p.file_name().ok_or_else(|| format!("`{path}` has no file name"))?.to_owned();
    let parent = p.parent().ok_or_else(|| format!("`{path}` has no directory"))?;
    let parent = std::fs::canonicalize(parent).map_err(|e| format!("directory of `{path}`: {e}"))?;
    if let Some(root) = std::env::var_os("QYMCAD_MCP_ROOT") {
        let root = std::fs::canonicalize(&root).map_err(|e| format!("QYMCAD_MCP_ROOT: {e}"))?;
        if !parent.starts_with(&root) {
            return Err(format!("`{path}` is outside QYMCAD_MCP_ROOT ({})", root.display()));
        }
    }
    Ok(parent.join(name))
}
