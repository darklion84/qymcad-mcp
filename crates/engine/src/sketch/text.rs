//! Native text glyph baking with fixed numeric geometry and an explicit, persisted font.
use super::purpose;
use crate::error::{Error, Result};
use crate::session::Session;
use qymcad_core::geom::Point2;
use qymcad_core::model::{FontRef, Id};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A deterministic font file and face; family substitution is never performed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextFont {
    /// Absolute or relative font file path, retained in the native document.
    pub path: String,
    /// Face within the file or collection (default 0).
    #[serde(default)]
    pub index: u32,
}

/// Fixed text geometry. Recreate the text to change height; upstream contour identities are unstable.
#[derive(Clone, Debug)]
pub struct TextSpec {
    /// Label characters, all of which must be supported by the font.
    pub text: String,
    /// Baseline anchor in sketch coordinates, mm.
    pub at: [f64; 2],
    /// Positive em height, mm; a literal value rather than a formula.
    pub height: f64,
    /// Rotation about the baseline anchor, degrees.
    pub angle: f64,
    /// Font file and face; omitted selects Supplemental Arial.ttf face 0.
    pub font: Option<TextFont>,
    /// Construction text is visible in the sketch but contributes no profile contours.
    pub construction: bool,
}

/// A native sketch text record and its baked contour count.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TextInfo {
    /// Native text id, accepted by sketch_remove's entity argument.
    pub id: Id,
    /// Original label.
    pub text: String,
    /// Recorded file and face, needed when retyping in the app.
    pub font: TextFont,
    /// Resolved font family recorded at creation.
    pub family: String,
    /// Em height in mm, rather than visible capital height.
    pub height: f64,
    /// Baseline anchor in sketch coordinates, mm.
    pub at: [f64; 2],
    /// Rotation about the baseline anchor, degrees.
    pub angle: f64,
    /// Construction text does not contribute profiles.
    pub construction: bool,
    /// Number of saved closed glyph loops, including counters.
    pub contours: usize,
}

impl Session {
    /// Bake and add a native text object. Geometry is fixed because native height edits can swap profiles.
    pub fn sketch_text(&mut self, sketch: Id, a: &TextSpec) -> Result<Id> {
        self.transaction(|s| {
            let si = s.sketch_si(sketch)?;
            if !a.at.iter().chain([&a.height, &a.angle]).all(|v| v.is_finite()) {
                return Err(Error::Invalid("text at, height and angle must be finite numbers".into()));
            }
            if a.height <= 0.0 {
                return Err(Error::Invalid("text height must be positive".into()));
            }
            let font = a.font.clone().unwrap_or_else(|| TextFont { path: "/System/Library/Fonts/Supplemental/Arial.ttf".into(), index: 0 });
            let bytes = std::fs::read(&font.path).map_err(|e| Error::Invalid(format!("cannot read text font {}: {e}", font.path)))?;
            let family = qymcad_core::text::family_name(&bytes, font.index)
                .ok_or_else(|| Error::Invalid(format!("invalid text font {} face {}", font.path, font.index)))?;
            let mut missing = Vec::new();
            for ch in a.text.chars().filter(|c| !c.is_whitespace()) {
                if !qymcad_core::text::can_write(&bytes, font.index, &ch.to_string()) && !missing.contains(&ch) {
                    missing.push(ch);
                }
            }
            if !missing.is_empty() {
                return Err(Error::Invalid(format!(
                    "text font {} cannot write characters: {}",
                    font.path,
                    missing.iter().collect::<String>()
                )));
            }
            let (sn, cs) = a.angle.to_radians().sin_cos();
            let glyphs: Vec<Vec<Point2>> =
                qymcad_core::text::text_outline_contours(&bytes, font.index, &a.text, a.height, a.at[0], a.at[1])
                    .into_iter()
                    .map(|c| {
                        c.points
                            .into_iter()
                            .map(|p| {
                                Point2::new(
                                    a.at[0] + (p.x - a.at[0]) * cs - (p.y - a.at[1]) * sn,
                                    a.at[1] + (p.x - a.at[0]) * sn + (p.y - a.at[1]) * cs,
                                )
                            })
                            .collect()
                    })
                    .collect();
            if glyphs.is_empty() {
                return Err(Error::Invalid("text produces no glyph outline; give visible supported characters".into()));
            }
            let id = s.p.add_sketch_text(
                si,
                qymcad_core::model::TextSpec {
                    at: Point2::new(a.at[0], a.at[1]),
                    height: a.height,
                    angle: a.angle,
                    text: a.text.clone(),
                    glyphs,
                    font: FontRef { family, path: font.path, index: font.index },
                },
                purpose(a.construction),
            );
            s.finish_sketch_edit(si)?;
            Ok(id)
        })
    }
}
