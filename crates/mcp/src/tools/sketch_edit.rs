//! `sketch_edit`: trim, extend, break, offset, corner and point mirror on sketch curves.
use super::common::{err, ObjRef};
use super::sketch::sketch_result;
use super::{tool, Tool};
use qymcad_engine::{
    BreakSpec, CornerSize, CornerSpec, CurvePickSpec, EntityRef, ExtendSpec, MirrorPointSpec, Num, PointSel, SketchCurveSel,
    SketchOffsetSpec,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

/// One sketch edit, chosen by `op`. Picks (`at`, `toward`) are used once; sizes stay driving dimensions.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum EditArgs {
    /// Remove the part of a curve between the intersections around `at`; a curve with none is removed whole.
    Trim {
        /// Sketch id or name.
        sketch: ObjRef,
        /// Exactly one curve.
        curve: EntityRef,
        /// A point near the part to remove, in sketch coordinates.
        at: [Num; 2],
    },
    /// Extend the open end nearest `at` (or both ends) to a boundary curve.
    Extend {
        /// Sketch id or name.
        sketch: ObjRef,
        /// Exactly one open curve.
        curve: EntityRef,
        /// A point near the end to extend.
        at: [Num; 2],
        /// The boundary to reach; default: the nearest one along the end.
        #[serde(default)]
        target: Option<EntityRef>,
        /// Extend both ends.
        #[serde(default)]
        both: bool,
    },
    /// Split a curve at `at`, or at all its intersections with `intersections: true`.
    Break {
        /// Sketch id or name.
        sketch: ObjRef,
        /// Exactly one curve.
        curve: EntityRef,
        /// The split point; give it or `intersections: true`, not both.
        #[serde(default)]
        at: Option<[Num; 2]>,
        /// Split at every intersection with other curves (construction curves included).
        #[serde(default)]
        intersections: bool,
    },
    /// Offset a chain or loop of curves by `distance` to the side of `toward`. The distance stays a driving
    /// dimension (every one the offset makes takes the formula); ellipse and spline offsets are native
    /// associative copies.
    Offset {
        /// Sketch id or name.
        sketch: ObjRef,
        /// The curves to offset.
        curves: SketchCurveSel,
        /// Distance, mm, number or formula; a negative value goes to the other side of `toward`.
        distance: Num,
        /// A point on the side to offset to.
        toward: [Num; 2],
        /// Name the distance (only when the offset makes exactly one dimension).
        #[serde(default)]
        dimension_name: Option<String>,
    },
    /// Round or bevel the corner where two curves meet at a point.
    Corner {
        /// Sketch id or name.
        sketch: ObjRef,
        /// The two curves, in order (a chamfer's `first` is measured along the first).
        curves: [EntityRef; 2],
        /// The shared end point.
        point: PointSel,
        /// Fillet by radius, chord or arc_length; chamfer symmetric (by the cut's length), two_dist or
        /// dist_angle (`second` in degrees).
        size: CornerSize,
    },
    /// Copy curves through a half turn about a point. The copy is tied to its source through the centre, so it
    /// follows the source's position, the centre and shared formulas; a size dimension it carried stays its own.
    MirrorPoint {
        /// Sketch id or name.
        sketch: ObjRef,
        /// The curves to copy.
        curves: SketchCurveSel,
        /// The mirror centre.
        about: PointSel,
    },
}

impl EditArgs {
    fn sketch(&self) -> &ObjRef {
        match self {
            Self::Trim { sketch, .. }
            | Self::Extend { sketch, .. }
            | Self::Break { sketch, .. }
            | Self::Offset { sketch, .. }
            | Self::Corner { sketch, .. }
            | Self::MirrorPoint { sketch, .. } => sketch,
        }
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![tool(
        "sketch_edit",
        "Edit sketch curves, atomically, the way the QymCAD app does: op trim, extend, break, offset, corner (fillet or chamfer) or mirror_point. New ends of trimmed, extended or split curves are tied to the curves they lie on, and the split points of a line stay on that line. Corner example: {\"op\":\"corner\",\"sketch\":\"s\",\"curves\":[12,13],\"point\":9,\"size\":{\"mode\":\"radius\",\"value\":\"r\"}}; point is the shared end point id; size modes radius, chord, arc_length (fillet; value) and symmetric (value = bevel length, not setback), two_dist, dist_angle (chamfer; first, second = setback along the first curve and the second setback or the angle in degrees from the first curve). Returns edit: created/removed/changed entity and point ids, dimensions (indices of the size dimensions it made), dropped_formulas (formula dimensions the edit removed), released_offsets, dof and free_points; then the sketch and the rebuild of features built on it. A sketch left with freedom is accepted: dimension the rest with sketch_constrain.",
        |st, a: EditArgs| {
            let s = st.doc()?;
            let sketch = a.sketch().resolve(s)?;
            let (edit, r) = s
                .sketch_edit(sketch, |s| match a {
                    EditArgs::Trim { curve, at, .. } => s.sketch_trim(sketch, &CurvePickSpec { curve, at }),
                    EditArgs::Extend { curve, at, target, both, .. } => s.sketch_extend(sketch, &ExtendSpec { curve, at, target, both }),
                    EditArgs::Break { curve, at, intersections, .. } => s.sketch_break(sketch, &BreakSpec { curve, at, intersections }),
                    EditArgs::Offset { curves, distance, toward, dimension_name, .. } => {
                        s.sketch_offset(sketch, &SketchOffsetSpec { curves, distance, toward, dimension_name })
                    }
                    EditArgs::Corner { curves, point, size, .. } => s.sketch_corner(sketch, &CornerSpec { curves, point, size }),
                    EditArgs::MirrorPoint { curves, about, .. } => s.sketch_mirror_point(sketch, &MirrorPointSpec { curves, about }),
                })
                .map_err(err)?;
            let mut out = sketch_result(s, sketch, r.as_ref())?;
            out["edit"] = json!(edit);
            Ok(out)
        },
    )]
}
