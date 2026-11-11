//! H3: a Revolute hinge on straight-edge anchors, the app's Joint tool with an edge picked on each Part (ADR 0054 §3).
//! Golden: a cabinet door. Carcass side panel 20×400×700 centred in XY (x −10..10, y −200..200), grounded. Door leaf
//! 400×18×700 centred in XY, placed at (210, −209, 0): world x 10..410, y −218..−200. The hinge is the carcass's
//! front-right vertical edge (10, −200) and the door's back-left vertical edge, local (−200, 9) → world (10, −200).
//! Hand values: turning by θ about the world Z line through (10, −200) maps a door point at (10+dx, −200+dy) to
//! (10 + dx·cosθ − dy·sinθ, −200 + dx·sinθ + dy·cosθ) with dx ∈ [0, 400], dy ∈ [−18, 0].
//! θ = −90° (cos 0, sin −1): x 10−18..10 = −8..10, y −200−400..−200 = −600..−200 (the door opens outward, away from
//! the carcass). θ = −110° (cos −0.3420201, sin −0.9396926): x 10 − 400·0.3420201 − 18·0.9396926 = −143.7225..10,
//! y −200 − 400·0.9396926 = −575.8770 .. −200 + 18·0.3420201 = −193.8436. `turned_box` evaluates the same corners.
mod common;
use common::*;
use qymcad_core::feature::{AnchorRef, JointKind};
use qymcad_engine::*;
use serde_json::{json, Value};

type Snapshot = (Value, Vec<(Id, Vec<u8>)>, DocInfo);

fn snapshot(s: &Session) -> Snapshot {
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        s.project().bodies.iter().filter_map(|b| s.shape(b.id).map(|shape| (b.id, shape.to_brep_bytes().unwrap()))).collect()
    };
    let mut recipe = serde_json::to_value(s.project()).unwrap();
    recipe.as_object_mut().unwrap().remove("next_id");
    (recipe, breps, s.info())
}

fn unchanged(s: &Session, before: &Snapshot) {
    let now = snapshot(s);
    assert!(now.0 == before.0, "recipe changed");
    assert!(now.1 == before.1, "live shapes changed");
    assert_eq!(now.2, before.2, "doc info changed");
}

fn extrude(sketch: Id, height: f64, name: &str) -> Extrude {
    serde_json::from_value(json!({"sketch":sketch,"height":height,"op":"add","name":name})).unwrap()
}

struct Cabinet {
    s: Session,
    carcass: Id,
    door: Id,
    side: Id,
    leaf: Id,
}

/// The vertical (Z) straight edge of `body` whose midpoint is nearest to part-local (x, y).
fn z_edge(s: &mut Session, body: Id, x: f64, y: f64) -> u32 {
    let topo = s.topology(Some(body), false).unwrap();
    let d = |m: [f64; 3]| (m[0] - x).powi(2) + (m[1] - y).powi(2);
    let e = topo
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Line && (e.b[2] - e.a[2]).abs() > 699.0)
        .min_by(|a, b| d(a.mid).total_cmp(&d(b.mid)))
        .unwrap();
    assert!(d(e.mid) < 1e-6, "edge at ({x}, {y}): {:?}", e.mid);
    e.id
}

fn cabinet(door_at: [f64; 3]) -> Cabinet {
    let mut s = Session::new_part();
    let carcass = s.project().active_component.unwrap();
    s.rename(RenameTarget::Part(carcass), "carcass").unwrap();
    s.param_set("swing", &90.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("side-sk")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &400.0.into(), false).unwrap();
    let (side, _) = s.extrude(&extrude(sk, 700.0, "side")).unwrap();
    let door = s.part_new("door").unwrap()["part"]["id"].as_u64().unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("leaf-sk")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &400.0.into(), &18.0.into(), false).unwrap();
    let (leaf, _) = s.extrude(&extrude(sk, 700.0, "leaf")).unwrap();
    s.part_place(&PartRef::Id(door), door_at, None).unwrap();
    s.ground(&PartRef::Id(carcass), true).unwrap();
    Cabinet { s, carcass, door, side, leaf }
}

fn hinge(c: &mut Cabinet, mode: JointMode, angle: Option<Num>, limits: Option<JointLimits>) -> Result<Value> {
    let ea = z_edge(&mut c.s, c.side, 10.0, -200.0);
    let eb = z_edge(&mut c.s, c.leaf, -200.0, 9.0);
    c.s.joint_create(&JointCreate {
        name: Some("hinge".into()),
        kind: AssemblyJointKind::Revolute,
        a: JointAnchor::edge(c.side, Sel::Ids(vec![ea])),
        b: JointAnchor::edge(c.leaf, Sel::Ids(vec![eb])),
        mode,
        axis: None,
        offset: None,
        angle,
        limits,
    })
}

fn door_box(s: &Session, door: Id) -> [f64; 6] {
    s.info().bodies.into_iter().find(|b| b.owner_part == Some(door)).unwrap().bbox_world
}

/// World bbox of the door turned by `deg` about the hinge line, from its four corners in XY.
fn turned_box(deg: f64) -> [f64; 6] {
    let (sin, cos) = deg.to_radians().sin_cos();
    let mut b = [f64::INFINITY, f64::INFINITY, 0.0, f64::NEG_INFINITY, f64::NEG_INFINITY, 700.0];
    for dx in [0.0, 400.0] {
        for dy in [-18.0, 0.0] {
            let (x, y) = (10.0 + dx * cos - dy * sin, -200.0 + dx * sin + dy * cos);
            b[0] = b[0].min(x);
            b[1] = b[1].min(y);
            b[3] = b[3].max(x);
            b[4] = b[4].max(y);
        }
    }
    b
}

