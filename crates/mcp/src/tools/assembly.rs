//! Typed root-level native Parts and Rigid/Slider joints.
use super::{
    common::{err, ObjRef},
    tool,
    topology::SelArg,
    Tool,
};
use qymcad_engine::{
    AssemblyJointKind, BaseName, JointAnchor, JointCreate, JointLimits, JointMode, JointRef, JointSlot, LimitEdit, Num, PartRef,
    PartRotation,
};
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
    /// Rigid/Slider: exactly one current planar face, persistent id or existing face selection description, in
    /// part-local coordinates. Give exactly one of face or edge.
    face: Option<SelArg>,
    /// Revolute: exactly one current straight edge (the hinge axis), persistent id or edge selection description, in
    /// part-local coordinates. Give exactly one of face or edge.
    edge: Option<SelArg>,
}
impl Anchor {
    fn resolve(self, s: &qymcad_engine::Session) -> Result<JointAnchor, String> {
        let body = self.body.resolve(s)?;
        match (self.face, self.edge) {
            (Some(face), None) => Ok(JointAnchor::face(body, face.resolve(s)?)),
            (None, Some(edge)) => Ok(JointAnchor::edge(body, edge.resolve(s)?)),
            _ => Err("joint anchor requires exactly one of face or edge".into()),
        }
    }
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
    /// Rigid holds a pose with no freedom; Slider permits motion along an explicit local travel axis; Revolute turns
    /// about a hinge made of two straight edges.
    kind: AssemblyJointKind,
    /// Anchor on grounded root-child Part A: a planar face (Rigid/Slider) or a straight edge (Revolute).
    a: Anchor,
    /// Anchor on a different root-child Part B; Slider/Revolute B must be ungrounded.
    b: Anchor,
    /// align aligns connector frames (a Revolute's B edge frame lands on A's, so the Part snaps onto the hinge);
    /// as_built captures the existing relative pose before the first solve (a Revolute then needs collinear edges).
    mode: JointMode,
    /// Required for Slider, refused for Rigid. The base-plane normal is the signed local travel axis.
    axis: Option<JointAxis>,
    /// Optional absolute Slider travel in mm: number or native formula. Zero explicitly requests zero travel.
    offset: Option<Num>,
    /// Optional absolute Revolute angle in degrees within (-180, 180]: number or native formula.
    angle: Option<Num>,
    /// Optional limits of the free slot (Slider offset in mm, Revolute angle in degrees within (-180, 180]),
    /// applied before the first drive, as joint_limits.
    limits: Option<CreateLimits>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateLimits {
    /// Lower bound: number or native formula.
    min: Option<Num>,
    /// Upper bound: number or native formula.
    max: Option<Num>,
}
/// Present-but-null is distinct from omitted: `Some(None)` clears a bound.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<Num>>, D::Error> {
    Option::<Num>::deserialize(d).map(Some)
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Limits {
    /// Native joint id or unique name (Slider or Revolute).
    joint: JointRef,
    /// The free slot to limit; defaults to the joint's only free slot (Slider: offset, Revolute: angle). Naming another slot is refused.
    slot: Option<JointSlot>,
    /// Lower bound: number or native formula ticks the bound on; null ticks it off (its formula stays dormant); omitted keeps it.
    #[serde(default, deserialize_with = "present")]
    min: Option<Option<Num>>,
    /// Upper bound: number or native formula ticks the bound on; null ticks it off (its formula stays dormant); omitted keeps it.
    #[serde(default, deserialize_with = "present")]
    max: Option<Option<Num>>,
}
fn edit(v: Option<Option<Num>>) -> LimitEdit {
    match v {
        None => LimitEdit::Keep,
        Some(None) => LimitEdit::Clear,
        Some(Some(n)) => LimitEdit::Set(n),
    }
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Release {
    Offset,
    Angle,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Set {
    /// Native joint id or unique name: a Slider (offset) or a Revolute (angle).
    joint: JointRef,
    /// Slider: absolute travel in mm, number or native formula. Numeric requests clear a previous formula.
    offset: Option<Num>,
    /// Revolute: absolute angle in degrees within (-180, 180], number or native formula.
    angle: Option<Num>,
    /// Give "offset" or "angle" instead of a value to clear both formula and drive, retaining the reached pose.
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
struct Derived {
    /// Source root-child Part id or unique name.
    source: PartRef,
    /// Required name of the new root-child Part; must differ from every existing component name.
    name: String,
    /// Optional parent-local translation [x,y,z] in mm, as part_place. Omitted keeps the app's own placement.
    translation_mm: Option<[f64; 3]>,
    /// Optional axis-angle rotation, as part_place; requires translation_mm.
    rotation: Option<RotationArg>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PartDelete {
    /// Root-child Part id or unique name to delete with all its bodies, sketches and datums.
    part: PartRef,
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
        tool("ground", "Set grounding on a root-child native Part, then solve and verify the mechanism atomically. Refuses ungrounding a joint's anchor A Part or grounding a Slider's or Revolute's moving B Part (a grounded B does not move and QymCAD reports nothing, F-159). No implicit grounding is introduced by joint_create. Undoable; allowed in batch; same-state no-op skips history.", |st, a: Ground| {
            let result = st.doc()?.ground(&a.part, a.grounded).map_err(err)?;
            Ok(changed(st, result))
        }),
        tool("joint_create", "Create a native Rigid or Slider between exactly one current planar face on each of two root-child Parts, or a Revolute hinge between exactly one straight edge on each (the app's Joint tool with an edge picked). A must be grounded. Slider requires ungrounded B and axis.base_plane; XZ travels -Y. Revolute requires ungrounded B, refuses face anchors (a planar face turns the Part about its normal, F-161), axis and offset, and takes angle in degrees within (-180, 180] (readings wrap, F-158); the signed angle follows the edge frames, so read the reached pose from the result. Revolute mode as_built needs collinear edges (place the Part on the hinge first: QymCAD turns about B's edge and ignores A's, F-162); mode align puts B's edge frame on A's, so the Part snaps onto the hinge (measured correct, F-163). A later feature (push_face, fillet, chamfer, reorder) that moves or removes an anchored hinge edge on the current body is refused as invalidating the anchors, because QymCAD would go on turning the Part about the consumed body's old edge (F-160). as_built preserves the existing pose before solving. Anchors accept descriptions so batch needs no topology call. Optional offset is absolute Slider drive, including zero; optional limits {min,max} are set before it, so an offset past them is clamped and reported. Returns connectors, placements, native slot/drive/formula/limits and health; failures restore everything. Undoable; allowed in batch.", |st, a: Create| {
            let s = st.doc()?;
            let create = JointCreate {name:a.name, kind:a.kind,
                a:a.a.resolve(s)?,
                b:a.b.resolve(s)?,
                mode:a.mode,axis:a.axis.map(|axis| axis.base_plane),offset:a.offset, angle:a.angle, limits:a.limits.map(|l| JointLimits {min:l.min, max:l.max})};
            s.joint_create(&create).map_err(err)
        }),
        tool("joint_set", "Set absolute Slider offset in mm or Revolute angle in degrees (within (-180, 180], F-158), or release the slot. Numeric offset clears a prior formula and writes a native drive. A formula persists natively and reasserts after app dragging on regeneration; release:'offset' or 'angle' clears both formula and drive and retains the pose. Exactly one action. A request outside the joint's limits is clamped as in the app and reported as clamped {joint, slot, requested, applied}, also on the step entry in a compact batch; the reached travel is checked against the applied value. Refused when this joint's limits or angle domain are faulty (limit_faults); faults of other joints do not block it. Undoable; allowed in batch.", |st, a: Set| {
            let s = st.doc()?;
            let result = match (a.offset, a.angle, a.release) {
                (None, Some(angle), None) => s.joint_set_slot(&a.joint, JointSlot::Angle, Some(&angle), false),
                (None, None, Some(Release::Angle)) => s.joint_set_slot(&a.joint, JointSlot::Angle, None, true),
                (offset, None, release) => s.joint_set(&a.joint, offset.as_ref(), release.map(|_| "offset")),
                _ => return Err("joint_set requires exactly one of offset, angle or release".into()),
            }
            .map_err(err)?;
            Ok(changed(st, result))
        }),
        tool("joint_limits", "The app's Limits section of the joint box for a Slider's offset (mm) or a Revolute's angle (degrees, bounds within [-180, 180]: QymCAD compares them with a reading wrapped to (-180, 180], F-158): each bound is a tick plus a number or formula (formulas persist natively under min<slot>/max<slot> and follow parameters); a typed number is held to ±100000 as the app's fields are, a formula's value is not. An angle bound must lie in (-180, 180]: -180 reads as 180. Omitted keeps a bound, null ticks it off as the app does: the value goes and the formula stays dormant, read by nothing (reported as dormant_limit_formulas; a formula given again, or a bound ticked on in the app, makes it live again, F-164). A limit clamps a value as QymCAD does: a bound past the current travel moves the Part to it, and the result reports clamped {joint, slot, requested, applied}; at_limit lists {joint, slot} wherever it is reported. Refused: min above max, now or after a parameter edit that this joint's formulas read (QymCAD would send every value to max, F-157); a Rigid joint (no free slot). A joint that arrives with such limits (a file edited in the app) does not block edits that do not touch it; it is listed in limit_faults. Undoable; allowed in batch.", |st, a: Limits| {
            let result = st.doc()?.joint_limits(&a.joint, a.slot, &edit(a.min), &edit(a.max)).map_err(err)?;
            Ok(changed(st, result))
        }),
        tool("joint_delete", "Delete one native joint and its unshared non-standalone auto-connectors while retaining current Part placements; solve and verify the remaining mechanism. Undoable; allowed in batch.", |st, a: Delete| st.doc()?.joint_delete(&a.joint).map_err(err)),
        tool("part_instance", "The app's tree action Make a clone (act-clone-part): a new root-child Part whose one native PartInstance body repeats the source Part's single solid body, so every edit of the source reaches it, while it is placed, grounded and jointed as a Part of its own. A clone of a clone repeats the original; the result reports part.instance_of. Without translation_mm the clone stands beside the source along X, as in the app. Refused unless the source has exactly one solid body: a native clone copies only the last body (FINDINGS F-146); use part_copy for multi-body Parts. Refused while a history rollback is active (the native clone would miss later source features, F-152). Later edits that would make a cloned source multi-body, or leave it bodiless while the clone keeps its old body (F-153), and any modelling inside a clone are refused; edit the source part. A clone is removed as a whole Part with part_delete, as in the app. Undoable; allowed in batch.", |st, a: Derived| {
            let rotation = a.rotation.map(|rotation| PartRotation {axis:rotation.axis, angle_deg:rotation.angle_deg});
            st.doc()?.part_instance(&a.source, &a.name, a.translation_mm, rotation.as_ref()).map_err(err)
        }),
        tool("part_copy", "The app's tree Copy then Paste here of a Part (clone_component into the Part's parent assembly): a deep copy of every body with an independent recipe, so later feature edits of either Part do not reach the other; formulas are copied verbatim, so global parameters drive both. Sketch, feature and body names are copied verbatim, so later references to them by name become ambiguous: use ids (body_map) or rename. A copy of a clone is natively another clone (part.instance_of). Joints and connectors are not copied; grounding is, and a grounded copy is not placed (as part_place), so translation_mm is refused for a grounded source. Without translation_mm the copy stands exactly where the source stands, as in the app. During a history rollback the copied nodes land past the bar and build when the history is rolled forward. Returns copied_from and body_map [[source body, copy body], ...] for built bodies. Undoable; allowed in batch.", |st, a: Derived| {
            let rotation = a.rotation.map(|rotation| PartRotation {axis:rotation.axis, angle_deg:rotation.angle_deg});
            st.doc()?.part_copy(&a.source, &a.name, a.translation_mm, rotation.as_ref()).map_err(err)
        }),
        tool("part_delete", "The app's tree Delete of a Part (delete_component): removes a root-child Part with its bodies, sketches and datums, and every joint that touches it; the other Parts keep their placements (a Part moved by a deleted joint becomes free). If the deleted Part was active, the root becomes active, as in the app. As natively, the surviving Part's connector of a deleted joint and the deleted joint's formulas stay in the file; they are reported as native_leftovers and do not count as parameter users (F-155). Deleting a clone Part keeps its source. Refused: the root; a nested Part; a Part that is the source of clones (QymCAD leaves them with a stale body, F-154: delete the clones first); an active history rollback (the bar would slide, F-156); Parts in component patterns, mirrors, constraints or relations. Returns removed part/bodies/joints/connectors, native_leftovers, active_component, placements and diagnostics. Undoable; allowed in batch.", |st, a: PartDelete| st.doc()?.part_delete(&a.part).map_err(err)),
        tool("assembly_info", "Inspect the root tree, active component, native Parts/placements/grounding/owned bodies, clone links (instance_of on clones, instances on sources), connectors and ordered joints. Reports local/world transforms, actual travel axes, native slots/readings/drives/formulas/limits/limit formulas (live and dormant)/at_limit ({joint, slot} entries), clamped (drive formulas a limit holds and numeric drives a limit clamped in the last call), limit_faults (joints whose limits or drive the app accepts and the server will not build on) and separate faults/violations/conflict. Existing unsupported records are preserved and reported. Read-only.", |st, _: Info| Ok(st.doc()?.assembly_info())),
    ]
}
