//! Typed root-level native Parts and Rigid/Slider joints.
use super::{
    common::{err, ObjRef},
    tool,
    topology::SelArg,
    Tool,
};
use qymcad_engine::{AssemblyJointKind, BaseName, JointAnchor, JointCreate, JointMode, JointRef, Num, PartRef, PartRotation};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PartNew {
    /// Name of the new root-child Part. Creates and activates a fresh Part; use rename for the initial Part.
    name: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PartSelect {
    /// Part id or unique name; {root:true} selects the root assembly navigation context.
    part: PartRef,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RotationArg {
    /// Finite nonzero parent-local rotation axis [x,y,z].
    axis: [f64; 3],
    /// Literal finite rotation angle in degrees.
    angle_deg: f64,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PartPlace {
    /// Root-child free, ungrounded Part id or unique name. Constrained Parts move with joint_set.
    part: PartRef,
    /// Replacement translation [x,y,z] in parent-local millimetres. Does not change local geometry.
    translation_mm: [f64; 3],
    /// Replacement axis-angle rotation in parent-local coordinates; omitted means identity rotation.
    rotation: Option<RotationArg>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Ground {
    /// Root-child Part id or unique name.
    part: PartRef,
    /// True fixes the Part in its parent; false releases grounding. The mechanism is solved and checked.
    grounded: bool,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Anchor {
    /// Current body id or unique name. Its owner Part determines the connector's local coordinate frame.
    body: ObjRef,
    /// Exactly one current planar face: persistent id or existing face selection description, in part-local coordinates.
    face: SelArg,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct JointAxis {
    /// Native local travel direction on both anchors: XY gives +Z, XZ gives -Y, YZ gives +X.
    base_plane: BaseName,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Create {
    /// Optional native joint label. Duplicate labels require ids when referenced later.
    name: Option<String>,
    /// Rigid holds a pose with no freedom; Slider permits motion along an explicit local travel axis.
    kind: AssemblyJointKind,
    /// Planar face anchor on grounded root-child Part A.
    a: Anchor,
    /// Planar face anchor on a different root-child Part B; Slider B must be ungrounded.
    b: Anchor,
    /// align aligns connector frames; as_built captures the existing relative pose before the first solve.
    mode: JointMode,
    /// Required for Slider, refused for Rigid. The base-plane normal is the signed local travel axis.
    axis: Option<JointAxis>,
    /// Optional absolute Slider travel in mm: number or native formula. Zero explicitly requests zero travel.
    offset: Option<Num>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Release {
    Offset,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Set {
    /// Native joint id or unique name; only Slider offset is editable in package A.
    joint: JointRef,
    /// Absolute travel in mm, number or native formula. Numeric requests clear a previous formula.
    offset: Option<Num>,
    /// Give "offset" instead of offset to clear both formula and drive, retaining the reached pose for dragging.
    release: Option<Release>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Delete {
    /// Native joint id or unique name. Removes only its unshared, non-standalone auto-connectors and keeps placements.
    joint: JointRef,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Info {}

fn changed(state: &mut super::State, result: Value) -> Value {
    state.skip_history = result["changed"] == false;
    result
}

pub fn tools() -> Vec<Tool> {
    vec![
        tool("part_new", "Create and activate a native multi-body Part directly under the root Assembly. The current navigation context does not change the parent. Undoable; allowed in batch. Returns part id/name/parent and active_component.", |st, a: PartNew| st.doc()?.part_new(&a.name).map_err(err)),
        tool("part_select", "Select a Part by typed id/name, or {root:true} for assembly navigation. Persisted and undoable when changed; a same-context no-op skips history. Allowed in batch. Root navigation does not allow geometry in an Assembly.", |st, a: PartSelect| {
            let result = st.doc()?.part_select(&a.part).map_err(err)?;
            Ok(changed(st, result))
        }),
        tool("part_place", "Replace a free ungrounded root-child Part's parent-local rigid placement. Leaves body ids, local B-reps and timeline unchanged. Grounded or constrained Parts refuse; use joint_set for mechanism travel. Undoable; allowed in batch.", |st, a: PartPlace| {
            let rotation = a.rotation.map(|rotation| PartRotation {axis:rotation.axis, angle_deg:rotation.angle_deg});
            let result = st.doc()?.part_place(&a.part, a.translation_mm, rotation.as_ref()).map_err(err)?;
            Ok(changed(st, result))
        }),
        tool("ground", "Set grounding on a root-child native Part, then solve and verify the mechanism atomically. Refuses ungrounding a joint's anchor A Part or grounding a Slider's moving B Part. No implicit grounding is introduced by joint_create. Undoable; allowed in batch; same-state no-op skips history.", |st, a: Ground| {
            let result = st.doc()?.ground(&a.part, a.grounded).map_err(err)?;
            Ok(changed(st, result))
        }),
        tool("joint_create", "Create a native Rigid or Slider between exactly one current planar face on each of two root-child Parts. A must be grounded. Slider requires ungrounded B and axis.base_plane; XZ travels -Y. as_built preserves the existing pose before solving. Anchors accept descriptions so batch needs no topology call. Optional offset is absolute Slider drive, including zero. Returns connectors, placements, native slot/drive/formula/limits and health; failures restore everything. Undoable; allowed in batch.", |st, a: Create| {
            let s = st.doc()?;
            let create = JointCreate {name:a.name, kind:a.kind,
                a:JointAnchor {body:a.a.body.resolve(s)?,face:a.a.face.resolve(s)?},
                b:JointAnchor {body:a.b.body.resolve(s)?,face:a.b.face.resolve(s)?},
                mode:a.mode,axis:a.axis.map(|axis| axis.base_plane),offset:a.offset};
            s.joint_create(&create).map_err(err)
        }),
        tool("joint_set", "Set absolute Slider offset in mm or release it. Numeric offset clears a prior formula and writes a native drive. A formula persists natively and reasserts after app dragging on regeneration; release:'offset' clears both formula and drive and retains the pose. Exactly one action. Checks requested versus read travel and existing limits; no limit editing. Undoable; allowed in batch.", |st, a: Set| {
            let release = a.release.map(|Release::Offset| "offset");
            let result = st.doc()?.joint_set(&a.joint,a.offset.as_ref(),release).map_err(err)?;
            Ok(changed(st, result))
        }),
        tool("joint_delete", "Delete one native joint and its unshared non-standalone auto-connectors while retaining current Part placements; solve and verify the remaining mechanism. Undoable; allowed in batch.", |st, a: Delete| st.doc()?.joint_delete(&a.joint).map_err(err)),
        tool("assembly_info", "Inspect the root tree, active component, native Parts/placements/grounding/owned bodies, connectors and ordered joints. Reports local/world transforms, actual travel axes, native slots/readings/drives/formulas/limits and separate faults/violations/conflict. Existing unsupported records are preserved and reported. Read-only.", |st, _: Info| Ok(st.doc()?.assembly_info())),
    ]
}
