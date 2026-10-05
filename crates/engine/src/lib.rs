//! Headless modelling engine over the QymCAD kernel.
//!
//! This crate is the ONLY place that touches the QymCAD API. When QymCAD is upgraded, the changes land here
//! and nowhere else (see docs/ARCHITECTURE.md and docs/UPGRADING.md).

mod error;
mod features;
mod info;
mod params;
mod session;
mod sketch;
mod value;

pub use error::{Error, Result};
pub use features::{Direction, Extrude, Op};
pub use info::{DocInfo, NodeInfo};
pub use params::ParamInfo;
pub use qymcad_core::model::Id;
pub use session::{BodyInfo, NodeIssue, Rebuild, Session};
pub use sketch::{
    Added, ArcSpec, BaseName, ConstrainSpec, Constrained, ConstraintInfo, ConstraintKind, ContourInfo, DistAxis, EntityInfo, FrameRef,
    LineSpec, PlaneRef, PointInfo, PolygonSpec, PolylineSpec, SketchDetail, SketchInfo, SketchRef, SlotSpec, Xy,
};
pub use value::Num;

/// The QymCAD release this engine is built against. Must match the tag in the workspace Cargo.toml.
pub const QYMCAD_VERSION: &str = "0.1.0-dev.20261001";
