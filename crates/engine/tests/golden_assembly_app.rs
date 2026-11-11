//! Package A's native saved-file path and the three-Part cabinet acceptance.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;
use std::collections::BTreeMap;

fn plate(s: &mut Session, name: &str, bounds: [f64; 6]) -> Id {
    let [x0, y0, z0, x1, y1, z1] = bounds;
    let plane = if z0 == 0. {
        PlaneRef::Base(BaseName::XY)
    } else {
        let (id, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &z0.into(), Some(&format!("{name}-plane"))).unwrap();
        PlaneRef::Plane(id)
    };
    let sk = s.sketch_create(&plane, Some(&format!("{name}-sk"))).unwrap();
    s.sketch_rect(sk, &((x0 + x1) / 2.).into(), &((y0 + y1) / 2.).into(), &(x1 - x0).into(), &(y1 - y0).into(), false).unwrap();
    let ex: Extrude = serde_json::from_value(json!({"sketch":sk,"height":z1-z0,"op":"new_body","name":name})).unwrap();
    let (id, r) = s.extrude(&ex).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    id
}

fn rail(s: &mut Session, name: &str, a: Id, b: Id) -> Id {
    s.joint_create(&JointCreate {
        name: Some(name.into()),
        kind: AssemblyJointKind::Slider,
        a: JointAnchor { body: a, face: Sel::Facing { dir: [1., 0., 0.], tol_deg: 1. } },
        b: JointAnchor { body: b, face: Sel::Facing { dir: [-1., 0., 0.], tol_deg: 1. } },
        mode: JointMode::AsBuilt,
        axis: Some(BaseName::XZ),
        offset: Some(0.0.into()),
        limits: None,
    })
    .unwrap()["joint"]["id"]
        .as_u64()
        .unwrap()
}

fn local_breps(s: &Session) -> BTreeMap<Id, Vec<u8>> {
    let ids: Vec<_> = s.info().bodies.into_iter().map(|b| b.id).collect();
    let _gate = qymcad_kernel::kernel_gate();
    ids.into_iter().map(|id| (id, s.shape(id).unwrap().to_brep_bytes().unwrap())).collect()
}

fn two_boxes() -> (Session, Id, Id, Id) {
    let mut s = Session::new_part();
    let a = s.project().active_component.unwrap();
    s.rename(RenameTarget::Part(a), "carcass").unwrap();
    let ba = plate(&mut s, "carcass-box", [-300., -400., 0., 300., 400., 700.]);
    s.ground(&PartRef::Id(a), true).unwrap();
    let b = s.part_new("drawer").unwrap()["part"]["id"].as_u64().unwrap();
    let bb = plate(&mut s, "drawer-box", [-250., -300., 0., 250., 300., 150.]);
    s.part_place(&PartRef::Id(b), [0., 0., 250.], None).unwrap();
    let joint = rail(&mut s, "drawer-slide", ba, bb);
    (s, a, b, joint)
}

fn native_pose(m: &NativeAssemblyModel, part: Id, opening: f64) {
    let t = m.project.world_transform(part);
    for (v, w) in [t[3], t[7], t[11]].into_iter().zip([0., -opening, 250.]) {
        assert_close(v, w, 1e-5, "absolute native drawer translation");
    }
    assert_eq!(m.project.joints.len(), 1);
    assert_eq!(m.project.joints[0].kind, qymcad_core::feature::JointKind::Slider);
    assert_eq!(m.project.connectors.len(), 2);
    assert_eq!(m.project.joints[0].a, m.project.connectors[0].id);
    assert_eq!(m.project.joints[0].b, m.project.connectors[1].id);
    for c in &m.project.connectors {
        assert!(matches!(c.anchor, qymcad_core::feature::AnchorRef::FaceCenter(..)));
        assert_eq!(c.axis_ref, Some(qymcad_core::feature::AnchorRef::BasePlane(qymcad_core::feature::BasePlane::XZ)));
    }
    native_assembly_health(m);
}

