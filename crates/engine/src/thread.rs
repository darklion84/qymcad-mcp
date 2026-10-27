//! Native helical thread creation, formula validation and basic geometry reporting.
use crate::{Element, Error, Id, Num, Rebuild, Result, Sel, Session};
use qymcad_core::{
    feature::FeatureKind,
    model::{FeatNumber, PlainNumber},
    thread::{ThreadGeom, ThreadSpec, ThreadStandard},
};
use serde::Serialize;

/// Thread creation arguments. Numeric defaults follow the application's command, not ThreadSpec::default.
#[derive(Clone, Debug)]
pub struct ThreadOptions {
    /// Exclusive designation alias: M8, M8x1.25 or Tr16x4.
    pub size: Option<String>,
    /// Profile family; defaults to metric_iso.
    pub standard: Option<String>,
    /// Nominal major diameter in mm; required without size.
    pub nominal: Option<Num>,
    /// Pitch in mm; zero selects the standard coarse pitch.
    pub pitch: Option<Num>,
    /// Required thread span in mm.
    pub length: Num,
    /// Side assertion: auto, external or internal.
    pub kind: String,
    /// Radial modelling clearance per side; default 0.2 mm is lowered when it would bury the groove.
    pub fit: Option<Num>,
    /// Entry runout, default 1.5 mm.
    pub lead_in: Option<Num>,
    /// Exit runout, default 1.5 mm.
    pub lead_out: Option<Num>,
    /// Crest radius; zero takes the standard value.
    pub crest_r: Option<Num>,
    /// Root radius; zero takes the standard value.
    pub root_r: Option<Num>,
    /// Custom profile included angle, strictly greater than 1 and less than 180 degrees; default 60.
    pub angle: Option<Num>,
    /// Custom profile depth; zero takes the native default.
    pub depth: Option<Num>,
    /// Integral start count 1..=64, default 1.
    pub starts: Option<Num>,
    /// Left handed helix.
    pub left: bool,
    /// Timeline feature name.
    pub name: Option<String>,
}
impl Default for ThreadOptions {
    fn default() -> Self {
        Self {
            size: None,
            standard: None,
            nominal: None,
            pitch: None,
            length: Num::Value(0.0),
            kind: "auto".into(),
            fit: None,
            lead_in: None,
            lead_out: None,
            crest_r: None,
            root_r: None,
            angle: None,
            depth: None,
            starts: None,
            left: false,
            name: None,
        }
    }
}

/// Created thread and basic standard geometry. Diameters are basic values, not measured tolerances.
#[derive(Debug, Serialize)]
pub struct ThreadResult {
    /// Created body and feature id.
    pub body: Id,
    /// Feature name.
    pub name: String,
    /// Consumed stock body.
    pub source: Id,
    /// Exactly one saved persistent circular edge id.
    pub edge: u32,
    /// Detected material side.
    pub kind: String,
    /// Profile family.
    pub standard: String,
    /// Evaluated nominal diameter, mm.
    pub nominal: f64,
    /// Effective pitch, mm.
    pub pitch: f64,
    /// Lead per turn, mm.
    pub lead: f64,
    /// Basic major diameter, mm.
    pub basic_major_diameter: f64,
    /// Basic pitch diameter, mm.
    pub basic_pitch_diameter: f64,
    /// Basic minor diameter, mm.
    pub basic_minor_diameter: f64,
    /// Modelling clearance per side, mm; no ISO tolerance class is implied.
    pub fit: f64,
    /// Why the explicit fit or adjusted app default was used.
    pub fit_reason: String,
    /// Additional radial relief from capped axial widening, mm.
    pub radial_relief: f64,
    /// Evaluated requested span, mm.
    pub length: f64,
    /// Available cylindrical span, when measurable, mm.
    pub available_length: Option<f64>,
    /// Normal rebuild bodies and diagnostics.
    pub rebuild: Rebuild,
}

