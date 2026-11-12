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
    /// Permit an empty document to overwrite an existing .qcad containing bodies. Default false.
    #[serde(default)]
    pub allow_empty: bool,
    /// Permit a pathless save to replace the last file after the saved model's first solid feature was removed or replaced. Default false; give an explicit path instead to choose the destination.
    #[serde(default)]
    pub overwrite: bool,
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
            "Open a .qcad file written by QymCAD of the same release (or by this server). Rebuilds what has no stored geometry. Both doc.bodies and rebuild.bodies report the geometry after opening and rebuilding; stored-file metrics appear only in stored-versus-rebuilt mismatch warnings. doc.assembly and rebuild.assembly report native mechanism health separately from feature errors: joint faults, solve errors, violations, conflict, clamped (drives a limit held while opening) and limit_faults (limits the app accepts and the server will not build on; edits that do not touch the joint still go on). Existing unsupported records are preserved and reported. Stored forward-timeline projections are refused. Stored cross-part projections are accepted with a rebuild warning per projection identifying sketch, source body and owner parts: projected in the source part's local coordinates; its placement is ignored, as in the QymCAD app. sketch_info marks their entities cross_part:true.",
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
            "Save the document as .qcad; the directory must exist. An explicit path permits replacing an existing model; the result reports replaced=true when the target existed before saving (false for a fresh path). A pathless save requires the first solid feature of the last loaded/saved model to remain; after undo/delete replaces that model, give a path or overwrite=true. Normal edits keep the association. doc_new needs an explicit path. Refuses to overwrite a file containing bodies when this document has no bodies, unless allow_empty=true; that explicit empty replacement also resets the association. Open it in the QymCAD app with File > Open; double-click does not work on macOS. When the document holds a joint anchored on an edge or vertex (a Revolute hinge) the result adds a note: QymCAD.app reports such an anchor as lost after opening until Edit > Rebuild all (F-166).",
            |st, a: SaveArgs| {
                let s = st.doc()?;
                let target = match &a.path {
                    Some(p) => Some(checked_path(p, &["qcad"])?),
                    None => None,
                };
                let replaced = target.as_deref().or_else(|| s.path()).map(|p| p.try_exists()).transpose()
                    .map_err(|e| format!("cannot inspect save target: {e}"))?.unwrap_or(false);
                let path = s.save_with_overwrite(target.as_deref(), a.allow_empty, a.overwrite).map_err(err)?;
                let mut out = json!({ "saved": path.display().to_string(), "replaced": replaced });
                if let Some(note) = s.edge_anchor_note() {
                    out["note"] = json!(note);
                }
                Ok(out)
            },
        ),
        tool(
            "doc_info",
            "The whole document: components, active_component, assembly health, parameters, sketches (owner_part, world_frame, contours, degrees of freedom), the timeline (owner_part), rollback{position,after,next_feature,active_nodes,total_nodes}, result bodies (owner_part, world_transform, volume_mm3 at native floating-point precision, bbox_local, bbox_world, saved visible flag, color_rgb and manual/palette color_source), errors and warnings. Legacy bbox keeps its existing frame, identified by bbox_frame: B-rep part_local, mesh world. New consumers should use bbox_local or bbox_world explicitly. assembly reports joint faults, solve errors, violations and conflict separately from feature errors. \
             Body name retains the existing feature/piece/component label; body_name is the localised native body label and can be renamed independently. \
             JSON bbox and sketch world_frame coordinates/directions use 4 decimal places for presentation (internal geometry, parameters, and input-ready dimensions retain full precision), rounded half away from zero; negative zero is normalized to zero. Bboxes use fresh tessellation with 0.005 mm nominal deflection (larger bodies over 500 mm diagonal use 1e-5 of the diagonal) plus f32 coordinate rounding, including after doc_open and rebuild; they approximate extrema and may slightly under-bound curves or over-bound after decimal rounding (native padded bounds are a fallback if meshing fails). \
             Use volume_mm3 for size checks and topology face positions for placement.",
            |st, _: NoArgs| Ok(serde_json::to_value(st.doc()?.info()).unwrap_or(Value::Null)),
        ),
    ]
}
