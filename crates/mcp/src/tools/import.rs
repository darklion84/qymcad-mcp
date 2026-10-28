use super::common::{checked_path, err, rebuild_json};
use super::{tool, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportArgs {
    /// STEP/IGES file path (.step, .stp, .iges or .igs); original bytes are embedded in the document.
    pub path: String,
    /// Positive finite literal scale correction applied uniformly to imported geometry and component offsets. Default 1.
    #[serde(default = "one")]
    pub scale: f64,
    /// Zero-based source solid indexes to import; default imports every solid. Names, tree structure and placements are preserved.
    #[serde(default)]
    pub solids: Option<Vec<u32>>,
    /// Optional import group/part name. Numeric-looking names are refused.
    #[serde(default)]
    pub name: Option<String>,
}

fn one() -> f64 {
    1.0
}

pub fn tools() -> Vec<Tool> {
    vec![tool(
        "import",
        "Import exact STEP or IGES solids as QymCAD parts using the file's component tree, names and placements. Units are normalized by OCCT to mm; scale is an additional literal correction (default 1), and scales the entire import about the file origin. The original bytes are embedded, so the external file is not needed after import. `solids` selects zero-based source solid indexes; default imports all. Imported bodies remain individually addressable by id/name for topology, select, fillet, body_boolean, batch, sketch-on-face, push_face, export and doc_info. The reported body name resolves as an alias; if it conflicts with another object, use the body id. Returns source id, tree root feature, each body/name/index/scale/volume/bbox, warnings and rebuild diagnostics.",
        |st, a: ImportArgs| {
            let path = checked_path(&a.path, &["step", "stp", "iges", "igs"])?;
            let s = st.doc()?;
            let result = s.import(&path, a.scale, a.solids.as_deref(), a.name.as_deref()).map_err(err)?;
            Ok(json!({"source":result.source,"feature":result.feature,"bodies":result.bodies,"warnings":result.warnings,"rebuild":rebuild_json(&result.rebuild)}))
        },
    )]
}
