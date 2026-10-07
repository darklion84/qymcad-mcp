//! Parameter tools.

use super::common::{err, rebuild_json};
use super::{tool, Tool};
use qymcad_engine::Num;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetArgs {
    /// Parameter name: letters, digits, `_`, starting with a letter. Stored lowercase.
    pub name: String,
    /// A number, or an expression over other parameters (`"w/2"`, `"2*t+1"`).
    pub value: Num,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteArgs {
    /// Name of the parameter to delete (case-insensitive). Refused while an expression still uses it.
    pub name: String,
}

pub fn tools() -> Vec<Tool> {
    vec![
        tool(
            "param_set",
            "Create or change a named parameter and rebuild everything that depends on it. Use parameters for every \
             dimension the user may want to change, then refer to them in expressions. If the model no longer \
             builds, the change is rolled back and the error returned.",
            |st, a: SetArgs| {
                let r = st.doc()?.param_set(&a.name, &a.value).map_err(err)?;
                Ok(json!({ "rebuild": rebuild_json(&r) }))
            },
        ),
        tool("param_delete", "Delete a parameter. Refused while an expression still uses it.", |st, a: DeleteArgs| {
            st.doc()?.param_delete(&a.name).map_err(err)?;
            Ok(json!({ "ok": true }))
        }),
    ]
}
