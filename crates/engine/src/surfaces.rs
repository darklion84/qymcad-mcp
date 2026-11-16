//! Surface tools: the app's FaceCopy and OffsetSurface nodes (ADR 0060), SurfaceReplace and Stitch (ADR 0061), Trim (ADR 0062),
//! Patch (ADR 0063).
//! FaceCopy and OffsetSurface take faces of a body (solid or sheet) and make a NEW sheet body beside it; the source is read,
//! not consumed. Several faces make ONE sheet body. SurfaceReplace swaps faces of a solid for a sheet that closes them and
//! Stitch joins sheets into one body; both consume their inputs. Faces are stored as `Ref::picks` of the resolved ids, the
//! way the app's own edit mode stores them (`apply/update.rs`).
use crate::error::{Error, Result};
use crate::localization;
use crate::modifiers::dim;
use crate::session::{Rebuild, Session};
use crate::topology::{Element, Sel};
use crate::value::Num;
use qymcad_core::errors::CoreError;
use qymcad_core::feature::FeatureKind;
use qymcad_core::geom::MeshEdge;
use qymcad_core::model::Id;
use qymcad_core::refs::Ref;

/// The app's Offset surface field range (`start_face_copy_cmd`: `CmdParam::new("f-distance", "dist", 5.0, -10000.0, 10000.0)`); the kernel has
/// no limit of its own. Zero is allowed: a copy in place.
pub(crate) const OFFSET_LIMIT: f64 = 10_000.0;

/// The app's Stitch tolerance field (`start_stitch_cmd`: `CmdParam::new("f-stitch-tol", "tol", 0.01, 1e-6, 10.0)`): the default and
/// the range. The kernel has no limit of its own.
pub const STITCH_TOLERANCE_DEFAULT: f64 = 0.01;
const STITCH_TOLERANCE_RANGE: (f64, f64) = (1e-6, 10.0);

fn stitch_tolerance(v: f64) -> Result<f64> {
    let (lo, hi) = STITCH_TOLERANCE_RANGE;
    if !(v.is_finite() && (lo..=hi).contains(&v)) {
        return Err(Error::Invalid(format!("stitch tolerance must be finite and within 1e-6..10 mm (the app's range), got {v}")));
    }
    Ok(v)
}

fn offset_distance(v: f64) -> Result<f64> {
    if !(v.is_finite() && v.abs() <= OFFSET_LIMIT) {
        return Err(Error::Invalid(format!(
            "offset distance must be finite and within -{OFFSET_LIMIT}..{OFFSET_LIMIT} mm (the app's range), got {v}"
        )));
    }
    Ok(v)
}

/// The key of an edge end for the app's pre-check: the point on a 1e-4 mm grid.
fn end_key(p: [f64; 3]) -> [i64; 3] {
    p.map(|c| (c * 1e4).round() as i64)
}

/// A ring (a circle, an ellipse): an edge whose ends are less than 1e-6 apart. It closes on itself.
fn is_ring(e: &MeshEdge) -> bool {
    let d = [e.a[0] - e.b[0], e.a[1] - e.b[1], e.a[2] - e.b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() < 1e-6
}

/// The picked ids that are edges of the body, `None` when one is not (or none is picked): the first part of the app's pre-check.
fn picked_edges<'a>(edges: &'a [MeshEdge], picked: &[u32]) -> Option<Vec<&'a MeshEdge>> {
    let found: Vec<&MeshEdge> = picked.iter().filter_map(|id| edges.iter().find(|e| e.id == *id)).collect();
    (!found.is_empty() && found.len() == picked.len()).then_some(found)
}

