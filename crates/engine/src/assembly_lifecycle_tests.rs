//! Real native face loss and retained-source controls for assembly acceptance.
use crate::*;
use qymcad_core::feature::RelationKind;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn exact(s: &Session) -> (Value, BTreeMap<Id, Vec<u8>>) {
    let mut project = serde_json::to_value(&s.p).unwrap();
    project.as_object_mut().unwrap().remove("next_id");
    let diagnostics = json!({
        "project":project, "faces":s.p.regen_faces, "edges":s.p.regen_edges,
        "errors":s.p.regen_errors, "warnings":s.p.regen_warnings,
        "violated":s.p.mates_violated, "conflict":s.p.mates_conflict,
        "advisories":s.advisory_warnings.iter().map(|w|w.issue()).collect::<Vec<_>>(),
        "assembly":s.assembly_info(), "history_len":s.undo.len(),
    });
    let shapes = {
        // Serialization is a direct shape operation; release before any self-gating call.
        let _gate = qymcad_kernel::kernel_gate();
        s.shapes.iter().map(|(id, shape)| (*id, shape.to_brep_bytes().unwrap())).collect()
    };
    (diagnostics, shapes)
}

fn face(s: &mut Session, body: Id, normal: [f64; 3], z: Option<f64>) -> u32 {
    s.topology(Some(body), false)
        .unwrap()
        .faces
        .iter()
        .find(|f| {
            f.kind == FaceKind::Plane
                && f.normal.is_some_and(|n| n.into_iter().zip(normal).all(|(a, b)| (a - b).abs() < 1e-5))
                && z.is_none_or(|z| (f.centroid[2] - z).abs() < 1e-5)
        })
        .expect("fixture's exact planar face")
        .id
}

fn part(s: &mut Session, name: &str, size: [f64; 3]) -> (Id, Id) {
    let id = s.part_new(name).unwrap()["part"]["id"].as_u64().unwrap();
    let body = s.p.add_box(size[0], size[1], size[2]);
    assert!(s.rebuild().errors.is_empty());
    (id, body)
}

fn attach(s: &mut Session, owner: Id, a: Id, af: u32, b: Id, bf: u32) -> Id {
    s.ground(&PartRef::Id(owner), true).unwrap();
    s.joint_create(&JointCreate {
        name: Some("lifecycle rail".into()),
        kind: AssemblyJointKind::Slider,
        a: JointAnchor::face(a, Sel::Ids(vec![af])),
        b: JointAnchor::face(b, Sel::Ids(vec![bf])),
        mode: JointMode::AsBuilt,
        axis: Some(BaseName::XZ),
        offset: Some(0.0.into()),
        angle: None,
        limits: None,
    })
    .unwrap()["joint"]["id"]
        .as_u64()
        .unwrap()
}

fn hole(s: &mut Session, body: Id) -> Id {
    let f = face(s, body, [0., 0., 1.], Some(20.));
    s.hole(&Hole {
        body: Some(body),
        face: Sel::Ids(vec![f]),
        at: Some([0., 0., 20.]),
        offset: None,
        diameter: 4.0.into(),
        depth: Some("bore_depth".into()),
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: Some("blind bore".into()),
    })
    .unwrap()
    .0
}

fn blind_fixture() -> (Session, Id, Id, Id, Id) {
    let mut s = Session::new_part();
    let a = s.p.active_component.unwrap();
    s.param_set("bore_depth", &5.0.into()).unwrap();
    let stock = s.p.add_box(20., 30., 20.); // 20×30×20 = 12000 mm³.
    assert!(s.rebuild().errors.is_empty());
    let drilled = hole(&mut s, stock);
    let af = face(&mut s, drilled, [0., 0., 1.], Some(15.));
    let (_, drawer) = part(&mut s, "drawer", [10., 15., 4.]);
    let bf = face(&mut s, drawer, [0., 0., 1.], Some(4.));
    let joint = attach(&mut s, a, drilled, af, drawer, bf);
    (s, a, stock, drilled, joint)
}

fn invalid_anchor(joint: Id) -> String {
    format!("edit would invalidate anchors of joints [{joint}]; delete those joints first; nothing was changed")
}

#[test]
fn a_lifecycle_parameter_lost_blind_bottom_refuses_and_blind_edit_succeeds() {
    let (mut s, _, _, drilled, joint) = blind_fixture();
    // Positive control: a 7mm blind bore retains its native bottom face.
    s.param_set("bore_depth", &7.0.into()).unwrap();
    assert!(s.assembly_info()["healthy"].as_bool().unwrap());
    let v = s.result_bodies().into_iter().find(|b| b.id == drilled).unwrap().volume;
    assert!((v - (12000.0 - 28.0 * std::f64::consts::PI)).abs() < 1e-5); // π×2²×7 removed.
    let before = exact(&s);
    // Depth30 passes through the 20mm plate: the real bottom disk ceases to exist.
    assert_eq!(s.param_set("bore_depth", &30.0.into()).unwrap_err().to_string(), invalid_anchor(joint));
    assert_eq!(exact(&s), before);
    s.param_set("unrelated", &3.0.into()).unwrap();
    assert!(s.assembly_info()["healthy"].as_bool().unwrap());
}

