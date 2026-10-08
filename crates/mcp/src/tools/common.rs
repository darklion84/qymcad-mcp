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
    Datum {
        /// Datum plane id or name.
        plane: ObjRef,
    },
    Face {
        /// Body id or name containing the face.
        body: ObjRef,
        /// Planar face id from the body's current topology.
        face: u32,
    },
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
    let scaled = v * k;
    if !scaled.is_finite() {
        return v;
    }
    scaled.round() / k
}

/// A rebuild report: full-precision volume, rounded bounds, empty lists omitted.
pub fn rebuild_json(r: &Rebuild) -> Value {
    let bodies: Vec<Value> = r
        .bodies
        .iter()
        .map(|b| {
            json!({
                "id": b.id,
                "name": b.name,
                "volume_mm3": b.volume,
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
/// - with `QYMCAD_MCP_ROOT` set, the file must lie inside that directory (symlinks resolved);
/// - the file itself and QymCAD's save companions (`<path>.tmp~`, `<path>.bak`) must not be symlinks, or a write
///   would follow the link to an arbitrary file.
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
    let file = parent.join(&name);
    for candidate in [file.clone(), sibling(&file, ".tmp~"), sibling(&file, ".bak")] {
        if std::fs::symlink_metadata(&candidate).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!("`{}` is a symbolic link; refusing to follow it", candidate.display()));
        }
    }
    Ok(file)
}

fn sibling(file: &std::path::Path, suffix: &str) -> PathBuf {
    let mut s = file.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::round;
    use serde_json::json;

    #[test]
    fn rounding_a_large_finite_value_preserves_it() {
        // Rounding to six decimal places needs a factor 10^6. For 10^303 the product
        // is 10^309, beyond f64::MAX (~1.8 × 10^308), although the input is finite.
        // Its f64 spacing is much larger than 10^-6, so rounding must leave it unchanged.
        for value in [1e303, -1e303] {
            assert_eq!(round(value, 6), value, "rounding must preserve a finite value when scaling overflows");
            assert_eq!(json!(round(value, 6)), json!(value));
        }
        // 1.23456 × 10^3 rounds to 1235, then dividing by 10^3 gives 1.235.
        assert_eq!(round(1.23456, 3), 1.235);
    }
}