impl ThreadOptions {
    fn numbers(&self) -> Result<(ThreadStandard, Vec<(&'static str, Num)>)> {
        let (standard, nominal, pitch) = if let Some(size) = &self.size {
            if self.standard.is_some() || self.nominal.is_some() || self.pitch.is_some() {
                return Err(Error::Invalid("thread size is exclusive with standard, nominal and pitch".into()));
            }
            let (standard, rest) = if let Some(s) = size.strip_prefix("Tr") {
                (ThreadStandard::TrapezoidalTr, s)
            } else if let Some(s) = size.strip_prefix('M') {
                (ThreadStandard::MetricIso, s)
            } else {
                return Err(Error::Invalid("thread size must be M8, M8x1.25 or Tr16x4 (length is separate)".into()));
            };
            let (d, p) = rest.split_once('x').unwrap_or((rest, "0"));
            let parse = |s: &str| s.parse::<f64>().map(Num::Value).map_err(|_| Error::Invalid(format!("invalid thread size `{size}`")));
            (standard, parse(d)?, parse(p)?)
        } else {
            let standard = match self.standard.as_deref().unwrap_or("metric_iso") {
                "metric_iso" => ThreadStandard::MetricIso,
                "trapezoidal_tr" => ThreadStandard::TrapezoidalTr,
                "acme" => ThreadStandard::Acme,
                "round_rd" => ThreadStandard::RoundRd,
                "buttress" => ThreadStandard::Buttress,
                "custom" => ThreadStandard::Custom,
                s => return Err(Error::Invalid(format!("unknown thread standard `{s}`"))),
            };
            (
                standard,
                self.nominal.clone().ok_or_else(|| Error::Invalid("thread needs nominal or size".into()))?,
                self.pitch.clone().unwrap_or(0.0.into()),
            )
        };
        let n = |v: &Option<Num>, d: f64| v.clone().unwrap_or(Num::Value(d));
        Ok((
            standard,
            vec![
                ("nominal", nominal),
                ("pitch", pitch),
                ("length", self.length.clone()),
                ("fit", n(&self.fit, 0.2)),
                ("lead_in", n(&self.lead_in, 1.5)),
                ("lead_out", n(&self.lead_out, 1.5)),
                ("crest_r", n(&self.crest_r, 0.0)),
                ("root_r", n(&self.root_r, 0.0)),
                ("angle", n(&self.angle, 60.0)),
                ("depth", n(&self.depth, 0.0)),
                ("starts", n(&self.starts, 1.0)),
            ],
        ))
    }
}

fn validate(key: &str, v: f64) -> Result<()> {
    let valid = match key {
        "starts" => v.fract() == 0.0 && (1.0..=64.0).contains(&v),
        "nominal" | "length" => v > 0.0,
        "angle" => v > 0.0 && v < 180.0,
        _ => v >= 0.0,
    };
    if !v.is_finite() || !valid {
        let rule = match key {
            "starts" => "an integer in 1..=64",
            "angle" => "strictly between 0 and 180 degrees",
            "nominal" | "length" => "positive",
            _ => "nonnegative",
        };
        return Err(Error::Invalid(format!("thread {key} must be {rule}, got {v}")));
    }
    Ok(())
}

// The pinned cutter overshoots by .15P (thread.rs:510). Keep .02P exposed even after relief.
fn fit_limit(mut spec: ThreadSpec) -> f64 {
    let ceiling = 0.13 * spec.geometry().pitch;
    let mut low = 0.0;
    let mut high = spec.geometry().pitch.max(0.2);
    spec.fit = high;
    while spec.radial_relief() <= ceiling {
        high *= 2.0;
        if !high.is_finite() {
            return f64::MAX;
        }
        spec.fit = high;
    }
    // Find the largest representable fit allowed by the public relief calculation.
    loop {
        let mid = low + (high - low) * 0.5;
        if mid == low || mid == high {
            return low;
        }
        spec.fit = mid;
        if spec.radial_relief() <= ceiling {
            low = mid;
        } else {
            high = mid;
        }
    }
}

fn check_fit(spec: ThreadSpec) -> Result<()> {
    if spec.radial_relief() > 0.13 * spec.geometry().pitch {
        return Err(Error::Invalid(format!(
            "thread fit {} exceeds limit {:.9} mm: radial relief would bury the groove; keep 0.02 pitch below the 0.15 pitch overshoot",
            spec.fit,
            fit_limit(spec)
        )));
    }
    Ok(())
}

fn check_custom_angle(spec: ThreadSpec) -> Result<()> {
    let maximum = (2.0 * 1.3_f64).to_degrees();
    if spec.standard == ThreadStandard::Custom && (spec.custom_angle <= 1.0 || spec.custom_angle > maximum) {
        return Err(Error::Invalid(format!(
            "custom thread angle must be greater than 1 degree and at most {maximum:.12} degrees, got {}; native ignores angles at or below 1 degree and clamps the half-angle above 1.3 radians",
            spec.custom_angle
        )));
    }
    Ok(())
}

// Public feasibility omits fit in the depth. Check it as well as the actual stock-specific
// profile: native closes the root at zero width instead of preserving an infeasible flank.
fn check_profile_depth(spec: ThreadSpec, g: &ThreadGeom) -> Result<()> {
    if let Some((_, maximum, _)) = spec.profile_overflow() {
        return Err(Error::Invalid(format!("thread profile is infeasible: native maximum depth {maximum:.12} mm at this angle/pitch/fit")));
    }
    let width = ((g.pitch - g.crest_flat) * 0.5 + spec.fit).clamp(0.02 * g.pitch, 0.49 * g.pitch);
    let maximum = width / (g.angle_deg.to_radians() * 0.5).tan();
    if g.depth + spec.fit > maximum {
        return Err(Error::Invalid(format!(
            "thread profile depth plus fit is infeasible: native maximum depth {maximum:.12} mm including fit; maximum bare depth {:.12} mm",
            maximum - spec.fit
        )));
    }
    Ok(())
}

// Reproduce only the pinned kernel's profile sampling (occt_helical.cpp:77-119).
// Side detection uses these vertices, not the analytic arc extrema. In particular an odd
// subdivision count can miss a fully rounded root's deepest point.
fn sampled_profile(g: &ThreadGeom) -> Vec<[f64; 2]> {
    use qymcad_core::geom::ProfEdge;
    let mut points = Vec::new();
    for edge in &g.groove {
        match edge {
            ProfEdge::Line { a, .. } => points.push([a.x, a.y]),
            ProfEdge::Arc { a, b, center, ccw } => {
                let r = (a.x - center.x).hypot(a.y - center.y);
                let from = (a.y - center.y).atan2(a.x - center.x);
                let mut sweep = (b.y - center.y).atan2(b.x - center.x) - from;
                if *ccw {
                    while sweep <= 0.0 {
                        sweep += std::f64::consts::TAU;
                    }
                } else {
                    while sweep >= 0.0 {
                        sweep -= std::f64::consts::TAU;
                    }
                }
                let step = if r > 0.01 { 2.0 * (1.0 - 0.01 / r).acos() } else { std::f64::consts::PI / 6.0 };
                let count = (sweep.abs() / step.max(1e-3)).ceil().clamp(2.0, 400.0) as usize;
                for i in 0..count {
                    let t = from + sweep * i as f64 / count as f64;
                    points.push([center.x + r * t.cos(), center.y + r * t.sin()]);
                }
            }
            _ => unreachable!("native thread profiles contain only lines and arcs"),
        }
    }
    points.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-9);
    while points.len() > 2 && (points[0][0] - points[points.len() - 1][0]).hypot(points[0][1] - points[points.len() - 1][1]) < 1e-9 {
        points.pop();
    }
    points
}

