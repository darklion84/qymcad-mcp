//! G3: native part clones (`Project::clone_part`, the app's tree action «Сделать клон», ADR 0053).
//! Hand volumes: the knob is a kw×30×20 box, 40×30×20 = 24000 mm³, 50×30×20 = 30000 mm³.
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

/// Root Part "knob": one kw×30×20 box centred on the origin in XY.
fn knob() -> (Session, Id, Id) {
    let mut s = Session::new_part();
    let part = s.project().active_component.unwrap();
    s.rename(RenameTarget::Part(part), "knob").unwrap();
    s.param_set("kw", &40.0.into()).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("knob-sk")).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &"kw".into(), &30.0.into(), false).unwrap();
    let (body, report) = s.extrude(&extrude(sketch, 20.0, Op::Add, "knob-body")).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_close(report.bodies[0].volume, 24000.0, 1e-6, "40*30*20");
    (s, part, body)
}

fn part_bodies(s: &Session, part: Id) -> Vec<BodyInfo> {
    s.info().bodies.into_iter().filter(|b| b.owner_part == Some(part)).collect()
}

fn component(s: &Session, id: Id) -> Value {
    serde_json::to_value(s.info().components).unwrap().as_array().unwrap().iter().find(|c| c["id"] == id).unwrap().clone()
}

fn instance(s: &mut Session, source: &str, name: &str, at: Option<[f64; 3]>) -> (Id, Value) {
    let r = s.part_instance(&PartRef::Name(source.into()), name, at, None).unwrap();
    (r["part"]["id"].as_u64().unwrap(), r)
}

#[test]
fn instance_matches_the_source_and_is_shifted_only_by_its_placement() {
    let (mut s, knob, body) = knob();
    let (clone, r) = instance(&mut s, "knob", "knob2", Some([100.0, 0.0, 0.0]));
    assert_eq!(r["part"]["name"], "knob2");
    assert_eq!(r["part"]["instance_of"], knob);
    let source = part_bodies(&s, knob);
    let copies = part_bodies(&s, clone);
    assert_eq!((source.len(), copies.len()), (1, 1));
    assert_eq!(source[0].id, body);
    assert_ne!(copies[0].id, body, "the clone owns a body of its own");
    assert_close(copies[0].volume, 24000.0, 1e-6, "clone volume");
    assert_eq!(copies[0].bbox_local, source[0].bbox_local);
    for k in 0..6 {
        let shift = if k % 3 == 0 { 100.0 } else { 0.0 };
        assert_close(copies[0].bbox_world[k], source[0].bbox_world[k] + shift, 1e-9, "world bbox shifted by placement");
    }
    // Reporting: doc_info and assembly_info name the source on the clone and the clones on the source.
    assert_eq!(component(&s, clone)["instance_of"], knob);
    assert_eq!(component(&s, knob)["instances"], json!([clone]));
    assert!(component(&s, knob).get("instance_of").is_none());
    let parts = s.assembly_info()["parts"].clone();
    let find = |id: Id| parts.as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
    assert_eq!(find(clone)["instance_of"], knob);
    assert_eq!(find(knob)["instances"], json!([clone]));
    // The native recipe: one PartInstance node under the new Part.
    let node = s.project().timeline.iter().find(|n| n.parent == Some(clone)).unwrap();
    assert!(matches!(node.kind, FeatureKind::PartInstance { src_comp, .. } if src_comp == knob));
}

