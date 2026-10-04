//! Document tools: new, open, save, info.

use super::common::{err, rebuild_json};
use super::{tool, Tool};
use qymcad_engine::Session;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenArgs {
    /// Path to a `.qcad` file.
    pub path: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveArgs {
    /// Where to write the `.qcad`. Default: the file the document came from or was last saved to.
    #[serde(default)]
    pub path: Option<String>,
}

pub fn tools() -> Vec<Tool> {
    vec![
        tool(
            "doc_new",
            "Start a new document with one empty part (replaces the open document; save it first if needed).",
            |st, _: NoArgs| {
                st.session = Some(Session::new_part());
                Ok(json!({ "ok": true, "qymcad_version": qymcad_engine::QYMCAD_VERSION }))
            },
        ),
        tool(
            "doc_open",
            "Open a .qcad file written by QymCAD of the same release (or by this server). Rebuilds what has no stored geometry.",
            |st, a: OpenArgs| {
                let (s, r) = Session::open(&PathBuf::from(&a.path)).map_err(err)?;
                let info = serde_json::to_value(s.info()).unwrap_or(Value::Null);
                st.session = Some(s);
                Ok(json!({ "rebuild": rebuild_json(&r), "doc": info }))
            },
        ),
        tool(
            "doc_save",
            "Save the document as .qcad (open it in the QymCAD app with File > Open; double-click does not work on macOS).",
            |st, a: SaveArgs| {
                let s = st.doc()?;
                let path = s.save(a.path.as_deref().map(std::path::Path::new)).map_err(err)?;
                let abs = std::fs::canonicalize(&path).unwrap_or(path);
                Ok(json!({ "saved": abs.display().to_string() }))
            },
        ),
        tool(
            "doc_info",
            "The whole document: parameters, sketches (contours, degrees of freedom), the timeline, result bodies (volume mm³, bbox), errors and warnings.",
            |st, _: NoArgs| Ok(serde_json::to_value(st.doc()?.info()).unwrap_or(Value::Null)),
        ),
    ]
}
