//! H2: joint limits, the app's «Limits» section of the joint box (ADR 0054 §2), on a Slider.
//! Hand values: the drawer box is 500×600×150 centred in XY, world y −300..300 at travel 0. The Slider's XZ axis
//! travels −Y, so travel v puts the drawer at y −300−v..300−v. A limit clamps the request, as natively.
mod common;
use common::*;
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

struct Drawer {
    s: Session,
    drawer: Id,
    joint: Id,
    carcass_body: Id,
    drawer_body: Id,
}

fn drawer(limits: Option<JointLimits>, offset: Option<Num>) -> Drawer {
    let mut s = Session::new_part();
    let carcass = s.project().active_component.unwrap();
    s.rename(RenameTarget::Part(carcass), "carcass").unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("carcass-sk")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &600.0.into(), &800.0.into(), false).unwrap();
    let (carcass_body, _) = s.extrude(&extrude(sk, 700.0, "carcass-box")).unwrap();
    let drawer = s.part_new("drawer").unwrap()["part"]["id"].as_u64().unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("drawer-sk")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &500.0.into(), &600.0.into(), false).unwrap();
    let (drawer_body, _) = s.extrude(&extrude(sk, 150.0, "drawer-box")).unwrap();
    s.part_place(&PartRef::Id(drawer), [0.0, 0.0, 100.0], None).unwrap();
    s.ground(&PartRef::Id(carcass), true).unwrap();
    let joint = s
        .joint_create(&JointCreate {
            name: Some("slide".into()),
            kind: AssemblyJointKind::Slider,
            a: JointAnchor { body: carcass_body, face: Sel::Facing { dir: [1., 0., 0.], tol_deg: 1. } },
            b: JointAnchor { body: drawer_body, face: Sel::Facing { dir: [-1., 0., 0.], tol_deg: 1. } },
            mode: JointMode::AsBuilt,
            axis: Some(BaseName::XZ),
            offset,
            limits,
        })
        .unwrap()["joint"]["id"]
        .as_u64()
        .unwrap();
    Drawer { s, drawer, joint, carcass_body, drawer_body }
}

fn y_span(s: &Session, part: Id) -> (f64, f64) {
    let b = s.info().bodies.into_iter().find(|b| b.owner_part == Some(part)).unwrap();
    (b.bbox_world[1], b.bbox_world[4])
}

fn assert_travel(s: &Session, part: Id, v: f64) {
    let (lo, hi) = y_span(s, part);
    assert_close(lo, -300.0 - v, 1e-5, "drawer min y");
    assert_close(hi, 300.0 - v, 1e-5, "drawer max y");
}

fn limits(s: &mut Session, j: Id, min: LimitEdit, max: LimitEdit) -> Result<Value> {
    s.joint_limits(&JointRef::Id(j), None, &min, &max)
}

#[test]
fn numeric_limits_clamp_a_request_and_say_so() {
    let Drawer { mut s, drawer, joint, .. } = drawer(None, Some(0.0.into()));
    let r = limits(&mut s, joint, LimitEdit::Set(0.0.into()), LimitEdit::Set(300.0.into())).unwrap();
    assert_eq!(r["changed"], true);
    assert_eq!(r["limit_min"], json!([null, 0.0, null]));
    assert_eq!(r["limit_max"], json!([null, 300.0, null]));
    assert_eq!(r["clamped"], Value::Null, "travel 0 is inside");
    let r = s.joint_set(&JointRef::Id(joint), Some(&400.0.into()), None).unwrap();
    assert_eq!(r["clamped"], json!({"joint":joint,"slot":1,"requested":400.0,"applied":300.0}));
    assert_eq!(r["drive"], json!([null, 300.0, null]));
    assert_travel(&s, drawer, 300.0);
    let info = s.assembly_info();
    assert_eq!(info["healthy"], true);
    assert_eq!(info["joints"][0]["at_limit"], json!(["offset"]));
    let r = s.joint_set(&JointRef::Id(joint), Some(&(-50.0).into()), None).unwrap();
    assert_eq!(r["clamped"]["applied"], 0.0);
    assert_travel(&s, drawer, 0.0);
    // Inside the limits nothing is clamped and nothing is at a limit.
    let r = s.joint_set(&JointRef::Id(joint), Some(&120.0.into()), None).unwrap();
    assert_eq!(r["clamped"], Value::Null);
    assert_travel(&s, drawer, 120.0);
    assert_eq!(s.assembly_info()["joints"][0]["at_limit"], json!([]));
}