#[test]
fn instance_without_placement_stands_beside_the_source_as_the_app_places_it() {
    let (mut s, knob, _) = knob();
    let (clone, _) = instance(&mut s, "knob", "knob2", None);
    // free_place_beside: step = 40 + max(40/5, 5) = 48 mm along X; the first spot clear of every body.
    let t = s.project().components.iter().find(|c| c.id == clone).unwrap().transform;
    assert_eq!([t[3], t[7], t[11]], [48.0, 0.0, 0.0]);
    // A clone of a clone repeats the original, as natively.
    let (third, r) = instance(&mut s, "knob2", "knob3", None);
    assert_eq!(r["part"]["instance_of"], knob);
    assert_eq!(component(&s, third)["instance_of"], knob);
    assert_eq!(component(&s, knob)["instances"], json!([clone, third]));
    // Names stay unique: the app's same-name clone would make PartRef ambiguous.
    let before = snapshot(&s);
    let error = s.part_instance(&PartRef::Id(knob), "knob2", None, None).unwrap_err().to_string();
    assert!(error.contains("a part named 'knob2' already exists"), "{error}");
    unchanged(&s, &before);
}

#[test]
fn instance_follows_a_source_parameter_edit() {
    let (mut s, knob, _) = knob();
    let (clone, _) = instance(&mut s, "knob", "knob2", Some([100.0, 0.0, 0.0]));
    let report = s.param_set("kw", &50.0.into()).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_close(part_bodies(&s, knob)[0].volume, 30000.0, 1e-6, "source 50*30*20");
    assert_close(part_bodies(&s, clone)[0].volume, 30000.0, 1e-6, "clone follows the source");
    assert_eq!(part_bodies(&s, clone)[0].bbox_local, part_bodies(&s, knob)[0].bbox_local);
}

#[test]
fn undo_removes_the_instance_exactly() {
    let (mut s, _, _) = knob();
    let before = snapshot(&s);
    instance(&mut s, "knob", "knob2", Some([100.0, 0.0, 0.0]));
    let undone = s.undo().unwrap();
    assert_eq!(undone.call.unwrap().tool, "part_instance");
    unchanged(&s, &before);
}

