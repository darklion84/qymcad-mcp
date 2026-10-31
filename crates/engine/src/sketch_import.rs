use crate::error::{Error, Result};
use crate::session::{Rebuild, Session};
use crate::{ContourInfo, Id, Num, PlaneRef};
use qymcad_core::feature::SketchPlane;
use qymcad_core::geom::{Point2, ProfEdge};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) const MAX_IMPORT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SketchImportFormat {
    Dxf,
    Svg,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SketchImportResult {
    pub sketch: Id,
    pub name: String,
    pub format: SketchImportFormat,
    pub entities: BTreeMap<String, usize>,
    pub contours: Vec<ContourInfo>,
    pub skipped: Vec<(String, usize)>,
    pub invalid: Vec<(String, usize)>,
    pub redrawn: Vec<(String, usize)>,
    pub scale: f64,
    pub unit_note: String,
    pub rebuild: Rebuild,
}

impl Session {
    pub fn sketch_import(
        &mut self,
        path: &Path,
        format: Option<SketchImportFormat>,
        plane: &PlaneRef,
        name: Option<&str>,
        scale: &Num,
    ) -> Result<SketchImportResult> {
        let format = format
            .or_else(|| match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
                Some("dxf") => Some(SketchImportFormat::Dxf),
                Some("svg") => Some(SketchImportFormat::Svg),
                _ => None,
            })
            .ok_or_else(|| Error::Invalid("sketch import format must be dxf or svg, or path must end in .dxf or .svg".into()))?;
        let size =
            std::fs::metadata(path).map_err(|e| Error::Io(format!("cannot read sketch import source {}: {e}", path.display())))?.len();
        if size > MAX_IMPORT_BYTES {
            return Err(Error::Invalid(format!(
                "sketch import file is {size} bytes; the maximum supported size is {} bytes (16 MiB)",
                MAX_IMPORT_BYTES
            )));
        }
        let parsed = match format {
            SketchImportFormat::Dxf => {
                qymcad_io::import_dxf(path.to_str().ok_or_else(|| Error::Invalid("sketch import path is not UTF-8".into()))?)
                    .map_err(|e| Error::Invalid(format!("cannot parse DXF: {e}")))?
            }
            SketchImportFormat::Svg => {
                qymcad_io::import_svg(path.to_str().ok_or_else(|| Error::Invalid("sketch import path is not UTF-8".into()))?)
                    .map_err(|e| Error::Invalid(format!("cannot parse SVG: {e}")))?
            }
        };
        let scale_value = scale.eval(&self.p.param_map())?;
        if !scale_value.is_finite() || scale_value <= 0.0 {
            return Err(Error::Invalid(format!("scale must be finite and positive, got {scale_value}")));
        }
        if parsed.curves.is_empty() {
            return Err(Error::Invalid("sketch import contains no curves".into()));
        }
        let plane = self.resolve_import_plane(plane)?;
        let curves = parsed.curves.into_iter().map(|curve| scale_curve(curve, scale_value)).collect::<Vec<_>>();
        let mut entities = BTreeMap::new();
        for curve in &curves {
            *entities.entry(curve_kind(curve).to_string()).or_insert(0) += 1;
        }
        let default_name = match format {
            SketchImportFormat::Dxf => "DXF sketch",
            SketchImportFormat::Svg => "SVG sketch",
        };
        let requested = Session::optional_object_name(name).unwrap_or(default_name).to_string();
        Session::check_object_name(&requested)?;
        let ((sketch, contours), rebuild) = self.atomic(|s| {
            let si = s.in_component(s.p.active_component, |p| p.import_sketch(requested.clone(), curves, None, plane));
            let sketch = s.p.sketches[si].id;
            s.track_datum_dependencies(sketch);
            let contours = s.sketch_info(sketch)?.contours;
            Ok((sketch, contours))
        })?;
        Ok(SketchImportResult {
            sketch,
            name: self.node_name(sketch),
            format,
            entities,
            contours,
            skipped: parsed.skipped,
            invalid: parsed.invalid,
            redrawn: parsed.redrawn,
            scale: scale_value,
            unit_note: match format {
                SketchImportFormat::Dxf => "DXF drawing units are interpreted as mm; $INSUNITS is not applied".into(),
                SketchImportFormat::Svg => "SVG units use 25.4/96 mm per px; SVG Y is flipped to CAD-up".into(),
            },
            rebuild,
        })
    }

    fn resolve_import_plane(&self, plane: &PlaneRef) -> Result<SketchPlane> {
        match plane {
            PlaneRef::Base(b) => Ok(SketchPlane::World((*b).into())),
            PlaneRef::Plane(id) if self.p.planes.iter().any(|p| p.id == *id) => Ok(SketchPlane::Datum(*id)),
            PlaneRef::Plane(id) => Err(Error::NotFound(format!("datum plane {id}"))),
            PlaneRef::Face { .. } => Err(Error::Invalid("sketch import supports base and datum planes only".into())),
        }
    }
}

fn point(p: Point2, scale: f64) -> Point2 {
    Point2::new(p.x * scale, p.y * scale)
}

fn scale_curve(curve: ProfEdge, scale: f64) -> ProfEdge {
    match curve {
        ProfEdge::Line { a, b } => ProfEdge::Line { a: point(a, scale), b: point(b, scale) },
        ProfEdge::Arc { a, b, center, ccw } => ProfEdge::Arc { a: point(a, scale), b: point(b, scale), center: point(center, scale), ccw },
        ProfEdge::Circle { center, r } => ProfEdge::Circle { center: point(center, scale), r: r * scale },
        ProfEdge::Oval { centre, major, minor, from, sweep } => {
            ProfEdge::Oval { centre: point(centre, scale), major: point(major, scale), minor: point(minor, scale), from, sweep }
        }
        ProfEdge::Spline(mut spline) => {
            for pole in &mut spline.poles {
                *pole = point(*pole, scale);
            }
            ProfEdge::Spline(spline)
        }
    }
}

fn curve_kind(curve: &ProfEdge) -> &'static str {
    match curve {
        ProfEdge::Line { .. } => "line",
        ProfEdge::Arc { .. } => "arc",
        ProfEdge::Circle { .. } => "circle",
        ProfEdge::Oval { .. } => "ellipse",
        ProfEdge::Spline(_) => "spline",
    }
}