/// WHETHER THE PICKED EDGES BOUND A PATCH: the app's `patch_boundary` (qymcad-part `apply/faces.rs:67-90`, `pub(crate)` there, so
/// reimplemented here rule for rule). Every picked id must be a live edge of the body (an id with no edge refuses); a ring (an edge
/// whose ends are less than 1e-6 apart: a circle, an ellipse) closes on itself and counts no ends; every end of the other edges
/// must meet exactly one other end, ends compared on a 1e-4 mm grid (`round(c * 1e4)`). So: one ring, or several rings, or an open
/// edge chain that closes. One straight edge, a chain with a loose end and an edge picked past the loop are refused. It is the
/// app's limit, not a kernel one: the kernel also fills open chains (a piece of a dome).
fn patch_boundary(edges: &[MeshEdge], picked: &[u32]) -> bool {
    let Some(found) = picked_edges(edges, picked) else { return false };
    let mut ends: std::collections::HashMap<[i64; 3], usize> = std::collections::HashMap::new();
    for e in found.into_iter().filter(|e| !is_ring(e)) {
        *ends.entry(end_key(e.a)).or_default() += 1;
        *ends.entry(end_key(e.b)).or_default() += 1;
    }
    ends.values().all(|&n| n == 2)
}

/// How many separate boundaries the picked edges make: every ring is one, and the other edges join into one boundary where their ends
/// meet (same 1e-4 mm grid as the pre-check). A patch spans ONE (F-199: the kernel drops all but one without a word). Counts only
/// edges the pre-check has accepted.
fn patch_loops(edges: &[MeshEdge], picked: &[u32]) -> usize {
    let Some(found) = picked_edges(edges, picked) else { return 0 };
    let rings = found.iter().filter(|e| is_ring(e)).count();
    let mut parent: std::collections::HashMap<[i64; 3], [i64; 3]> = std::collections::HashMap::new();
    fn root(parent: &mut std::collections::HashMap<[i64; 3], [i64; 3]>, k: [i64; 3]) -> [i64; 3] {
        let p = *parent.entry(k).or_insert(k);
        if p == k {
            return k;
        }
        let r = root(parent, p);
        parent.insert(k, r);
        r
    }
    for e in found.iter().filter(|e| !is_ring(e)) {
        let (a, b) = (root(&mut parent, end_key(e.a)), root(&mut parent, end_key(e.b)));
        parent.insert(a, b);
    }
    let keys: Vec<[i64; 3]> = parent.keys().copied().collect();
    let chains: std::collections::HashSet<[i64; 3]> = keys.into_iter().map(|k| root(&mut parent, k)).collect();
    rings + chains.len()
}

impl Session {
    /// Recheck evaluated stitch tolerances, as `check_offset_values` does for distances.
    pub(crate) fn check_stitch_values(&self) -> std::result::Result<(), (Id, Error)> {
        let vars = self.p.param_map();
        for node in &self.p.timeline {
            if let FeatureKind::Stitch { tol, .. } = node.kind {
                let v = self.p.feat_dim(node.id, "tol").filter(|e| !e.is_empty()).map_or(Ok(tol), |e| Num::from(e).eval(&vars));
                v.and_then(stitch_tolerance).map_err(|e| (node.id, e))?;
            }
        }
        Ok(())
    }

    /// Recheck evaluated offset distances: native regeneration bounds none (a parameter or dimension edit can push `dist` past
    /// the app's range, or to a non-finite value).
    pub(crate) fn check_offset_values(&self) -> std::result::Result<(), (Id, Error)> {
        let vars = self.p.param_map();
        for node in &self.p.timeline {
            if let FeatureKind::OffsetSurface { dist, .. } = node.kind {
                let v = self.p.feat_dim(node.id, "dist").filter(|e| !e.is_empty()).map_or(Ok(dist), |e| Num::from(e).eval(&vars));
                v.and_then(offset_distance).map_err(|e| (node.id, e))?;
            }
        }
        Ok(())
    }

    /// The faces of `src` that `sel` resolves to, as ids and as the app's stored picks. An empty set is refused (the app's
    /// `msg-faces-not-found`).
    fn surface_faces(&self, src: Id, sel: &Sel, what: &str) -> Result<(Vec<u32>, Ref)> {
        let ids = self.resolve_sel(src, Element::Faces, sel)?;
        if ids.is_empty() {
            return Err(Error::Invalid(format!("the {what} face selection matched no face of body {src}")));
        }
        let picks = Ref::picks(&ids);
        Ok((ids, picks))
    }