fn assert_box(s: &Session, door: Id, want: [f64; 6], what: &str) {
    let got = door_box(s, door);
    for k in 0..6 {
        assert_close(got[k], want[k], 1e-4, what);
    }
}

fn set_angle(s: &mut Session, j: Id, v: Num) -> Result<Value> {
    s.joint_set_slot(&JointRef::Id(j), JointSlot::Angle, Some(&v), false)
}

#[test]
fn the_door_opens_outward_at_minus_90_about_a_fixed_pivot_edge() {
    let mut c = cabinet([210.0, -209.0, 0.0]);
    let r = hinge(&mut c, JointMode::AsBuilt, None, None).unwrap();
    let j = r["joint"]["id"].as_u64().unwrap();
    assert_eq!(r["joint"]["kind"], "revolute");
    assert_eq!(r["unit"], "deg");
    assert_eq!(r["signed_world_axis"], json!([0.0, 0.0, 1.0]));
    assert!(
        matches!(c.s.project().connector(r["connector_ids"][0].as_u64().unwrap()).unwrap().anchor, AnchorRef::EdgeMid(b, _) if b == c.side)
    );
    // Closed: as built, the door stands where it was placed.
    assert_box(&c.s, c.door, [10.0, -218.0, 0.0, 410.0, -200.0, 700.0], "closed");
    let r = set_angle(&mut c.s, j, (-90.0).into()).unwrap();
    assert_close(r["angle"].as_f64().unwrap(), -90.0, 1e-9, "reading");
    assert_eq!(r["clamped"], Value::Null);
    assert_box(&c.s, c.door, [-8.0, -600.0, 0.0, 10.0, -200.0, 700.0], "open -90: x -8..10, y -600..-200");
    assert_box(&c.s, c.door, turned_box(-90.0), "open -90 from the corners");
    // The pivot edge stays put: the door's local hinge edge (-200, 9) is still at world (10, -200).
    let t = c.s.project().world_transform(c.door);
    let (x, y) = (-200.0 * t[0] + 9.0 * t[1] + t[3], -200.0 * t[4] + 9.0 * t[5] + t[7]);
    assert_close(x, 10.0, 1e-6, "pivot x");
    assert_close(y, -200.0, 1e-6, "pivot y");
    assert_eq!(c.s.assembly_info()["healthy"], true);
    // Back to zero closes it exactly.
    set_angle(&mut c.s, j, 0.0.into()).unwrap();
    assert_box(&c.s, c.door, [10.0, -218.0, 0.0, 410.0, -200.0, 700.0], "closed again");
}

#[test]
fn limits_clamp_the_swing_and_say_so() {
    let mut c = cabinet([210.0, -209.0, 0.0]);
    let r = hinge(&mut c, JointMode::AsBuilt, None, Some(JointLimits { min: Some((-110.0).into()), max: Some(0.0.into()) })).unwrap();
    let j = r["joint"]["id"].as_u64().unwrap();
    assert_eq!(r["limit_min"], json!([-110.0, null, null]));
    let r = set_angle(&mut c.s, j, (-150.0).into()).unwrap();
    assert_eq!(r["clamped"], json!({"joint":j,"slot":0,"requested":-150.0,"applied":-110.0}));
    assert_eq!(r["drive"], json!([-110.0, null, null]));
    assert_box(&c.s, c.door, [-143.7225, -575.8770, 0.0, 10.0, -193.8436, 700.0], "clamped at -110 (hand values)");
    assert_box(&c.s, c.door, turned_box(-110.0), "clamped at -110 from the corners");
    assert_eq!(c.s.assembly_info()["joints"][0]["at_limit"], json!([{"joint":j,"slot":"angle"}]));
    // A new bound past the current angle clamps it too; joint_limits names the angle slot.
    let r = c.s.joint_limits(&JointRef::Id(j), Some(JointSlot::Angle), &LimitEdit::Set((-60.0).into()), &LimitEdit::Keep).unwrap();
    assert_eq!(r["clamped"], json!({"joint":j,"slot":0,"requested":-110.0,"applied":-60.0}));
    assert_box(&c.s, c.door, turned_box(-60.0), "clamped at -60");
}

#[test]
fn a_formula_drives_the_door_and_a_limit_holds_it() {
    let mut c = cabinet([210.0, -209.0, 0.0]);
    let r =
        hinge(&mut c, JointMode::AsBuilt, Some("-swing".into()), Some(JointLimits { min: Some((-110.0).into()), max: Some(0.0.into()) }))
            .unwrap();
    let j = r["joint"]["id"].as_u64().unwrap();
    assert_eq!(r["expression"], "-swing");
    assert_box(&c.s, c.door, turned_box(-90.0), "swing 90");
    let report = c.s.param_set("swing", &45.0.into()).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_box(&c.s, c.door, turned_box(-45.0), "swing 45");
    let report = c.s.param_set("swing", &150.0.into()).unwrap();
    assert_eq!(report.assembly["clamped"], json!([{"joint":j,"slot":0,"requested":-150.0,"applied":-110.0}]));
    assert_box(&c.s, c.door, turned_box(-110.0), "swing 150 held at the stop");
    // Release keeps the reached pose and drops the formula and the drive.
    let r = c.s.joint_set_slot(&JointRef::Id(j), JointSlot::Angle, None, true).unwrap();
    assert_eq!(r["drive"], json!([null, null, null]));
    assert_eq!(r["expression"], Value::Null);
    assert_box(&c.s, c.door, turned_box(-110.0), "released in place");
}