#[test]
fn a_new_bound_beyond_the_current_value_clamps_it_and_reports_the_clamp() {
    let Drawer { mut s, drawer, joint, .. } = drawer(None, Some(100.0.into()));
    assert_travel(&s, drawer, 100.0);
    let r = limits(&mut s, joint, LimitEdit::Keep, LimitEdit::Set(50.0.into())).unwrap();
    assert_eq!(r["clamped"], json!({"joint":joint,"slot":1,"requested":100.0,"applied":50.0}));
    assert_travel(&s, drawer, 50.0);
    // A released (free) slot: the solver pins the reading at the bound.
    s.joint_set(&JointRef::Id(joint), None, Some("offset")).unwrap();
    let r = limits(&mut s, joint, LimitEdit::Keep, LimitEdit::Set(20.0.into())).unwrap();
    assert_close(r["clamped"]["requested"].as_f64().unwrap(), 50.0, 1e-5, "free slot reading before");
    assert_close(r["clamped"]["applied"].as_f64().unwrap(), 20.0, 1e-5, "free slot pinned");
    assert_travel(&s, drawer, 20.0);
}

#[test]
fn formula_limits_and_formula_drives_follow_parameters_and_clamps_are_reported() {
    let Drawer { mut s, drawer, joint, .. } = drawer(None, Some(0.0.into()));
    s.param_set("travel", &250.0.into()).unwrap();
    s.param_set("open", &100.0.into()).unwrap();
    let r = limits(&mut s, joint, LimitEdit::Set(0.0.into()), LimitEdit::Set("travel".into())).unwrap();
    assert_eq!(r["limit_max"], json!([null, 250.0, null]));
    assert_eq!(r["limit_formulas"], json!({"max":"travel"}));
    s.joint_set(&JointRef::Id(joint), Some(&"open".into()), None).unwrap();
    assert_travel(&s, drawer, 100.0);
    // The drive formula past the limit is clamped natively; the rebuild reports it.
    let report = s.param_set("open", &350.0.into()).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.assembly["clamped"], json!([{"joint":joint,"slot":1,"requested":350.0,"applied":250.0}]));
    assert_travel(&s, drawer, 250.0);
    // The limit formula follows its parameter.
    s.param_set("travel", &300.0.into()).unwrap();
    assert_travel(&s, drawer, 300.0);
    assert_eq!(s.assembly_info()["joints"][0]["limit_max"], json!([null, 300.0, null]));
    // Clearing a bound removes its value and its formula.
    let r = limits(&mut s, joint, LimitEdit::Keep, LimitEdit::Clear).unwrap();
    assert_eq!(r["limit_max"], json!([null, null, null]));
    assert_eq!(r["limit_formulas"], json!({}));
    assert_eq!(r["clamped"], Value::Null, "the drive formula is no longer clamped");
    assert_travel(&s, drawer, 350.0);
}

#[test]
fn joint_create_applies_limits_before_the_first_drive() {
    let Drawer { s, drawer, joint, .. } = drawer(Some(JointLimits { min: Some(0.0.into()), max: Some(300.0.into()) }), Some(400.0.into()));
    let j = &s.project().joints[0];
    assert_eq!((j.id, j.limit_min[1], j.limit_max[1], j.drive[1]), (joint, Some(0.0), Some(300.0), Some(300.0)));
    assert_travel(&s, drawer, 300.0);
}