#[test]
fn save_and_reopen_keep_the_part_instance_and_its_placement() {
    let (mut s, knob, _) = knob();
    let (clone, _) = instance(&mut s, "knob", "knob2", Some([100.0, 0.0, 25.0]));
    let path = scratch("g3-instance.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let node = opened.project().timeline.iter().find(|n| n.parent == Some(clone)).unwrap();
    assert!(matches!(node.kind, FeatureKind::PartInstance { src_comp, .. } if src_comp == knob));
    let t = opened.project().components.iter().find(|c| c.id == clone).unwrap().transform;
    assert_eq!([t[3], t[7], t[11]], [100.0, 0.0, 25.0]);
    assert_close(part_bodies(&opened, clone)[0].volume, 24000.0, 1e-6, "reopened clone");
    assert_eq!(component(&opened, clone)["instance_of"], knob);
    // The reopened clone still follows its source.
    opened.param_set("kw", &50.0.into()).unwrap();
    assert_close(part_bodies(&opened, clone)[0].volume, 30000.0, 1e-6, "reopened clone follows");
}

#[test]
fn a_two_body_source_is_refused_with_the_reason_and_the_alternative() {
    let (mut s, _, _) = knob();
    let pair = s.part_new("pair").unwrap()["part"]["id"].as_u64().unwrap();
    for (name, x) in [("a", 0.0), ("b", 50.0)] {
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some(&format!("{name}-sk"))).unwrap();
        s.sketch_rect(sk, &x.into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
        s.extrude(&extrude(sk, 10.0, Op::NewBody, name)).unwrap();
    }
    assert_eq!(part_bodies(&s, pair).len(), 2);
    let before = snapshot(&s);
    let error = s.part_instance(&PartRef::Name("pair".into()), "pair2", None, None).unwrap_err().to_string();
    assert!(error.contains("part_instance requires a single-body source"), "{error}");
    assert!(error.contains("F-146"), "{error}");
    assert!(error.contains("part_copy copies all bodies"), "{error}");
    unchanged(&s, &before);
}

#[test]
fn a_new_body_in_a_cloned_source_is_refused_atomically() {
    let (mut s, knob, _) = knob();
    let (clone, _) = instance(&mut s, "knob", "knob2", Some([100.0, 0.0, 0.0]));
    assert_eq!(s.project().active_component, Some(knob), "part_instance keeps the active Part");
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("extra-sk")).unwrap();
    s.sketch_rect(sk, &25.0.into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    // Settle the pending sketch first: a feature edit rebuilds pending nodes before its own snapshot (F-034).
    assert!(s.rebuild().errors.is_empty());
    let before = snapshot(&s);
    let error = s.extrude(&extrude(sk, 10.0, Op::NewBody, "extra")).unwrap_err().to_string();
    assert_eq!(
        error,
        format!(
            "edit makes part {knob} multi-body while instances [{clone}] copy it (native clones copy only the last body, F-146); nothing was changed"
        )
    );
    unchanged(&s, &before);
    // Adding material to the single body stays allowed and reaches the clone.
    let (_, report) = s.extrude(&extrude(sk, 10.0, Op::Add, "boss")).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_close(part_bodies(&s, clone)[0].volume, 25000.0, 1e-6, "24000 + 10*10*10 reaches the clone");
}

#[test]
fn modelling_inside_a_clone_is_refused_atomically() {
    let (mut s, knob, _) = knob();
    let (clone, _) = instance(&mut s, "knob", "knob2", Some([100.0, 0.0, 0.0]));
    let clone_body = part_bodies(&s, clone)[0].id;
    let message = format!("part {clone} is an instance of {knob}; edit the source part");
    // A feature whose target is the instance body.
    let before = snapshot(&s);
    let error = s.fillet(Some(clone_body), &Sel::EdgesOf(Box::new(Sel::Facing { dir: [0., 0., 1.], tol_deg: 1. })), &1.0.into(), None);
    assert_eq!(error.unwrap_err().to_string(), message);
    unchanged(&s, &before);
    // A sketch hosted on an instance face.
    let top = s.topology(Some(clone_body), false).unwrap().faces.into_iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap();
    let before = snapshot(&s);
    let error = s.sketch_create(&PlaneRef::Face { body: clone_body, face: top.id }, None).unwrap_err().to_string();
    assert_eq!(error, message);
    unchanged(&s, &before);
    // Modelling with the clone active (the GUI redirects a double click into the original instead).
    s.part_select(&PartRef::Id(clone)).unwrap();
    let before = snapshot(&s);
    let error = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap_err().to_string();
    assert_eq!(error, message);
    unchanged(&s, &before);
}

#[test]
fn a_slider_on_a_clone_moves_only_the_clone() {
    let mut s = Session::new_part();
    let carcass = s.project().active_component.unwrap();
    s.rename(RenameTarget::Part(carcass), "carcass").unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("carcass-sk")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &600.0.into(), &800.0.into(), false).unwrap();
    let (carcass_body, _) = s.extrude(&extrude(sk, 700.0, Op::Add, "carcass-box")).unwrap();
    let drawer = s.part_new("drawer").unwrap()["part"]["id"].as_u64().unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("drawer-sk")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &500.0.into(), &600.0.into(), false).unwrap();
    s.extrude(&extrude(sk, 150.0, Op::Add, "drawer-box")).unwrap();
    s.part_place(&PartRef::Id(drawer), [0.0, 0.0, 100.0], None).unwrap();
    s.ground(&PartRef::Id(carcass), true).unwrap();
    let (clone, _) = instance(&mut s, "drawer", "drawer2", Some([0.0, 0.0, 390.0]));
    let clone_body = part_bodies(&s, clone)[0].id;
    let joint = s
        .joint_create(&JointCreate {
            name: Some("drawer2-slide".into()),
            kind: AssemblyJointKind::Slider,
            a: JointAnchor { body: carcass_body, face: Sel::Facing { dir: [1., 0., 0.], tol_deg: 1. } },
            b: JointAnchor { body: clone_body, face: Sel::Facing { dir: [-1., 0., 0.], tol_deg: 1. } },
            mode: JointMode::AsBuilt,
            axis: Some(BaseName::XZ),
            offset: Some(0.0.into()),
        })
        .unwrap()["joint"]["id"]
        .as_u64()
        .unwrap();
    let before = s.info().bodies;
    s.joint_set(&JointRef::Id(joint), Some(&40.0.into()), None).unwrap();
    assert_eq!(s.assembly_info()["healthy"], true);
    for b in s.info().bodies {
        let old = before.iter().find(|o| o.id == b.id).unwrap();
        let dy = if b.owner_part == Some(clone) { -40.0 } else { 0.0 };
        for k in 0..6 {
            assert_close(b.bbox_world[k], old.bbox_world[k] + if k % 3 == 1 { dy } else { 0.0 }, 1e-5, "only the clone slides");
        }
    }
    // The source edit keeps the clone's joint healthy and persists.
    let path = scratch("g3-instance-slider.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(opened.assembly_info()["healthy"], true);
    let t = opened.project().world_transform(clone);
    assert_close(t[7], -40.0, 1e-5, "reopened slider travel");
}