#[test]
fn assembly_native_cached_forced_and_parameter_window_follow_formula_without_brep_builds() {
    let (mut s, a, b, j) = two_boxes();
    s.param_set("opening", &0.0.into()).unwrap();
    s.joint_set(&JointRef::Id(j), Some(&"opening".into()), None).unwrap();
    s.part_select(&PartRef::Root { root: true }).unwrap();
    let path = scratch("a-app-formula.qcad");
    s.save(Some(&path)).unwrap();
    for force in [false, true] {
        let mut m = native_assembly_load(&path);
        assert_eq!(m.project.active_component, Some(m.project.root));
        assert!(m.project.components.iter().find(|c| c.id == a).unwrap().grounded);
        assert_eq!(m.project.feat_dim(j, "offset"), Some("opening"));
        let rebuilt = native_assembly_rebuild(&mut m, force);
        assert_eq!(rebuilt, if force { 2 } else { 0 }, "cached reopen does not rebuild shapes; forced path builds both boxes");
        native_pose(&m, b, 0.);
        let before = native_assembly_breps(&m);
        // Carcass=600*800*700=336000000; drawer=500*600*150=45000000 mm³.
        let volumes: Vec<_> = {
            let _gate = qymcad_kernel::kernel_gate();
            m.shapes.values().map(|s| s.volume()).collect()
        };
        assert_close(volumes.iter().sum(), 381000000., 1e-4, "sum of the two envelope volumes");
        for opening in [300., 0., 300., 0.] {
            assert_eq!(native_assembly_parameter(&mut m, "opening", &opening.to_string()), 0, "only placement changes");
            native_pose(&m, b, opening);
            assert!(native_assembly_breps(&m) == before, "joint formula must not touch local B-reps");
        }
    }
}

#[test]
fn assembly_native_released_formula_and_numeric_drag_survive_regenerate_save_reopen() {
    for release in [true, false] {
        let (mut s, _, b, j) = two_boxes();
        s.param_set("opening", &300.0.into()).unwrap();
        s.joint_set(&JointRef::Id(j), Some(&"opening".into()), None).unwrap();
        if release {
            s.joint_set(&JointRef::Id(j), None, Some("offset")).unwrap();
        } else {
            s.joint_set(&JointRef::Id(j), Some(&300.0.into()), None).unwrap();
        }
        s.part_select(&PartRef::Root { root: true }).unwrap();
        let path = scratch(&format!("a-app-drag-{release}.qcad"));
        s.save(Some(&path)).unwrap();
        let mut m = native_assembly_load(&path);
        native_assembly_rebuild(&mut m, false);
        native_pose(&m, b, 300.);
        assert!(m.project.feat_dim(j, "offset").is_none());
        if release {
            assert_eq!(m.project.joints[0].drive[1], None);
        } else {
            assert_close(m.project.joints[0].drive[1].unwrap(), 300., 1e-5, "numeric drive");
        }
        m.project.drag_pull = Some((b, [0., 0., 0.], [0., -120., 250.]));
        let r = m.project.solve_joints();
        assert!(r.errors.is_empty());
        m.project.drag_pull = None;
        let r = m.project.solve_joints();
        assert!(r.errors.is_empty());
        native_pose(&m, b, 120.);
        native_assembly_rebuild(&mut m, false);
        native_pose(&m, b, 120.);
        if release {
            assert_eq!(m.project.joints[0].drive[1], None);
        } else {
            assert_close(m.project.joints[0].drive[1].unwrap(), 120., 1e-5, "numeric drive follows native drag");
        }
        native_assembly_save(&m, &path);
        let mut again = native_assembly_load(&path);
        native_assembly_rebuild(&mut again, false);
        native_pose(&again, b, 120.);
        native_assembly_rebuild(&mut again, true);
        native_pose(&again, b, 120.);
    }
}

