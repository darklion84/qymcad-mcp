//! Package A acceptance. Native boxes are centered: hand volumes and world bounds below.
use super::*;
use crate::assembly::*;
use qymcad_core::feature::{AnchorRef, JointKind};
use std::collections::BTreeMap;

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-5, "{a} != {b}");
}
fn pose(s: &Session, p: Id) -> [f64; 3] {
    let t = s.p.world_transform(p);
    [t[3], t[7], t[11]]
}
fn breps(s: &Session) -> BTreeMap<Id, Vec<u8>> {
    let _gate = qymcad_kernel::kernel_gate();
    s.shapes.iter().map(|(id, sh)| (*id, sh.to_brep_bytes().unwrap())).collect()
}
fn state(s: &Session) -> serde_json::Value {
    let mut v = serde_json::to_value(&s.p).unwrap();
    v.as_object_mut().unwrap().remove("next_id");
    v
}
fn fixture() -> (Session, Id, Id, Id, Id) {
    let mut s = Session::new_part();
    let a = s.p.active_component.unwrap();
    let ba = s.p.add_box(600., 800., 700.); // 600*800*700 = 336000000 mm3.
    let b = s.part_new("drawer").unwrap()["part"]["id"].as_u64().unwrap();
    let bb = s.p.add_box(500., 600., 150.); // 500*600*150 = 45000000 mm3.
    assert!(s.rebuild().errors.is_empty());
    s.part_place(&PartRef::Id(b), [0., 0., 250.], None).unwrap();
    s.ground(&PartRef::Id(a), true).unwrap();
    (s, a, b, ba, bb)
}
fn create(s: &mut Session, ba: Id, bb: Id, kind: AssemblyJointKind) -> Id {
    let r = s
        .joint_create(&JointCreate {
            name: Some("rail".into()),
            kind,
            a: JointAnchor { body: ba, face: Sel::Facing { dir: [1., 0., 0.], tol_deg: 1. } },
            b: JointAnchor { body: bb, face: Sel::Facing { dir: [-1., 0., 0.], tol_deg: 1. } },
            mode: JointMode::AsBuilt,
            axis: if kind == AssemblyJointKind::Slider { Some(BaseName::XZ) } else { None },
            offset: None,
        })
        .unwrap();
    r["joint"]["id"].as_u64().unwrap()
}
#[test]
fn a_slider_absolute_zero_three_hundred_zero_stable_local_breps() {
    let (mut s, _, b, ba, bb) = fixture();
    let original = breps(&s);
    let ids = s.current_body_ids();
    let j = create(&mut s, ba, bb, AssemblyJointKind::Slider);
    for value in [0., 300., 0.] {
        s.joint_set(&JointRef::Id(j), Some(&value.into()), None).unwrap();
        for _ in 0..5 {
            s.assembly_accept(None).unwrap();
        }
        for (v, w) in pose(&s, b).into_iter().zip([0., -value, 250.]) {
            close(v, w);
        }
        let bounds = s.result_bodies().into_iter().find(|v| v.id == bb).unwrap().bbox_world;
        for (v, w) in bounds.into_iter().zip([-250., -300. - value, 250., 250., 300. - value, 400.]) {
            close(v, w);
        }
        assert_eq!(breps(&s), original);
        assert_eq!(s.current_body_ids(), ids);
    }
}
#[test]
fn a_rigid_holds_exact_as_built_without_move_feature() {
    let (mut s, _, b, ba, bb) = fixture();
    let before = pose(&s, b);
    let n = s.p.timeline.len();
    let j = create(&mut s, ba, bb, AssemblyJointKind::Rigid);
    assert_eq!(s.p.joints[0].kind, JointKind::Rigid);
    assert_eq!(s.p.timeline.len(), n);
    s.p.move_component(b, [100., 200., 300.]);
    s.assembly_accept(None).unwrap();
    for (v, w) in pose(&s, b).into_iter().zip(before) {
        close(v, w);
    }
    assert!(s.joint_set(&JointRef::Id(j), Some(&1.0.into()), None).unwrap_err().to_string().contains("has no editable offset slot"));
}
#[test]
fn a_formula_numeric_clear_release_and_native_drag_persist() {
    let (mut s, _, b, ba, bb) = fixture();
    let j = create(&mut s, ba, bb, AssemblyJointKind::Slider);
    let shapes = breps(&s);
    s.param_set("opening", &0.0.into()).unwrap();
    s.joint_set(&JointRef::Id(j), Some(&"opening".into()), None).unwrap();
    s.param_set("opening", &300.0.into()).unwrap();
    close(pose(&s, b)[1], -300.);
    assert_eq!(shapes, breps(&s));
    for expr in ["absent", "NaN", "1/0"] {
        let before = state(&s);
        let before_brep = breps(&s);
        assert!(s.joint_set(&JointRef::Id(j), Some(&expr.into()), None).is_err());
        assert_eq!(state(&s), before);
        assert_eq!(breps(&s), before_brep);
    }
    let before = state(&s);
    assert!(s.param_set("opening", &"opening+1".into()).is_err());
    assert_eq!(state(&s), before);
    s.joint_set(&JointRef::Id(j), None, Some("offset")).unwrap();
    assert_eq!(s.p.joints[0].drive[1], None);
    assert!(s.p.feat_dim(j, "offset").is_none_or(|v| v.is_empty()));
    close(pose(&s, b)[1], -300.);
    s.p.drag_pull = Some((b, [0., 0., 0.], [0., -120., 250.]));
    s.assembly_accept(None).unwrap();
    s.p.drag_pull = None;
    assert!(s.rebuild().errors.is_empty());
    close(pose(&s, b)[1], -120.);
    let path = std::env::temp_dir().join(format!("qymcad-a-release-{}.qcad", std::process::id()));
    s.part_select(&PartRef::Root { root: true }).unwrap();
    s.save(Some(&path)).unwrap();
    let (s, _) = Session::open(&path).unwrap();
    close(pose(&s, b)[1], -120.);
    let _ = std::fs::remove_file(path);
}
#[test]
fn a_numeric_drag_updates_drive_and_survives_rebuild() {
    let (mut s, _, b, ba, bb) = fixture();
    let j = create(&mut s, ba, bb, AssemblyJointKind::Slider);
    s.joint_set(&JointRef::Id(j), Some(&300.0.into()), None).unwrap();
    s.p.drag_pull = Some((b, [0., 0., 0.], [0., -120., 250.]));
    s.assembly_accept(None).unwrap();
    s.p.drag_pull = None;
    close(s.p.joints[0].drive[1].unwrap(), 120.);
    assert!(s.rebuild().errors.is_empty());
    close(pose(&s, b)[1], -120.);
}
#[test]
fn a_fault_is_unhealthy_despite_no_feature_errors() {
    let (mut s, a, _, _, _) = fixture();
    let c = s.p.add_connector(a, AnchorRef::Origin);
    let j = s.p.add_joint(c, u64::MAX, JointKind::Slider);
    assert!(s.p.regen_errors.is_empty());
    assert!(!s.assembly_info()["healthy"].as_bool().unwrap());
    assert!(s.assembly_accept(None).unwrap_err().to_string().contains(&format!("faulty joints [{j}]")));
    let path = std::env::temp_dir().join(format!("qymcad-a-fault-{}.qcad", std::process::id()));
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(!opened.info().assembly["healthy"].as_bool().unwrap());
    assert!(!r.assembly["healthy"].as_bool().unwrap());
    assert!(!opened.rebuild().assembly["healthy"].as_bool().unwrap());
    let _ = std::fs::remove_file(path);
}
#[test]
fn a_anchor_lifecycle_refuses_and_unrelated_edits_succeed() {
    for action in ["delete", "suppress", "rollback"] {
        let (mut s, _, _, ba, bb) = fixture();
        let j = create(&mut s, ba, bb, AssemblyJointKind::Slider);
        // Positive control next to the destructive edit: independent geometry remains supported.
        let other = s.part_new("independent").unwrap()["part"]["id"].as_u64().unwrap();
        s.p.add_box(7., 11., 13.);
        assert!(s.rebuild().errors.is_empty());
        s.part_place(&PartRef::Id(other), [70., 0., 0.], None).unwrap();
        let before = state(&s);
        let shapes = breps(&s);
        let error = match action {
            "delete" => s.feature_delete(ba, false).unwrap_err(),
            "suppress" => s.feature_suppress(ba, true).unwrap_err(),
            _ => s.history_rollback(None, Some("start")).unwrap_err(),
        };
        assert_eq!(
            error.to_string(),
            format!("edit would invalidate anchors of joints [{j}]; delete those joints first; nothing was changed")
        );
        assert_eq!(state(&s), before);
        assert_eq!(breps(&s), shapes);
        s.param_set("unrelated", &2.0.into()).unwrap();
        assert!(s.assembly_info()["healthy"].as_bool().unwrap());
    }
}
#[test]
fn a_violated_joint_refuses_atomic_ground_change_and_disconnected_control() {
    let (mut s, a, b, ba, bb) = fixture();
    let j = create(&mut s, ba, bb, AssemblyJointKind::Slider);
    s.param_set("pa", &0.0.into()).unwrap();
    s.joint_set(&JointRef::Id(j), Some(&"pa".into()), None).unwrap();
    // Review F1: grounding the slider's moving part, or ungrounding its anchor, is refused like joint_create.
    let before = state(&s);
    let e = s.ground(&PartRef::Id(b), true).unwrap_err().to_string();
    assert!(e.contains(&format!("slider joint {j} moves part {b}, which must stay ungrounded")), "{e}");
    assert_eq!(state(&s), before);
    let e = s.ground(&PartRef::Id(a), false).unwrap_err().to_string();
    assert!(e.contains(&format!("joint {j} needs its anchor part {a} grounded")), "{e}");
    assert_eq!(state(&s), before);
    // Chained slider: the remedy must not be "ground the moving part".
    let (_, tray) = {
        let t = s.part_new("tray").unwrap()["part"]["id"].as_u64().unwrap();
        let tb = s.p.add_box(100., 100., 20.);
        assert!(s.rebuild().errors.is_empty());
        (t, tb)
    };
    let e = s
        .joint_create(&JointCreate {
            name: Some("tray_rail".into()),
            kind: AssemblyJointKind::Slider,
            a: JointAnchor { body: bb, face: Sel::Facing { dir: [0., 0., 1.], tol_deg: 1. } },
            b: JointAnchor { body: tray, face: Sel::Facing { dir: [0., 0., -1.], tol_deg: 1. } },
            mode: JointMode::AsBuilt,
            axis: Some(BaseName::XZ),
            offset: None,
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains(&format!("part {b} is moved by slider joint {j}")) && !e.contains("ground part"), "{e}");
    // Review F2: a user Rigid that conflicts with the slider is reported as a conflict, not as lost anchors.
    s.joint_create(&JointCreate {
        name: Some("lock".into()),
        kind: AssemblyJointKind::Rigid,
        a: JointAnchor { body: ba, face: Sel::Facing { dir: [1., 0., 0.], tol_deg: 1. } },
        b: JointAnchor { body: bb, face: Sel::Facing { dir: [-1., 0., 0.], tol_deg: 1. } },
        mode: JointMode::AsBuilt,
        axis: None,
        offset: None,
    })
    .unwrap();
    let before = state(&s);
    let shapes = breps(&s);
    let e = s.param_set("pa", &300.0.into()).unwrap_err().to_string();
    assert!(e.contains("unsatisfied") && !e.contains("invalidate anchors"), "{e}");
    let e = s.joint_set(&JointRef::Id(j), Some(&300.0.into()), None).unwrap_err().to_string();
    assert!(!e.contains("[]"), "a refusal must name a present cause: {e}");
    assert_eq!(state(&s), before);
    assert_eq!(breps(&s), shapes);
    s.part_new("unrelated").unwrap();
    s.param_set("unrelated", &4.0.into()).unwrap();
}
#[test]
fn a_huge_travel_refusal_names_its_cause() {
    let (mut s, _, _, ba, bb) = fixture();
    let j = create(&mut s, ba, bb, AssemblyJointKind::Slider);
    let e = s.joint_set(&JointRef::Id(j), Some(&1e308.into()), None).unwrap_err().to_string();
    assert!(!e.contains("[]") && !e.contains("conflict=false"), "review F2: {e}");
}
#[test]
fn a_hole_at_is_world_on_a_placed_part() {
    let (mut s, _, b, _, bb) = fixture();
    s.part_place(&PartRef::Id(b), [400., 0., 250.], None).unwrap();
    let top = s.topology(Some(bb), false).unwrap().faces.into_iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    let (h, _) = s
        .hole(&crate::modifiers::Hole {
            body: Some(bb),
            face: Sel::Ids(vec![top]),
            at: Some([630., 0., 0.]), // World x 630 = Part-local x 230 (placed at x 400); box spans local x ±250.
            offset: None,
            diameter: 20.0.into(),
            depth: Some(10.0.into()),
            kind: crate::modifiers::HoleKind::Plain,
            dia2: None,
            depth2: None,
            name: None,
        })
        .unwrap();
    let cyl = s.topology(Some(h), false).unwrap().faces.into_iter().find(|f| f.kind == crate::FaceKind::Cylinder).expect("hole wall");
    close(cyl.centroid[0], 230.);
    close(cyl.centroid[1], 0.);
}
#[test]
fn a_parts_history_noop_and_native_connector_cleanup() {
    let mut s = Session::new_part();
    let original = state(&s);
    let original_active = s.p.active_component;
    let b = s.part_new("drawer").unwrap()["part"]["id"].as_u64().unwrap();
    s.undo().unwrap();
    assert_eq!(state(&s), original);
    let b2 = s.part_new("drawer").unwrap()["part"]["id"].as_u64().unwrap();
    assert!(b2 > b);
    s.part_select(&PartRef::Id(b2)).unwrap();
    s.undo().unwrap();
    assert_eq!(s.p.active_component, original_active);
    let (mut s, _, _, ba, bb) = fixture();
    let j = create(&mut s, ba, bb, AssemblyJointKind::Slider);
    let shapes = breps(&s);
    s.joint_delete(&JointRef::Id(j)).unwrap();
    assert!(s.p.joints.is_empty());
    assert!(s.p.connectors.is_empty());
    assert_eq!(breps(&s), shapes);
    s.undo().unwrap();
    assert_eq!(s.p.joints.len(), 1);
    assert_eq!(s.p.connectors.len(), 2);
}
#[test]
fn a_existing_limits_preserved_and_requests_checked() {
    let (mut s, _, _, ba, bb) = fixture();
    let j = create(&mut s, ba, bb, AssemblyJointKind::Slider);
    s.p.joints[0].limit_min[1] = Some(0.);
    s.p.joints[0].limit_max[1] = Some(300.);
    for v in [-1., 301.] {
        let before = state(&s);
        assert_eq!(
            s.joint_set(&JointRef::Id(j), Some(&v.into()), None).unwrap_err().to_string(),
            format!("joint {j} offset {v} mm is outside [0, 300] mm; nothing was changed")
        );
        assert_eq!(state(&s), before);
    }
    for v in [300., 0.] {
        s.joint_set(&JointRef::Id(j), Some(&v.into()), None).unwrap();
    }
    assert_eq!(s.p.joints[0].limit_max[1], Some(300.));
}
