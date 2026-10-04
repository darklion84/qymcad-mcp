//! Argument types and result formatting shared by the tool groups.

use qymcad_engine::{BaseName, Id, PlaneRef, Rebuild, Session};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

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