#[test]
fn assembly_cabinet_three_parts_twenty_plates_two_sliders_moves_exactly_six_bodies() {
    let plan: serde_json::Value = serde_json::from_str(include_str!("data/assembly-cabinet-plan.json")).unwrap();
    let mut s = Session::new_part();
    let carcass = s.project().active_component.unwrap();
    s.rename(RenameTarget::Part(carcass), "carcass").unwrap();
    let mut parts = BTreeMap::new();
    let mut bodies = BTreeMap::new();
    for part in plan["parts"].as_array().unwrap() {
        let name = part["name"].as_str().unwrap();
        let id = if name == "carcass" { carcass } else { s.part_new(name).unwrap()["part"]["id"].as_u64().unwrap() };
        parts.insert(name.to_string(), id);
        for item in part["plates"].as_array().unwrap() {
            let name = item["name"].as_str().unwrap();
            let bounds = std::array::from_fn(|k| item["bbox_local"][k].as_f64().unwrap());
            let body = plate(&mut s, name, bounds);
            bodies.insert(name.to_string(), body);
            {
                let _gate = qymcad_kernel::kernel_gate();
                assert_close(s.shape(body).unwrap().volume(), item["volume_mm3"].as_f64().unwrap(), 1e-4, "plate's hand volume");
            }
        }
        let translation = std::array::from_fn(|k| part["translation_mm"][k].as_f64().unwrap());
        s.part_place(&PartRef::Id(id), translation, None).unwrap();
    }
    s.ground(&PartRef::Id(carcass), true).unwrap();
    let j1 = rail(&mut s, "drawer1-slide", bodies["carcass-left"], bodies["drawer1-left"]);
    rail(&mut s, "drawer2-slide", bodies["carcass-left"], bodies["drawer2-left"]);
    assert_eq!(s.info().bodies.len(), 20);
    assert_eq!(s.project().components.len(), 4);
    assert_eq!(s.project().joints.len(), 2);
    assert_eq!(s.project().connectors.len(), 4);
    assert_eq!(s.assembly_info()["healthy"], true);
    let before = s.info().bodies;
    // 35604432 + 2*12323520 = 60251472 mm³. This is a sum, not a Boolean union.
    assert_close(before.iter().map(|b| b.volume).sum(), 60251472., 1e-3, "twenty plate volume sum");
    for (name, expected) in [("carcass", 35604432.), ("drawer1", 12323520.), ("drawer2", 12323520.)] {
        assert_close(
            before.iter().filter(|b| b.owner_part == Some(parts[name])).map(|b| b.volume).sum(),
            expected,
            1e-3,
            "Part plate volume sum",
        );
    }
    let geometry = local_breps(&s);
    s.joint_set(&JointRef::Id(j1), Some(&300.0.into()), None).unwrap();
    let after = s.info().bodies;
    let mut moved = 0;
    for b in after {
        let previous = before.iter().find(|old| old.id == b.id).unwrap();
        assert_eq!(b.bbox_local, previous.bbox_local);
        assert_eq!(b.volume, previous.volume);
        let dy = if b.owner_part == Some(parts["drawer1"]) {
            moved += 1;
            -300.
        } else {
            0.
        };
        for k in 0..6 {
            assert_close(
                b.bbox_world[k],
                previous.bbox_world[k] + if k % 3 == 1 { dy } else { 0. },
                1e-5,
                "only drawer1 moves along world -Y",
            );
        }
    }
    assert_eq!(moved, 6);
    assert!(local_breps(&s) == geometry, "placement leaves every local B-rep unchanged");
    assert_eq!(s.assembly_info()["healthy"], true);
    s.joint_set(&JointRef::Id(j1), Some(&0.0.into()), None).unwrap();
    for (a, b) in s.info().bodies.iter().zip(before) {
        for k in 0..6 {
            assert_close(a.bbox_world[k], b.bbox_world[k], 1e-5, "cabinet closes absolutely");
        }
    }
}
