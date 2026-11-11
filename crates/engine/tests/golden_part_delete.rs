//! H1: `part_delete`, the app's tree Delete of a Part (`Project::delete_component`, ADR 0054 §1).
//! Hand volumes: carcass 600×800×700 = 336 000 000 mm³; drawer 500×600×150 = 45 000 000 mm³;
//! knob 40×30×20 = 24 000 mm³.
mod common;
use common::*;
use qymcad_core::feature::FeatureKind;
use qymcad_engine::*;
use serde_json::{json, Value};

type Snapshot = (Value, Vec<(Id, Vec<u8>)>, DocInfo);

/// Recipe without the id allocator (undo keeps ids past the live document), exact B-reps and the report.
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

fn extrude(sketch: Id, height: f64, op: Op, name: &str) -> Extrude {
    serde_json::from_value(json!({"sketch":sketch,"height":height,"op":op,"name":name})).unwrap()
}

fn part_bodies(s: &Session, part: Id) -> Vec<BodyInfo> {
    s.info().bodies.into_iter().filter(|b| b.owner_part == Some(part)).collect()
}

struct Cabinet {
    s: Session,
    carcass: Id,
    drawer: Id,
    copy: Id,
    joint: Id,
}

/// Grounded carcass, a drawer on a Slider whose travel is the formula `open` = 40, and a deep copy of the drawer.
fn cabinet() -> Cabinet {
    let mut s = Session::new_part();
    let carcass = s.project().active_component.unwrap();
    s.rename(RenameTarget::Part(carcass), "carcass").unwrap();
    s.param_set("open", &40.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("carcass-sk")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &600.0.into(), &800.0.into(), false).unwrap();
    let (carcass_body, _) = s.extrude(&extrude(sk, 700.0, Op::Add, "carcass-box")).unwrap();
    let drawer = s.part_new("drawer").unwrap()["part"]["id"].as_u64().unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("drawer-sk")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &500.0.into(), &600.0.into(), false).unwrap();
    let (drawer_body, _) = s.extrude(&extrude(sk, 150.0, Op::Add, "drawer-box")).unwrap();
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
            offset: Some("open".into()),
            angle: None,
            limits: None,
        })
        .unwrap()["joint"]["id"]
        .as_u64()
        .unwrap();
    let copy = s.part_copy(&PartRef::Id(drawer), "drawer-copy", Some([0.0, 0.0, 390.0]), None).unwrap()["part"]["id"].as_u64().unwrap();
    assert_eq!(s.assembly_info()["healthy"], true);
    assert_eq!(s.project().active_component, Some(drawer));
    Cabinet { s, carcass, drawer, copy, joint }
}

#[test]
fn deleting_the_moving_part_removes_it_with_its_joint_and_keeps_the_rest() {
    let Cabinet { mut s, carcass, drawer, copy, joint } = cabinet();
    let drawer_body = part_bodies(&s, drawer)[0].id;
    let carcass_connector = s.project().joints[0].a;
    let r = s.part_delete(&PartRef::Name("drawer".into())).unwrap();
    assert_eq!(r["changed"], true);
    assert_eq!(r["removed"]["part"], drawer);
    assert_eq!(r["removed"]["bodies"], json!([drawer_body]));
    assert_eq!(r["removed"]["joints"], json!([joint]));
    // The app's GUI leaves the navigation on the root when the active Part goes (`set_context_to(root)`).
    assert_eq!(r["active_component"], s.project().root);
    assert_eq!(s.project().active_component, Some(s.project().root));
    assert!(!s.project().components.iter().any(|c| c.id == drawer));
    assert!(s.project().joints.is_empty());
    // The rest is untouched: the grounded carcass and the independent copy.
    assert_close(part_bodies(&s, carcass)[0].volume, 336_000_000.0, 1e-3, "600*800*700");
    assert_close(part_bodies(&s, copy)[0].volume, 45_000_000.0, 1e-3, "500*600*150 copy");
    assert_eq!(part_bodies(&s, copy)[0].bbox_world[2], 390.0, "the copy keeps its placement");
    assert!(s.project().components.iter().find(|c| c.id == carcass).unwrap().grounded);
    // F-155: as the app does, the carcass connector of the deleted joint and the joint's formula stay in the file.
    assert!(s.project().connector(carcass_connector).is_some());
    assert_eq!(s.project().feat_dims.get(&joint).and_then(|d| d.get("offset")).map(String::as_str), Some("open"));
    assert_eq!(r["native_leftovers"]["connectors"], json!([carcass_connector]));
    assert_eq!(r["native_leftovers"]["formulas"], json!([joint]));
    assert_eq!(s.assembly_info()["healthy"], true);
    // A leftover formula of a deleted joint is not a user of its parameter.
    assert!(s.param_users("open").is_empty(), "{:?}", s.param_users("open"));
    s.param_delete("open").unwrap();
}

#[test]
fn undo_restores_the_deleted_part_exactly() {
    let Cabinet { mut s, drawer, .. } = cabinet();
    let before = snapshot(&s);
    s.part_delete(&PartRef::Id(drawer)).unwrap();
    let undone = s.undo().unwrap();
    assert_eq!(undone.call.unwrap().tool, "part_delete");
    unchanged(&s, &before);
    assert_eq!(s.assembly_info()["healthy"], true);
}