fn check_profile_side(spec: ThreadSpec, g: &ThreadGeom) -> Result<()> {
    let points = sampled_profile(g);
    let low = points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let high = points.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    if (high > -low) != spec.internal {
        return Err(Error::Invalid(format!(
            "thread profile side would be inferred incorrectly by native: sampled radial extents [{low:.12}, {high:.12}] mm; depth plus fit must reach the 0.15 pitch overshoot (strictly exceed it internally)"
        )));
    }
    Ok(())
}

// Inputs the pinned geometry ignores or replaces (qymcad-core thread.rs:301, 321, 350-355, 529, 554-575).
// Each would build something other than what is stored, so it is refused rather than passed on.
fn check_profile_inputs(spec: ThreadSpec, g: &ThreadGeom) -> Result<()> {
    if spec.standard == ThreadStandard::Buttress {
        return Err(Error::Invalid(
            "thread standard buttress is refused: native builds a symmetric 52 degree profile, not the 7/45 degree buttress".into(),
        ));
    }
    if spec.standard != ThreadStandard::Custom && (spec.custom_angle != 60.0 || spec.custom_depth != 0.0) {
        return Err(Error::Invalid(format!(
            "thread angle and depth apply only to standard custom; native ignores angle {} and depth {} for this standard",
            spec.custom_angle, spec.custom_depth
        )));
    }
    if spec.pitch > 0.0 && spec.pitch <= 1e-9 {
        return Err(Error::Invalid(format!(
            "thread pitch {} is at or below 1e-9 mm, which native replaces by the standard pitch; use 0 for the standard pitch",
            spec.pitch
        )));
    }
    if spec.standard == ThreadStandard::Custom && spec.custom_depth > 0.0 && spec.custom_depth <= 1e-9 {
        return Err(Error::Invalid(format!(
            "thread depth {} is at or below 1e-9 mm, which native replaces by its default; use 0 for the default",
            spec.custom_depth
        )));
    }
    let radii: Vec<f64> = g
        .groove
        .iter()
        .filter_map(|e| match e {
            qymcad_core::geom::ProfEdge::Arc { a, center, .. } => Some((a.x - center.x).hypot(a.y - center.y)),
            _ => None,
        })
        .collect();
    for (key, asked) in [("crest_r", spec.crest_r), ("root_r", spec.root_r)] {
        if let Some(r) = asked {
            if !radii.iter().any(|m| (m - r).abs() <= 1e-9 * r.max(1.0)) {
                return Err(Error::Invalid(format!(
                    "thread {key} {r} mm is not built as asked: native clamps or rounds it (built arc radii {radii:?}); choose a radius the profile can hold"
                )));
            }
        }
    }
    Ok(())
}

