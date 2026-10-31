use super::common::{checked_path, err, rebuild_json, PlaneArg};
use super::{tool, Tool};
use qymcad_engine::{Num, SketchImportFormat};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SketchImportArgs {
    /// DXF or SVG file path. The server reads only this exact path.
    pub path: String,
    /// Import format: dxf, svg, or auto. Auto uses the file extension.
    #[serde(default)]
    pub format: ImportFormatArg,
    /// Sketch plane: XY, XZ, YZ, or a datum plane reference.
    pub plane: PlaneArg,
    /// Optional sketch name.
    #[serde(default)]
    pub name: Option<String>,
    /// Positive finite scale evaluated once before import. This is a one-time transform, not a driver and cannot be edited with dimension_set.
    #[serde(default = "one")]
    pub scale: Num,
}

#[derive(Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ImportFormatArg {
    Dxf,
    Svg,
    #[default]
    Auto,
}

fn one() -> Num {
    Num::Value(1.0)
}

pub fn tools() -> Vec<Tool> {
    vec![tool("sketch_import", "Import DXF or SVG curves into an editable native sketch. Parsing happens before the atomic project edit. DXF drawing units are taken as mm and $INSUNITS is not applied. SVG uses 25.4/96 mm per px and flips Y to CAD-up. Scale is evaluated once and is a literal one-time transform, not a driver; text is reported as skipped/ignored and never promised.", |st, a: SketchImportArgs| {
        let exts = match a.format { ImportFormatArg::Dxf => &["dxf"][..], ImportFormatArg::Svg => &["svg"][..], ImportFormatArg::Auto => &["dxf", "svg"][..] };
        let path = checked_path(&a.path, exts)?;
        let format = match a.format { ImportFormatArg::Dxf => Some(SketchImportFormat::Dxf), ImportFormatArg::Svg => Some(SketchImportFormat::Svg), ImportFormatArg::Auto => None };
        let s = st.doc()?;
        let plane = a.plane.resolve(s)?;
        let result = s.sketch_import(&path, format, &plane, a.name.as_deref(), &a.scale).map_err(err)?;
        Ok(json!({"sketch":result.sketch,"name":result.name,"format":result.format,"entities":result.entities,"contours":result.contours,"skipped":result.skipped,"invalid":result.invalid,"redrawn":result.redrawn,"scale":result.scale,"unit_note":result.unit_note,"rebuild":rebuild_json(&result.rebuild)}))
    })]
}
