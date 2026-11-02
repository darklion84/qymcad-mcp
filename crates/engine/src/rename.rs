//! Independent native feature, sketch and body label edits.
use crate::{Error, Id, Result, Session};
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub enum RenameTarget {
    Feature(Id),
    Sketch(Id),
    Body(Id),
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RenamedTarget {
    pub kind: &'static str,
    pub id: Id,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RenameResult {
    pub target: RenamedTarget,
    pub previous_name: String,
    pub name: String,
    pub display_name: String,
    pub changed: bool,
    pub duplicates: Vec<Id>,
}

impl Session {
    /// Rename one native label store, without rebuilding geometry.
    pub fn rename(&mut self, target: RenameTarget, raw_name: &str) -> Result<RenameResult> {
        if raw_name.chars().any(char::is_control) {
            return Err(Error::Invalid("object name must not contain Unicode control characters".into()));
        }
        let name = raw_name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("object name must not be empty after trimming".into()));
        }
        Self::check_object_name(name)?;
        // Retain the validated index: the native body setter silently ignores an invalid one.
        let (kind, id, index, previous_name) = match target {
            RenameTarget::Feature(id) => {
                let index =
                    self.p.timeline.iter().position(|n| n.id == id).ok_or_else(|| Error::NotFound(format!("no feature with id {id}")))?;
                ("feature", id, index, self.p.timeline[index].name.clone())
            }
            RenameTarget::Sketch(id) => {
                let index = self.p.sketch_index(id).ok_or_else(|| Error::Invalid(format!("object {id} is not a sketch")))?;
                ("sketch", id, index, self.p.sketches[index].name.clone())
            }
            RenameTarget::Body(id) => {
                let index = self.p.mesh_index(id).ok_or_else(|| Error::Invalid(format!("object {id} is not a body")))?;
                ("body", id, index, self.p.bodies[index].name.clone())
            }
        };
        let changed = previous_name != name;
        let snapshot = if changed && !self.tool_edit_active {
            let mut snapshot = self.begin_tool_edit()?;
            snapshot.record_call("rename", serde_json::json!({"target":{kind:id},"name":raw_name}));
            Some(snapshot)
        } else {
            None
        };
        if changed {
            match target {
                RenameTarget::Feature(id) => {
                    self.p.rename_node(id, name);
                }
                RenameTarget::Sketch(_) => self.p.sketches[index].name = name.into(),
                RenameTarget::Body(_) => self.p.set_mesh_name(index, name.into()),
            }
        }
        let display_name = crate::localization::name(name);
        let mut duplicates = self.name_matches(name);
        duplicates.extend(self.name_matches(&display_name));
        duplicates.retain(|other| *other != id);
        duplicates.sort_unstable();
        duplicates.dedup();
        if let Some(snapshot) = snapshot {
            self.finish_tool_edit(snapshot, true);
        }
        Ok(RenameResult { target: RenamedTarget { kind, id }, previous_name, name: name.into(), display_name, changed, duplicates })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BaseName, Direction, Extrude, Op, PlaneRef};
    use qymcad_core::errors::CoreError;

    #[test]
    fn metadata_rename_preserves_dirty_errors_warnings_and_live_breps() {
        let mut s = Session::new_part();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
        s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
        let (body, report) = s
            .extrude(&Extrude {
                sketch: sk,
                profiles: None,
                profiles_at: None,
                height: 5.0.into(),
                down: None,
                op: Op::NewBody,
                direction: Direction::Normal,
                through: false,
                target: None,
                name: Some("plate".into()),
            })
            .unwrap();
        assert!(report.errors.is_empty());
        assert!((report.bodies[0].volume - 10.0 * 6.0 * 5.0).abs() < 1e-6); // Rectangle prism = 300 mm³.
        s.p.timeline.iter_mut().find(|n| n.id == body).unwrap().dirty = true;
        s.p.regen_errors.insert(body, CoreError::Kernel("prior failure".into()));
        s.p.regen_warnings.insert(body, CoreError::EdgesDropped { asked: 2, dropped: 1 });
        let brep = |s: &Session| {
            let _gate = qymcad_kernel::kernel_gate();
            s.shapes[&body].to_brep_bytes().unwrap()
        };
        let before_brep = brep(&s);
        let mut expected = s.p.clone();
        expected.timeline.iter_mut().find(|n| n.id == body).unwrap().name = "renamed".into();
        let result = s.rename(RenameTarget::Feature(body), "renamed").unwrap();
        assert_eq!(result.name, "renamed");
        assert!(result.changed);
        assert_eq!(ron::to_string(&s.p).unwrap(), ron::to_string(&expected).unwrap());
        assert_eq!(brep(&s), before_brep);
        assert_eq!(s.p.regen_errors.len(), 1);
        assert_eq!(s.p.regen_warnings.len(), 1);
        assert!(s.p.timeline.iter().find(|n| n.id == body).unwrap().dirty);
    }
}