    /// Copy faces into a new sheet body (the app's Copy faces). Returns the node (also the sheet's id), the face ids and the report.
    pub fn face_copy(&mut self, body: Option<Id>, faces: &Sel, name: Option<&str>) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let (ids, picks) = s.surface_faces(src, faces, "copy")?;
            let id = s.in_component(s.p.body_owner(src), |p| p.add_face_copy(src, picks));
            s.set_node_name(id, name)?;
            Ok((id, ids))
        })
        .map(|((id, ids), rebuild)| (id, ids, rebuild))
    }

    /// Replace the faces of an existing FaceCopy node, rebuilding what depends on it.
    pub fn repick_face_copy(&mut self, feature: Id, faces: &Sel) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        let (src, body) = self.feature_as(feature, "face_copy", |kind| match kind {
            FeatureKind::FaceCopy { src, body, .. } => Some((*src, *body)),
            _ => None,
        })?;
        let (ids, picks) = self.surface_faces(src, faces, "copy")?;
        let bodies = self.repick_rebuild(feature, "face_copy", |kind, _| {
            let FeatureKind::FaceCopy { faces: stored, .. } = kind else {
                return None;
            };
            *stored = picks;
            Some(())
        })?;
        Ok((body, ids, self.current_rebuild_after_edit(bodies)))
    }

    /// Move faces along their normals into a new sheet body (the app's Offset surface): outward of a solid is positive, zero is
    /// a copy in place. `dist` is a feature dimension, editable with `dimension_set`.
    pub fn offset_surface(&mut self, body: Option<Id>, faces: &Sel, dist: &Num, name: Option<&str>) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.source_body(body)?;
            // The range is judged by the rebuild (`check_offset_values`), creation included.
            let d = dist.eval(&s.p.param_map())?;
            let (ids, picks) = s.surface_faces(src, faces, "offset")?;
            let id = s.in_component(s.p.body_owner(src), |p| p.add_offset_surface(src, picks, d));
            dim(s, id, "dist", dist);
            s.set_node_name(id, name)?;
            Ok((id, ids))
        })
        .map(|((id, ids), rebuild)| (id, ids, rebuild))
    }

    /// Replace the faces of an existing OffsetSurface node; the distance formula stays.
    pub fn repick_offset_surface(&mut self, feature: Id, faces: &Sel) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        let (src, body) = self.feature_as(feature, "offset_surface", |kind| match kind {
            FeatureKind::OffsetSurface { src, body, .. } => Some((*src, *body)),
            _ => None,
        })?;
        let (ids, picks) = self.surface_faces(src, faces, "offset")?;
        let bodies = self.repick_rebuild(feature, "offset_surface", |kind, _| {
            let FeatureKind::OffsetSurface { faces: stored, .. } = kind else {
                return None;
            };
            *stored = picks;
            Some(())
        })?;
        Ok((body, ids, self.current_rebuild_after_edit(bodies)))
    }

    /// A body that a node takes as an input: built in the active document, not consumed by a later feature, not a mesh. `node` is
    /// the node being re-picked: the bodies it consumes now are still free to it.
    fn live_input(&self, body: Id, node: Option<Id>) -> Result<()> {
        if !self.is_active_body(body) {
            return Err(Error::NotFound(format!("body {body} is not built in the active document")));
        }
        let mut consumed = self.p.consumed_bodies();
        if let Some(n) = self.p.timeline.iter().find(|n| Some(n.id) == node) {
            for b in n.kind.consumed() {
                consumed.remove(&b);
            }
        }
        if consumed.contains(&body) {
            return Err(Error::Invalid(format!("body {body} was consumed by a later feature")));
        }
        if self.p.mesh_born(body) {
            return Err(Error::Invalid(crate::topology::mesh_body_refusal(body)));
        }
        Ok(())
    }

    /// A body built after the node being re-picked cannot feed it.
    fn built_before(&self, body: Id, node: Option<Id>) -> Result<()> {
        if let Some(node) = node {
            let built_at = |id: Id| self.p.timeline.iter().position(|n| n.kind.bodies().contains(&id));
            if built_at(body) >= built_at(node) {
                return Err(Error::Invalid(format!("body {body} is built after feature {node}, which cannot take it")));
            }
        }
        Ok(())
    }

    /// A sheet body that SurfaceReplace, Stitch or Trim takes (`what` names the role in refusals).
    fn sheet_input(&self, body: Id, node: Option<Id>, what: &str) -> Result<Id> {
        self.live_input(body, node)?;
        if !self.is_sheet(body) {
            return Err(Error::Invalid(format!(
                "body {body} is a solid; {what} must be a sheet (a surface: face_copy, offset_surface, loft surface:true)"
            )));
        }
        self.built_before(body, node)?;
        Ok(body)
    }

    /// The solid whose faces SurfaceReplace drops: the usual source body, which must not be a sheet.
    fn solid_to_replace(&self, body: Option<Id>) -> Result<Id> {
        let src = self.source_body(body)?;
        if self.is_sheet(src) {
            return Err(Error::Invalid(format!(
                "body {src} is a sheet; `body` is the solid whose faces are replaced, the sheet goes in `surface`"
            )));
        }
        Ok(src)
    }

    /// The app's pre-check before it lays a SurfaceReplace node (`apply_surface_replace_cmd`): the sheet must close exactly what
    /// the dropped faces leave open. Asked of the live shapes; with either missing the rebuild judges, as in the app.
    fn sheet_must_close(&self, src: Id, faces: &[u32], surface: Id) -> Result<()> {
        let (Some(base), Some(sheet)) = (self.shapes.get(&src), self.shapes.get(&surface)) else {
            return Ok(());
        };
        qymcad_kernel::ask::sheet_closes(base, faces, sheet).map_err(|free| {
            Error::Invalid(format!(
                "the sheet does not close what it replaces: {}",
                localization::error(&CoreError::SurfaceDoesNotClose { free })
            ))
        })
    }

    /// Replace faces of a solid with a sheet (the app's Replace face): `src` and `surface` are consumed, the node's body is a
    /// new solid. Returns the node (also the body's id), the face ids and the report.
    pub fn surface_replace(&mut self, body: Option<Id>, faces: &Sel, surface: Id, name: Option<&str>) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.solid_to_replace(body)?;
            let surface = s.sheet_input(surface, None, "`surface`")?;
            let (ids, picks) = s.surface_faces(src, faces, "replace")?;
            s.sheet_must_close(src, &ids, surface)?;
            let id = s.in_component(s.p.body_owner(src), |p| p.add_surface_replace(src, picks, surface));
            s.set_node_name(id, name)?;
            Ok((id, ids))
        })
        .map(|((id, ids), rebuild)| (id, ids, rebuild))
    }

    /// Re-pick the faces and/or the sheet of an existing SurfaceReplace node, rebuilding what depends on it (the app's edit mode
    /// changes either). `ids` are empty when only the sheet changes.
    pub fn repick_surface_replace(&mut self, feature: Id, faces: Option<&Sel>, surface: Option<Id>) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        let (src, body) = self.feature_as(feature, "surface_replace", |kind| match kind {
            FeatureKind::SurfaceReplace { src, body, .. } => Some((*src, *body)),
            _ => None,
        })?;
        let (ids, picks) = match faces {
            Some(sel) => {
                let (ids, picks) = self.surface_faces(src, sel, "replace")?;
                (ids, Some(picks))
            }
            None => (Vec::new(), None),
        };
        let surface = surface.map(|b| self.sheet_input(b, Some(feature), "`surface`")).transpose()?;
        let bodies = self.repick_rebuild(feature, "surface_replace", |kind, _| {
            let FeatureKind::SurfaceReplace { faces: stored, surface: sheet, .. } = kind else {
                return None;
            };
            if let Some(picks) = picks {
                *stored = picks;
            }
            if let Some(s) = surface {
                *sheet = s;
            }
            Some(())
        })?;
        Ok((body, ids, self.current_rebuild_after_edit(bodies)))
    }

    /// The sheets a Stitch takes, in the order given (the first is the lineage): at least two, each once, none a solid.
    fn stitch_parts(&self, parts: &[Id], node: Option<Id>) -> Result<Vec<Id>> {
        if parts.len() < 2 {
            return Err(Error::Invalid("stitch needs at least two sheets in `parts`".into()));
        }
        let mut seen = Vec::new();
        for &part in parts {
            if seen.contains(&part) {
                return Err(Error::Invalid(format!("body {part} is listed twice in `parts`")));
            }
            seen.push(self.sheet_input(part, node, "every part of a stitch")?);
        }
        Ok(seen)
    }

    /// Stitch sheets into one body (the app's Stitch): every part is consumed; the result is a SOLID when the sheets close a
    /// shell, otherwise a sheet. `tol` is a feature dimension (`tol`), 1e-6..10 mm. Returns the node (also the body's id).
    pub fn stitch(&mut self, parts: &[Id], tol: &Num, name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let parts = s.stitch_parts(parts, None)?;
            let t = tol.eval(&s.p.param_map())?;
            let owner = s.p.body_owner(parts[0]);
            let id = s.in_component(owner, |p| p.add_stitch(parts, t));
            dim(s, id, "tol", tol);
            s.set_node_name(id, name)?;
            Ok(id)
        })
    }

    /// Replace the sheets of an existing Stitch node; the tolerance formula stays.
    pub fn repick_stitch(&mut self, feature: Id, parts: &[Id]) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        let body = self.feature_as(feature, "stitch", |kind| match kind {
            FeatureKind::Stitch { body, .. } => Some(*body),
            _ => None,
        })?;
        let new = self.stitch_parts(parts, Some(feature))?;
        let bodies = self.repick_rebuild(feature, "stitch", |kind, _| {
            let FeatureKind::Stitch { parts: stored, .. } = kind else {
                return None;
            };
            *stored = new;
            Some(())
        })?;
        Ok((body, self.current_rebuild_after_edit(bodies)))
    }

    /// The point that says which piece of a trimmed sheet stays: three finite numbers, in the Part's own frame (the frame the kernel
    /// works in, see ADR 0062).
    fn trim_keep(keep: [f64; 3]) -> Result<[f64; 3]> {
        if keep.iter().all(|c| c.is_finite()) {
            Ok(keep)
        } else {
            Err(Error::Invalid(format!("trim `keep` must be three finite numbers, got {keep:?}")))
        }
    }

    /// The body that cuts: a solid or a sheet (the app takes either), live, built before the node, not the trimmed sheet itself and
    /// in the same Part as it (the kernel works in one part-local frame).
    fn trim_tool(&self, tool: Id, sheet: Id, node: Option<Id>) -> Result<Id> {
        self.live_input(tool, node)?;
        if tool == sheet {
            return Err(Error::Invalid(format!("body {tool} cannot trim itself; `tool` is another body that cuts the sheet")));
        }
        if self.p.body_owner(tool) != self.p.body_owner(sheet) {
            return Err(Error::Invalid(format!("the sheet {sheet} and the tool {tool} must belong to the same part")));
        }
        self.built_before(tool, node)?;
        Ok(tool)
    }

    /// Trim a sheet by a body (the app's Trim): the piece of `body` nearest to `keep` stays, as a NEW sheet; `body` is consumed, the
    /// tool is not. `keep` is part-local. Returns the node (also the new sheet's id) and the report.
    pub fn trim(&mut self, body: Id, tool: Id, keep: [f64; 3], name: Option<&str>) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.sheet_input(body, None, "the body to trim")?;
            let tool = s.trim_tool(tool, src, None)?;
            let keep = Self::trim_keep(keep)?;
            let id = s.in_component(s.p.body_owner(src), |p| p.add_trim(src, tool, keep));
            s.set_node_name(id, name)?;
            Ok(id)
        })
    }

    /// Re-pick the sheet, the tool and/or the keep point of an existing Trim node (the app's edit mode sets all three together, so a
    /// new sheet needs its own `keep`).
    pub fn repick_trim(&mut self, feature: Id, body: Option<Id>, tool: Option<Id>, keep: Option<[f64; 3]>) -> Result<(Id, Rebuild)> {
        self.ensure_topology()?;
        let (old_src, old_tool, old_keep, node_body) = self.feature_as(feature, "trim", |kind| match kind {
            FeatureKind::Trim { src, tool, keep, body } => Some((*src, *tool, *keep, *body)),
            _ => None,
        })?;
        if body.is_some() && keep.is_none() {
            return Err(Error::Invalid("a new sheet needs its own `keep`: the old point lies on the old sheet".into()));
        }
        let src = body.map(|b| self.sheet_input(b, Some(feature), "the body to trim")).transpose()?.unwrap_or(old_src);
        let tool = self.trim_tool(tool.unwrap_or(old_tool), src, Some(feature))?;
        let keep = keep.map(Self::trim_keep).transpose()?.unwrap_or(old_keep);
        let bodies = self.repick_rebuild(feature, "trim", |kind, _| {
            let FeatureKind::Trim { src: s, tool: t, keep: k, .. } = kind else {
                return None;
            };
            (*s, *t, *k) = (src, tool, keep);
            Some(())
        })?;
        Ok((node_body, self.current_rebuild_after_edit(bodies)))
    }

    /// The edges a Patch spans, as ids: a selection of edges of `src` (a set: each id once) that the app's `patch_boundary` accepts, which
    /// is checked on creation as the app does before it lays a node. `check` is off on a re-pick: the app's edit mode does not ask
    /// (`cmd_ready` is true for any edit), the rebuild judges. F-199 is held either way: the edges make ONE boundary.
    fn patch_edges(&self, src: Id, sel: &Sel, check: bool) -> Result<Vec<u32>> {
        let ids = self.resolve_sel(src, Element::Edges, sel)?;
        let live = self.p.regen_edges.get(&src).map(Vec::as_slice).unwrap_or_default();
        if ids.is_empty() || (check && !patch_boundary(live, &ids)) {
            return Err(Error::Invalid(format!("{} (edges {ids:?} of body {src})", localization::text("msg-patch-needs-edges"))));
        }
        let loops = patch_loops(live, &ids);
        if loops > 1 {
            return Err(Error::Invalid(format!(
                "the edges {ids:?} of body {src} make {loops} separate boundaries (a ring, or a chain of edges that meet end to end, is one); a patch \
                 spans one: QymCAD builds the surface over one of them and drops the other edges without a word (F-199). Pick one boundary: \
                 on a shelled opening the outer outline or the inner one, not both"
            )));
        }
        Ok(ids)
    }

    /// Span a surface over a boundary of edges (the app's Patch): a NEW sheet body of one face, `tangent` meeting the neighbouring faces
    /// smoothly instead of flat. The source is not consumed. Returns the node (also the sheet's id), the edge ids and the report.
    pub fn patch(&mut self, body: Option<Id>, edges: &Sel, tangent: bool, name: Option<&str>) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        self.atomic(|s| {
            let src = s.source_body(body)?;
            let ids = s.patch_edges(src, edges, true)?;
            let id = s.in_component(s.p.body_owner(src), |p| p.add_patch(src, Ref::picks(&ids), tangent));
            s.set_node_name(id, name)?;
            Ok((id, ids))
        })
        .map(|((id, ids), rebuild)| (id, ids, rebuild))
    }

    /// Re-pick the edges and/or the mode of an existing Patch node (the app's edit mode sets both). `ids` are empty when only the
    /// mode changes.
    pub fn repick_patch(&mut self, feature: Id, edges: Option<&Sel>, tangent: Option<bool>) -> Result<(Id, Vec<u32>, Rebuild)> {
        self.ensure_topology()?;
        let (src, body) = self.feature_as(feature, "patch", |kind| match kind {
            FeatureKind::Patch { src, body, .. } => Some((*src, *body)),
            _ => None,
        })?;
        let ids = edges.map(|sel| self.patch_edges(src, sel, false)).transpose()?;
        let bodies = self.repick_rebuild(feature, "patch", |kind, _| {
            let FeatureKind::Patch { edges: stored, tangent: mode, .. } = kind else {
                return None;
            };
            if let Some(ids) = &ids {
                *stored = Ref::picks(ids);
            }
            if let Some(t) = tangent {
                *mode = t;
            }
            Some(())
        })?;
        Ok((body, ids.unwrap_or_default(), self.current_rebuild_after_edit(bodies)))
    }

    /// FINDINGS F-198: Stitch nodes picked by `check` whose SOLID result is not one. When the tolerance is wider than the gap
    /// between the edges of two sheets, the sewing glues them into a closed shell that encloses nothing (two parallel plates 5 apart
    /// at tol 5.1: kernel volume 4000, mesh 0), and the node is green. A solid's kernel volume and the volume its mesh encloses agree
    /// to within the mesh's own error ([`collapsed_solid`]); here they do not. The mesh is the exact reporting one (the one
    /// `doc_info` measures), not the display mesh, whose error grows with the document's `GeomQuality`. When that mesh is partial
    /// nothing can be said and the node is not judged.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()`.
    pub(crate) fn stitch_refusals(&self, check: impl Fn(Id) -> bool) -> Vec<(Id, String)> {
        self.p
            .timeline
            .iter()
            .filter(|n| check(n.id))
            .filter_map(|n| {
                let FeatureKind::Stitch { body, .. } = n.kind else { return None };
                let shape = self.shapes.get(&body)?;
                let (sheet, kernel) = {
                    let _gate = qymcad_kernel::kernel_gate();
                    (shape.is_sheet(), shape.volume())
                };
                if sheet {
                    return None;
                }
                let measured = crate::session::reporting_metrics(shape);
                let (mesh, area) = (measured.mesh_volume?, measured.area?);
                collapsed_solid(kernel, mesh, area, measured.deflection).then(|| {
                    (
                        n.id,
                        format!(
                            "the stitch gave a solid that is not one (kernel volume {:.1} mm3, its faces enclose {mesh:.1} mm3): most likely the tolerance is wider than the gap between the sheets' edges, so it joined sheets that do not meet; use a smaller `tolerance`",
                            kernel.abs()
                        ),
                    )
                })
            })
            .collect()
    }
}