#[test]
fn a_lifecycle_dimension_lost_blind_bottom_refuses_and_blind_edit_succeeds() {
    let (mut s, _, _, drilled, joint) = blind_fixture();
    let target = DimensionTarget::Feature { feature: drilled, key: "depth".into() };
    s.dimension_set(&target, Some(&7.0.into()), None).unwrap();
    assert!(s.assembly_info()["healthy"].as_bool().unwrap());
    let before = exact(&s);
    assert_eq!(s.dimension_set(&target, Some(&30.0.into()), None).unwrap_err().to_string(), invalid_anchor(joint));
    assert_eq!(exact(&s), before);
    s.dimension_set(&target, Some(&6.0.into()), None).unwrap();
    assert!(s.assembly_info()["healthy"].as_bool().unwrap());
}

#[test]
fn a_lifecycle_shell_repick_lost_bottom_refuses_and_retained_face_succeeds() {
    let mut s = Session::new_part();
    let a = s.p.active_component.unwrap();
    let stock = s.p.add_box(20., 30., 20.);
    assert!(s.rebuild().errors.is_empty());
    let top = face(&mut s, stock, [0., 0., 1.], Some(20.));
    let shell = s.shell(Some(stock), &Sel::Ids(vec![top]), &1.0.into(), Side::Inward, None).unwrap().0;
    // The inner floor vanishes when the bottom is opened. The outer bottom's
    // native face survives as the wall rim, so it is a valid retained witness.
    let bottom = face(&mut s, shell, [0., 0., 1.], Some(1.));
    let (_, drawer) = part(&mut s, "drawer", [10., 15., 4.]);
    let bf = face(&mut s, drawer, [0., 0., 1.], Some(4.));
    let joint = attach(&mut s, a, shell, bottom, drawer, bf);
    let side = face(&mut s, stock, [1., 0., 0.], None);
    s.repick_shell(shell, &Sel::Ids(vec![side])).unwrap();
    assert!(s.assembly_info()["healthy"].as_bool().unwrap());
    let bottom_stock = face(&mut s, stock, [0., 0., -1.], Some(0.));
    let before = exact(&s);
    assert_eq!(s.repick_shell(shell, &Sel::Ids(vec![bottom_stock])).unwrap_err().to_string(), invalid_anchor(joint));
    assert_eq!(exact(&s), before);
    s.param_set("unrelated", &3.0.into()).unwrap();
    assert!(s.assembly_info()["healthy"].as_bool().unwrap());
}

#[test]
fn a_lifecycle_shell_opening_retains_real_outer_rim_anchor() {
    let mut s = Session::new_part();
    let a = s.p.active_component.unwrap();
    let stock = s.p.add_box(20., 30., 20.);
    assert!(s.rebuild().errors.is_empty());
    let top = face(&mut s, stock, [0., 0., 1.], Some(20.));
    let shell = s.shell(Some(stock), &Sel::Ids(vec![top]), &1.0.into(), Side::Inward, None).unwrap().0;
    let outer = face(&mut s, shell, [0., 0., -1.], Some(0.));
    let (_, drawer) = part(&mut s, "drawer", [10., 15., 4.]);
    let bf = face(&mut s, drawer, [0., 0., -1.], Some(0.));
    attach(&mut s, a, shell, outer, drawer, bf);
    let bottom_stock = face(&mut s, stock, [0., 0., -1.], Some(0.));
    s.repick_shell(shell, &Sel::Ids(vec![bottom_stock])).unwrap();
    let topo = s.topology(Some(shell), false).unwrap();
    let witness = topo.faces.iter().find(|f| f.id == outer).unwrap();
    // Outer 20×30 rectangle minus inner 18×28 opening leaves a real 96mm² rim.
    assert!((witness.area - 96.0).abs() < 1e-5);
    assert!(s.assembly_info()["healthy"].as_bool().unwrap());
}

#[test]
fn a_lifecycle_reorder_lost_array_face_refuses_and_consumed_anchor_succeeds() {
    for on_array in [true, false] {
        let mut s = Session::new_part();
        let a = s.p.active_component.unwrap();
        s.param_set("bore_depth", &5.0.into()).unwrap();
        let stock = s.p.add_box(20., 30., 20.);
        assert!(s.rebuild().errors.is_empty());
        let (_, drawer) = part(&mut s, "drawer", [10., 15., 4.]);
        let bf = face(&mut s, drawer, [0., 0., 1.], Some(4.));
        let mut joint = None;
        if !on_array {
            let sf = face(&mut s, stock, [0., 0., 1.], Some(20.));
            joint = Some(attach(&mut s, a, stock, sf, drawer, bf));
        }
        let drilled = hole(&mut s, stock);
        let arr = s
            .linear_array(Some(drilled), &ArrayDir { dx: 40.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: 2.0.into() }, None, None)
            .unwrap()
            .0;
        if on_array {
            let af = face(&mut s, arr, [0., 0., 1.], Some(15.));
            joint = Some(attach(&mut s, a, arr, af, drawer, bf));
        }
        assert!(s.assembly_info()["healthy"].as_bool().unwrap());
        let before = exact(&s);
        let result = s.feature_reorder(drilled, "down");
        if on_array {
            assert_eq!(result.unwrap_err().to_string(), invalid_anchor(joint.unwrap()));
            assert_eq!(exact(&s), before);
            s.param_set("unrelated", &3.0.into()).unwrap();
        } else {
            assert!(result.unwrap().changed);
            // The original stock is consumed, but still has a live native face witness.
            assert!(s.p.consumed_bodies().contains(&stock));
            assert!(s.assembly_info()["healthy"].as_bool().unwrap());
        }
    }
}

