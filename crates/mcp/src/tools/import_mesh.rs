use super::common::{checked_path, err, rebuild_json};
use super::{tool, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportMeshArgs {
    /// Mesh file path: `.stl`, `.obj`, `.ply`, `.gltf`, `.glb`, `.3mf` or `.amf`; original bytes are embedded.
    pub path: String,
    /// Format, or `auto` to use the exact extension rule. Explicit formats must agree with the extension.
    #[serde(default = "auto")]
    pub format: String,
    /// Unit for unitless STL, OBJ and PLY coordinates. Embedded-unit formats use their own declared convention; glTF/GLB accepts only `m`.
    #[serde(default)]
    pub unit: Option<String>,
    /// Positive finite one-shot scale stored in the native MeshPiece scale. Default 1.
    #[serde(default = "one")]
    pub scale: f64,
    /// Optional import group name.
    #[serde(default)]
    pub name: Option<String>,
}

fn auto() -> String {
    "auto".into()
}
fn one() -> f64 {
    1.0
}

pub fn tools() -> Vec<Tool> {
    vec![tool(
        "import_mesh",
        "Import STL, OBJ, PLY, glTF/GLB, 3MF or AMF as native mesh pieces using the app's piece names, placements and group ancestry. The 16 MiB input, 64 MiB declared 3MF expansion and 200,000 decoded-triangle caps are checked before native readers and project mutation. STL/OBJ/PLY use the requested unit (default mm); glTF/GLB use metres and Y-up to millimetres and Z-up; 3MF/AMF use their declared unit. Scale is one-shot and stored as MeshPiece.scale. Saved documents embed source bytes and mesh geometry. Mesh bodies can move, export and render; booleans and fillets require stage C3 recognition.",
        |st, a: ImportMeshArgs| {
            let path = checked_path(&a.path, &["stl", "obj", "ply", "gltf", "glb", "3mf", "amf"])?;
            let format = match a.format.as_str() {
                "auto" => None,
                value if ["stl", "obj", "ply", "gltf", "glb", "3mf", "amf"].contains(&value) => Some(value),
                _ => return Err(format!("unknown mesh format `{}`", a.format)),
            };
            let unit = match a.unit.as_deref() {
                None => None,
                Some("mm" | "cm" | "m" | "in" | "ft") => a.unit.as_deref(),
                Some(value) => return Err(format!("unknown mesh unit `{value}`")),
            };
            let s = st.doc()?;
            let result = s.import_mesh(&path, format, unit, a.scale, a.name.as_deref()).map_err(err)?;
            Ok(json!({"source":result.source,"feature":result.feature,"pieces":result.pieces,"unit":result.unit,"scale":result.scale,"warnings":result.warnings,"rebuild":rebuild_json(&result.rebuild)}))
        },
    )]
}