#[test]
fn save_reopen_and_undo() {
    let mut c = cabinet([210.0, -209.0, 0.0]);
    let before = snapshot(&c.s);
    let r =
        hinge(&mut c, JointMode::AsBuilt, Some("-swing".into()), Some(JointLimits { min: Some((-110.0).into()), max: Some(0.0.into()) }))
            .unwrap();
    let j = r["joint"]["id"].as_u64().unwrap();
    assert_eq!(c.s.undo().unwrap().call.unwrap().tool, "joint_create");
    unchanged(&c.s, &before);
    hinge(&mut c, JointMode::AsBuilt, Some("-swing".into()), Some(JointLimits { min: Some((-110.0).into()), max: Some(0.0.into()) }))
        .unwrap();
    let path = scratch("h3-door.qcad");
    c.s.save(Some(&path)).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let joint = &opened.project().joints[0];
    assert_eq!(
        (joint.kind, joint.limit_min[0], joint.limit_max[0], joint.drive[0]),
        (JointKind::Revolute, Some(-110.0), Some(0.0), Some(-90.0))
    );
    assert!(opened.project().connectors.iter().all(|c| matches!(c.anchor, AnchorRef::EdgeMid(..))));
    assert_eq!(opened.assembly_info()["healthy"], true);
    assert_box(&opened, c.door, turned_box(-90.0), "reopened at -90");
    opened.param_set("swing", &30.0.into()).unwrap();
    assert_box(&opened, c.door, turned_box(-30.0), "reopened formula follows");
    let _ = j;
}

#[test]
fn creation_refusals_leave_the_document_unchanged() {
    // Axes 50 mm apart: QymCAD would pivot about B's edge and ignore A's (research R10).
    let mut c = cabinet([210.0, -259.0, 0.0]);
    let before = snapshot(&c.s);
    let ea = z_edge(&mut c.s, c.side, 10.0, -200.0);
    let eb = z_edge(&mut c.s, c.leaf, -200.0, 9.0);
    let create = |a: JointAnchor, b: JointAnchor, mode| JointCreate {
        name: None,
        kind: AssemblyJointKind::Revolute,
        a,
        b,
        mode,
        axis: None,
        offset: None,
        angle: None,
        limits: None,
    };
    let error = c.s.joint_create(&create(
        JointAnchor::edge(c.side, Sel::Ids(vec![ea])),
        JointAnchor::edge(c.leaf, Sel::Ids(vec![eb])),
        JointMode::AsBuilt,
    ));
    assert_eq!(
        error.unwrap_err().to_string(),
        "revolute hinge axes are not collinear (distance 50 mm, angle 0°): QymCAD would turn the part about its own edge and ignore the anchor's; place the part on the hinge first; nothing was changed"
    );
    unchanged(&c.s, &before);
    let mut c = cabinet([210.0, -209.0, 0.0]);
    let before = snapshot(&c.s);
    // A planar-face Revolute turns the door about the face normal, like a wheel (research R9).
    let error = c.s.joint_create(&create(
        JointAnchor::face(c.side, Sel::Facing { dir: [0., -1., 0.], tol_deg: 1. }),
        JointAnchor::face(c.leaf, Sel::Facing { dir: [0., 1., 0.], tol_deg: 1. }),
        JointMode::AsBuilt,
    ));
    assert_eq!(
        error.unwrap_err().to_string(),
        "revolute joint_create needs a straight edge anchor on each part: a planar face would turn the part about the face normal; nothing was changed"
    );
    unchanged(&c.s, &before);
    // A grounded door would not turn and QymCAD would say nothing (F-159).
    c.s.ground(&PartRef::Id(c.door), true).unwrap();
    let before = snapshot(&c.s);
    let error = hinge(&mut c, JointMode::AsBuilt, None, None).unwrap_err().to_string();
    assert_eq!(
        error,
        format!(
            "revolute target part {} is grounded; unground it first (a grounded part does not turn and QymCAD reports nothing, F-159)",
            c.door
        )
    );
    unchanged(&c.s, &before);
    // Angle bounds outside ±180 and min above max.
    c.s.ground(&PartRef::Id(c.door), false).unwrap();
    let before = snapshot(&c.s);
    let error = hinge(&mut c, JointMode::AsBuilt, None, Some(JointLimits { min: Some((-270.0).into()), max: Some(0.0.into()) }));
    assert!(error.unwrap_err().to_string().contains("angle limit -270 is outside (-180, 180] degrees"));
    unchanged(&c.s, &before);
    let error = hinge(&mut c, JointMode::AsBuilt, None, Some(JointLimits { min: Some(0.0.into()), max: Some((-110.0).into()) }));
    assert!(error.unwrap_err().to_string().contains("angle limit min 0 is greater than max -110"));
    unchanged(&c.s, &before);
    let error = hinge(&mut c, JointMode::AsBuilt, Some(200.0.into()), None).unwrap_err().to_string();
    assert!(error.contains("angle 200° is outside (-180, 180]") && error.contains("F-158"), "{error}");
    unchanged(&c.s, &before);
    // Edge anchors are for Revolute only.
    let mut slider =
        create(JointAnchor::edge(c.side, Sel::Ids(vec![ea])), JointAnchor::edge(c.leaf, Sel::Ids(vec![eb])), JointMode::AsBuilt);
    slider.kind = AssemblyJointKind::Slider;
    slider.axis = Some(BaseName::XY);
    let error = c.s.joint_create(&slider).unwrap_err().to_string();
    assert_eq!(error, "slider joint_create needs planar face anchors; edge anchors are for revolute");
    unchanged(&c.s, &before);
}

