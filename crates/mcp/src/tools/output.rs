//! Output tools: export to exchange files, render a picture.

use super::common::{checked_path, err, round, ObjRef};
use super::{tool, tool_content, Tool};
use qymcad_engine::{ExportFormat, Id, Quality, Session, View};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportArgs {
    /// step (exact B-rep, mm), stl (binary, mm), 3mf (mm), glb (metres, +Y up), obj (mm).
    pub format: ExportFormat,
    /// Output file; its extension must match the format (.step/.stp, .stl, .3mf, .glb, .obj). Overwritten if it exists.
    pub path: String,
    /// Mesh detail for stl/3mf/glb/obj (chordal deflection): draft 0.2 mm, standard 0.05 mm (default), high 0.02 mm,
    /// max 0.005 mm. Ignored for step.
    #[serde(default)]
    pub quality: Quality,
    /// Bodies to write (ids or names of the features that made them). Default: every current result body.
    #[serde(default)]
    pub bodies: Option<Vec<ObjRef>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderArgs {
    /// iso (default; from front-right-top), top, bottom, front (from −Y), back, left (from −X), right (from +X).
    #[serde(default)]
    pub view: View,
    /// Image width in pixels, 64..=2048. Default 512.
    #[serde(default = "default_width")]
    #[schemars(range(min = 64, max = 2048))]
    pub width: u32,
    /// Image height in pixels, 64..=2048. Default 384.
    #[serde(default = "default_height")]
    #[schemars(range(min = 64, max = 2048))]
    pub height: u32,
    /// Bodies to draw (ids or names). Default: every current result body.
    #[serde(default)]
    pub bodies: Option<Vec<ObjRef>>,
}

fn default_width() -> u32 {
    512
}

fn default_height() -> u32 {
    384
}

fn resolve_bodies(s: &Session, bodies: &Option<Vec<ObjRef>>) -> Result<Option<Vec<Id>>, String> {
    bodies.as_ref().map(|v| v.iter().map(|b| b.resolve(s)).collect()).transpose()
}

pub fn tools() -> Vec<Tool> {
    vec![
        tool(
            "export",
            "Write the result bodies to a STEP (exact) or mesh file (STL/3MF/GLB/OBJ) for printing, CAM or other CAD. Each body \
             is placed where it stands in the document; one object per body, no colours. Returns the bodies written, \
             triangle count and per-body mesh volume (compare with the B-rep volume from doc_info). Refuses export if \
             any timeline node has a regeneration error, even when the selected bodies are clean.",
            |st, a: ExportArgs| {
                let path = checked_path(&a.path, a.format.extensions())?;
                let s = st.doc()?;
                let ids = resolve_bodies(s, &a.bodies)?;
                let r = s.export(a.format, &path, a.quality, ids.as_deref()).map_err(err)?;
                let mut v = serde_json::to_value(&r).unwrap_or(Value::Null);
                if let Some(bodies) = v["bodies"].as_array_mut() {
                    for b in bodies {
                        if let Some(mv) = b["mesh_volume"].as_f64() {
                            b["mesh_volume"] = json!(round(mv, 3));
                        }
                    }
                }
                Ok(v)
            },
        ),
        tool_content(
            "render",
            "Look at the model: a shaded orthographic PNG of the result bodies with dark edges on a light background, fitted \
             to the frame (not to scale between calls). Use it to check shape and feature placement after building. Refuses rendering if \
             any timeline node has a regeneration error, even when the selected bodies are clean.",
            |st, a: RenderArgs| {
                let s = st.doc()?;
                let ids = resolve_bodies(s, &a.bodies)?;
                let r = s.render(a.view, a.width, a.height, ids.as_deref()).map_err(err)?;
                let b = r.bbox.map(|v| round(v, 2));
                let text = format!(
                    "{} view, {}: bodies {:?}, bbox x {}..{} y {}..{} z {}..{} mm",
                    format!("{:?}", a.view).to_lowercase(),
                    a.view.describe(),
                    r.bodies,
                    b[0],
                    b[3],
                    b[1],
                    b[4],
                    b[2],
                    b[5]
                );
                Ok(vec![
                    json!({ "type": "image", "data": base64(&r.png), "mimeType": "image/png" }),
                    json!({ "type": "text", "text": text }),
                ])
            },
        ),
    ]
}

/// Standard base64 with padding (RFC 4648 §4).
pub fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                out.push(A[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_rfc4648_vectors() {
        for (inp, want) in
            [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
        {
            assert_eq!(base64(inp.as_bytes()), want, "{inp:?}");
        }
        assert_eq!(base64(&[0xFF, 0xFE, 0xFD]), "//79");
    }
}