#[test]
fn save_reopen_and_undo_keep_limits_and_their_formulas() {
    let Drawer { mut s, drawer, joint, .. } = drawer(None, Some(0.0.into()));
    s.param_set("travel", &250.0.into()).unwrap();
    let before = snapshot(&s);
    limits(&mut s, joint, LimitEdit::Set((-10.0).into()), LimitEdit::Set("travel".into())).unwrap();
    s.joint_set(&JointRef::Id(joint), Some(&400.0.into()), None).unwrap();
    // Undo: the two calls go one by one, back to the exact state before.
    assert_eq!(s.undo().unwrap().call.unwrap().tool, "joint_set");
    assert_eq!(s.undo().unwrap().call.unwrap().tool, "joint_limits");
    unchanged(&s, &before);
    limits(&mut s, joint, LimitEdit::Set((-10.0).into()), LimitEdit::Set("travel".into())).unwrap();
    s.joint_set(&JointRef::Id(joint), Some(&400.0.into()), None).unwrap();
    let path = scratch("h2-limits.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let j = &opened.project().joints[0];
    assert_eq!((j.limit_min[1], j.limit_max[1], j.drive[1]), (Some(-10.0), Some(250.0), Some(250.0)));
    assert_eq!(opened.project().feat_dims[&joint].get("max1").map(String::as_str), Some("travel"));
    assert_travel(&opened, drawer, 250.0);
    opened.param_set("travel", &200.0.into()).unwrap();
    assert_travel(&opened, drawer, 200.0);
}

#[test]
fn refusals_leave_the_document_unchanged() {
    let Drawer { mut s, joint, .. } = drawer(None, Some(0.0.into()));
    limits(&mut s, joint, LimitEdit::Keep, LimitEdit::Set(100.0.into())).unwrap();
    s.param_set("lo", &0.0.into()).unwrap();
    let before = snapshot(&s);
    // F-157: min > max would send every value to max.
    let error = limits(&mut s, joint, LimitEdit::Set(200.0.into()), LimitEdit::Keep).unwrap_err().to_string();
    assert_eq!(
        error,
        format!(
            "joint {joint} offset limit min 200 is greater than max 100; QymCAD would send every value to max (F-157); nothing was changed"
        )
    );
    unchanged(&s, &before);
    let error = limits(&mut s, joint, LimitEdit::Set(100_001.0.into()), LimitEdit::Keep).unwrap_err().to_string();
    assert_eq!(error, format!("joint {joint} offset limit 100001 is outside the app's range [-100000, 100000]; nothing was changed"));
    let error = s.joint_limits(&JointRef::Id(joint), Some(JointSlot::Angle), &LimitEdit::Set(0.0.into()), &LimitEdit::Keep);
    assert_eq!(error.unwrap_err().to_string(), format!("joint {joint} has no free angle slot; slider limits apply to offset"));
    let error = limits(&mut s, joint, LimitEdit::Keep, LimitEdit::Keep).unwrap_err().to_string();
    assert_eq!(error, "joint_limits requires min or max");
    unchanged(&s, &before);
    // A formula bound that a later parameter edit pushes past the other bound.
    limits(&mut s, joint, LimitEdit::Set("lo".into()), LimitEdit::Keep).unwrap();
    let before = snapshot(&s);
    let error = s.param_set("lo", &150.0.into()).unwrap_err().to_string();
    assert!(error.contains("limit min 150 is greater than max 100") && error.contains("F-157"), "{error}");
    unchanged(&s, &before);
}

#[test]
fn a_rigid_joint_has_no_limits() {
    let Drawer { mut s, joint, carcass_body, drawer_body, .. } = drawer(None, Some(0.0.into()));
    s.joint_delete(&JointRef::Id(joint)).unwrap();
    let rigid = s
        .joint_create(&JointCreate {
            name: None,
            kind: AssemblyJointKind::Rigid,
            a: JointAnchor { body: carcass_body, face: Sel::Facing { dir: [1., 0., 0.], tol_deg: 1. } },
            b: JointAnchor { body: drawer_body, face: Sel::Facing { dir: [-1., 0., 0.], tol_deg: 1. } },
            mode: JointMode::AsBuilt,
            axis: None,
            offset: None,
            limits: None,
        })
        .unwrap()["joint"]["id"]
        .as_u64()
        .unwrap();
    let before = snapshot(&s);
    let error = limits(&mut s, rigid, LimitEdit::Set(0.0.into()), LimitEdit::Keep).unwrap_err().to_string();
    assert_eq!(error, format!("joint {rigid} of kind rigid has no free slot to limit"));
    unchanged(&s, &before);
}

/// F-157 native evidence (app data path, no Session): with min 0 above max −100, a request of −50 inside neither
/// bound order is silently sent to max.
#[test]
fn native_min_above_max_sends_every_value_to_max_f157() {
    let Drawer { mut s, joint, .. } = drawer(None, Some(0.0.into()));
    let path = scratch("h2-native-min-max.qcad");
    s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    let j = m.project.joints.iter_mut().find(|j| j.id == joint).unwrap();
    j.limit_min[1] = Some(0.0);
    j.limit_max[1] = Some(-100.0);
    m.project.set_joint_drive(joint, 1, Some(-50.0));
    let solved = m.project.solve_joints();
    assert!(solved.errors.is_empty() && !m.project.mates_conflict && m.project.mates_violated.is_empty());
    let j = m.project.joints.iter().find(|j| j.id == joint).unwrap();
    assert_eq!(j.drive[1], Some(-100.0));
    assert_close(j.offset, -100.0, 1e-6, "the reading is max, not the request");
}
