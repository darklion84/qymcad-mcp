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
            a: JointAnchor::face(carcass_body, Sel::Facing { dir: [1., 0., 0.], tol_deg: 1. }),
            b: JointAnchor::face(drawer_body, Sel::Facing { dir: [-1., 0., 0.], tol_deg: 1. }),
            mode: JointMode::AsBuilt,
            axis: Some(BaseName::XZ),
            offset,
            angle: None,
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
    // One format everywhere: {joint, slot name}.
    let at = json!([{"joint":joint,"slot":"offset"}]);
    assert_eq!(info["joints"][0]["at_limit"], at);
    assert_eq!(info["diagnostics"]["at_limit"], at);
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
    // Clearing a bound removes its value; its formula stays dormant, as the app's tick-off leaves it.
    let r = limits(&mut s, joint, LimitEdit::Keep, LimitEdit::Clear).unwrap();
    assert_eq!(r["limit_max"], json!([null, null, null]));
    assert_eq!(r["limit_formulas"], json!({}));
    assert_eq!(r["dormant_limit_formulas"], json!({"max":"travel"}));
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
            a: JointAnchor::face(carcass_body, Sel::Facing { dir: [1., 0., 0.], tol_deg: 1. }),
            b: JointAnchor::face(drawer_body, Sel::Facing { dir: [-1., 0., 0.], tol_deg: 1. }),
            mode: JointMode::AsBuilt,
            axis: None,
            offset: None,
            angle: None,
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

// ---- H5 fix round -------------------------------------------------------------------------------------------

/// A copy of the drawer's document with `edit` applied to the native Project the way the app's Limits box would,
/// saved through the native io. `settle` runs the native rebuild and solve after the edit.
fn patched_copy(d: &mut Drawer, name: &str, settle: bool, edit: impl FnOnce(&mut NativeAssemblyModel, Id)) -> std::path::PathBuf {
    let src = scratch(&format!("{name}-src.qcad"));
    d.s.save(Some(&src)).unwrap();
    let mut m = native_assembly_load(&src);
    native_assembly_rebuild(&mut m, true);
    edit(&mut m, d.joint);
    if settle {
        native_assembly_rebuild(&mut m, true);
    }
    let out = scratch(&format!("{name}.qcad"));
    native_assembly_save(&m, &out);
    out
}

#[test]
fn a_joint_with_min_above_max_from_a_file_blocks_only_the_edits_that_touch_it() {
    let mut d = drawer(None, Some(0.0.into()));
    let joint = d.joint;
    let path = patched_copy(&mut d, "h5-min-max", true, |m, j| {
        let jj = m.project.joints.iter_mut().find(|x| x.id == j).unwrap();
        jj.limit_min[1] = Some(0.0);
        jj.limit_max[1] = Some(-100.0);
    });
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    // Reported as a diagnostic, not as a lock.
    let faults = report.assembly["limit_faults"].as_array().unwrap().clone();
    assert_eq!(faults.len(), 1, "{:?}", report.assembly);
    assert_eq!(faults[0]["joint"], joint);
    let message = faults[0]["message"].as_str().unwrap();
    assert!(message.contains("min 0 is greater than max -100") && message.contains("F-157"), "{message}");
    assert!(!message.contains("nothing was changed"), "a diagnostic is not a refusal: {message}");
    assert_eq!(s.assembly_info()["diagnostics"]["limit_faults"], json!(faults));
    // Unrelated edits go on: a parameter, a Part selection, a sketch, a Part.
    s.param_set("fresh", &5.0.into()).unwrap();
    s.part_select(&PartRef::Root { root: true }).unwrap();
    s.part_new("extra").unwrap();
    s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("extra-sk")).unwrap();
    // An edit that touches the joint is still refused, and the refusal leaves the document as it was.
    let before = snapshot(&s);
    let error = limits(&mut s, joint, LimitEdit::Keep, LimitEdit::Set((-50.0).into())).unwrap_err().to_string();
    assert!(error.contains("min 0 is greater than max -50") && error.contains("F-157"), "{error}");
    unchanged(&s, &before);
    // The edit that repairs it is allowed, and the diagnostic goes away.
    limits(&mut s, joint, LimitEdit::Set((-200.0).into()), LimitEdit::Keep).unwrap();
    assert_eq!(s.assembly_info()["diagnostics"]["limit_faults"], json!([]));
    s.joint_set(&JointRef::Id(joint), Some(&(-150.0).into()), None).unwrap();
}

