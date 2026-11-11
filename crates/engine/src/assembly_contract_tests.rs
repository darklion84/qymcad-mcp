//! Additional package A review regressions: imported state and acceptance coverage.
use super::*;
use qymcad_core::feature::{AnchorRef, JointKind};
use std::collections::BTreeMap;

fn breps(s: &Session) -> BTreeMap<Id, Vec<u8>> {
    let _gate = qymcad_kernel::kernel_gate();
    s.shapes.iter().map(|(id, shape)| (*id, shape.to_brep_bytes().unwrap())).collect()
}

fn state(s: &Session) -> serde_json::Value {
    let mut v = serde_json::to_value(&s.p).unwrap();
    v.as_object_mut().unwrap().remove("next_id");
    v
}
fn joint_fixture() -> (Session, Id, Id, Id, Id, Id) {
    let mut s = Session::new_part();
    let a = s.p.active_component.unwrap();
    let ba = s.p.add_box(600., 800., 700.);
    let b = s.part_new("drawer").unwrap()["part"]["id"].as_u64().unwrap();
    let bb = s.p.add_box(500., 600., 150.);
    assert!(s.rebuild().errors.is_empty());
    s.part_place(&PartRef::Id(b), [0., 0., 250.], None).unwrap();
    s.ground(&PartRef::Id(a), true).unwrap();
    let j = s
        .joint_create(&JointCreate {
            name: Some("slide".into()),
            kind: AssemblyJointKind::Slider,
            a: JointAnchor { body: ba, face: Sel::Facing { dir: [1., 0., 0.], tol_deg: 1. } },
            b: JointAnchor { body: bb, face: Sel::Facing { dir: [-1., 0., 0.], tol_deg: 1. } },
            mode: JointMode::AsBuilt,
            axis: Some(BaseName::XZ),
            offset: Some(0.0.into()),
            limits: None,
        })
        .unwrap()["joint"]["id"]
        .as_u64()
        .unwrap();
    (s, a, b, ba, bb, j)
}

#[test]
fn a_contract_imported_bad_joint_formula_is_unhealthy_on_info_open_and_rebuild() {
    let (mut s, _, _, _, _, j) = joint_fixture();
    // Native saved records may contain an unresolved expression: native fallback is not acceptance.
    s.p.set_feat_dim(j, "offset", "missing_parameter".into());
    assert_eq!(s.assembly_info()["healthy"], false, "formula validation belongs to diagnostic acceptance too");
    assert_eq!(s.rebuild().assembly["healthy"], false);
    let path = std::env::temp_dir().join(format!("a-contract-formula-{}.qcad", std::process::id()));
    s.save(Some(&path)).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert_eq!(opened.info().assembly["healthy"], false);
    assert_eq!(report.assembly["healthy"], false);
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_contract_multibody_instance_is_preserved_and_reported_but_edit_refuses() {
    let mut s = Session::new_part();
    let source = s.p.active_component.unwrap();
    s.p.add_box(10., 20., 30.);
    s.p.add_box(7., 11., 13.); // Independent body volumes 6000 + 1001 = 7001.
    assert!(s.rebuild().errors.is_empty());
    let clone = s.p.clone_part(source, s.p.root).unwrap();
    assert!(s.rebuild().errors.is_empty());
    assert!(s.assembly_info()["parts"].as_array().unwrap().iter().any(|p| p["id"] == clone));
    let before = state(&s);
    let shapes = breps(&s);
    let error =
        s.part_place(&PartRef::Id(clone), [100., 0., 0.], None).expect_err("package A refuses editing an incomplete multi-body instance");
    assert_eq!(error.to_string(), "part instances are not supported for multi-body source parts in package A");
    assert_eq!(state(&s), before);
    assert!(breps(&s) == shapes, "exact live B-reps must be restored");
    // Positive control: an unrelated Part remains editable with this imported instance present.
    let other = s.part_new("independent").unwrap()["part"]["id"].as_u64().unwrap();
    s.part_place(&PartRef::Id(other), [0., 50., 0.], None).unwrap();
}

#[test]
fn a_contract_group_geometry_parameter_edit_is_touched_but_disconnected_edit_works() {
    let mut s = Session::new_part();
    let a = s.p.active_component.unwrap();
    s.param_set("width", &10.0.into()).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &"width".into(), &20.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch,
        profiles: None,
        profiles_at: None,
        height: 30.0.into(),
        down: None,
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: Some("plate".into()),
    })
    .unwrap();
    let b = s.part_new("group-peer").unwrap()["part"]["id"].as_u64().unwrap();
    s.p.add_box(7., 11., 13.);
    assert!(s.rebuild().errors.is_empty());
    s.p.set_grounded(a, true);
    let group = s.p.add_group(&[a, b]);
    assert_eq!(s.assembly_info()["healthy"], true);
    let before = state(&s);
    let shapes = breps(&s);
    let error =
        s.param_set("width", &20.0.into()).expect_err("a geometry edit touches the unsupported Group even without moving its origins");
    assert_eq!(
        error.to_string(),
        format!(
            "assembly edit touches unsupported constraints or relations [{group}]; package A supports root-level Rigid and Slider joints"
        )
    );
    assert_eq!(state(&s), before);
    assert!(breps(&s) == shapes, "exact live B-reps must be restored");
    s.param_set("unrelated", &4.0.into()).unwrap();
    assert_eq!(s.assembly_info()["healthy"], true);
}

