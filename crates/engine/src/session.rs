//! `Session`: one open QymCAD document plus its live B-rep shapes, and the regenerate pipeline.

use crate::error::{Error, Result};
use qymcad_core::model::{Id, Project};
use qymcad_kernel::Shape;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A problem attached to a timeline node after a rebuild.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct NodeIssue {
    pub node: Id,
    pub name: String,
    pub message: String,
}

/// A body that is a current result of the model (not consumed by a later feature).
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct BodyInfo {
    pub id: Id,
    pub name: String,
    /// mm³
    pub volume: f64,
    /// `[xmin, ymin, zmin, xmax, ymax, zmax]` in mm. OCCT bounds include edge tolerances: after booleans they
    /// exceed the exact geometry by up to ~0.01 mm per side (FINDINGS F-016).
    pub bbox: [f64; 6],
}

/// What a rebuild produced.
#[derive(Clone, Debug, Serialize, Default, PartialEq)]
pub struct Rebuild {
    pub errors: Vec<NodeIssue>,
    pub warnings: Vec<NodeIssue>,
    pub bodies: Vec<BodyInfo>,
}

pub struct Session {
    pub(crate) p: Project,
    pub(crate) shapes: HashMap<Id, Shape>,
    path: Option<PathBuf>,
}

impl Session {
    /// A new document with one empty part, ready to model in.
    pub fn new_part() -> Session {
        qymcad_core::model::set_producer(&format!("qymcad-mcp {}", env!("CARGO_PKG_VERSION")));
        let mut p = Project::default();
        p.new_document();
        Session { p, shapes: HashMap::new(), path: None }
    }

    /// Open a `.qcad`, restoring live bodies from the file as the app does (FINDINGS F-007), and rebuilding
    /// whatever has no stored geometry.
    pub fn open(path: &Path) -> Result<(Session, Rebuild)> {
        qymcad_core::model::set_producer(&format!("qymcad-mcp {}", env!("CARGO_PKG_VERSION")));
        let s = path.to_str().ok_or_else(|| Error::Invalid(format!("path is not UTF-8: {}", path.display())))?;
        let qymcad_io::LoadedProject { mut project, breps } =
            qymcad_io::load_project_with_brep(s).map_err(|e| Error::Io(format!("cannot open {s}: {e}")))?;
        project.ensure_document();
        let shapes: HashMap<Id, Shape> = {
            let _gate = qymcad_kernel::kernel_gate();
            breps.into_iter().filter_map(|(id, b)| Shape::from_brep_bytes(&b).map(|sh| (id, sh))).collect()
        };
        let missing = project.timeline.iter().filter_map(|n| n.kind.body()).any(|b| !shapes.contains_key(&b));
        let mut sess = Session { p: project, shapes, path: Some(path.to_path_buf()) };
        sess.p.eval_parameters();
        if missing {
            sess.p.mark_all_dirty();
        }
        let r = sess.rebuild();
        Ok((sess, r))
    }

    /// Save to `path`, or to the path the document was opened from / last saved to.
    pub fn save(&mut self, path: Option<&Path>) -> Result<PathBuf> {
        let target = match path {
            Some(p) => p.to_path_buf(),
            None => self.path.clone().ok_or_else(|| Error::Invalid("the document has no file yet; give a path".into()))?,
        };
        let s = target.to_str().ok_or_else(|| Error::Invalid(format!("path is not UTF-8: {}", target.display())))?;
        let breps: Vec<(Id, Vec<u8>)> = {
            let _gate = qymcad_kernel::kernel_gate();
            self.shapes.iter().filter_map(|(id, sh)| sh.to_brep_bytes().map(|b| (*id, b))).collect()
        };
        qymcad_io::save_project_guarded_with_brep(&self.p, s, &breps).map_err(|e| Error::Io(format!("cannot save {s}: {e}")))?;
        self.path = Some(target.clone());
        Ok(target)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Read access to the underlying QymCAD document (tests, diagnostics).
    pub fn project(&self) -> &Project {
        &self.p
    }

    /// The live B-rep of a body, if built.
    pub fn shape(&self, body: Id) -> Option<&Shape> {
        self.shapes.get(&body)
    }

    /// Rebuild what is dirty. Errors are retried once with the whole timeline dirty: a datum plane created in
    /// the same edit is resolved only during regenerate (FINDINGS F-005), and `retryable()` errors need a pass
    /// after their source exists.
    pub fn rebuild(&mut self) -> Rebuild {
        let shapes = std::mem::take(&mut self.shapes);
        let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut self.p, shapes);
        let mut built = report.built;
        let (errors, shapes) = if report.errors.is_empty() {
            (report.errors, shapes)
        } else {
            self.p.mark_all_dirty();
            let (again, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut self.p, shapes);
            built.extend(again.built);
            (again.errors, shapes)
        };
        self.shapes = shapes;
        // The GUI copies faces into bodies; a headless regenerate does not (FINDINGS F-007).
        for (id, faces) in built {
            self.p.set_body_faces(id, faces);
        }
        Rebuild {
            errors: errors.iter().map(|(id, e)| self.issue(*id, e.to_string())).collect(),
            warnings: self.p.regen_warnings.iter().map(|(id, e)| self.issue(*id, e.to_string())).collect(),
            bodies: self.result_bodies(),
        }
    }