#[test]
fn a_parameter_edit_validates_the_joints_whose_formulas_read_it() {
    let mut d = drawer(None, Some(0.0.into()));
    let joint = d.joint;
    d.s.param_set("lo", &0.0.into()).unwrap();
    limits(&mut d.s, joint, LimitEdit::Set("lo".into()), LimitEdit::Set(100.0.into())).unwrap();
    // The app moves the parameter past the bound: its limit formula now reads 200 above max 100.
    let path = patched_copy(&mut d, "h5-formula-fault", false, |m, _| {
        native_assembly_parameter(m, "lo", "200");
    });
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.assembly["limit_faults"][0]["joint"], joint, "{:?}", report.assembly);
    s.param_set("unrelated", &1.0.into()).unwrap();
    // A parameter the joint's formula reads: the edit touches the joint, and the joint is still faulty.
    let before = snapshot(&s);
    let error = s.param_set("lo", &300.0.into()).unwrap_err().to_string();
    assert!(error.contains("F-157"), "{error}");
    unchanged(&s, &before);
    // The edit that repairs it is allowed.
    s.param_set("lo", &50.0.into()).unwrap();
    assert_eq!(s.assembly_info()["diagnostics"]["limit_faults"], json!([]));
}

#[test]
fn clearing_a_bound_keeps_its_formula_dormant_and_nothing_reads_a_dormant_formula() {
    let Drawer { mut s, joint, drawer: part, .. } = drawer(None, Some(0.0.into()));
    s.param_set("travel", &250.0.into()).unwrap();
    limits(&mut s, joint, LimitEdit::Set(0.0.into()), LimitEdit::Set("travel".into())).unwrap();
    let r = limits(&mut s, joint, LimitEdit::Keep, LimitEdit::Clear).unwrap();
    assert_eq!(r["limit_max"], json!([null, null, null]));
    assert_eq!(r["limit_formulas"], json!({}));
    assert_eq!(r["dormant_limit_formulas"], json!({"max":"travel"}));
    assert_eq!(s.project().feat_dims[&joint].get("max1").map(String::as_str), Some("travel"), "the app keeps it");
    assert_eq!(s.assembly_info()["joints"][0]["dormant_limit_formulas"], json!({"max":"travel"}));
    // The bound is off: the travel is no longer held, and nothing reads the formula.
    s.joint_set(&JointRef::Id(joint), Some(&400.0.into()), None).unwrap();
    assert_travel(&s, part, 400.0);
    assert!(s.param_users("travel").is_empty(), "{:?}", s.param_users("travel"));
    s.param_delete("travel").unwrap();
    // The orphan formula is evaluated nowhere: edits, diagnostics, save and reopen stay clean.
    s.param_set("other", &1.0.into()).unwrap();
    assert_eq!(s.assembly_info()["healthy"], true);
    let path = scratch("h5-dormant.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.assembly["healthy"], true, "{:?}", report.assembly);
    opened.param_set("another", &2.0.into()).unwrap();
    // A formula typed again revives the bound as a live one.
    opened.param_set("travel", &300.0.into()).unwrap();
    let r = limits(&mut opened, joint, LimitEdit::Keep, LimitEdit::Set("travel".into())).unwrap();
    assert_eq!(r["limit_max"], json!([null, 300.0, null]));
    assert_eq!(r["limit_formulas"], json!({"max":"travel"}));
    assert_eq!(r["dormant_limit_formulas"], json!({}));
    assert_travel(&opened, part, 300.0);
}

/// Native evidence for the dormant formula (app data path, no Session): the app's tick-off keeps the formula, the
/// formula is not read while the bound is off, ticking the bound back on re-imposes it at the next evaluation, and a
/// driver rename rewrites it while dormant.
#[test]
fn native_a_ticked_off_limit_keeps_its_formula_and_ticking_it_on_reimposes_it() {
    let mut d = drawer(None, Some(0.0.into()));
    d.s.param_set("lim", &100.0.into()).unwrap();
    limits(&mut d.s, d.joint, LimitEdit::Keep, LimitEdit::Set("lim".into())).unwrap();
    let path = scratch("h5-native-dormant.qcad");
    d.s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    let p = &mut m.project;
    assert_eq!(p.joints.iter().find(|j| j.id == d.joint).unwrap().limit_max[1], Some(100.0));
    // The app's tick-off (`popup.rs`): the value becomes None, the formula stays.
    p.joints.iter_mut().find(|j| j.id == d.joint).unwrap().limit_max[1] = None;
    p.parameters.iter_mut().find(|q| q.name == "lim").unwrap().expr = "50".into();
    assert!(p.eval_parameters().is_empty(), "a dormant formula is not evaluated");
    assert_eq!(p.joints.iter().find(|j| j.id == d.joint).unwrap().limit_max[1], None);
    assert_eq!(p.feat_dim(d.joint, "max1"), Some("lim"));
    // Ticking it on again (the app sets 0.0): the next evaluation re-imposes the formula's value.
    p.joints.iter_mut().find(|j| j.id == d.joint).unwrap().limit_max[1] = Some(0.0);
    p.eval_parameters();
    assert_eq!(p.joints.iter().find(|j| j.id == d.joint).unwrap().limit_max[1], Some(50.0));
    // A rename of the driver rewrites the dormant formula too.
    p.joints.iter_mut().find(|j| j.id == d.joint).unwrap().limit_max[1] = None;
    p.rename_driver("lim", "lim2").unwrap();
    assert_eq!(p.feat_dim(d.joint, "max1"), Some("lim2"));
}

#[test]
fn a_numeric_drive_a_limit_formula_clamps_is_reported_on_param_set_dimension_set_and_open() {
    // param_set: the drive 200 was asked once and the limit formula now holds it at 100.
    let Drawer { mut s, drawer: part, joint, carcass_body, .. } = drawer(None, Some(200.0.into()));
    s.param_set("travel", &300.0.into()).unwrap();
    limits(&mut s, joint, LimitEdit::Set(0.0.into()), LimitEdit::Set("travel".into())).unwrap();
    assert_travel(&s, part, 200.0);
    let report = s.param_set("travel", &100.0.into()).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.assembly["clamped"], json!([{"joint":joint,"slot":1,"requested":200.0,"applied":100.0}]), "{:?}", report.assembly);
    assert_travel(&s, part, 100.0);
    // dimension_set: a named dimension feeds the limit formula.
    let height = DimensionTarget::Feature { feature: carcass_body, key: "height".into() };
    s.dimension_set(&height, None, Some("cab_h")).unwrap();
    limits(&mut s, joint, LimitEdit::Keep, LimitEdit::Set("cab_h/2".into())).unwrap();
    s.joint_set(&JointRef::Id(joint), Some(&340.0.into()), None).unwrap();
    assert_travel(&s, part, 340.0);
    let edit = s.dimension_set(&height, Some(&600.0.into()), None).unwrap();
    assert_eq!(
        edit.rebuild.assembly["clamped"],
        json!([{"joint":joint,"slot":1,"requested":340.0,"applied":300.0}]),
        "{:?}",
        edit.rebuild.assembly
    );
    assert_travel(&s, part, 300.0);
    // doc_open: the file still holds the drive 200 and a limit 100 the app wrote without solving.
    let mut d = drawer(None, Some(200.0.into()));
    let path = patched_copy(&mut d, "h5-open-clamp", false, |m, j| {
        m.project.joints.iter_mut().find(|x| x.id == j).unwrap().limit_max[1] = Some(100.0);
    });
    let (_, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.assembly["clamped"], json!([{"joint":d.joint,"slot":1,"requested":200.0,"applied":100.0}]), "{:?}", report.assembly);
}