#[test]
fn edit_refusals_leave_the_document_unchanged() {
    let mut c = cabinet([210.0, -209.0, 0.0]);
    let j = hinge(&mut c, JointMode::AsBuilt, Some((-30.0).into()), None).unwrap()["joint"]["id"].as_u64().unwrap();
    let before = snapshot(&c.s);
    // Grounding the door.
    let error = c.s.ground(&PartRef::Id(c.door), true).unwrap_err().to_string();
    assert_eq!(
        error,
        format!("revolute joint {j} moves part {}, which must stay ungrounded (a grounded part does not turn and QymCAD reports nothing, F-159); nothing was changed", c.door)
    );
    unchanged(&c.s, &before);
    // Min above max; bounds outside ±180; a drive outside (−180, 180].
    let angle = |s: &mut Session, min: LimitEdit, max: LimitEdit| s.joint_limits(&JointRef::Id(j), None, &min, &max);
    let error = angle(&mut c.s, LimitEdit::Set(0.0.into()), LimitEdit::Set((-110.0).into())).unwrap_err().to_string();
    assert_eq!(
        error,
        format!("joint {j} angle limit min 0 is greater than max -110; QymCAD would send every value to max (F-157); nothing was changed")
    );
    let error = angle(&mut c.s, LimitEdit::Set((-270.0).into()), LimitEdit::Keep).unwrap_err().to_string();
    assert_eq!(
        error,
        format!("joint {j} angle limit -270 is outside (-180, 180] degrees: QymCAD reads angles wrapped to (-180, 180], so -180 reads as 180 and a limit outside the range snaps a released part to the other bound (F-158); nothing was changed")
    );
    let error = set_angle(&mut c.s, j, 200.0.into()).unwrap_err().to_string();
    assert_eq!(
        error,
        format!("joint {j} angle 200° is outside (-180, 180]: QymCAD reads angles wrapped to (-180, 180], so the reading would disagree with the drive (F-158); nothing was changed")
    );
    let error = c.s.joint_set_slot(&JointRef::Id(j), JointSlot::Offset, Some(&10.0.into()), false).unwrap_err().to_string();
    assert_eq!(error, format!("joint {j} has no editable offset slot for kind revolute"));
    unchanged(&c.s, &before);
    // A formula pushed out of the domain by a parameter edit.
    set_angle(&mut c.s, j, "-swing".into()).unwrap();
    let before = snapshot(&c.s);
    let error = c.s.param_set("swing", &200.0.into()).unwrap_err().to_string();
    assert!(error.contains("angle -200° is outside (-180, 180]") && error.contains("F-158"), "{error}");
    unchanged(&c.s, &before);
    let _ = c.carcass;
}

/// F-158 native evidence (app data path, no Session): limits [−270, 0] and drive −200 read +160; released, the
/// free-slot active set sees 160 > max 0 and snaps the door shut, with no error.
#[test]
fn native_angle_limits_beyond_180_snap_a_released_door_shut_f158() {
    let mut c = cabinet([210.0, -209.0, 0.0]);
    let j = hinge(&mut c, JointMode::AsBuilt, None, None).unwrap()["joint"]["id"].as_u64().unwrap();
    let path = scratch("h3-native-wrap.qcad");
    c.s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    let joint = m.project.joints.iter_mut().find(|x| x.id == j).unwrap();
    joint.limit_min[0] = Some(-270.0);
    joint.limit_max[0] = Some(0.0);
    m.project.set_joint_drive(j, 0, Some(-200.0));
    let solved = m.project.solve_joints();
    assert!(solved.errors.is_empty());
    let joint = m.project.joints.iter().find(|x| x.id == j).unwrap();
    assert_eq!(joint.drive[0], Some(-200.0));
    assert_close(joint.angle, 160.0, 1e-6, "the reading wraps");
    m.project.set_joint_drive(j, 0, None);
    let solved = m.project.solve_joints();
    assert!(solved.errors.is_empty() && !m.project.mates_conflict && m.project.mates_violated.is_empty());
    let joint = m.project.joints.iter().find(|x| x.id == j).unwrap();
    assert_close(joint.angle, 0.0, 1e-6, "released, the door snaps shut");
}

/// F-159 native evidence: a driven Revolute whose B Part is grounded does not move and reports nothing.
#[test]
fn native_a_grounded_door_ignores_its_drive_silently_f159() {
    let mut c = cabinet([210.0, -209.0, 0.0]);
    let j = hinge(&mut c, JointMode::AsBuilt, None, None).unwrap()["joint"]["id"].as_u64().unwrap();
    let path = scratch("h3-native-grounded.qcad");
    c.s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    m.project.set_grounded(c.door, true);
    m.project.set_joint_drive(j, 0, Some(-45.0));
    let solved = m.project.solve_joints();
    assert!(solved.errors.is_empty() && !m.project.mates_conflict && m.project.mates_violated.is_empty());
    assert!(m.project.joint_faults().is_empty());
    let joint = m.project.joints.iter().find(|x| x.id == j).unwrap();
    assert_eq!(joint.drive[0], Some(-45.0));
    assert_close(joint.angle, 0.0, 1e-6, "the grounded door did not turn");
}

// ---- H5 fix round -------------------------------------------------------------------------------------------

fn hinged(angle: f64) -> (Cabinet, Id) {
    let mut c = cabinet([210.0, -209.0, 0.0]);
    let j = hinge(&mut c, JointMode::AsBuilt, Some(angle.into()), None).unwrap()["joint"]["id"].as_u64().unwrap();
    (c, j)
}

fn facing(dir: [f64; 3]) -> Sel {
    Sel::Facing { dir, tol_deg: 1.0 }
}

/// World position of the door's local hinge edge (-200, 9): the pivot the door really turns about.
fn pivot(s: &Session, door: Id) -> (f64, f64) {
    let t = s.project().world_transform(door);
    (-200.0 * t[0] + 9.0 * t[1] + t[3], -200.0 * t[4] + 9.0 * t[5] + t[7])
}

