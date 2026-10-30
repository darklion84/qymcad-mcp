//! Native hole placement at isolated sketch points and persisted parameter dependencies.
use crate::modifiers::{dim, THROUGH_DEPTH};
use crate::{Error, HoleKind, Id, Num, Rebuild, Result, Session};
use qymcad_core::{
    feature::{FeatureKind, SketchPlane},
    model::{DimTarget, HoleTool, PlaneDef, Project},
    names::Role,
};
use std::collections::{HashMap, HashSet};

/// Sketch placement for the native Hole feature.
#[derive(Clone, Debug, PartialEq)]
pub struct HoleSketch {
    /// Current stock body, defaulting to the active part.
    pub body: Option<Id>,
    /// Marker sketch in the same part as the stock.
    pub sketch: Id,
    /// Explicit complete isolated-point set; the pin cannot filter a subset.
    pub points: Option<Vec<Id>>,
    /// Reverse the drill direction (default cuts against sketch normal).
    pub flip: bool,
    /// Bore diameter in mm.
    pub diameter: Num,
    /// Depth from the marker plane; absent means fixed 10000 mm with directed-span guard.
    pub depth: Option<Num>,
    /// Plain, counterbore or countersink.
    pub kind: HoleKind,
    /// Recess diameter in mm.
    pub dia2: Option<Num>,
    /// Recess depth in mm.
    pub depth2: Option<Num>,
    /// Optional native feature name.
    pub name: Option<String>,
}

impl Session {
    /// Drill every isolated marker with the app's native sketch-hole recipe.
    pub fn hole_by_sketch(&mut self, a: &HoleSketch) -> Result<(Id, Rebuild)> {
        let before = self.p.clone();
        let nodes = self.p.timeline.iter().map(|n| n.id).collect();
        self.with_rebuild_copies(before, &nodes, "sketch hole", |s| {
            s.ensure_topology()?;
            s.atomic(|s| {
                let src = s.source_body(a.body)?;
                s.sketch_si(a.sketch)?;
                check_support(&s.p, a.sketch, src)?;
                if s.p.sketch_owner(a.sketch) != s.p.body_owner(src) {
                    return Err(Error::Invalid("hole sketch and stock must belong to the same part".into()));
                }
                let ids = s.p.sketch_hole_point_ids(a.sketch);
                if ids.is_empty() {
                    return Err(Error::Invalid(format!("sketch {} has no isolated hole points", a.sketch)));
                }
                if let Some(points) = &a.points {
                    let requested: HashSet<_> = points.iter().copied().collect();
                    if requested.len() != points.len() || requested != ids.iter().copied().collect() {
                        return Err(Error::Invalid(
                            "points must contain every isolated point exactly once: pinned native sketch holes cannot filter a subset"
                                .into(),
                        ));
                    }
                }
                let vars = s.p.param_map();
                let diameter = positive(a.diameter.eval(&vars)?, "diameter")?;
                let depth = match &a.depth {
                    Some(n) => positive(n.eval(&vars)?, "depth")?,
                    None => THROUGH_DEPTH,
                };
                let (kind, dia2, depth2) = match a.kind {
                    HoleKind::Plain => {
                        if a.dia2.is_some() || a.depth2.is_some() {
                            return Err(Error::Invalid("plain hole excludes dia2 and depth2".into()));
                        }
                        (0, 0., 0.)
                    }
                    kind => {
                        let (Some(d), Some(h)) = (&a.dia2, &a.depth2) else {
                            return Err(Error::Invalid("counterbore or countersink needs dia2 and depth2".into()));
                        };
                        let d = positive(d.eval(&vars)?, "dia2")?;
                        let h = positive(h.eval(&vars)?, "depth2")?;
                        recess(diameter, depth, d, h)?;
                        (if kind == HoleKind::Counterbore { 1 } else { 2 }, d, h)
                    }
                };
                if a.depth.is_none() {
                    s.check_hole_span(src, a.sketch, a.flip)?;
                }
                let id = s.in_component(s.p.body_owner(src), |p| {
                    p.add_hole_from_sketch(src, a.sketch, HoleTool { kind, diameter, depth, dia2, depth2 }, a.flip)
                });
                dim(s, id, "diameter", &a.diameter);
                if let Some(n) = &a.depth {
                    dim(s, id, "depth", n);
                } else {
                    s.p.set_feat_dim(id, "sketch_through", "1".into());
                }
                if let Some(n) = &a.dia2 {
                    dim(s, id, "dia2", n);
                }
                if let Some(n) = &a.depth2 {
                    dim(s, id, "depth2", n);
                }
                s.reconcile_hole_dependencies();
                s.set_node_name(id, a.name.as_deref())?;
                Ok(id)
            })
        })
    }

