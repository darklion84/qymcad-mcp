//! Native body visibility and lineage colour: metadata only, like rename (ADR 0042/0044).
use crate::{Error, Id, Result, Session};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct AppearanceBody {
    pub id: Id,
    pub name: String,
    pub body_name: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct AppearanceScope {
    pub visibility: &'static str,
    pub color: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct AppearanceResult {
    pub body: AppearanceBody,
    pub lineage_root: Id,
    pub visible: bool,
    pub color_rgb: [u8; 3],
    pub color_source: &'static str,
    pub scope: AppearanceScope,
    pub changed: bool,
}

impl Session {
    /// Edit one current body's appearance without dirtying or rebuilding geometry.
    /// `color`: None leaves it unchanged, Some(None) resets, Some(Some(rgb)) sets manual RGB.
    pub fn appearance(&mut self, body: Id, visible: Option<bool>, color: Option<Option<[u8; 3]>>) -> Result<AppearanceResult> {
        if visible.is_none() && color.is_none() {
            return Err(Error::Invalid("appearance requires visible or color".into()));
        }
        self.source_body_or_mesh(Some(body))?;
        // Both native colour setters silently ignore a bad index; validate before any write.
        let index = self.p.mesh_index(body).ok_or_else(|| Error::Invalid(format!("object {body} is not a body")))?;
        let root = self.p.lineage_root(body);
        let changed = visible.is_some_and(|v| self.p.bodies[index].visible != v)
            || color.is_some_and(|c| self.p.part_colors.get(&root).copied() != c);
        let snapshot = if changed && !self.tool_edit_active {
            let mut snapshot = self.begin_tool_edit()?;
            let mut args = serde_json::json!({"body":body});
            if let Some(v) = visible {
                args["visible"] = v.into();
            }
            if let Some(c) = color {
                args["color"] = serde_json::json!(c);
            }
            snapshot.record_call("appearance", args);
            Some(snapshot)
        } else {
            None
        };
        if changed {
            if let Some(v) = visible {
                self.p.bodies[index].visible = v;
            }
            match color {
                Some(Some(rgb)) => self.p.set_mesh_color(index, rgb),
                Some(None) => self.p.reset_mesh_color(index),
                None => {}
            }
        }
        let result = AppearanceResult {
            body: AppearanceBody { id: body, name: self.body_name(body), body_name: self.native_body_name(body) },
            lineage_root: root,
            visible: self.p.bodies[index].visible,
            color_rgb: self.p.mesh_color(index),
            color_source: self.body_color_source(body),
            scope: AppearanceScope { visibility: "body", color: "lineage" },
            changed,
        };
        if let Some(snapshot) = snapshot {
            self.finish_tool_edit(snapshot, true);
        }
        Ok(result)
    }

    pub(crate) fn body_color_source(&self, body: Id) -> &'static str {
        if self.p.part_colors.contains_key(&self.p.lineage_root(body)) {
            "manual"
        } else {
            "palette"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BaseName, PlaneRef};
    use qymcad_core::errors::CoreError;

    #[test]
    fn metadata_preserves_dirty_diagnostics_and_exact_undo_depth() {
        let mut s = Session::new_part();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        assert!(s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &20.0.into(), false).is_ok());
        let (body, report) = s.extrude(&serde_json::from_value(serde_json::json!({"sketch":sk,"height":4})).unwrap()).unwrap();
        assert!(report.errors.is_empty());
        assert!((report.bodies[0].volume - 10.0 * 20.0 * 4.0).abs() < 1e-6); // Prism = 800 mm³.
        s.p.timeline.iter_mut().find(|n| n.id == body).unwrap().dirty = true;
        s.p.regen_errors.insert(body, CoreError::Kernel("prior failure".into()));
        s.p.regen_warnings.insert(body, CoreError::EdgesDropped { asked: 2, dropped: 1 });
        let brep = |s: &Session| {
            let _gate = qymcad_kernel::kernel_gate();
            s.shapes[&body].to_brep_bytes().unwrap()
        };
        let before_brep = brep(&s);
        let before = ron::to_string(&s.p).unwrap();
        let depth = s.undo.len();
        let before_issues = (s.info().errors, s.info().warnings);
        for (id, v, c) in [(body, None, None), (sk, Some(false), None), (u64::MAX, None, Some(None))] {
            let info = s.info();
            assert!(s.appearance(id, v, c).is_err());
            assert_eq!(s.info(), info);
            assert_eq!(ron::to_string(&s.p).unwrap(), before);
            assert_eq!(s.undo.len(), depth);
            assert_eq!(brep(&s), before_brep);
        }
        let mut expected = s.p.clone();
        let i = expected.mesh_index(body).unwrap();
        expected.bodies[i].visible = false;
        expected.part_colors.insert(body, [17, 34, 51]);
        let set = s.appearance(body, Some(false), Some(Some([17, 34, 51]))).unwrap();
        assert_eq!((set.visible, set.color_rgb, set.color_source, set.changed), (false, [17, 34, 51], "manual", true));
        assert_eq!(ron::to_string(&s.p).unwrap(), ron::to_string(&expected).unwrap());
        assert_eq!(brep(&s), before_brep);
        assert_eq!(s.undo.len(), depth + 1);
        assert_eq!((s.info().errors, s.info().warnings), before_issues);
        assert!(s.p.timeline.iter().find(|n| n.id == body).unwrap().dirty);
        assert!(!s.appearance(body, Some(false), Some(Some([17, 34, 51]))).unwrap().changed);
        assert_eq!(s.undo.len(), depth + 1);
        assert_eq!(s.undo().unwrap().call.unwrap().tool, "appearance");
        assert_eq!(s.undo.len(), depth);
        assert_eq!(ron::to_string(&s.p).unwrap(), before);
        assert_eq!(brep(&s), before_brep);
    }
}
