//! Headless modelling engine over the QymCAD kernel.
//!
//! This crate is the ONLY place that touches the QymCAD API. When QymCAD is upgraded, the changes land here
//! and nowhere else (see docs/ARCHITECTURE.md and docs/UPGRADING.md).

mod error;
mod export;
mod features;
mod history;
#[cfg(test)]
mod history_tests;
mod info;
mod modifiers;
mod params;
mod patterns;
mod pngfile;
mod render;
mod revolve;
mod session;
mod sketch;
mod topology;
mod value;

pub use error::{Error, Result};
pub use export::{ExportFormat, ExportReport, ExportedBody, Quality};
pub use features::{Direction, Extrude, Op};
pub use history::{Snapshot, ToolCall, Undone, UNDO_LIMIT};
pub use info::{DocInfo, NodeInfo};
pub use modifiers::{Hole, HoleKind, Side};
pub use params::ParamInfo;
pub use patterns::{ArrayDir, AxisRef};
pub use qymcad_core::model::Id;
pub use render::{Rendered, View, RENDER_BACKGROUND, RENDER_MAX_SIDE, RENDER_MIN_SIDE};
pub use revolve::Revolve;
pub use session::{BodyInfo, NodeIssue, Rebuild, Session};
pub use sketch::{
    Added, ArcSpec, BaseName, ConstrainSpec, Constrained, ConstraintInfo, ConstraintKind, ContourInfo, DistAxis, EntityInfo, FrameRef,
    LineSpec, PlaneRef, PointInfo, PolygonSpec, PolylineSpec, SketchDetail, SketchInfo, SketchRef, SketchWorldFrame, SlotSpec, Xy,
};
pub use topology::{Axis, EdgeInfo, EdgeKind, Element, FaceInfo, FaceKind, Role, Sel, Topology};
pub use value::Num;

/// The QymCAD release this engine is built against. Must match the tag in the workspace Cargo.toml.
pub const QYMCAD_VERSION: &str = "0.1.0-dev.20261001";