#[test]
fn deleting_the_anchor_part_frees_the_moved_part_at_its_reached_placement() {
    let Cabinet { mut s, carcass, drawer, .. } = cabinet();
    let reached = s.project().world_transform(drawer);
    assert_close(reached[7], -40.0, 1e-5, "XZ travel 40 is -Y");
    let r = s.part_delete(&PartRef::Id(carcass)).unwrap();
    assert_eq!(r["active_component"], drawer, "the active drawer stays active");
    assert!(s.project().joints.is_empty());
    assert_eq!(s.project().world_transform(drawer), reached);
    assert!(!s.project().components.iter().find(|c| c.id == drawer).unwrap().grounded);
    // The drawer is free now: part_place moves it.
    s.part_place(&PartRef::Id(drawer), [0.0, 0.0, 0.0], None).unwrap();
    assert_eq!(part_bodies(&s, drawer)[0].bbox_world[1], -300.0);
}

#[test]
fn save_and_reopen_after_a_delete() {
    let Cabinet { mut s, carcass, drawer, copy, .. } = cabinet();
    s.part_delete(&PartRef::Id(drawer)).unwrap();
    let path = scratch("h1-part-delete.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let ids: Vec<Id> = opened.project().components.iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![opened.project().root, carcass, copy]);
    assert_eq!(opened.assembly_info()["healthy"], true);
    assert_close(part_bodies(&opened, copy)[0].volume, 45_000_000.0, 1e-3, "reopened copy");
    opened.param_delete("open").unwrap();
}

#[test]
fn deleting_a_clone_part_keeps_its_source() {
    let mut s = Session::new_part();
    let knob = s.project().active_component.unwrap();
    s.rename(RenameTarget::Part(knob), "knob").unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("knob-sk")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    s.extrude(&extrude(sk, 20.0, Op::Add, "knob-body")).unwrap();
    let clone =
        s.part_instance(&PartRef::Name("knob".into()), "knob2", Some([100.0, 0.0, 0.0]), None).unwrap()["part"]["id"].as_u64().unwrap();
    let source_before = part_bodies(&s, knob);
    let r = s.part_delete(&PartRef::Id(clone)).unwrap();
    assert_eq!(r["removed"]["part"], clone);
    assert!(!s.project().timeline.iter().any(|n| matches!(n.kind, FeatureKind::PartInstance { .. })));
    let source_after = part_bodies(&s, knob);
    assert_eq!((source_after.len(), source_after[0].id, source_after[0].volume), (1, source_before[0].id, source_before[0].volume));
    assert_eq!(source_after[0].bbox_world, source_before[0].bbox_world);
    let info = serde_json::to_value(s.info().components).unwrap();
    let source = info.as_array().unwrap().iter().find(|c| c["id"] == knob).unwrap().clone();
    assert!(source.get("instances").is_none(), "{source}");
    // The instance-node refusal now names the tool that removes a clone.
    let again =
        s.part_instance(&PartRef::Name("knob".into()), "knob3", Some([100.0, 0.0, 0.0]), None).unwrap()["part"]["id"].as_u64().unwrap();
    let node = s.project().timeline.iter().find(|n| n.parent == Some(again)).unwrap().id;
    let error = s.feature_delete(node, false).unwrap_err().to_string();
    assert!(error.contains("part_delete"), "{error}");
}

#[test]
fn refusals_leave_the_document_unchanged() {
    let Cabinet { mut s, drawer, .. } = cabinet();
    let before = snapshot(&s);
    let error = s.part_delete(&PartRef::Root { root: true }).unwrap_err().to_string();
    assert_eq!(error, "the root assembly cannot be deleted");
    let root = s.project().root;
    let error = s.part_delete(&PartRef::Id(root)).unwrap_err().to_string();
    assert_eq!(error, "the root assembly cannot be deleted");
    let error = s.part_delete(&PartRef::Name("nope".into())).unwrap_err().to_string();
    assert_eq!(error, "no part named 'nope'");
    unchanged(&s, &before);
    // F-154: a source with clones.
    let clone = s.part_instance(&PartRef::Id(drawer), "drawer2", Some([0.0, 0.0, 600.0]), None).unwrap()["part"]["id"].as_u64().unwrap();
    let before = snapshot(&s);
    let error = s.part_delete(&PartRef::Id(drawer)).unwrap_err().to_string();
    assert_eq!(
        error,
        format!("part {drawer} is the source of instances [{clone}]; delete the instances first (QymCAD leaves them pointing at a missing part with their old body, F-154); nothing was changed")
    );
    unchanged(&s, &before);
}

#[test]
fn part_delete_while_a_history_rollback_is_active_is_refused() {
    let mut s = Session::new_part();
    let a = s.project().active_component.unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("a-sk")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let (a_body, _) = s.extrude(&extrude(sk, 20.0, Op::Add, "a-box")).unwrap();
    s.part_new("spare").unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("spare-sk")).unwrap();
    s.sketch_rect(sk, &100.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    s.extrude(&extrude(sk, 20.0, Op::Add, "spare-box")).unwrap();
    s.history_rollback(Some(a_body), None).unwrap();
    let before = snapshot(&s);
    let error = s.part_delete(&PartRef::Id(a)).unwrap_err().to_string();
    assert_eq!(
        error,
        "part_delete is refused while a history rollback is active: QymCAD keeps the bar's index, so nodes past the bar would be built (F-156); roll the history to the end first; nothing was changed"
    );
    unchanged(&s, &before);
}

/// F-154 native evidence (app data path, no Session): the app's tree Delete of a cloned source, followed by the
/// GUI's dirty-only resync, leaves the clone's old body with no error; a forced rebuild reports an error yet keeps
/// the stale body.
#[test]
fn native_deleting_a_cloned_source_leaves_a_stale_clone_f154() {
    let mut s = Session::new_part();
    let knob = s.project().active_component.unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("knob-sk")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    s.extrude(&extrude(sk, 20.0, Op::Add, "knob-body")).unwrap();
    let clone = s.part_instance(&PartRef::Id(knob), "knob2", Some([100.0, 0.0, 0.0]), None).unwrap()["part"]["id"].as_u64().unwrap();
    let clone_body = part_bodies(&s, clone)[0].id;
    let path = scratch("h1-native-delete-source.qcad");
    s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    let removed = m.project.delete_component(knob);
    assert!(!removed.is_empty());
    let node = m.project.timeline.iter().find(|n| n.parent == Some(clone)).unwrap();
    assert!(!node.dirty, "delete_component does not mark the copier dirty");
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut m.project, std::mem::take(&mut m.shapes));
    m.shapes = shapes;
    assert!(report.errors.is_empty(), "the GUI's dirty-only resync reports nothing: {:?}", report.errors);
    assert!(m.project.mesh_index(clone_body).is_some(), "the clone still shows its body");
    {
        let _gate = qymcad_kernel::kernel_gate();
        assert_close(m.shapes[&clone_body].volume(), 24000.0, 1e-6, "stale clone volume");
    }
    m.project.mark_all_dirty();
    let (report, _) = qymcad_testkit::regenerate_dirty_with_shapes(&mut m.project, std::mem::take(&mut m.shapes));
    assert!(!report.errors.is_empty(), "a forced rebuild finally reports the missing source");
    assert!(m.project.mesh_index(clone_body).is_some(), "and still keeps the stale body");
}