    /// Run `edit`, rebuild, and keep the result only if the nodes it created built cleanly. Otherwise restore
    /// the document as it was and return the errors. Returns the edit's value and the rebuild report.
    pub(crate) fn atomic<T>(&mut self, edit: impl FnOnce(&mut Session) -> Result<T>) -> Result<(T, Rebuild)> {
        let before = self.p.clone();
        let old_nodes: HashSet<Id> = self.p.timeline.iter().map(|n| n.id).collect();
        let value = match edit(self) {
            Ok(v) => v,
            Err(e) => {
                self.p = before;
                return Err(e);
            }
        };
        let r = self.rebuild();
        let new_errors: Vec<String> =
            r.errors.iter().filter(|i| !old_nodes.contains(&i.node)).map(|i| format!("{} ({}): {}", i.name, i.node, i.message)).collect();
        if !new_errors.is_empty() {
            self.p = before;
            let live: HashSet<Id> = self.p.timeline.iter().flat_map(|n| n.kind.bodies()).collect();
            self.shapes.retain(|id, _| live.contains(id));
            return Err(Error::Rebuild(new_errors));
        }
        Ok((value, r))
    }

    /// Run an edit that does not need a rebuild (sketch geometry); on error the document is restored.
    pub(crate) fn transact<T>(&mut self, edit: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
        let before = self.p.clone();
        let r = edit(self);
        if r.is_err() {
            self.p = before;
        }
        r
    }

    /// Bodies that are current results: built, and not consumed by a later feature.
    pub fn result_bodies(&self) -> Vec<BodyInfo> {
        let consumed = self.p.consumed_bodies();
        self.p
            .timeline
            .iter()
            .filter(|n| !n.suppressed)
            .flat_map(|n| n.kind.bodies())
            .filter(|b| !consumed.contains(b))
            .filter_map(|b| {
                let sh = self.shapes.get(&b)?;
                Some(BodyInfo { id: b, name: self.node_name(b), volume: sh.volume(), bbox: sh.bbox().unwrap_or([0.0; 6]) })
            })
            .collect()
    }

    /// The single current body of the active part, which features modify by default.
    pub(crate) fn tip_body(&self) -> Option<Id> {
        let consumed = self.p.consumed_bodies();
        let part = self.p.active_component;
        let tips: Vec<Id> = self
            .p
            .timeline
            .iter()
            .filter(|n| !n.suppressed)
            .flat_map(|n| n.kind.bodies())
            .filter(|b| !consumed.contains(b) && (part.is_none() || self.p.body_owner(*b) == part))
            .collect();
        tips.last().copied()
    }

    pub(crate) fn node_name(&self, id: Id) -> String {
        self.p.timeline.iter().find(|n| n.id == id).map(|n| n.name.clone()).unwrap_or_default()
    }

    pub(crate) fn set_node_name(&mut self, id: Id, name: Option<&str>) {
        if let (Some(name), Some(n)) = (name, self.p.timeline.iter_mut().find(|n| n.id == id)) {
            n.name = name.to_string();
        }
    }

    /// Resolve an object reference: a numeric id, or the name of a timeline node (sketch, plane, feature).
    pub fn resolve(&self, r: &str) -> Result<Id> {
        let r = r.trim();
        if let Ok(id) = r.parse::<Id>() {
            if self.p.timeline.iter().any(|n| n.id == id) || self.p.sketches.iter().any(|s| s.id == id) {
                return Ok(id);
            }
            return Err(Error::NotFound(format!("no object with id {id}")));
        }
        let hits: Vec<Id> = self.p.timeline.iter().filter(|n| n.name == r).map(|n| n.id).collect();
        match hits.as_slice() {
            [id] => Ok(*id),
            [] => Err(Error::NotFound(format!("no object named `{r}`"))),
            _ => Err(Error::Invalid(format!("several objects are named `{r}` ({hits:?}); use the id"))),
        }
    }

    fn issue(&self, node: Id, message: String) -> NodeIssue {
        NodeIssue { node, name: self.node_name(node), message }
    }
}