#[test]
fn part_instance_while_a_history_rollback_is_active_is_refused() {
    // Native clone_part inserts the PartInstance at the bar: the clone misses the source's later features (a
    // filleted source, an unfilleted clone) and a source that is multi-body past the bar passes the guard
    // (FINDINGS F-152).
    let (mut s, _, body) = knob();
    s.fillet(None, &Sel::EdgesOf(Box::new(Sel::Facing { dir: [0., 0., 1.], tol_deg: 1. })), &2.0.into(), Some("round")).unwrap();
    s.history_rollback(Some(body), None).unwrap();
    let before = snapshot(&s);
    let error = s.part_instance(&PartRef::Name("knob".into()), "knob2", None, None).unwrap_err().to_string();
    assert!(error.contains("part_instance is refused while a history rollback is active"), "{error}");
    assert!(error.contains("F-152"), "{error}");
    unchanged(&s, &before);
    s.history_rollback(None, Some("end")).unwrap();
    instance(&mut s, "knob", "knob2", None);
}

#[test]
fn rollback_to_start_and_back_keeps_a_clone_healthy() {
    let (mut s, _, _) = knob();
    let (clone, _) = instance(&mut s, "knob", "knob2", Some([100.0, 0.0, 0.0]));
    let r = s.history_rollback(None, Some("start")).unwrap();
    assert!(r.rebuild.errors.is_empty(), "{:?}", r.rebuild);
    assert!(part_bodies(&s, clone).is_empty(), "the clone node is past the bar");
    let r = s.history_rollback(None, Some("end")).unwrap();
    assert!(r.rebuild.errors.is_empty(), "{:?}", r.rebuild);
    assert_close(part_bodies(&s, clone)[0].volume, 24000.0, 1e-6, "clone back after rolling forward");
    assert_eq!(s.assembly_info()["healthy"], true);
}

#[test]
fn suppressing_a_cloned_source_feature_reaches_the_clone_but_its_base_is_refused() {
    let (mut s, knob, body) = knob();
    let (round, _) =
        s.fillet(None, &Sel::EdgesOf(Box::new(Sel::Facing { dir: [0., 0., 1.], tol_deg: 1. })), &2.0.into(), Some("round")).unwrap();
    let (clone, _) = instance(&mut s, "knob", "knob2", Some([100.0, 0.0, 0.0]));
    let filleted = part_bodies(&s, knob)[0].volume;
    assert!(filleted < 24000.0);
    assert_close(part_bodies(&s, clone)[0].volume, filleted, 1e-6, "clone filleted");
    // A modifier suppressed in the source reaches the clone, and back.
    s.feature_suppress(round, true).unwrap();
    assert_close(part_bodies(&s, clone)[0].volume, 24000.0, 1e-6, "clone follows the suppressed fillet");
    s.feature_suppress(round, false).unwrap();
    assert_close(part_bodies(&s, clone)[0].volume, filleted, 1e-6, "clone follows the restored fillet");
    // Suppressing the base leaves the source bodiless while QymCAD keeps the clone's old body (F-153).
    let before = snapshot(&s);
    let error = s.feature_suppress(body, true).unwrap_err().to_string();
    assert_eq!(
        error,
        format!("edit leaves part {knob} without a solid body, but QymCAD keeps the old body in its instances [{clone}] without an error (F-153); nothing was changed")
    );
    unchanged(&s, &before);
}