    /// Refresh native expression reads without replacing real hole dimensions or unknown metadata.
    pub(crate) fn reconcile_hole_dependencies(&mut self) {
        let holes: Vec<_> = self
            .p
            .timeline
            .iter()
            .filter_map(|n| match n.kind {
                FeatureKind::Hole { sketch, .. } if sketch != 0 => Some((n.id, sketch)),
                _ => None,
            })
            .collect();
        for (hole, sketch) in holes {
            // The native app can replace a named depth without knowing our through marker.
            if self.p.feat_dim(hole, "sketch_through").is_some() && self.p.feat_dim(hole, "depth").is_some() {
                self.p.set_feat_dim(hole, "sketch_through", String::new());
            }
            let owners = sketch_sources(&self.p, sketch);
            let mut desired = HashMap::new();
            for &owner in &owners {
                if let Some(dims) = self.p.feat_dims.get(&owner) {
                    for (key, expr) in dims {
                        if !key.starts_with("sketch_dep_") {
                            desired.insert(format!("sketch_dep_{owner}_{key}"), expr.clone());
                        }
                    }
                }
                if let Some(si) = self.p.sketch_index(owner) {
                    for (i, c) in self.p.sketches[si].constraints.iter().enumerate() {
                        if let Some(expr) = c.expr().filter(|e| !e.trim().is_empty()) {
                            desired.insert(format!("sketch_dep_{owner}_constraint_{i}"), expr.to_string());
                        }
                    }
                }
            }
            for (i, named) in self.p.named_dims.iter().enumerate() {
                let owner = match named.target {
                    DimTarget::Sketch { sketch, .. } => sketch,
                    DimTarget::Feature { node, .. } => node,
                };
                if owners.contains(&owner) {
                    desired.insert(format!("sketch_dep_driver_{i}"), named.name.clone());
                }
            }
            let obsolete: Vec<_> = self
                .p
                .feat_dims
                .get(&hole)
                .into_iter()
                .flat_map(|m| m.keys())
                .filter(|k| k.starts_with("sketch_dep_") && !desired.contains_key(*k))
                .cloned()
                .collect();
            for key in obsolete {
                self.p.set_feat_dim(hole, &key, String::new());
            }
            for (key, expr) in desired {
                if self.p.feat_dim(hole, &key) != Some(expr.as_str()) {
                    self.p.set_feat_dim(hole, &key, expr);
                }
            }
        }
    }

    /// Native regeneration must also see the extended structural edges, even for unnamed edits.
    pub(crate) fn dirty_hole_dependents(&mut self) {
        let planned = crate::dependencies::rebuild_nodes(&self.p);
        for n in &mut self.p.timeline {
            if planned.contains(&n.id) {
                n.dirty = true;
            }
        }
    }

    pub(crate) fn check_hole_values(&self) -> std::result::Result<(), (Id, Error)> {
        let vars = self.p.param_map();
        for n in self.p.timeline.iter().filter(|n| !n.suppressed) {
            if let FeatureKind::Hole { sketch, diameter, depth, kind, dia2, depth2, .. } = n.kind {
                if sketch == 0 {
                    continue;
                }
                let check = || {
                    let value = |key, fallback| -> Result<f64> {
                        let v = match self.p.feat_dim(n.id, key) {
                            Some(expr) => Num::Expr(expr.into()).eval(&vars)?,
                            None => fallback,
                        };
                        positive(v, key)
                    };
                    check_support(
                        &self.p,
                        sketch,
                        match n.kind {
                            FeatureKind::Hole { src, .. } => src,
                            _ => unreachable!(),
                        },
                    )?;
                    let hole_index = self.p.timeline.iter().position(|node| node.id == n.id).unwrap();
                    if !self.p.timeline[..hole_index]
                        .iter()
                        .any(|node| matches!(node.kind, FeatureKind::Sketch { sketch: id } if id == sketch))
                    {
                        return Err(Error::Invalid(format!("hole sketch {sketch} must precede hole {} in the timeline", n.id)));
                    }
                    let d = value("diameter", diameter)?;
                    let h = value("depth", depth)?;
                    if kind > 2 {
                        return Err(Error::Invalid(format!("unsupported native hole kind {kind}")));
                    }
                    if kind != 0 {
                        recess(d, h, value("dia2", dia2)?, value("depth2", depth2)?)?;
                    } else if dia2 != 0.
                        || depth2 != 0.
                        || self.p.feat_dim(n.id, "dia2").is_some()
                        || self.p.feat_dim(n.id, "depth2").is_some()
                    {
                        return Err(Error::Invalid("plain sketch hole excludes dia2 and depth2".into()));
                    }
                    Ok(())
                };
                check().map_err(|e| (n.id, e))?;
            }
        }
        Ok(())
    }