/// The edit moves or removes the hinge edge on the current body: QymCAD would keep turning the door about the
/// consumed body's old edge, a place that is no longer on the body (F-160). Refused, nothing changed.
fn refused(c: &mut Cabinet, j: Id, what: &str, edit: impl FnOnce(&mut Session) -> Result<()>) {
    let before = snapshot(&c.s);
    let error = edit(&mut c.s).expect_err(what).to_string();
    assert!(error.contains(&format!("invalidate anchors of joints [{j}]")), "{what}: {error}");
    unchanged(&c.s, &before);
}

/// The edit leaves the hinge edge where it was: allowed, the hinge is healthy and the pivot has not moved.
fn accepted(c: &mut Cabinet, what: &str, edit: impl FnOnce(&mut Session) -> Result<()>) {
    edit(&mut c.s).unwrap_or_else(|e| panic!("{what}: {e}"));
    assert_eq!(c.s.assembly_info()["healthy"], true, "{what}");
    let (x, y) = pivot(&c.s, c.door);
    assert_close(x, 10.0, 1e-5, what);
    assert_close(y, -200.0, 1e-5, what);
}

#[test]
fn a_feature_that_moves_or_removes_the_hinge_edge_of_the_carcass_is_refused() {
    let (mut c, j) = hinged(-90.0);
    let (side, hinge_edge) = (c.side, z_edge(&mut c.s, c.side, 10.0, -200.0));
    // push_face of the face the hinge edge bounds: the edge moves to x = 30, the stale anchor stays at x = 10.
    refused(&mut c, j, "push_face +X", |s| s.push_face(Some(side), &facing([1.0, 0.0, 0.0]), &20.0.into(), None).map(|_| ()));
    refused(&mut c, j, "chamfer of the hinge edge", |s| {
        s.chamfer(Some(side), &Sel::Ids(vec![hinge_edge]), &2.0.into(), None, None).map(|_| ())
    });
    refused(&mut c, j, "fillet of the hinge edge", |s| s.fillet(Some(side), &Sel::Ids(vec![hinge_edge]), &2.0.into(), None).map(|_| ()));
    // The same hinge survives every edit that leaves its edge where it is.
    let other = z_edge(&mut c.s, c.side, -10.0, 200.0);
    accepted(&mut c, "fillet elsewhere", |s| s.fillet(Some(side), &Sel::Ids(vec![other]), &2.0.into(), None).map(|_| ()));
    assert_box(&c.s, c.door, turned_box(-90.0), "still open at -90");
}

#[test]
fn a_push_face_or_chamfer_away_from_the_hinge_edge_of_the_carcass_is_allowed() {
    let (mut c, _) = hinged(-90.0);
    let side = c.side;
    accepted(&mut c, "push_face -X", |s| s.push_face(Some(side), &facing([-1.0, 0.0, 0.0]), &20.0.into(), None).map(|_| ()));
    let (mut c, _) = hinged(-90.0);
    let other = z_edge(&mut c.s, c.side, -10.0, 200.0);
    accepted(&mut c, "chamfer elsewhere", |s| s.chamfer(Some(side), &Sel::Ids(vec![other]), &2.0.into(), None, None).map(|_| ()));
}

#[test]
fn a_feature_that_moves_or_removes_the_hinge_edge_of_the_door_is_refused() {
    let (mut c, j) = hinged(-90.0);
    let (leaf, hinge_edge) = (c.leaf, z_edge(&mut c.s, c.leaf, -200.0, 9.0));
    refused(&mut c, j, "push_face -X", |s| s.push_face(Some(leaf), &facing([-1.0, 0.0, 0.0]), &20.0.into(), None).map(|_| ()));
    refused(&mut c, j, "chamfer of the hinge edge", |s| {
        s.chamfer(Some(leaf), &Sel::Ids(vec![hinge_edge]), &2.0.into(), None, None).map(|_| ())
    });
    refused(&mut c, j, "fillet of the hinge edge", |s| s.fillet(Some(leaf), &Sel::Ids(vec![hinge_edge]), &2.0.into(), None).map(|_| ()));
    // Edits on the far side leave the pivot where it is.
    let far = z_edge(&mut c.s, c.leaf, 200.0, 9.0);
    accepted(&mut c, "push_face +X", |s| s.push_face(Some(leaf), &facing([1.0, 0.0, 0.0]), &20.0.into(), None).map(|_| ()));
    let (mut c, _) = hinged(-90.0);
    accepted(&mut c, "chamfer far edge", |s| s.chamfer(Some(leaf), &Sel::Ids(vec![far]), &2.0.into(), None, None).map(|_| ()));
    let (mut c, _) = hinged(-90.0);
    accepted(&mut c, "fillet far edge", |s| s.fillet(Some(leaf), &Sel::Ids(vec![far]), &2.0.into(), None).map(|_| ()));
}

