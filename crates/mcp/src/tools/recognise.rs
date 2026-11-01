use super::common::{err, ObjRef};
use super::{tool, Tool};
use qymcad_engine::{MeshRecogniseMode, Num};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Fit exact analytic and free-form surfaces from the mesh.
    Exact,
    /// Build a faceted B-rep polyhedron from the mesh triangles.
    Polyhedron,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecogniseArgs {
    /// Mesh body id or name. Defaults to the current body.
    #[serde(default)]
    pub body: Option<ObjRef>,
    /// Recognition mode. Exact is the default; polyhedron preserves the faceted mesh.
    #[serde(default = "default_mode")]
    pub mode: Mode,
    /// Exact-surface fitting tolerance in mm, as a number or parameter formula. Range 0.1..=1000.
    #[serde(default)]
    pub tolerance: Option<Num>,
    /// Sharp-edge angle in degrees, as a number or parameter formula. Range 5..=90.
    #[serde(default)]
    pub sharp: Option<Num>,
    /// Mesh simplification deviation in mm, as a number or parameter formula. Range 0..=10.
    #[serde(default = "default_simplify")]
    pub simplify: Num,
    /// Optional name for the recognised feature and body.
    #[serde(default)]
    pub name: Option<String>,
}

fn default_mode() -> Mode {
    Mode::Exact
}
fn default_simplify() -> Num {
    Num::Value(0.0)
}

pub fn tools() -> Vec<Tool> {
    vec![tool(
        "recognise",
        "Recognise an imported mesh body as a closed B-rep solid. Exact mode uses the app's analytic-surface recognition with tolerance and sharp-edge angle; polyhedron mode makes a faceted solid. The mesh is consumed on success, formulas remain editable through param_set and dimension_set, and failed or shell-only recognition is rolled back. Use import_mesh first, then recognise before topology, select, fillet, boolean or STEP tools.",
        |st, a: RecogniseArgs| {
            let s = st.doc()?;
            let body = a.body.map(|r| r.resolve(s)).transpose()?;
            let mode = match a.mode { Mode::Exact => MeshRecogniseMode::Exact, Mode::Polyhedron => MeshRecogniseMode::Polyhedron };
            if matches!(mode, MeshRecogniseMode::Polyhedron) && (a.tolerance.is_some() || a.sharp.is_some()) {
                return Err("tolerance/sharp are refused for polyhedron mode".into());
            }
            let tolerance = a.tolerance.unwrap_or(Num::Value(1.0));
            let sharp = a.sharp.unwrap_or(Num::Value(30.0));
            let result = s.recognise_mesh(body, mode, &tolerance, &sharp, &a.simplify, a.name.as_deref()).map_err(err)?;
            Ok(json!({
                "feature": result.0.feature,
                "body": result.0.body,
                "name": result.0.name,
                "mode": result.0.mode,
                "source_mesh_body": result.0.source_mesh_body,
                "tolerance": result.0.tolerance,
                "sharp": result.0.sharp,
                "simplify": result.0.simplify,
                "tolerance_formula": result.0.tolerance_formula,
                "sharp_formula": result.0.sharp_formula,
                "simplify_formula": result.0.simplify_formula,
                "faces": result.0.faces,
                "solids": result.0.solids,
                "shells": result.0.shells,
                "volume_mm3": result.0.volume,
                "bbox": result.0.bbox,
                "warnings": result.0.warnings,
                "rebuild": super::common::rebuild_json(&result.1),
            }))
        },
    )]
}