#[test]
fn a_lifecycle_disconnected_group_is_preserved_and_affected_edit_refuses() {
    let (mut s, _, _, drilled, _) = blind_fixture();
    let (c, cb) = part(&mut s, "group c", [7., 11., 13.]);
    let (d, _) = part(&mut s, "group d", [9., 12., 14.]);
    let group = s.p.add_group(&[c, d]);
    s.assembly_accept(None).unwrap();
    let records = serde_json::to_value(&s.p.mate_constraints).unwrap();
    // A healthy disconnected unsupported mechanism must not block unrelated modelling.
    s.dimension_set(&DimensionTarget::Feature { feature: drilled, key: "depth".into() }, Some(&7.0.into()), None).unwrap();
    assert_eq!(serde_json::to_value(&s.p.mate_constraints).unwrap(), records);
    assert_eq!(s.assembly_info()["constraints"], records);
    let before = exact(&s);
    let err =
        s.linear_array(Some(cb), &ArrayDir { dx: 20.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: 2.0.into() }, None, None).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "assembly edit touches unsupported constraints or relations [{group}]; package A supports root-level Rigid, Slider and Revolute joints"
        )
    );
    assert_eq!(exact(&s), before);
    s.param_set("unrelated", &4.0.into()).unwrap();
    assert!(s.assembly_info()["healthy"].as_bool().unwrap());
}

#[test]
fn a_lifecycle_disconnected_relation_is_preserved_and_affected_edit_refuses() {
    let (mut s, _, _, _, first) = blind_fixture();
    let (c, cb) = part(&mut s, "second carcass", [20., 30., 20.]);
    let (_, db) = part(&mut s, "second drawer", [10., 15., 4.]);
    let cf = face(&mut s, cb, [0., 0., 1.], Some(20.));
    let df = face(&mut s, db, [0., 0., 1.], Some(4.));
    let second = attach(&mut s, c, cb, cf, db, df);
    let relation = s.p.add_relation(RelationKind::Linear, first, 1, second, 1, 1.0);
    s.assembly_accept(None).unwrap();
    let records = serde_json::to_value(&s.p.relations).unwrap();
    // Independent geometry is the positive control; both relation mechanisms remain untouched.
    let (_, free) = part(&mut s, "independent", [7., 11., 13.]);
    s.linear_array(Some(free), &ArrayDir { dx: 20.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: 2.0.into() }, None, None).unwrap();
    assert_eq!(serde_json::to_value(&s.p.relations).unwrap(), records);
    assert_eq!(s.assembly_info()["relations"], records);
    let before = exact(&s);
    assert_eq!(s.joint_set(&JointRef::Id(first),Some(&10.0.into()),None).unwrap_err().to_string(),
        format!("assembly edit touches unsupported constraints or relations [{relation}]; package A supports root-level Rigid, Slider and Revolute joints"));
    assert_eq!(exact(&s), before);
    s.param_set("unrelated", &4.0.into()).unwrap();
    assert!(s.assembly_info()["healthy"].as_bool().unwrap());
}

#[test]
fn a_lifecycle_face_ids_resolve_in_the_named_body_only() {
    let mut s = Session::new_part();
    let a = s.p.active_component.unwrap();
    let stock = s.p.add_box(20., 30., 20.);
    assert!(s.rebuild().errors.is_empty());
    let af = face(&mut s, stock, [0., 0., 1.], Some(20.));
    let (_, drawer) = part(&mut s, "drawer", [10., 15., 4.]);
    let bf = face(&mut s, drawer, [0., 0., 1.], Some(4.));
    assert_ne!(af, bf, "fixture needs distinct persistent names");
    s.ground(&PartRef::Id(a), true).unwrap();
    let mut request = JointCreate {
        name: None,
        kind: AssemblyJointKind::Slider,
        a: JointAnchor::face(stock, Sel::Ids(vec![af])),
        b: JointAnchor::face(drawer, Sel::Ids(vec![af])),
        mode: JointMode::AsBuilt,
        axis: Some(BaseName::XZ),
        offset: None,
        angle: None,
        limits: None,
    };
    let before = exact(&s);
    assert!(s.joint_create(&request).is_err());
    assert_eq!(exact(&s), before);
    request.b.pick = AnchorPick::Face(Sel::Ids(vec![bf]));
    s.joint_create(&request).unwrap();
    assert!(s.assembly_info()["healthy"].as_bool().unwrap());
}