/// Two modifiers on one body, hinge on the current body's edge, then the modifiers swap order: the anchored body is
/// consumed by the other feature now, and the hinge edge is where it was.
#[test]
fn reordering_modifiers_of_a_hinge_body_keeps_the_hinge_on_the_current_body() {
    for door_side in [false, true] {
        let mut c = cabinet([210.0, -209.0, 0.0]);
        let (body, a, b) = if door_side {
            (c.leaf, z_edge(&mut c.s, c.leaf, 200.0, 9.0), z_edge(&mut c.s, c.leaf, 200.0, -9.0))
        } else {
            (c.side, z_edge(&mut c.s, c.side, -10.0, 200.0), z_edge(&mut c.s, c.side, -10.0, -200.0))
        };
        let (fillet, _) = c.s.fillet(Some(body), &Sel::Ids(vec![a]), &2.0.into(), None).unwrap();
        let (chamfer, _) = c.s.chamfer(Some(fillet), &Sel::Ids(vec![b]), &2.0.into(), None, None).unwrap();
        // The hinge sits on the current body (the chamfer's result).
        if door_side {
            c.leaf = chamfer;
        } else {
            c.side = chamfer;
        }
        let j = hinge(&mut c, JointMode::AsBuilt, Some((-90.0).into()), None).unwrap()["joint"]["id"].as_u64().unwrap();
        accepted(&mut c, "hinge on the chamfer result", |_| Ok(()));
        accepted(&mut c, "reorder the chamfer above the fillet", |s| s.feature_reorder(chamfer, "up").map(|_| ()));
        assert_eq!(c.s.project().joints[0].id, j);
        assert_box(&c.s, c.door, turned_box(-90.0), "still open at -90");
    }
}

/// F-160 native evidence (app data path, no Session): a feature that consumes the anchored body leaves the
/// connector on the consumed body's edge. Pushing the carcass's +X face moves the real edge to x = 30; the anchor
/// still names the old body, whose stale edge stays at x = 10, and QymCAD reports nothing.
#[test]
fn native_a_pushed_body_keeps_the_hinge_on_the_consumed_bodys_old_edge_f160() {
    let (mut c, j) = hinged(-90.0);
    let path = scratch("h5-native-pushed.qcad");
    c.s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    m.project.set_active_component(Some(c.carcass));
    let f = m.project.regen_faces[&c.side].iter().find(|f| f.normal[0] > 0.99).unwrap().clone();
    let key = qymcad_core::feature::FaceKey { index: 0, centroid: [f.centroid.x, f.centroid.y, f.centroid.z], normal: f.normal, id: f.id };
    let pushed = m.project.add_push_face(c.side, key, 20.0);
    native_assembly_rebuild(&mut m, false); // asserts: no feature error, no joint fault, no violation, no conflict
    assert!(m.project.consumed_bodies().contains(&c.side));
    let anchor = m.project.connectors.iter().find(|x| x.owner == c.carcass).unwrap();
    let AnchorRef::EdgeMid(body, edge) = anchor.anchor else { panic!("{:?}", anchor.anchor) };
    assert_eq!(body, c.side, "the anchor still names the consumed body");
    let x_of = |body: Id| m.project.regen_edges[&body].iter().find(|e| e.id == edge).unwrap().mid[0];
    assert_close(x_of(c.side), 10.0, 1e-6, "the stale edge the pivot is read from");
    assert_close(x_of(pushed), 30.0, 1e-6, "the edge of the current body, 20 mm further");
    let (x, y) = {
        let t = m.project.world_transform(c.door);
        (-200.0 * t[0] + 9.0 * t[1] + t[3], -200.0 * t[4] + 9.0 * t[5] + t[7])
    };
    assert_close(x, 10.0, 1e-6, "the door still turns about x = 10, now inside the pushed panel");
    assert_close(y, -200.0, 1e-6, "pivot y");
    let _ = j;
}

// ---- native evidence for the Revolute rules without a finding so far (research R9, R10) -----------------------

/// A cabinet on the native Project alone: carcass side 20×400×700 and door leaf 400×18×700, both centred in XY.
fn native_cabinet(door_at: [f64; 3]) -> (qymcad_core::model::Project, Id, Id, Id, Id) {
    use qymcad_core::model::Project;
    let mut p = Project::default();
    let root = p.ensure_root();
    p.set_active_component(Some(root));
    let carcass = p.add_part("carcass");
    p.set_active_component(Some(carcass));
    let side = p.add_box(20.0, 400.0, 700.0);
    p.set_active_component(Some(root));
    let door = p.add_part("door");
    p.set_active_component(Some(door));
    let leaf = p.add_box(400.0, 18.0, 700.0);
    p.set_grounded(carcass, true);
    p.move_component(door, door_at);
    let (report, _) = qymcad_testkit::regenerate(&mut p);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    (p, carcass, side, door, leaf)
}

fn native_z_edge(p: &qymcad_core::model::Project, body: Id, at: [f64; 3]) -> u32 {
    let d2 = |m: [f64; 3]| (0..3).map(|k| (m[k] - at[k]).powi(2)).sum::<f64>();
    p.regen_edges[&body].iter().filter(|e| e.dir[2].abs() > 0.99).min_by(|a, b| d2(a.mid).total_cmp(&d2(b.mid))).unwrap().id
}

/// World xy of the door's four corners (local x ±200, y ±9) under the native placement.
fn native_door_xy(p: &qymcad_core::model::Project, door: Id) -> Vec<[f64; 2]> {
    let t = p.world_transform(door);
    [(-200.0, -9.0), (-200.0, 9.0), (200.0, -9.0), (200.0, 9.0)]
        .iter()
        .map(|(x, y)| [t[0] * x + t[1] * y + t[3], t[4] * x + t[5] * y + t[7]])
        .collect()
}

fn native_extent(corners: &[[f64; 2]]) -> [f64; 4] {
    let min = |k: usize| corners.iter().map(|c| c[k]).fold(f64::INFINITY, f64::min);
    let max = |k: usize| corners.iter().map(|c| c[k]).fold(f64::NEG_INFINITY, f64::max);
    [min(0), min(1), max(0), max(1)]
}