/// F-155 native evidence: deleting the moving Part removes its joint but keeps the joint's formula and the
/// connector on the surviving Part.
#[test]
fn native_deleting_a_part_leaves_joint_formulas_and_connectors_f155() {
    let Cabinet { s, drawer, joint, .. } = cabinet();
    let carcass_connector = s.project().joints[0].a;
    let path = scratch("h1-native-orphans.qcad");
    let mut s = s;
    s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    m.project.delete_component(drawer);
    assert!(m.project.joints.is_empty());
    assert!(m.project.connector(carcass_connector).is_some(), "the surviving connector stays");
    assert!(m.project.connector_users(carcass_connector).is_empty(), "with no joint using it");
    assert_eq!(m.project.feat_dims.get(&joint).and_then(|d| d.get("offset")).map(String::as_str), Some("open"));
}

/// F-156 native evidence: deleting a Part under an active rollback keeps the bar's index, so a node that was past
/// the bar is built at the next rebuild.
#[test]
fn native_deleting_a_part_under_rollback_slides_the_bar_f156() {
    let mut s = Session::new_part();
    let a = s.project().active_component.unwrap();
    s.rename(RenameTarget::Part(a), "a").unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("a-sk")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    s.extrude(&extrude(sk, 20.0, Op::Add, "a-box")).unwrap();
    let b = s.part_new("b").unwrap()["part"]["id"].as_u64().unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("b-sk")).unwrap();
    s.sketch_rect(sk, &100.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let (first, _) = s.extrude(&extrude(sk, 20.0, Op::Add, "b-box")).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("b2-sk")).unwrap();
    s.sketch_rect(sk, &200.0.into(), &0.0.into(), &30.0.into(), &30.0.into(), false).unwrap();
    let (second, _) = s.extrude(&extrude(sk, 30.0, Op::NewBody, "b-second")).unwrap();
    let path = scratch("h1-native-rollback.qcad");
    s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    let bar = m.project.timeline.iter().position(|n| n.id == first).unwrap() + 1;
    m.project.set_rollback(Some(bar));
    native_assembly_rebuild(&mut m, true);
    assert!(m.project.mesh_index(second).is_none(), "the second body is past the bar");
    m.project.delete_component(a);
    assert_eq!(m.project.rollback, Some(bar), "the index is kept");
    native_assembly_rebuild(&mut m, true);
    assert!(m.project.mesh_index(second).is_some(), "the node past the old bar is now built");
    let _gate = qymcad_kernel::kernel_gate();
    assert_close(m.shapes[&second].volume(), 27000.0, 1e-6, "30*30*30 built");
    let _ = b;
}
