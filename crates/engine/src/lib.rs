//! Headless modelling engine over the QymCAD kernel.
//!
//! This crate is the ONLY place that touches the QymCAD API. When QymCAD is upgraded, the changes land here
//! and nowhere else (see docs/ARCHITECTURE.md and docs/UPGRADING.md).

mod appearance;
mod assembly;
#[cfg(test)]
mod assembly_contract_tests;
#[cfg(test)]
mod assembly_lifecycle_tests;
#[cfg(test)]
mod assembly_tests;
mod body_boolean;
mod dependencies;
mod dimensions;
mod error;
mod export;
mod features;
mod history;
#[cfg(test)]
mod history_tests;
mod hole_sketch;
mod import_tool;
mod imports;
mod info;
mod instances;
mod localization;
#[cfg(test)]
mod localization_tests;
mod measure;
mod mesh_import;
mod mesh_recognise;
mod modifiers;
mod move_body;
#[cfg(test)]
mod ownership_tests;
mod params;
mod part_delete;
mod patterns;
mod pngfile;
mod rename;
mod render;
mod reorder;
#[cfg(test)]
mod reorder_tests;
mod revolve;
mod session;
mod sketch;
mod sketch_import;
mod split;
mod sweep;
#[cfg(test)]
mod test_support;
mod thread;
mod topology;
mod value;

pub use appearance::{AppearanceBody, AppearanceResult, AppearanceScope};
pub use assembly::{
    AssemblyJointKind, JointAnchor, JointCreate, JointLimits, JointMode, JointRef, JointSlot, LimitEdit, PartRef, PartRotation,
};
pub use body_boolean::{BodyBooleanOptions, BodyBooleanResult, BooleanOp};
pub use dimensions::{DimensionEdit, DimensionInfo, DimensionTarget};
pub use error::{Error, Result};
pub use export::{ExportFormat, ExportReport, ExportedBody, Quality};
pub use features::{Direction, Extrude, Op};
pub use history::{
    LostBodyMetadata, RollbackInfo, RollbackResult, Snapshot, SuppressResult, SuppressedFeature, ToolCall, Undone, UNDO_LIMIT,
};
pub use hole_sketch::HoleSketch;
pub use import_tool::{ImportResult, ImportedBody};
pub use info::{ComponentInfo, DocInfo, NodeInfo};
pub use measure::{MeasureAt, MeasureOperand, MeasureResult, ResolvedMeasure};
pub use mesh_import::{MeshFormat, MeshImportResult, MeshPieceResult, MeshUnit};
pub use mesh_recognise::{MeshRecogniseMode, MeshRecogniseResult};
pub use modifiers::{Chamfer, ChamferInfo, Draft, Hole, HoleKind, Side};
pub use move_body::{Move, MoveResult, Rotation};
pub use params::ParamInfo;
pub use patterns::{ArrayDir, AxisRef};
pub use qymcad_core::model::Id;
pub use qymcad_core::model::ProjSource;
pub use rename::{RenameResult, RenameTarget, RenamedTarget};
pub use render::{Rendered, View, RENDER_BACKGROUND, RENDER_MAX_SIDE, RENDER_MIN_SIDE};
pub use reorder::{RehostedSketch, ReorderResult, RewiredFeature};
pub use revolve::Revolve;
pub use session::{BodyInfo, NodeIssue, Rebuild, Session};
pub use sketch::{
    Added, AddedPoint, ArcSpec, BaseName, BreakSpec, ConstrainSpec, Constrained, ConstraintInfo, ConstraintKind, ContourInfo, CornerSize,
    CornerSpec, CurvePickSpec, CurveQuery, DistAxis, EditedSketch, EllipseArcSpec, EllipseSpec, EntityInfo, EntityRef, ExtendSpec,
    FrameRef, IsolatedPointInfo, LineSpec, MirrorPointSpec, OriginRef, PlaneRef, PointInfo, PointRole, PointRoleSel, PointSel, PolygonSpec,
    PolylineSpec, ProjectionResult, RectAnchor, RectSpec, RotateSpec, SketchCurveKind, SketchCurveSel, SketchDetail, SketchInfo,
    SketchOffsetSpec, SketchPatternKind, SketchPatternResult, SketchRef, SketchWorldFrame, SlotSpec, SplineSpec, TangentCircleSpec,
    TextFont, TextInfo, TextSpec, TransformSpec, Xy,
};
pub use sketch_import::{SketchImportFormat, SketchImportResult};
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
