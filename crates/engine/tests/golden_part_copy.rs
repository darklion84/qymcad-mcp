//! G4: the app's tree Copy/Paste of a Part (`Project::clone_component`, ADR 0053) on a cabinet with two drawers.
//! Hand volumes (mm³), every plate a separate body, no Boolean union:
//! carcass: left/right 18×640×700 = 8064000 each, bottom 564×640×18 = 6497280; sum 22625280.
//! drawer: left/right 18×600×150 = 1620000 each, back 464×18×150 = 1252800, bottom 464×564×12 = 3140352,
//! front 520×18×fh: fh = 170 → 1591200 (drawer 9224352), fh = 200 → 1872000 (drawer 9505152).
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const DRAWER_170: f64 = 9224352.0;
const DRAWER_200: f64 = 9505152.0;

fn plate(s: &mut Session, name: &str, bounds: [f64; 6], height: Num) -> Id {
    let [x0, y0, _, x1, y1, _] = bounds;
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some(&format!("{name}-sk"))).unwrap();
    s.sketch_rect(sk, &((x0 + x1) / 2.).into(), &((y0 + y1) / 2.).into(), &(x1 - x0).into(), &(y1 - y0).into(), false).unwrap();
    let ex: Extrude = serde_json::from_value(json!({"sketch":sk,"height":height,"op":"new_body","name":name})).unwrap();
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
    })
    .unwrap()["joint"]["id"]
        .as_u64()
        .unwrap()
}

struct Cabinet {
    s: Session,
    carcass: Id,
    drawer: Id,
    bodies: BTreeMap<&'static str, Id>,
}

/// A grounded carcass of three plates and drawer1 of five separate plates, placed at z = 100.
fn cabinet() -> Cabinet {
    let mut s = Session::new_part();
    let carcass = s.project().active_component.unwrap();
    s.rename(RenameTarget::Part(carcass), "carcass").unwrap();
    s.param_set("fh", &170.0.into()).unwrap();
    let mut bodies = BTreeMap::new();
    for (name, b, h) in [
        ("carcass-left", [-300., -320., 0., -282., 320., 700.], 700.0),
        ("carcass-right", [282., -320., 0., 300., 320., 700.], 700.0),
        ("carcass-bottom", [-282., -320., 0., 282., 320., 18.], 18.0),
    ] {
        bodies.insert(name, plate(&mut s, name, b, h.into()));
    }
    s.ground(&PartRef::Id(carcass), true).unwrap();
    let drawer = s.part_new("drawer").unwrap()["part"]["id"].as_u64().unwrap();
    for (name, b, h) in [
        ("drawer-left", [-250., -300., 0., -232., 300., 150.], Num::from(150.0)),
        ("drawer-right", [232., -300., 0., 250., 300., 150.], Num::from(150.0)),
        ("drawer-back", [-232., 282., 0., 232., 300., 150.], Num::from(150.0)),
        ("drawer-bottom", [-232., -282., 0., 232., 282., 12.], Num::from(12.0)),
        ("drawer-front", [-260., -318., 0., 260., -300., 0.], Num::from("fh")),
    ] {
        bodies.insert(name, plate(&mut s, name, b, h));
    }
    s.part_place(&PartRef::Id(drawer), [0., 0., 100.], None).unwrap();
    Cabinet { s, carcass, drawer, bodies }
}

fn part_bodies(s: &Session, part: Id) -> Vec<BodyInfo> {
    s.info().bodies.into_iter().filter(|b| b.owner_part == Some(part)).collect()
}

fn part_volume(s: &Session, part: Id) -> f64 {
    part_bodies(s, part).iter().map(|b| b.volume).sum()
}

fn copied(r: &Value, source: Id) -> Id {
    r["body_map"].as_array().unwrap().iter().find(|pair| pair[0] == source).unwrap()[1].as_u64().unwrap()
}