/// F-198: whether a stitched solid's kernel volume and the volume its exact mesh encloses (`area` mm2 of faces, tessellated within
/// `deflection` mm) differ by more than the mesh can explain. A chord of a curved face lies at most `deflection` inside the surface,
/// so the mesh volume is off by at most `area * deflection`; a relative 0.1 % and 1e-6 mm3 cover the rounding of the sums. Signs do
/// not matter (the mesh volume is absolute; a kernel volume of the opposite sign is the same solid turned inside out).
pub(crate) fn collapsed_solid(kernel: f64, mesh: f64, area: f64, deflection: f64) -> bool {
    let (kernel, mesh) = (kernel.abs(), mesh.abs());
    (kernel - mesh).abs() > area * deflection + 1e-3 * kernel.max(mesh) + 1e-6
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(id: u32, a: [f64; 3], b: [f64; 3]) -> MeshEdge {
        MeshEdge { id, a, b, ..Default::default() }
    }

    /// The four edges of a square of side 10 at z 0, ids 1..=4, ends meeting; ids 11..=14 the same square at x offset 100.
    fn squares() -> Vec<MeshEdge> {
        let q = |dx: f64, base: u32| {
            [
                edge(base, [dx, 0., 0.], [dx + 10., 0., 0.]),
                edge(base + 1, [dx + 10., 0., 0.], [dx + 10., 10., 0.]),
                edge(base + 2, [dx + 10., 10., 0.], [dx, 10., 0.]),
                edge(base + 3, [dx, 10., 0.], [dx, 0., 0.]),
            ]
        };
        q(0., 1).into_iter().chain(q(100., 11)).collect()
    }

    #[test]
    fn the_app_pre_check_rules() {
        let mut e = squares();
        e.push(edge(21, [50., 0., 0.], [50., 0., 0.])); // a ring: ends equal
        e.push(edge(22, [70., 0., 0.], [70., 0., 5e-7])); // a ring: ends 5e-7 apart (< 1e-6)
        e.push(edge(23, [80., 0., 0.], [80., 0., 2e-6])); // not a ring (2e-6 apart) but both ends in one grid cell
        e.push(edge(24, [90., 0., 0.], [90., 0., 3e-4])); // not a ring, ends in two cells
        assert!(patch_boundary(&e, &[1, 2, 3, 4]), "a closed chain");
        assert!(patch_boundary(&e, &[21]), "one ring");
        assert!(patch_boundary(&e, &[22]), "a ring within 1e-6");
        assert!(patch_boundary(&e, &[23]), "ends 2e-6 apart fall in one cell and count each other, as in the app");
        assert!(!patch_boundary(&e, &[24]), "ends 3e-4 apart are two cells, each met by nothing");
        assert!(patch_boundary(&e, &[21, 22]), "two rings pass");
        assert!(patch_boundary(&e, &[1, 2, 3, 4, 11, 12, 13, 14]), "two closed chains pass");
        assert!(patch_boundary(&e, &[1, 2, 3, 4, 21]), "a chain and a ring pass");
        assert!(!patch_boundary(&e, &[1]), "one straight edge");
        assert!(!patch_boundary(&e, &[1, 2, 3]), "an open chain");
        assert!(!patch_boundary(&e, &[1, 3]), "two opposite edges");
        assert!(!patch_boundary(&e, &[1, 2, 3, 4, 11]), "an edge past the loop");
        assert!(!patch_boundary(&e, &[]), "no edge");
        assert!(!patch_boundary(&e, &[1, 2, 3, 4, 999]), "an id that is no edge of the body");
        assert!(!patch_boundary(&[], &[1]), "a body without edges");
    }

    /// Ends meet on a 1e-4 mm grid: `round(c * 1e4)`.
    #[test]
    fn ends_meet_on_the_grid_of_a_ten_thousandth() {
        let near = |gap: f64| vec![edge(1, [0., 0., 0.], [10., 0., 0.]), edge(2, [10. + gap, 0., 0.], [0., 0., 0.])];
        assert!(patch_boundary(&near(0.), &[1, 2]));
        assert!(patch_boundary(&near(4e-5), &[1, 2]), "10.00004 rounds to the same cell as 10");
        assert!(!patch_boundary(&near(6e-5), &[1, 2]), "10.00006 rounds to the next cell");
    }

    /// A ring is told by the distance of its ends, not by the grid: an edge whose ends straddle a cell boundary 4e-9 apart is a ring, and
    /// three edges meeting at two points (every end met three times) are not a boundary.
    #[test]
    fn rings_by_distance_and_ends_met_exactly_twice() {
        let straddle = [edge(1, [4.9998e-5, 0., 0.], [5.0002e-5, 0., 0.])];
        assert_ne!(end_key(straddle[0].a), end_key(straddle[0].b), "setup: the ends are in two cells");
        assert!(patch_boundary(&straddle, &[1]), "a ring all the same");
        let theta = [edge(1, [0., 0., 0.], [10., 0., 0.]), edge(2, [0., 0., 0.], [10., 0., 0.]), edge(3, [0., 0., 0.], [10., 0., 0.])];
        assert!(!patch_boundary(&theta, &[1, 2, 3]), "each end is met by two others");
        assert!(patch_boundary(&theta, &[1, 2]), "two edges between two points close a loop");
    }

    #[test]
    fn separate_boundaries_are_counted() {
        let mut e = squares();
        e.push(edge(21, [50., 0., 0.], [50., 0., 0.]));
        e.push(edge(22, [60., 0., 0.], [60., 0., 0.]));
        assert_eq!(patch_loops(&e, &[1, 2, 3, 4]), 1);
        assert_eq!(patch_loops(&e, &[1, 2, 3]), 1, "an open chain is one");
        assert_eq!(patch_loops(&e, &[21]), 1);
        assert_eq!(patch_loops(&e, &[1, 2, 3, 4, 11, 12, 13, 14]), 2, "two squares");
        assert_eq!(patch_loops(&e, &[1, 2, 3, 4, 21]), 2, "a chain and a ring");
        assert_eq!(patch_loops(&e, &[21, 22]), 2, "two rings");
        assert_eq!(patch_loops(&e, &[1, 3]), 2, "two edges that do not meet");
        assert_eq!(patch_loops(&e, &[1, 2, 3, 4, 11, 12, 13, 14, 21, 22]), 4);
        assert_eq!(patch_loops(&e, &[999]), 0);
    }
}