// The pinned kernel drops a runout at or below 1e-6 mm and scales both runouts down when they
// exceed .95 of the length (occt_helical.cpp:134-136, 676-677). Either would silently replace the stored value.
fn check_runout(lead_in: f64, lead_out: f64, length: f64) -> Result<()> {
    for (key, v) in [("lead_in", lead_in), ("lead_out", lead_out)] {
        if v > 0.0 && v <= 1e-6 {
            return Err(Error::Invalid(format!(
                "thread {key} runout {v} is at or below 1e-6 mm, which native treats as 0; use 0 or a larger runout"
            )));
        }
    }
    if lead_in + lead_out > 0.95 * length {
        return Err(Error::Invalid(format!(
            "thread runouts lead_in {lead_in} + lead_out {lead_out} exceed 0.95 of the length {length} mm ({:.12}); native would scale them down",
            0.95 * length
        )));
    }
    Ok(())
}

// Native ISO internal prep moves an undersize hole's cutter start to D1/2. Every other
// profile starts at the actual stock rim, including oversized external stock.
fn check_stock_open(spec: ThreadSpec, radius: f64) -> Result<()> {
    let g = spec.geometry();
    let relief = spec.radial_relief();
    // occt_helical.cpp only translates the cutter above this exact threshold.
    let relief = if relief > 1e-6 { relief } else { 0.0 };
    let start_gap = if spec.internal && spec.standard == ThreadStandard::MetricIso { (g.minor_d * 0.5 - radius).max(0.0) } else { 0.0 };
    if start_gap + relief > 0.13 * g.pitch {
        let minimum = g.minor_d + 2.0 * (relief - 0.13 * g.pitch);
        return Err(Error::Invalid(format!(
            "thread bore diameter {} is below open-groove limit {minimum:.9} mm: cutter start and radial relief would bury the groove; keep 0.02 pitch exposed",
            2.0 * radius
        )));
    }
    Ok(())
}