/// R9: a Revolute on planar faces turns about the face normal through the face centre, so the door spins in its own
/// plane like a wheel. The server refuses planar-face Revolute anchors (F-161).
#[test]
fn native_a_revolute_on_planar_faces_spins_the_door_like_a_wheel_f161() {
    let (mut p, carcass, side, door, leaf) = native_cabinet([210.0, -209.0, 0.0]);
    let face = |body: Id, d: [f64; 3]| {
        let f = p.regen_faces[&body].iter().find(|f| f.normal[0] * d[0] + f.normal[1] * d[1] + f.normal[2] * d[2] > 0.99).unwrap().clone();
        qymcad_core::feature::FaceKey { index: 0, centroid: [f.centroid.x, f.centroid.y, f.centroid.z], normal: f.normal, id: f.id }
    };
    let (ka, kb) = (face(side, [0.0, -1.0, 0.0]), face(leaf, [0.0, 1.0, 0.0]));
    let ca = p.add_connector(carcass, AnchorRef::FaceCenter(side, ka));
    let cb = p.add_connector(door, AnchorRef::FaceCenter(leaf, kb));
    let j = p.add_joint(ca, cb, JointKind::Revolute);
    assert!(p.set_joint_as_built(j));
    p.set_joint_drive(j, 0, Some(30.0));
    assert!(p.solve_joints().errors.is_empty());
    let axis = p.joint_slot_axis(j, 0, p.root).unwrap();
    assert_close(axis[1].abs(), 1.0, 1e-9, "the axis is the face normal (Y), not a vertical hinge line");
    // The door turns in the XZ plane: its world z extent grows beyond the 0..700 it had.
    let t = p.world_transform(door);
    let z = |lx: f64, lz: f64| t[8] * lx + t[10] * lz + t[11];
    assert!(z(-200.0, 0.0) < -50.0 || z(200.0, 0.0) < -50.0 || z(-200.0, 700.0) > 750.0 || z(200.0, 700.0) > 750.0);
}

/// R10: with the hinge axes 50 mm apart an as-built Revolute turns the door about B's own edge and ignores A's.
/// The server refuses non-collinear axes (F-162).
#[test]
fn native_as_built_pivots_about_bs_edge_and_ignores_as_when_the_axes_are_apart_f162() {
    let (mut p, carcass, side, door, leaf) = native_cabinet([210.0, -259.0, 0.0]);
    let ea = native_z_edge(&p, side, [10.0, -200.0, 350.0]);
    let eb = native_z_edge(&p, leaf, [-200.0, 9.0, 350.0]);
    let ca = p.add_connector(carcass, AnchorRef::EdgeMid(side, ea));
    let cb = p.add_connector(door, AnchorRef::EdgeMid(leaf, eb));
    let j = p.add_joint(ca, cb, JointKind::Revolute);
    assert!(p.set_joint_as_built(j));
    p.set_joint_drive(j, 0, Some(90.0));
    assert!(p.solve_joints().errors.is_empty() && p.mates_violated.is_empty() && !p.mates_conflict);
    // The door's hinge edge started at world (10, -250); it stays there. A's edge (10, -200) is not the pivot.
    let e = native_extent(&native_door_xy(&p, door));
    assert_close(e[0], 10.0, 1e-4, "door box x min");
    assert_close(e[2], 28.0, 1e-4, "door box x max");
    assert_close(e[1], -250.0, 1e-4, "door box y min");
    assert_close(e[3], 150.0, 1e-4, "door box y max");
}

/// A fresh align Revolute (never as built) on straight edges puts B's edge frame on A's: the door snaps onto the
/// hinge, closed at 0° and opening outward at −90°, with the axes collinear or 50 mm apart. The research reading
/// that align "folds the door into the carcass" came from a joint whose as-built capture was cleared afterwards
/// (stale flip/roll decisions), a state the server cannot reach (F-163). The server therefore allows align.
#[test]
fn native_a_fresh_align_revolute_snaps_the_door_onto_the_hinge_and_is_correct_f163() {
    for door_y in [-209.0, -259.0] {
        let (mut p, carcass, side, door, leaf) = native_cabinet([210.0, door_y, 0.0]);
        let ea = native_z_edge(&p, side, [10.0, -200.0, 350.0]);
        let eb = native_z_edge(&p, leaf, [-200.0, 9.0, 350.0]);
        let ca = p.add_connector(carcass, AnchorRef::EdgeMid(side, ea));
        let cb = p.add_connector(door, AnchorRef::EdgeMid(leaf, eb));
        let j = p.add_joint(ca, cb, JointKind::Revolute);
        for (angle, want) in
            [(0.0, [10.0, -218.0, 410.0, -200.0]), (-90.0, [-8.0, -600.0, 10.0, -200.0]), (90.0, [10.0, -200.0, 28.0, 200.0])]
        {
            p.set_joint_drive(j, 0, Some(angle));
            assert!(p.solve_joints().errors.is_empty() && p.mates_violated.is_empty() && !p.mates_conflict);
            let e = native_extent(&native_door_xy(&p, door));
            for k in 0..4 {
                assert_close(e[k], want[k], 1e-4, &format!("door at y {door_y}, angle {angle}, extent {k}"));
            }
        }
    }
}

// ---- angle limits and drives from a file ---------------------------------------------------------------------

/// A copy of the hinged cabinet's document with `edit` applied to the native Project, saved through the native io.
fn patched_hinge(c: &mut Cabinet, j: Id, name: &str, edit: impl FnOnce(&mut NativeAssemblyModel, Id)) -> std::path::PathBuf {
    let src = scratch(&format!("{name}-src.qcad"));
    c.s.save(Some(&src)).unwrap();
    let mut m = native_assembly_load(&src);
    native_assembly_rebuild(&mut m, true);
    edit(&mut m, j);
    let out = scratch(&format!("{name}.qcad"));
    native_assembly_save(&m, &out);
    out
}