#[test]
fn deleting_or_suppressing_a_clone_node_says_how_to_remove_a_clone() {
    let (mut s, _, _) = knob();
    let (clone, _) = instance(&mut s, "knob", "knob2", Some([100.0, 0.0, 0.0]));
    let node = s.project().timeline.iter().find(|n| n.parent == Some(clone)).unwrap().id;
    let before = snapshot(&s);
    for error in [s.feature_delete(node, false).unwrap_err().to_string(), s.feature_suppress(node, true).unwrap_err().to_string()] {
        assert!(error.contains("the app removes a clone as a whole Part"), "{error}");
        assert!(error.contains("undo"), "{error}");
        assert!(!error.contains("edit the source part"), "{error}");
    }
    unchanged(&s, &before);
}

/// F-153 native evidence (app data path, no Session): suppressing a cloned source's base removes the source's
/// body, while the clone keeps a body and no error is reported, even after a forced rebuild.
#[test]
fn native_suppressed_source_base_keeps_a_stale_clone_body_f153() {
    let (mut s, _, body) = knob();
    let (clone, _) = instance(&mut s, "knob", "knob2", Some([100.0, 0.0, 0.0]));
    let clone_body = part_bodies(&s, clone)[0].id;
    let path = scratch("g3-native-suppress.qcad");
    s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    let ti = m.project.timeline.iter().position(|n| n.id == body).unwrap();
    m.project.set_feature_suppressed(ti, true);
    for force in [false, true] {
        native_assembly_rebuild(&mut m, force); // asserts no native errors
        assert!(m.project.mesh_index(body).is_none(), "the suppressed source has no body");
        assert!(m.project.mesh_index(clone_body).is_some(), "the clone still shows a body");
        let _gate = qymcad_kernel::kernel_gate();
        assert_close(m.shapes[&clone_body].volume(), 24000.0, 1e-6, "stale clone volume");
    }
}

/// F-152 native evidence (app data path, no Session): a clone made while the history is rolled back is inserted at
/// the bar; rolled forward, the source is filleted and the clone is not, with no error.
#[test]
fn native_clone_at_the_rollback_bar_misses_later_source_features_f152() {
    let (mut s, knob, body) = knob();
    s.fillet(None, &Sel::EdgesOf(Box::new(Sel::Facing { dir: [0., 0., 1.], tol_deg: 1. })), &2.0.into(), Some("round")).unwrap();
    let path = scratch("g3-native-rollback-clone.qcad");
    s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    let ti = m.project.timeline.iter().position(|n| n.id == body).unwrap();
    m.project.set_rollback(Some(ti + 1));
    native_assembly_rebuild(&mut m, false);
    let root = m.project.root;
    let clone = m.project.clone_part(knob, root).unwrap();
    native_assembly_rebuild(&mut m, false);
    m.project.set_rollback(None);
    native_assembly_rebuild(&mut m, true); // asserts no native errors
    let volume = |m: &NativeAssemblyModel, part: Id| {
        let b = m.project.active_body(part).unwrap();
        let _gate = qymcad_kernel::kernel_gate();
        m.shapes[&b].volume()
    };
    assert!(volume(&m, knob) < 24000.0 - 1.0, "the source is filleted");
    assert_close(volume(&m, clone), 24000.0, 1e-6, "the clone is the unfilleted box");
}