impl Session {
    /// Cut a native thread on exactly one circular stock rim. Side assertions are checked before creation.
    /// Precondition: the caller must not hold the kernel gate.
    pub fn thread(&mut self, body: Option<Id>, edge: &Sel, options: &ThreadOptions) -> Result<ThreadResult> {
        self.ensure_topology()?;
        let ((src, rim_id, spec, length, available, fit_reason), rebuild) = self.atomic(|s| {
            let src = s.source_body(body)?;
            let ids = s.resolve_sel(src, Element::Edges, edge)?;
            if ids.len() != 1 {
                return Err(Error::Invalid(format!(
                    "thread edge must select exactly one circular rim of body {src}, matched {}",
                    ids.len()
                )));
            }
            let rim =
                s.p.regen_edges
                    .get(&src)
                    .into_iter()
                    .flatten()
                    .find(|e| e.id == ids[0] && e.is_circular() && e.a.iter().zip(e.b).map(|(a, b)| (a - b).powi(2)).sum::<f64>() < 1e-12)
                    .ok_or_else(|| Error::Invalid(format!("thread edge {} is not a circular rim of source body {src}", ids[0])))?;
            let (c, axis, r) = (rim.center, rim.axis, rim.radius);
            let (standard, mut numbers) = options.numbers()?;
            let vars = s.p.param_map();
            let mut values = numbers
                .iter()
                .map(|(k, n)| {
                    let v = n.eval(&vars)?;
                    validate(k, v)?;
                    Ok((*k, v))
                })
                .collect::<Result<std::collections::HashMap<_, _>>>()?;
            let length = values["length"];
            check_runout(values["lead_in"], values["lead_out"], length)?;
            let mi = s.p.mesh_index(src).ok_or_else(|| Error::NotFound(format!("body {src}")))?;
            let mesh = &s.p.bodies[mi].mesh;
            let axis = qymcad_core::geom::axis_along_cylinder(mesh, c, axis, r)
                .ok_or_else(|| Error::Invalid("thread rim has no adjacent cylindrical stock".into()))?;
            let internal = qymcad_core::geom::cyl_side_from_mesh(mesh, c, axis, r, 0.0, length)
                .ok_or_else(|| Error::Invalid("cannot detect the material side of this thread rim".into()))?;
            match options.kind.as_str() {
                "auto" => (),
                "external" if !internal => (),
                "internal" if internal => (),
                "external" | "internal" => {
                    return Err(Error::Invalid(format!(
                        "thread kind {} disagrees with detected {} material side",
                        options.kind,
                        if internal { "internal" } else { "external" }
                    )))
                }
                _ => return Err(Error::Invalid("thread kind must be auto, external or internal".into())),
            }
            let mut spec = ThreadSpec {
                standard,
                nominal_d: values["nominal"],
                pitch: values["pitch"],
                starts: values["starts"] as u32,
                left: options.left,
                internal,
                fit: values["fit"],
                crest_r: (values["crest_r"] > 0.0).then_some(values["crest_r"]),
                root_r: (values["root_r"] > 0.0).then_some(values["root_r"]),
                custom_angle: values["angle"],
                custom_depth: values["depth"],
            };
            check_custom_angle(spec)?;
            let fit_reason = if options.fit.is_none() && spec.radial_relief() > 0.13 * spec.geometry().pitch {
                spec.fit = fit_limit(spec);
                values.insert("fit", spec.fit);
                *numbers.iter_mut().find(|(key, _)| *key == "fit").unwrap() = ("fit", spec.fit.into());
                "App default 0.2 mm lowered to the largest fit keeping radial relief 0.02 pitch below the 0.15 pitch cutter overshoot"
                    .into()
            } else if options.fit.is_none() {
                "App default 0.2 mm keeps the groove open with the required 0.02 pitch margin".into()
            } else {
                "Explicit fit keeps the groove open with the required 0.02 pitch margin".into()
            };
            check_fit(spec)?;
            check_stock_open(spec, r)?;
            let g = spec.geometry_in(Some(2.0 * r));
            check_profile_inputs(spec, &g)?;
            check_profile_depth(spec, &g)?;
            check_profile_side(spec, &g)?;
            if let Some(reason) = s.p.thread_refusal(src, (c, axis, r), spec, length) {
                return Err(Error::Invalid(crate::localization::error(&reason)));
            }
            let available = s.p.thread_face_span(src, (c, axis, r));
            let id = s.p.add_thread(src, ids[0], spec, length, values["lead_in"], values["lead_out"]);
            for (key, n) in &numbers {
                let text = n.expr().unwrap_or_else(|| values[key].to_string());
                s.p.write_feat_formula(&FeatNumber { node: id, key }, &text, PlainNumber::Stands)
                    .map_err(|e| Error::Expr(format!("thread {key}: {e:?}")))?;
            }
            s.set_node_name(id, options.name.as_deref())?;
            Ok((src, (id, ids[0]), spec, length, available, fit_reason))
        })?;
        let (body, edge) = rim_id;
        let g = spec.geometry();
        Ok(ThreadResult {
            body,
            name: self.node_name(body),
            source: src,
            edge,
            kind: if spec.internal { "internal" } else { "external" }.into(),
            standard: options
                .standard
                .clone()
                .unwrap_or_else(|| if spec.standard == ThreadStandard::TrapezoidalTr { "trapezoidal_tr" } else { "metric_iso" }.into()),
            nominal: spec.nominal_d,
            pitch: g.pitch,
            lead: g.lead,
            basic_major_diameter: g.major_d,
            basic_pitch_diameter: g.pitch_d,
            basic_minor_diameter: g.minor_d,
            fit: spec.fit,
            fit_reason,
            radial_relief: spec.radial_relief(),
            length,
            available_length: available,
            rebuild,
        })
    }