#[test]
fn a_contract_formula_limit_crossing_parameter_edit_is_clamped_and_reported() {
    let (mut s, _, _, _, _, j) = joint_fixture();
    s.p.joints[0].limit_min[1] = Some(0.);
    s.p.joints[0].limit_max[1] = Some(300.);
    s.param_set("opening", &0.0.into()).unwrap();
    s.joint_set(&JointRef::Id(j), Some(&"opening".into()), None).unwrap();
    s.param_set("opening", &300.0.into()).unwrap();
    // ADR 0054 §2: the formula past the limit is clamped natively and reported, not refused.
    let report = s.param_set("opening", &301.0.into()).unwrap();
    assert_eq!(report.assembly["clamped"], serde_json::json!([{"joint":j,"slot":1,"requested":301.0,"applied":300.0}]));
    assert!((s.p.joints[0].offset - 300.0).abs() < 1e-5);
    s.param_set("unrelated", &4.0.into()).unwrap();
}

#[test]
fn a_contract_unsupported_joint_edit_refuses_but_disconnected_part_edit_works() {
    let (mut s, _, b, _, _, j) = joint_fixture();
    s.p.joints.iter_mut().find(|joint| joint.id == j).unwrap().kind = JointKind::Revolute;
    // Preserve and expose the unsupported imported record, including existing native slots/limits.
    assert_eq!(s.assembly_info()["joints"][0]["kind"], "revolute");
    let before = state(&s);
    let error = s.ground(&PartRef::Id(b), true).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("assembly edit touches unsupported constraints or relations [{j}]; package A supports root-level Rigid and Slider joints")
    );
    assert_eq!(state(&s), before);
    let other = s.part_new("independent").unwrap()["part"]["id"].as_u64().unwrap();
    s.part_place(&PartRef::Id(other), [0., 50., 0.], None).unwrap();
    // Existing records with origin anchors stay native; package A never flattens them.
    let connector = s.p.add_connector(other, AnchorRef::Origin);
    assert!(s.p.connector(connector).is_some());
}

#[test]
fn a_contract_named_joint_slot_formula_cycle_refuses_before_native_evaluation() {
    let (mut s, _, _, _, _, j) = joint_fixture();
    assert!(s.p.name_dim("travel".into(), qymcad_core::model::DimTarget::Feature { node: j, key: "offset".into() }));
    let before = state(&s);
    let shapes = breps(&s);
    let error = s.joint_set(&JointRef::Id(j), Some(&"travel+1".into()), None).unwrap_err();
    assert!(matches!(error, Error::Expr(_)), "a native cycle preflight must refuse the formula before solve: {error}");
    assert_eq!(state(&s), before);
    assert!(breps(&s) == shapes, "exact live B-reps must be restored");
}

#[test]
fn a_contract_failed_creation_inside_active_boundary_restores_rebuilt_live_shapes() {
    let (mut s, _, _, ba, _, _) = joint_fixture();
    let node = s.p.timeline.iter_mut().find(|node| node.id == ba).unwrap();
    match &mut node.kind {
        qymcad_core::feature::FeatureKind::Box3 { dx, .. } => *dx = 700.,
        _ => unreachable!(),
    }
    node.dirty = true;
    s.p.regen_faces.clear();
    // Missing native face caches make joint anchor resolution perform a rebuild. A later same-Part
    // refusal must restore the original live 600*800*700 B-rep even within an enclosing tool boundary.
    let snapshot = s.begin_tool_edit().unwrap();
    let before = state(&s);
    let shapes = breps(&s);
    let error = s
        .joint_create(&JointCreate {
            name: None,
            kind: AssemblyJointKind::Slider,
            a: JointAnchor { body: ba, face: Sel::Facing { dir: [1., 0., 0.], tol_deg: 1. } },
            b: JointAnchor { body: ba, face: Sel::Facing { dir: [-1., 0., 0.], tol_deg: 1. } },
            mode: JointMode::AsBuilt,
            axis: Some(BaseName::XZ),
            offset: None,
            limits: None,
        })
        .unwrap_err();
    let after = state(&s);
    let after_shapes = breps(&s);
    s.finish_tool_edit(snapshot, false);
    assert!(error.to_string().contains("requires anchors in two different parts"), "{error}");
    assert_eq!(after, before);
    assert!(after_shapes == shapes, "the failed engine call itself must restore shapes before its enclosing boundary is closed");
}

