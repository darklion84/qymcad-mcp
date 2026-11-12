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
        let before = self.sketch_info(sketch)?.contours;
        let edit = self.sketch_edit_report(sketch, |s| {
            s.p.toggle_construction(si, entities);
            s.complete_sketch_edit(si)?;
            s.refuse_dropped_profiles(sketch, &before, &s.sketch_info(sketch)?.contours)?;
            Ok(Vec::new())
        })?;
        let entities = entities
            .iter()
            .filter_map(|id| self.p.sketches[si].entity(*id).map(|e| ConstructionState { id: *id, construction: e.construction }))
            .collect();
        Ok(ConstructionEdit { edit, entities })
    }
}

/// The contour ids a feature holds as its profiles (the contours it was built from).
fn held_contours(kind: &qymcad_core::feature::FeatureKind) -> Vec<Id> {
    use qymcad_core::feature::FeatureKind as K;
    match kind {
        K::Extrude { profiles, .. } | K::Combine { profiles, .. } | K::Revolve { profiles, .. } | K::Sweep { profiles, .. } => {
            profiles.clone()
        }
        K::Loft { contours, .. } => contours.clone(),
        _ => Vec::new(),
    }
}

impl Session {
    /// Features hold contours by id, and the app finds a contour again after an edit by its boundary entities, then by
    /// place and size (`replace_contours`), so changing the shape of a held contour (a hole turned construction) is a
    /// legal edit that the consumer follows. A held contour that is GONE is different: a feature with one profile
    /// fails to rebuild ("profile was not found") and the edit rolls back, but a feature with several profiles builds
    /// from the rest and the missing island silently drops out (F-168). That case is refused.
    fn refuse_dropped_profiles(&self, sketch: Id, before: &[super::ContourInfo], after: &[super::ContourInfo]) -> Result<()> {
        let gone = |id: &Id| before.iter().any(|c| c.id == *id) && !after.iter().any(|c| c.id == *id);
        for node in &self.p.timeline {
            let held = held_contours(&node.kind);
            let dropped: Vec<Id> = held.iter().copied().filter(gone).collect();
            if !dropped.is_empty() && dropped.len() < held.len() {
                return Err(Error::Invalid(format!(
                    "turning these curves into construction would remove contour {dropped:?} of sketch {sketch}, which {} ({}) was built from along with {:?}; \
                     the app would rebuild it from the remaining contours without any error, so the profile silently drops out (F-168). \
                     Delete or rebuild the feature first, or make the change on a copy of the sketch",
                    self.node_name(node.id),
                    node.id,
                    held.iter().filter(|c| !dropped.contains(c)).collect::<Vec<_>>()
                )));
            }
        }
        Ok(())
    }
}
