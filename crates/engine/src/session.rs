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
        restore_faces(&mut project);
        let shapes: HashMap<Id, Shape> = {
            let _gate = qymcad_kernel::kernel_gate();
            breps.into_iter().filter_map(|(id, b)| Shape::from_brep_bytes(&b).map(|sh| (id, sh))).collect()
        };
        let missing = project.timeline.iter().filter_map(|n| n.kind.body()).any(|b| !shapes.contains_key(&b));
        let mut sess = Session { p: project, shapes, path: Some(path.to_path_buf()) };
        // Clean documents remain usable even without named edges, but rebuilding with an empty edge pool
        // can silently round every edge of a stored query (F-024).
        if let Err(e) = sess.restore_edges() {
            if missing || sess.p.timeline.iter().any(|n| n.dirty) {
                return Err(Error::Invalid(format!("cannot open safely: {e}; the document requires a rebuild")));
            }
        }
        sess.p.eval_parameters();
        let sketches: Vec<Id> = sess.p.sketches.iter().map(|s| s.id).collect();
        for sid in sketches {
            sess.track_datum_dependencies(sid);
        }
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
        self.rebuild_retrying(None)
    }

    /// `rebuild`, with the retry pass limited to the nodes in `only` when given. An edit retries only the nodes
    /// it created: rebuilding the whole timeline would replace every old body's shape with a fresh rebuild, which
    /// need not be bit-identical — a refused edit must retain the exact old representation (F-034).
    /// F-005 still holds: a datum created by the edit is one of its nodes.
    pub(crate) fn rebuild_retrying(&mut self, only: Option<&HashSet<Id>>) -> Rebuild {
        let blocked = self.edge_query_rebuild_errors(&self.p);
        if !blocked.is_empty() {
            return Rebuild { errors: blocked, bodies: self.result_bodies(), ..Default::default() };
        }
        // A safe first pass may fail on an unrelated node, expanding the retry into a blocked query.
        // Preserve original handles only in that case, so refusing the retry also undoes the first pass.
        let saved = if only.is_none() && !self.p.regen_plan().nodes.is_empty() {
            let mut retry = self.p.clone();
            retry.mark_all_dirty();
            if self.edge_query_rebuild_errors(&retry).is_empty() {
                None
            } else {
                let copies: std::result::Result<HashMap<Id, Shape>, Id> = {
                    let _gate = qymcad_kernel::kernel_gate();
                    self.shapes
                        .iter()
                        .map(|(&id, sh)| sh.to_brep_bytes().and_then(|b| Shape::from_brep_bytes(&b)).map(|copy| (id, copy)).ok_or(id))
                        .collect()
                };
                match copies {
                    Ok(copies) => Some((self.p.clone(), std::mem::replace(&mut self.shapes, copies))),
                    Err(body) => {
                        return Rebuild {
                            errors: vec![self.issue(body, format!("cannot snapshot body {body} before a guarded rebuild"))],
                            bodies: self.result_bodies(),
                            ..Default::default()
                        }
                    }
                }
            }
        } else {
            None
        };
        let shapes = std::mem::take(&mut self.shapes);
        let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut self.p, shapes);
        self.shapes = shapes;
        let mut built = report.built;
        let mut errors: Vec<NodeIssue> = report.errors.iter().map(|(id, e)| self.issue(*id, e.to_string())).collect();
        if !errors.is_empty() {
            // A full retry can reach queries absent from the first plan. Check that plan before changing
            // dirty flags or handing shapes to the kernel; callers retain their existing rollback paths.
            let mut retry = self.p.clone();
            match only {
                None => retry.mark_all_dirty(),
                Some(nodes) => retry.timeline.iter_mut().filter(|n| nodes.contains(&n.id)).for_each(|n| n.dirty = true),
            }
            let blocked = self.edge_query_rebuild_errors(&retry);
            if blocked.is_empty() {
                self.p = retry;
                let shapes = std::mem::take(&mut self.shapes);
                let (again, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut self.p, shapes);
                self.shapes = shapes;
                built.extend(again.built);
                errors = again.errors.iter().map(|(id, e)| self.issue(*id, e.to_string())).collect();
            } else {
                errors.extend(blocked);
                if let Some((project, shapes)) = saved {
                    self.p = project;
                    self.shapes = shapes;
                    return Rebuild { errors, bodies: self.result_bodies(), ..Default::default() };
                }
            }
        }
        // The GUI copies faces into bodies; a headless regenerate does not (FINDINGS F-007).
        for (id, faces) in built {
            self.p.set_body_faces(id, faces);
        }
        Rebuild {
            errors,
            warnings: self.p.regen_warnings.iter().map(|(id, e)| self.issue(*id, e.to_string())).collect(),
            bodies: self.result_bodies(),
        }
    }

    /// Reject only planned stored edge queries, including transitive dependents of an unrestorable body.
    /// Pick lists use live kernel edges; queries require the restored pool (F-023/F-024).
    fn edge_query_rebuild_errors(&self, p: &Project) -> Vec<NodeIssue> {
        use qymcad_core::feature::FeatureKind;
        let mut planned: HashSet<Id> = p.regen_plan().nodes.into_iter().collect();
        // regen_plan omits dirty sketch outputs, while regenerate inserts the sketch into its dirty set
        // (regen.rs:1159). Include consumers that will become dirty during that pass.
        for node in &p.timeline {
            if planned.contains(&node.id) {
                if let FeatureKind::Sketch { sketch } = node.kind {
                    planned.extend(p.dependents(sketch));
                }
            }
        }
        let queries: Vec<Id> = p
            .timeline
            .iter()
            .take(p.rollback.unwrap_or(usize::MAX))
            .filter(|n| !n.suppressed && planned.contains(&n.id))
            .filter(|n| matches!(&n.kind, FeatureKind::Fillet { edges, .. } | FeatureKind::Chamfer { edges, .. } if !edges.query.is_pick_list()))
            .map(|n| n.id)
            .collect();
        if queries.is_empty() {
            return Vec::new();
        }
        let _gate = qymcad_kernel::kernel_gate();
        let mut errors = Vec::new();
        for (&body, shape) in &self.shapes {
            if p.regen_edges.get(&body).is_some_and(|edges| !edges.is_empty()) {
                continue;
            }
            let edges = shape.edges_info();
            // Match Kernel::edges' usable-name filter exactly (kernel.rs:386-399).
            if edges.is_empty() || edges.iter().any(|e| e.id != 0 && e.poly.len() >= 2) {
                continue;
            }
            let dependents = p.dependents(body);
            for &node in queries.iter().filter(|node| dependents.contains(node)) {
                errors.push(self.issue(
                    node,
                    format!(
                        "body {body} `{}` has live edges but no usable named edges; refusing a stored edge query rebuild (F-024)",
                        self.node_name(body)
                    ),
                ));
            }
        }
        errors
    }

    /// Run `edit`, rebuild, and keep the result only if the nodes it created built cleanly. Otherwise restore
    /// the document as it was and return the errors. Pending edits are rebuilt before the snapshot so the
    /// project and its live shapes describe the same baseline (F-034). Returns the edit's value and report.
    pub(crate) fn atomic<T>(&mut self, edit: impl FnOnce(&mut Session) -> Result<T>) -> Result<(T, Rebuild)> {
        if self.p.timeline.iter().any(|n| n.dirty) {
            let baseline = self.rebuild();
            if !baseline.errors.is_empty() {
                return Err(Error::Rebuild(
                    baseline.errors.iter().map(|i| format!("baseline: {} ({}): {}", i.name, i.node, i.message)).collect(),
                ));
            }
        }
        let before = self.p.clone();
        let old_nodes: HashSet<Id> = self.p.timeline.iter().map(|n| n.id).collect();
        let value = match edit(self) {
            Ok(v) => v,
            Err(e) => {
                self.p = before;
                return Err(e);
            }
        };
        let new_nodes: HashSet<Id> = self.p.timeline.iter().map(|n| n.id).filter(|id| !old_nodes.contains(id)).collect();
        let r = self.rebuild_retrying(Some(&new_nodes));
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

    /// Run edits that do not need a rebuild (sketch geometry) as one unit: on error the document is restored.
    pub fn transaction<T>(&mut self, edit: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
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

/// Like QymCAD.app's `finish_project_load`: the B-rep faces stored in the bodies go back into `regen_faces`, so
/// face references resolve by id without a rebuild. Edges are NOT restored by the app either (F-023).
impl Session {
    /// Fill `regen_edges` for live bodies that lack them, from their B-reps, through the same `Kernel::edges` the
    /// regenerate post pass uses (F-023). Without it a stored edge query resolves against an empty pool and rounds
    /// every edge (F-024), on the first rebuild after opening.
    pub(crate) fn restore_edges(&mut self) -> Result<()> {
        use qymcad_core::feature::Kernel;
        let need: Vec<Id> = self.shapes.keys().copied().filter(|b| !self.p.regen_edges.contains_key(b)).collect();
        if need.is_empty() {
            return Ok(());
        }
        let _gate = qymcad_kernel::kernel_gate();
        let kernel = qymcad_kernel::OcctKernel { shapes: std::cell::RefCell::new(std::mem::take(&mut self.shapes)), ..Default::default() };
        let mut error = None;
        for b in need {
            let edges = kernel.edges(b);
            if edges.is_empty() && kernel.shapes.borrow().get(&b).is_some_and(|sh| !sh.edges_info().is_empty()) {
                error = Some(Error::Invalid(format!(
                    "body {b} has live edges but no usable named edges; refusing a full topology rebuild (F-024)"
                )));
                // Keep restoring other bodies: an unrelated named query must still receive its real pool.
                continue;
            }
            if !edges.is_empty() {
                self.p.regen_edges.insert(b, edges);
            }
        }
        self.shapes = kernel.shapes.into_inner();
        match error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

pub(crate) fn restore_faces(p: &mut Project) {
    for i in 0..p.bodies.len() {
        if let (Some(body), false) = (p.mesh_id(i), p.bodies[i].faces.is_empty()) {
            let faces = p.bodies[i].faces.clone();
            p.regen_faces.insert(body, faces);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_unnamed_edges_refuse_topology_restoration() {
        let mut s = Session::new_part();
        let sh = {
            let _gate = qymcad_kernel::kernel_gate();
            let sh = Shape::cylinder(2.0, 3.0).unwrap();
            let edges = sh.edges_info();
            assert!(!edges.is_empty(), "the cylinder has live edges");
            sh.rename_edges(&edges.iter().map(|e| (e.id, 0)).collect::<Vec<_>>());
            assert!(!sh.edges_info().is_empty());
            sh
        };
        s.shapes.insert(42, sh);
        let e = s.ensure_topology().unwrap_err();
        assert!(e.to_string().contains("body 42 has live edges but no usable named edges"), "{e}");
        assert!(s.p.regen_edges.is_empty());
        assert!(s.shapes.contains_key(&42), "error preserves original live shape");
        // A cylinder r=2, h=3 has volume pi*r²*h = 12*pi.
        let _gate = qymcad_kernel::kernel_gate();
        assert!((s.shapes[&42].volume() - 12.0 * std::f64::consts::PI).abs() < 1e-8);
    }
}