#[test]
fn a_contract_nested_native_mechanism_is_readable_but_affected_geometry_edit_refuses() {
    let mut s = Session::new_part();
    let independent = s.p.active_component.unwrap();
    s.p.set_active_component(Some(s.p.root));
    let assembly = s.p.add_assembly("subassembly");
    s.p.set_active_component(Some(assembly));
    let a = s.p.add_part("nested-a");
    s.p.set_active_component(Some(assembly));
    let b = s.p.add_part("nested-b");
    let ca = s.p.add_connector(a, AnchorRef::Origin);
    let cb = s.p.add_connector(b, AnchorRef::Origin);
    s.p.set_grounded(a, true);
    let joint = s.p.add_joint(ca, cb, JointKind::Rigid);
    assert!(s.p.set_joint_as_built(joint));
    assert_eq!(s.assembly_info()["healthy"], true);
    s.p.set_active_component(Some(a));
    let before = state(&s);
    let error = s
        .sketch_create(&PlaneRef::Base(BaseName::XY), Some("unsupported-edit"))
        .expect_err("editing geometry owned by an imported nested mechanism is outside package A");
    assert!(error.to_string().contains("nested"), "{error}");
    assert_eq!(state(&s), before);
    // Positive control: the disconnected original root-child Part can still be modelled.
    s.p.set_active_component(Some(independent));
    s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("independent-edit")).unwrap();
}

#[test]
fn a_contract_imported_derived_components_are_preserved_but_affected_edits_refuse() {
    for kind in ["mirror", "pattern", "piece"] {
        let mut s = Session::new_part();
        let source = s.p.active_component.unwrap();
        let first = s.p.add_box(10., 20., 30.); // V=6000; Piece requires another current body.
        if kind == "piece" {
            s.p.add_box(7., 11., 13.); // V=1001; the source Part must not become empty.
        }
        assert!(s.rebuild().errors.is_empty());
        let derived = match kind {
            "mirror" => {
                let body = s.p.add_mirror_part(source, [50., 0., 0.], [1., 0., 0.]);
                s.p.body_owner(body).unwrap()
            }
            "pattern" => {
                s.p.set_active_component(Some(s.p.root));
                let node = s.p.add_comp_pattern(source, qymcad_core::model::CompPatternKind::linear([1., 0., 0.], 100., 2));
                let qymcad_core::feature::FeatureKind::ComponentPattern { copies, .. } =
                    &s.p.timeline.iter().find(|n| n.id == node).unwrap().kind
                else {
                    unreachable!()
                };
                copies[0]
            }
            "piece" => s.p.piece_to_part(first, "promoted-piece".into()).unwrap().0,
            _ => unreachable!(),
        };
        assert!(s.rebuild().errors.is_empty());
        assert_eq!(s.assembly_info()["healthy"], true);
        for part in [source, derived] {
            let before = state(&s);
            let shapes = breps(&s);
            let error =
                s.ground(&PartRef::Id(part), true).expect_err("an imported associative component cannot be edited through package A");
            assert!(error.to_string().contains("not supported in package A"), "{kind}: {error}");
            assert_eq!(state(&s), before);
            assert!(breps(&s) == shapes, "{kind}: restore original geometry exactly");
        }
        // A disconnected root-child Part still supports ordinary modelling and placement.
        let other = s.part_new("independent").unwrap()["part"]["id"].as_u64().unwrap();
        s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("independent-edit")).unwrap();
        s.part_place(&PartRef::Id(other), [0., 50., 0.], None).unwrap();
    }
}

#[test]
fn a_contract_joint_noop_reports_requested_and_evaluated_offset() {
    let (mut s, _, _, _, _, j) = joint_fixture();
    let r = s.joint_set(&JointRef::Id(j), Some(&0.0.into()), None).unwrap();
    assert_eq!(r["changed"], false);
    assert_eq!(r["requested"], 0.0);
    assert_eq!(r["evaluated"], 0.0);
    assert_eq!(r["previous"], r["read"]);
}

#[test]
fn a_contract_imported_ball_reports_angular_slot_units() {
    let (mut s, _, _, _, _, j) = joint_fixture();
    s.p.joints.iter_mut().find(|joint| joint.id == j).unwrap().kind = JointKind::Ball;
    let info = s.assembly_info();
    for slot in info["joints"][0]["slots"].as_array().unwrap() {
        assert_eq!(slot["unit"], "deg", "Ball offset/offset2 are X/Y rotations, not distances");
    }
}
