//! Document tools: new, open, save, info.

use super::common::{checked_path, err, rebuild_json};
use super::{tool, Tool};
use qymcad_engine::Session;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

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
                let path = checked_path(&a.path, &["qcad"])?;
                let (s, r) = Session::open(&path).map_err(err)?;
                let info = serde_json::to_value(s.info()).unwrap_or(Value::Null);
                st.session = Some(s);
                Ok(json!({ "rebuild": rebuild_json(&r), "doc": info }))
            },
        ),
        tool(
            "doc_save",
            "Save the document as .qcad; the directory must exist. Open it in the QymCAD app with File > Open; double-click does not work on macOS.",
            |st, a: SaveArgs| {
                let s = st.doc()?;
                let target = match &a.path {
                    Some(p) => Some(checked_path(p, &["qcad"])?),
                    None => None,
                };
                let path = s.save(target.as_deref()).map_err(err)?;
                Ok(json!({ "saved": path.display().to_string() }))
            },
        ),
        tool(
            "doc_info",
            "The whole document: parameters, sketches (world_frame, contours, degrees of freedom), the timeline, result bodies (volume mm³, bbox), errors and warnings. \
             Bboxes include OCCT tolerance padding in-session (allow about 0.05 mm); after doc_open stored B-reps may report tighter bounds. \
             Use volume for size checks and topology face positions for placement.",
            |st, _: NoArgs| Ok(serde_json::to_value(st.doc()?.info()).unwrap_or(Value::Null)),
        ),
    ]
}
