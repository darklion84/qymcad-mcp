//! `sketch_edit` op construction: the app's toggle of sketch curves between ordinary and construction geometry.

use super::edits::EditedSketch;
use crate::error::{Error, Result};
use crate::session::Session;
use qymcad_core::model::Id;
use serde::Serialize;

/// A curve and whether it is construction geometry after the edit.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ConstructionState {
    pub id: Id,
    pub construction: bool,
}

/// What a construction edit did: the usual sketch-edit report and the final state of each named curve.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ConstructionEdit {
    pub edit: EditedSketch,
    pub entities: Vec<ConstructionState>,
}

impl Session {
    /// The app's `toggle_construction` (button, X key): one state for the whole set, construction while any
    /// member is ordinary, ordinary once all are construction. `to` states the state the caller wants; when the
    /// app's rule would give the other state the call is refused unchanged, so a repeated call cannot invert.
    pub fn sketch_construction(&mut self, sketch: Id, entities: &[Id], to: bool) -> Result<ConstructionEdit> {
        if entities.is_empty() {
            return Err(Error::Invalid("entities must be non-empty".into()));
        }
        let si = self.sketch_si(sketch)?;
        self.refuse_pattern_instance_edit(sketch, entities)?;
        for id in entities {
            if self.p.sketches[si].entity(*id).is_some() {
                continue;
            }
            let what = if self.p.sketches[si].texts.iter().any(|t| t.id == *id) {
                "is a sketch text, which the app's construction toggle does not switch (it is set when the text is created)"
            } else if self.p.sketches[si].points.iter().any(|p| p.id == *id) {
                "is a sketch point, not a curve; the app's construction toggle switches curves only"
            } else {
                "is not a curve of this sketch"
            };
            return Err(Error::Invalid(format!("entity {id} {what}")));
        }
        let ordinary_left = entities.iter().any(|id| self.p.sketches[si].entity(*id).is_some_and(|e| !e.construction));
        if ordinary_left != to {
            return Err(Error::Invalid(format!(
                "the app's toggle gives the whole set one state, and these curves would become {}; they are already all {}",
                if ordinary_left { "construction" } else { "ordinary" },
                if to { "construction" } else { "ordinary" }
            )));
        }
        let consumed = crate::dependencies::closure(&self.p, std::collections::HashSet::from([sketch])).len() > 1;
        let before = self.sketch_info(sketch)?.contours;
        let edit = self.sketch_edit_report(sketch, |s| {
            s.p.toggle_construction(si, entities);
            s.complete_sketch_edit(si)?;
            if consumed {
                refuse_changed_regions(sketch, &before, &s.sketch_info(sketch)?.contours)?;
            }
            Ok(Vec::new())
        })?;
        let entities = entities
            .iter()
            .filter_map(|id| self.p.sketches[si].entity(*id).map(|e| ConstructionState { id: *id, construction: e.construction }))
            .collect();
        Ok(ConstructionEdit { edit, entities })
    }
}

/// Contours keep their ids "by position" when the sketch is rebuilt, so a feature holding contour N still builds
/// from contour N, whatever it now is (F-168). A region that survives with another area or other holes is silently
/// a different profile for its consumers: refused. A contour that vanishes needs no check here, the consumer's own
/// rebuild reports "profile not found" and the edit rolls back.
fn refuse_changed_regions(sketch: Id, before: &[super::ContourInfo], after: &[super::ContourInfo]) -> Result<()> {
    let holes = |all: &[super::ContourInfo], id: Id| {
        let mut ids: Vec<Id> = all.iter().filter(|c| c.parent == Some(id)).map(|c| c.id).collect();
        ids.sort_unstable();
        ids
    };
    for old in before {
        let Some(new) = after.iter().find(|c| c.id == old.id) else { continue };
        let same_area = (old.area - new.area).abs() <= 1e-6 * old.area.abs().max(1.0);
        if !same_area || holes(before, old.id) != holes(after, old.id) {
            return Err(Error::Invalid(format!(
                "turning these curves into construction would change contour {} of sketch {sketch} that features built from it hold by id \
                 (area {:.4} -> {:.4} mm2, holes {:?} -> {:?}); the app rebuilds them without any error as a different profile (F-168). \
                 Delete the consumers first, or make the change on a copy of the sketch",
                old.id,
                old.area,
                new.area,
                holes(before, old.id),
                holes(after, old.id)
            )));
        }
    }
    Ok(())
}