    /// Native Hole does not wait for its marker support. Resolve upstream dirty placement first,
    /// then repeat the hole/consumer portion on the settled frames, retaining upstream errors.
    pub(crate) fn regenerate_hole_ordered(&mut self) -> qymcad_core::feature::RegenReport {
        let planned: HashSet<_> = self.p.regen_plan().nodes.into_iter().collect();
        let roots: HashSet<_> = self
            .p
            .timeline
            .iter()
            .filter_map(|n| match n.kind {
                FeatureKind::Hole { sketch, .. } if sketch != 0 => {
                    let sources = sketch_sources(&self.p, sketch);
                    sources
                        .iter()
                        .any(|id| *id != sketch && planned.contains(id) && self.p.timeline.iter().any(|n| n.id == *id))
                        .then_some(n.id)
                }
                _ => None,
            })
            .collect();
        let repeat = crate::dependencies::closure(&self.p, roots);
        let shapes = std::mem::take(&mut self.shapes);
        let (mut report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut self.p, shapes);
        self.shapes = shapes;
        if !repeat.is_empty() {
            for node in &mut self.p.timeline {
                if repeat.contains(&node.id) {
                    node.dirty = true;
                }
            }
            let shapes = std::mem::take(&mut self.shapes);
            let (again, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut self.p, shapes);
            self.shapes = shapes;
            report.built.extend(again.built);
            report.errors.retain(|(id, _)| !repeat.contains(id));
            report.errors.extend(again.errors);
        }
        report
    }

    /// Bounds the entire directed distance from the plane, including air before the stock.
    fn check_hole_span(&self, src: Id, sketch: Id, flip: bool) -> Result<()> {
        let frame = self.p.sketch_frame_by_id(sketch).ok_or_else(|| Error::Invalid(format!("hole sketch {sketch} has no frame")))?;
        let n = frame.normal();
        let direction = n.map(|v| if flip { v } else { -v });
        let bb = {
            let _gate = qymcad_kernel::kernel_gate();
            self.shapes.get(&src).and_then(|sh| sh.bbox())
        }
        .ok_or_else(|| Error::Invalid(format!("hole source {src} has no bounding box")))?;
        let mut span = 0.0_f64;
        for x in [bb[0], bb[3]] {
            for y in [bb[1], bb[4]] {
                for z in [bb[2], bb[5]] {
                    let p = [x, y, z];
                    span = span.max((0..3).map(|i| (p[i] - frame.origin[i]) * direction[i]).sum());
                }
            }
        }
        if !span.is_finite() || span > THROUGH_DEPTH {
            return Err(Error::Invalid(format!(
                "through hole directed span {span} from sketch {sketch} exceeds fixed 10000 mm; use explicit depth"
            )));
        }
        Ok(())
    }

    /// Check every requested native site, even when an aggregate cut reports success.
    pub(crate) fn hole_site_refusals(&self) -> Vec<(Id, String)> {
        let mut refused = Vec::new();
        for n in self.p.timeline.iter().take(self.p.rollback.unwrap_or(usize::MAX)).filter(|n| !n.suppressed) {
            let FeatureKind::Hole { sketch, src, body, flip, .. } = n.kind else { continue };
            if sketch == 0 {
                continue;
            }
            if self.p.feat_dim(n.id, "sketch_through").is_some() {
                if let Err(e) = self.check_hole_span(src, sketch, flip) {
                    refused.push((n.id, e.to_string()));
                    continue;
                }
            }
            let ids = self.p.sketch_hole_point_ids(sketch);
            if ids.is_empty() {
                refused.push((n.id, format!("hole sketch {sketch} has no isolated points")));
                continue;
            }
            for point in ids {
                let faces: Vec<_> = self
                    .p
                    .regen_faces
                    .get(&body)
                    .into_iter()
                    .flatten()
                    .filter(|f| {
                        self.p.names.get(f.id).is_some_and(|name| name.feature == n.id && name.role == Role::Hole && name.src == point)
                            && f.area.is_finite()
                            && f.area > 0.
                    })
                    .collect();
                if faces.is_empty() {
                    refused.push((n.id, format!("hole point {point} produced no non-zero-area bore face")));
                }
            }
        }
        refused
    }
}

