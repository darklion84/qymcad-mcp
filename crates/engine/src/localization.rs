//! English presentation from QymCAD's pinned Fluent catalogue; stored document keys remain unchanged.
use qymcad_core::errors::CoreError;

pub(crate) fn name(stored: &str) -> String {
    // Upstream language selection is thread-local, so each calling thread must choose server English.
    qymcad_i18n::set_language("en");
    qymcad_i18n::name(stored)
}

pub(crate) fn error(error: &CoreError) -> String {
    let native = error.to_string();
    // Keep already-readable native diagnostics (including I2's kernel chamfer reason). Translate keys
    // through the upstream formatter, which also supplies substitutions and resolves nested bridge keys.
    let mut message = if native == error.key() || matches!(error, CoreError::Kernel(_) | CoreError::RemoveFacesFailed { .. }) {
        qymcad_i18n::set_language("en");
        qymcad_i18n::error_words::error_text(error)
    } else {
        native
    };
    if matches!(error, CoreError::CutRemovedNothing) {
        message.push_str("; the cut does not reach the body; check direction (reverse) or height");
    }
    message
}
