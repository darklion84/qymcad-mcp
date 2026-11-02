//! Body metadata uses the same history/no-op boundary as rename.
use super::{
    common::{err, ObjRef},
    tool, Tool,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};
use serde_json::json;

// Serde's ordinary Option merges omitted and explicit null. Here null means native reset.
fn color<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<[u8; 3]>>, D::Error> {
    Option::<[u8; 3]>::deserialize(d).map(Some)
}
fn visible<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    bool::deserialize(d).map(Some)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AppearanceArgs {
    /// Current, unconsumed body id/name. Sketches and datums are refused: the pinned app has no persisted sketch visibility.
    body: ObjRef,
    /// Set this body's own saved visibility flag. Omitted leaves it unchanged; null is refused.
    #[serde(default, deserialize_with = "visible", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    visible: Option<bool>,
    /// Omitted leaves colour unchanged; [r,g,b] strictly integer bytes 0..=255 sets manual lineage colour; explicit null resets to the native palette. Imported face/triangle colours are kept.
    #[serde(default, deserialize_with = "color", skip_serializing_if = "Option::is_none")]
    color: Option<Option<[u8; 3]>>,
}

pub fn tools() -> Vec<Tool> {
    vec![tool("appearance", "Set a current body's saved visibility and/or lineage colour without dirtying nodes or rebuilding geometry. Requires visible or color. Visibility belongs to this body id; colour follows its lineage root. color omitted = unchanged, [r,g,b] integer bytes 0..=255 = manual, null = native reset. Returns body {id,name,body_name}, lineage_root, visible, color_rgb, color_source (manual if the root has a map entry, even for palette-equal RGB; otherwise palette), scope {visibility:body,color:lineage}, changed. A no-op adds no undo entry. Undoable and allowed in batch. Consumed bodies, sketches, datums and unresolved/ambiguous refs are refused. The pinned app has no persisted sketch visibility. Render and export still show/write every current result body regardless of visible, colourless; colour and opt-in visibility filtering are pending E4.", |st, a: AppearanceArgs| {
        let s = st.doc()?;
        let body = a.body.resolve(s)?;
        let result = s.appearance(body, a.visible, a.color).map_err(err)?;
        st.skip_history = !result.changed;
        Ok(json!(result))
    })]
}