fn positive(v: f64, key: &str) -> Result<f64> {
    if !v.is_finite() || v <= 0. {
        return Err(Error::Invalid(format!("hole {key} must be finite and positive, got {v}")));
    }
    Ok(v)
}
fn recess(diameter: f64, depth: f64, dia2: f64, depth2: f64) -> Result<()> {
    if dia2 <= diameter || depth2 >= depth {
        return Err(Error::Invalid("hole needs dia2 > diameter and 0 < depth2 < depth".into()));
    }
    Ok(())
}

/// Walk upstream recipe and placement sources, including projected/derived geometry and sketch items.
pub(crate) fn sketch_sources(p: &Project, sketch: Id) -> HashSet<Id> {
    let mut seen = HashSet::new();
    let mut pending = vec![sketch];
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(n) = p.timeline.iter().find(|n| n.id == id) {
            pending.extend(n.kind.inputs());
        }
        if let Some(si) = p.sketch_index(id) {
            let sk = &p.sketches[si];
            match sk.plane {
                SketchPlane::Datum(id) | SketchPlane::Face(id, _) => pending.push(id),
                _ => {}
            }
            for pr in &sk.projections {
                pending.extend([pr.body, crate::dependencies::projection_source(p, id, pr.body)]);
            }
            pending.extend(sk.points.iter().map(|pt| pt.id));
            pending.extend(sk.patterns.iter().map(|pt| pt.id));
            pending.extend(sk.texts.iter().map(|pt| pt.id));
        }
        if let Some(plane) = p.planes.iter().find(|pl| pl.id == id) {
            match plane.def {
                PlaneDef::OffsetPlane { plane, .. } => pending.push(plane),
                PlaneDef::OffsetFace { body, .. } => pending.push(body),
                _ => {}
            }
        }
    }
    seen
}

/// The persisted expression bridge supplies dirtying, not native wave ordering (F-118).
fn check_support(p: &Project, sketch: Id, src: Id) -> Result<()> {
    let si = p.sketch_index(sketch).ok_or_else(|| Error::Invalid(format!("hole sketch {sketch} is missing")))?;
    if !p.sketches[si].projections.is_empty() {
        return Err(Error::Invalid(format!(
            "hole sketch {sketch}: projected marker sketches lack native rebuild ordering; use a fixed marker sketch"
        )));
    }
    let mut pending = match p.sketches[si].plane {
        SketchPlane::Datum(id) => vec![id],
        SketchPlane::Face(host, _) => {
            check_host(p, host, src)?;
            vec![]
        }
        _ => vec![],
    };
    let mut seen = HashSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            return Err(Error::Invalid("hole datum support contains a cycle".into()));
        }
        let plane = p.planes.iter().find(|pl| pl.id == id).ok_or_else(|| Error::Invalid(format!("hole datum {id} is missing")))?;
        let has_formula =
            p.feat_dims.get(&id).is_some_and(|dims| dims.values().any(|e| qymcad_core::expr::eval(e, &HashMap::new()).is_err()));
        let named = p.named_dims.iter().any(|d| matches!(d.target, DimTarget::Feature { node, .. } if node == id));
        if has_formula || named {
            return Err(Error::Invalid(format!(
                "hole sketch {sketch}: parameterized or named datum support {id} lacks native rebuild ordering; use a fixed datum"
            )));
        }
        match plane.def {
            PlaneDef::OffsetPlane { plane, .. } => pending.push(plane),
            PlaneDef::OffsetFace { body, .. } => check_host(p, body, src)?,
            _ => {}
        }
    }
    Ok(())
}

fn check_host(p: &Project, host: Id, src: Id) -> Result<()> {
    let mut seen = HashSet::new();
    let mut pending = vec![src];
    while let Some(id) = pending.pop() {
        if id == host {
            return Ok(());
        }
        if !seen.insert(id) {
            continue;
        }
        if let Some(n) = p.timeline.iter().find(|n| n.kind.bodies().contains(&id)) {
            pending.extend(n.kind.inputs());
        }
    }
    Err(Error::Invalid(format!("hole face support {host} is independent of source {src}; native rebuild ordering is unavailable")))
}