#[test]
fn cabinet_copy_slides_independently_and_follows_one_parameter() {
    let Cabinet { mut s, carcass, drawer, bodies } = cabinet();
    assert_close(part_volume(&s, carcass), 22625280.0, 1e-3, "carcass plates");
    assert_close(part_volume(&s, drawer), DRAWER_170, 1e-3, "drawer1 plates");
    let r = s.part_copy(&PartRef::Name("drawer".into()), "drawer2", Some([0., 0., 390.]), None).unwrap();
    let drawer2 = r["part"]["id"].as_u64().unwrap();
    assert_eq!(r["part"]["name"], "drawer2");
    assert_eq!(r["copied_from"], drawer);
    assert!(r["part"].get("instance_of").is_none(), "a copy is not an instance");
    // Every plate is copied as a separate body: five plates in each drawer, an independent recipe.
    assert_eq!(part_bodies(&s, drawer).len(), 5);
    assert_eq!(part_bodies(&s, drawer2).len(), 5);
    assert_eq!(r["body_map"].as_array().unwrap().len(), 5);
    assert_close(part_volume(&s, drawer2), DRAWER_170, 1e-3, "drawer2 plates");
    for b in part_bodies(&s, drawer) {
        let copy = part_bodies(&s, drawer2).into_iter().find(|c| c.id == copied(&r, b.id)).unwrap();
        assert_eq!(copy.bbox_local, b.bbox_local);
        assert_close(copy.volume, b.volume, 1e-6, "plate copy volume");
        for k in 0..6 {
            assert_close(copy.bbox_world[k], b.bbox_world[k] + if k % 3 == 2 { 290.0 } else { 0.0 }, 1e-6, "copy placement");
        }
    }
    // Joints and connectors of the source are not copied (clone_subtree_into copies no joints).
    assert!(s.project().joints.is_empty() && s.project().connectors.is_empty());
    let j1 = rail(&mut s, "drawer1-slide", bodies["carcass-left"], bodies["drawer-left"]);
    let j2 = rail(&mut s, "drawer2-slide", bodies["carcass-left"], copied(&r, bodies["drawer-left"]));
    assert_eq!(s.assembly_info()["healthy"], true);
    // Each drawer slides on its own joint.
    let start = s.info().bodies;
    s.joint_set(&JointRef::Id(j1), Some(&300.0.into()), None).unwrap();
    s.joint_set(&JointRef::Id(j2), Some(&150.0.into()), None).unwrap();
    let moved = |s: &Session| {
        for b in s.info().bodies {
            let old = start.iter().find(|o| o.id == b.id).unwrap();
            let dy = match b.owner_part {
                Some(p) if p == drawer => -300.0,
                Some(p) if p == drawer2 => -150.0,
                _ => 0.0,
            };
            for k in 0..6 {
                let expected =
                    old.bbox_world[k] + if k % 3 == 1 { dy } else { 0.0 } + if k == 5 { b.bbox_local[5] - old.bbox_local[5] } else { 0.0 };
                assert_close(b.bbox_world[k], expected, 1e-5, "each drawer slides by its own joint");
            }
        }
    };
    moved(&s);
    // One parameter edit changes the front plate of both drawers; both joints stay healthy and in place.
    let report = s.param_set("fh", &200.0.into()).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(s.assembly_info()["healthy"], true);
    assert_close(part_volume(&s, drawer), DRAWER_200, 1e-3, "drawer1 follows fh");
    assert_close(part_volume(&s, drawer2), DRAWER_200, 1e-3, "drawer2 follows fh");
    assert_close(part_volume(&s, carcass), 22625280.0, 1e-3, "carcass unchanged");
    moved(&s);
    // Save and reopen keep both drawers, their plates, both joints and their travel.
    let path = scratch("g4-cabinet.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(opened.assembly_info()["healthy"], true);
    assert_eq!(part_bodies(&opened, drawer).len(), 5);
    assert_eq!(part_bodies(&opened, drawer2).len(), 5);
    assert_close(part_volume(&opened, drawer2), DRAWER_200, 1e-3, "reopened drawer2");
    assert_close(opened.project().world_transform(drawer)[7], -300.0, 1e-5, "reopened drawer1 travel");
    assert_close(opened.project().world_transform(drawer2)[7], -150.0, 1e-5, "reopened drawer2 travel");
}

#[test]
fn copy_has_an_independent_recipe() {
    let Cabinet { mut s, drawer, .. } = cabinet();
    let r = s.part_copy(&PartRef::Id(drawer), "drawer2", Some([0., 0., 390.]), None).unwrap();
    let drawer2 = r["part"]["id"].as_u64().unwrap();
    // A plate added to drawer1 later does not reach drawer2: a copy, not a clone.
    s.part_select(&PartRef::Id(drawer)).unwrap();
    plate(&mut s, "drawer-divider", [-232., -9., 0., 232., 9., 100.], 100.0.into());
    assert_eq!(part_bodies(&s, drawer).len(), 6);
    assert_eq!(part_bodies(&s, drawer2).len(), 5);
    assert_close(part_volume(&s, drawer2), DRAWER_170, 1e-3, "drawer2 keeps its own recipe");
}

#[test]
fn copy_without_placement_stands_on_the_source_as_the_app_pastes_it_and_undo_removes_it() {
    let Cabinet { mut s, carcass, drawer, .. } = cabinet();
    let before = {
        let mut v = serde_json::to_value(s.project()).unwrap();
        v.as_object_mut().unwrap().remove("next_id");
        (v, s.info())
    };
    let r = s.part_copy(&PartRef::Id(drawer), "drawer2", None, None).unwrap();
    let drawer2 = r["part"]["id"].as_u64().unwrap();
    let source = s.project().components.iter().find(|c| c.id == drawer).unwrap().clone();
    let copy = s.project().components.iter().find(|c| c.id == drawer2).unwrap().clone();
    assert_eq!(copy.transform, source.transform, "clone_subtree_into copies the placement");
    assert_eq!(copy.parent, source.parent, "pasted into the source's parent assembly");
    assert_eq!(copy.grounded, source.grounded);
    // The grounded carcass copies grounded, as natively.
    let r = s.part_copy(&PartRef::Id(carcass), "carcass2", Some([1000., 0., 0.]), None).unwrap();
    assert_eq!(r["grounded"], true);
    // Names stay unique.
    let error = s.part_copy(&PartRef::Id(drawer), "drawer2", None, None).unwrap_err().to_string();
    assert!(error.contains("a part named 'drawer2' already exists"), "{error}");
    assert_eq!(s.undo().unwrap().call.unwrap().tool, "part_copy");
    assert_eq!(s.undo().unwrap().call.unwrap().tool, "part_copy");
    let mut now = serde_json::to_value(s.project()).unwrap();
    now.as_object_mut().unwrap().remove("next_id");
    assert!(now == before.0, "undo restores the recipe");
    assert_eq!(s.info(), before.1);
}
