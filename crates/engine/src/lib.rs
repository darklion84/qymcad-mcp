//! Headless modelling engine over the QymCAD kernel.
//!
//! This crate is the ONLY place that touches the QymCAD API. When QymCAD is upgraded, the changes land here
//! and nowhere else (see docs/ARCHITECTURE.md and docs/UPGRADING.md).

mod dependencies;
mod dimensions;
mod error;
mod export;
mod features;
mod history;
#[cfg(test)]
mod history_tests;
mod imports;
mod info;
mod localization;
#[cfg(test)]
mod localization_tests;
mod modifiers;
mod params;
mod patterns;
mod pngfile;
mod render;
mod revolve;
mod session;
mod sketch;
mod split;
mod sweep;
#[cfg(test)]
mod test_support;
mod thread;
mod topology;
mod value;

pub use dimensions::{DimensionEdit, DimensionInfo, DimensionTarget};
pub use error::{Error, Result};
pub use export::{ExportFormat, ExportReport, ExportedBody, Quality};
pub use features::{Direction, Extrude, Op};
pub use history::{Snapshot, ToolCall, Undone, UNDO_LIMIT};
pub use info::{DocInfo, NodeInfo};
pub use modifiers::{Hole, HoleKind, Side};
pub use params::ParamInfo;
pub use patterns::{ArrayDir, AxisRef};
pub use qymcad_core::model::Id;
pub use qymcad_core::model::ProjSource;
pub use render::{Rendered, View, RENDER_BACKGROUND, RENDER_MAX_SIDE, RENDER_MIN_SIDE};
pub use revolve::Revolve;
pub use session::{BodyInfo, NodeIssue, Rebuild, Session};
pub use sketch::{
    Added, ArcSpec, BaseName, BreakSpec, ConstrainSpec, Constrained, ConstraintInfo, ConstraintKind, ContourInfo, CornerSize, CornerSpec,
    CurvePickSpec, CurveQuery, DistAxis, EditedSketch, EllipseArcSpec, EllipseSpec, EntityInfo, EntityRef, ExtendSpec, FrameRef, LineSpec,
    MirrorPointSpec, OriginRef, PlaneRef, PointInfo, PointRole, PointRoleSel, PointSel, PolygonSpec, PolylineSpec, ProjectionResult,
    RectAnchor, RectSpec, SketchCurveKind, SketchCurveSel, SketchDetail, SketchInfo, SketchOffsetSpec, SketchRef, SketchWorldFrame,
    SlotSpec, SplineSpec, TangentCircleSpec, TextFont, TextInfo, TextSpec, Xy,
};
pub use split::{SplitOptions, SplitResult};
pub use sweep::{Loft, LoftSection, Sweep, SweepPath};
pub use thread::{ThreadOptions, ThreadResult};
pub use topology::{Axis, EdgeInfo, EdgeKind, Element, FaceInfo, FaceKind, Role, Sel, SelectionKind, Topology};
pub use value::Num;

/// The QymCAD release this engine is built against. Must match the tag in the workspace Cargo.toml.
pub const QYMCAD_VERSION: &str = "0.1.1-dev.20261026";

#[cfg(test)]
mod version_tests {
    /// The server is numbered after the QymCAD release it is built for; a server fix adds `.N` (ADR 0003).
    #[test]
    fn the_server_version_is_the_qymcad_release() {
        let ours = env!("CARGO_PKG_VERSION");
        let fix = ours.strip_prefix(super::QYMCAD_VERSION).and_then(|rest| rest.strip_prefix('.'));
        assert!(
            ours == super::QYMCAD_VERSION || fix.is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())),
            "workspace version {ours} must be QymCAD {} or {}.N",
            super::QYMCAD_VERSION,
            super::QYMCAD_VERSION
        );
    }
}