#[test]
fn an_angle_bound_outside_the_domain_from_a_file_blocks_only_the_edits_that_touch_the_joint() {
    let mut c = cabinet([210.0, -209.0, 0.0]);
    let j = hinge(&mut c, JointMode::AsBuilt, None, None).unwrap()["joint"]["id"].as_u64().unwrap();
    // The app's field takes ±100000: min −270 is a value it accepts.
    let path = patched_hinge(&mut c, j, "h5-angle-bound", |m, j| {
        let jj = m.project.joints.iter_mut().find(|x| x.id == j).unwrap();
        jj.limit_min[0] = Some(-270.0);
        jj.limit_max[0] = Some(0.0);
    });
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let faults = report.assembly["limit_faults"].as_array().unwrap();
    assert_eq!(faults.len(), 1, "{:?}", report.assembly);
    assert_eq!(faults[0]["joint"], j);
    assert!(faults[0]["message"].as_str().unwrap().contains("-270") && faults[0]["message"].as_str().unwrap().contains("F-158"));
    s.param_set("fresh", &5.0.into()).unwrap();
    s.part_select(&PartRef::Id(c.carcass)).unwrap();
    s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("extra")).unwrap();
    let before = snapshot(&s);
    let error = set_angle(&mut s, j, (-30.0).into()).unwrap_err().to_string();
    assert!(error.contains("angle limit -270 is outside (-180, 180]"), "{error}");
    unchanged(&s, &before);
    s.joint_limits(&JointRef::Id(j), None, &LimitEdit::Set((-110.0).into()), &LimitEdit::Keep).unwrap();
    set_angle(&mut s, j, (-30.0).into()).unwrap();
}

#[test]
fn a_drive_outside_the_angle_domain_from_a_file_blocks_only_the_edits_that_touch_the_joint() {
    let mut c = cabinet([210.0, -209.0, 0.0]);
    let j = hinge(&mut c, JointMode::AsBuilt, None, None).unwrap()["joint"]["id"].as_u64().unwrap();
    // A drive of 200 reads as −160 (F-158); the app accepts the typed value.
    let path = patched_hinge(&mut c, j, "h5-angle-drive", |m, j| {
        m.project.set_joint_drive(j, 0, Some(200.0));
        assert!(m.project.solve_joints().errors.is_empty());
    });
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    // Reported, never hidden.
    assert_eq!(report.assembly["healthy"], false, "{:?}", report.assembly);
    assert_eq!(report.assembly["request_mismatches"][0]["joint"], j);
    assert_eq!(report.assembly["limit_faults"][0]["joint"], j, "{:?}", report.assembly);
    // Unrelated edits are not blocked by it.
    s.param_set("fresh", &5.0.into()).unwrap();
    s.part_select(&PartRef::Id(c.carcass)).unwrap();
    s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("extra")).unwrap();
    // The joint itself: a new out-of-domain drive is refused, the repair is allowed.
    let before = snapshot(&s);
    let error = set_angle(&mut s, j, 300.0.into()).unwrap_err().to_string();
    assert!(error.contains("angle 300° is outside (-180, 180]"), "{error}");
    unchanged(&s, &before);
    set_angle(&mut s, j, (-30.0).into()).unwrap();
    assert_eq!(s.assembly_info()["healthy"], true);
}

#[test]
fn an_angle_bound_of_minus_180_is_refused_and_the_message_names_the_wrap() {
    let mut c = cabinet([210.0, -209.0, 0.0]);
    let j = hinge(&mut c, JointMode::AsBuilt, None, None).unwrap()["joint"]["id"].as_u64().unwrap();
    let before = snapshot(&c.s);
    let error = c.s.joint_limits(&JointRef::Id(j), None, &LimitEdit::Set((-180.0).into()), &LimitEdit::Keep).unwrap_err().to_string();
    assert!(
        error.contains("angle limit -180 is outside (-180, 180]") && error.contains("-180 reads as 180") && error.contains("F-158"),
        "{error}"
    );
    unchanged(&c.s, &before);
    // 180 is inside the domain, as for a drive.
    c.s.joint_limits(&JointRef::Id(j), None, &LimitEdit::Keep, &LimitEdit::Set(180.0.into())).unwrap();
}

#[test]
fn suppressing_the_anchored_body_is_refused() {
    let (mut c, j) = hinged(-90.0);
    let before = snapshot(&c.s);
    let error = c.s.feature_suppress(c.leaf, true).unwrap_err().to_string();
    assert!(error.contains(&format!("invalidate anchors of joints [{j}]")), "{error}");
    unchanged(&c.s, &before);
}

/// Align mode on straight edges, as the app's Joint tool makes it without «as built»: the door snaps onto the hinge,
/// whether its axis was already on the carcass's edge or 50 mm in front of it.
#[test]
fn align_mode_places_the_door_on_the_hinge_and_turns_about_it() {
    for door_y in [-209.0, -259.0] {
        let mut c = cabinet([210.0, door_y, 0.0]);
        let r = hinge(&mut c, JointMode::Align, None, None).unwrap();
        let j = r["joint"]["id"].as_u64().unwrap();
        assert_eq!(r["mode"], "align");
        assert_eq!(c.s.assembly_info()["healthy"], true);
        assert_box(&c.s, c.door, [10.0, -218.0, 0.0, 410.0, -200.0, 700.0], &format!("closed on the hinge from y {door_y}"));
        set_angle(&mut c.s, j, (-90.0).into()).unwrap();
        assert_box(&c.s, c.door, turned_box(-90.0), "open -90");
        let (x, y) = pivot(&c.s, c.door);
        assert_close(x, 10.0, 1e-5, "pivot x");
        assert_close(y, -200.0, 1e-5, "pivot y");
    }
}