    /// Check every evaluated thread key before native regeneration can round counts or clamp dimensions.
    pub(crate) fn check_thread_dimensions(&self) -> std::result::Result<(), (Id, Error)> {
        for n in self.p.timeline.iter().filter(|n| !n.suppressed) {
            if let FeatureKind::Thread { spec, .. } = n.kind {
                self.evaluated_thread(n.id, spec).map_err(|e| (n.id, e))?;
            }
        }
        Ok(())
    }

    fn evaluated_thread(&self, id: Id, mut spec: ThreadSpec) -> Result<(ThreadSpec, f64)> {
        let vars = self.p.param_map();
        let (mut length, mut lead_in, mut lead_out) = (0.0, 0.0, 0.0);
        for key in ["length", "lead_in", "lead_out", "nominal", "pitch", "fit", "crest_r", "root_r", "angle", "depth", "starts"] {
            let target = qymcad_core::model::DimTarget::Feature { node: id, key: key.into() };
            let v = self
                .p
                .feat_dims
                .get(&id)
                .and_then(|m| m.get(key))
                .filter(|e| !e.trim().is_empty())
                .map(|e| Num::Expr(e.clone()).eval(&vars))
                .transpose()?
                .or_else(|| self.p.dim_target_value(&target))
                .ok_or_else(|| Error::Invalid(format!("thread {id} missing {key}")))?;
            validate(key, v)?;
            match key {
                "length" => length = v,
                "lead_in" => lead_in = v,
                "lead_out" => lead_out = v,
                "nominal" => spec.nominal_d = v,
                "pitch" => spec.pitch = v,
                "fit" => spec.fit = v,
                "crest_r" => spec.crest_r = (v > 0.0).then_some(v),
                "root_r" => spec.root_r = (v > 0.0).then_some(v),
                "angle" => spec.custom_angle = v,
                "depth" => spec.custom_depth = v,
                "starts" => spec.starts = v as u32,
                _ => (),
            }
        }
        check_runout(lead_in, lead_out, length)?;
        check_custom_angle(spec)?;
        check_fit(spec)?;
        check_profile_inputs(spec, &spec.geometry())?;
        check_profile_depth(spec, &spec.geometry())?;
        check_profile_side(spec, &spec.geometry())?;
        Ok((spec, length))
    }

    /// Stock is checked after regeneration: a source-sketch edit can move the saved rim during this pass.
    pub(crate) fn check_thread_stock(&self) -> Vec<crate::NodeIssue> {
        self.p
            .timeline
            .iter()
            .filter(|n| !n.suppressed)
            .filter_map(|n| {
                let FeatureKind::Thread { src, edge, spec, .. } = n.kind else { return None };
                let check = || -> Result<()> {
                    let (mut spec, length) = self.evaluated_thread(n.id, spec)?;
                    let rim = self
                        .p
                        .regen_edges
                        .get(&src)
                        .into_iter()
                        .flatten()
                        .find(|e| e.id == edge)
                        .ok_or_else(|| Error::Invalid(format!("thread source rim {edge} is unavailable")))?;
                    let mi = self.p.mesh_index(src).ok_or_else(|| Error::NotFound(format!("thread source body {src}")))?;
                    let mesh = &self.p.bodies[mi].mesh;
                    let axis = qymcad_core::geom::axis_along_cylinder(mesh, rim.center, rim.axis, rim.radius)
                        .ok_or_else(|| Error::Invalid("thread rim has no adjacent cylindrical stock".into()))?;
                    spec.internal = qymcad_core::geom::cyl_side_from_mesh(mesh, rim.center, axis, rim.radius, 0.0, length)
                        .ok_or_else(|| Error::Invalid("cannot detect the material side of this thread rim".into()))?;
                    check_fit(spec)?;
                    check_stock_open(spec, rim.radius)?;
                    let g = spec.geometry_in(Some(2.0 * rim.radius));
                    check_profile_inputs(spec, &g)?;
                    check_profile_depth(spec, &g)?;
                    check_profile_side(spec, &g)?;
                    if let Some(reason) = self.p.thread_refusal(src, (rim.center, axis, rim.radius), spec, length) {
                        return Err(Error::Invalid(crate::localization::error(&reason)));
                    }
                    Ok(())
                };
                check().err().map(|e| crate::NodeIssue { node: n.id, name: self.node_name(n.id), message: e.to_string() })
            })
            .collect()
    }
}