#[test]
fn an_evaluated_limit_formula_is_not_held_to_the_field_range() {
    let Drawer { mut s, joint, .. } = drawer(None, Some(0.0.into()));
    s.param_set("k", &10.0.into()).unwrap();
    limits(&mut s, joint, LimitEdit::Set(0.0.into()), LimitEdit::Set("k*5".into())).unwrap();
    // The GUI bounds the typed field only; a formula that comes to 150000 breaks nothing natively.
    s.param_set("k", &30000.0.into()).unwrap();
    assert_eq!(s.assembly_info()["joints"][0]["limit_max"], json!([null, 150000.0, null]));
    limits(&mut s, joint, LimitEdit::Keep, LimitEdit::Set("k*20000".into())).unwrap();
    // A typed number is still held to the field's range.
    let error = limits(&mut s, joint, LimitEdit::Keep, LimitEdit::Set(100_001.0.into())).unwrap_err().to_string();
    assert!(error.contains("outside the app's range"), "{error}");
}

/// F-165 native evidence (app data path, no Session): a limit that holds a numeric drive overwrites it with the
/// clamped value; the request is gone, and raising the limit again does not bring it back. Nothing is reported but
/// the at-limit mark.
#[test]
fn native_a_limit_overwrites_a_numeric_drive_and_the_request_is_gone_f165() {
    let mut d = drawer(None, Some(200.0.into()));
    let path = scratch("h5-native-overwrite.qcad");
    d.s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    let joint = d.joint;
    m.project.joints.iter_mut().find(|j| j.id == joint).unwrap().limit_max[1] = Some(100.0);
    let solved = m.project.solve_joints();
    assert!(solved.errors.is_empty() && !m.project.mates_conflict && m.project.mates_violated.is_empty());
    let j = m.project.joints.iter().find(|j| j.id == joint).unwrap();
    assert_eq!(j.drive[1], Some(100.0), "the request 200 is overwritten with the clamped value");
    assert_eq!(m.project.joints_at_limit(), vec![(joint, 1)]);
    m.project.joints.iter_mut().find(|j| j.id == joint).unwrap().limit_max[1] = Some(300.0);
    assert!(m.project.solve_joints().errors.is_empty());
    assert_eq!(m.project.joints.iter().find(|j| j.id == joint).unwrap().drive[1], Some(100.0), "raising the limit does not restore 200");
}
