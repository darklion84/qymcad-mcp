//! H5b: the face-anchor witness of Rigid/Slider joints follows the anchored body to the current one (F-160, ADR 0051
//! and 0054). Natively a connector keeps the body it was made on and reads the face from that body's last-built
//! faces, so a feature that moves the face leaves the joint on the old geometry with no error. Fixture: carcass
//! 600×800×700 centred in XY (x −300..300), drawer 500×600×150 on a Slider anchored on the carcass's +X face and the
//! drawer's −X face.
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

fn drawer(kind: AssemblyJointKind) -> Drawer {
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
            kind,
            a: JointAnchor::face(carcass_body, Sel::Facing { dir: [1., 0., 0.], tol_deg: 1. }),
            b: JointAnchor::face(drawer_body, Sel::Facing { dir: [-1., 0., 0.], tol_deg: 1. }),
            mode: JointMode::AsBuilt,
            axis: (kind == AssemblyJointKind::Slider).then_some(BaseName::XZ),
            offset: (kind == AssemblyJointKind::Slider).then(|| 50.0.into()),
            angle: None,
            limits: None,
        })
        .unwrap()["joint"]["id"]
        .as_u64()
        .unwrap();
    Drawer { s, drawer, joint, carcass_body, drawer_body }
}

fn facing(dir: [f64; 3]) -> Sel {
    Sel::Facing { dir, tol_deg: 1.0 }
}

fn world(s: &Session, part: Id) -> [f64; 12] {
    s.project().world_transform(part)
}

/// The edit moves the anchored face (or its outline, so its centroid): QymCAD would keep placing the joint on the
/// consumed body's old face. Refused atomically, nothing changed.
fn refused(d: &mut Drawer, what: &str, edit: impl FnOnce(&mut Session) -> Result<()>) {
    let before = snapshot(&d.s);
    let error = edit(&mut d.s).expect_err(what).to_string();
    assert!(error.contains(&format!("invalidate anchors of joints [{}]", d.joint)), "{what}: {error}");
    unchanged(&d.s, &before);
}

/// The edit leaves the face where it was: allowed, healthy, the joint places the Part exactly as before.
fn accepted(d: &mut Drawer, what: &str, edit: impl FnOnce(&mut Session) -> Result<()>) {
    let placed = world(&d.s, d.drawer);
    edit(&mut d.s).unwrap_or_else(|e| panic!("{what}: {e}"));
    assert_eq!(d.s.assembly_info()["healthy"], true, "{what}");
    for (a, b) in placed.iter().zip(world(&d.s, d.drawer)) {
        assert_close(b, *a, 1e-6, what);
    }
}

#[test]
fn a_feature_that_moves_the_anchored_face_of_the_carcass_is_refused() {
    for kind in [AssemblyJointKind::Slider, AssemblyJointKind::Rigid] {
        let mut d = drawer(kind);
        let body = d.carcass_body;
        refused(&mut d, "push_face of the anchored face", |s| {
            s.push_face(Some(body), &facing([1., 0., 0.]), &20.0.into(), None).map(|_| ())
        });
        // An adjoining face pushed outward lengthens the anchored face: its centroid moves.
        refused(&mut d, "push_face of an adjoining face", |s| {
            s.push_face(Some(body), &facing([0., 1., 0.]), &20.0.into(), None).map(|_| ())
        });
        let rim = Sel::EdgesOf(Box::new(facing([1., 0., 0.])));
        let one = Sel::Ids(vec![d.s.select(Some(body), Element::Edges, &rim).unwrap().1[0]]);
        refused(&mut d, "fillet of an edge of the anchored face", |s| s.fillet(Some(body), &one, &20.0.into(), None).map(|_| ()));
        refused(&mut d, "chamfer of an edge of the anchored face", |s| s.chamfer(Some(body), &one, &20.0.into(), None, None).map(|_| ()));
    }
}

#[test]
fn a_feature_that_leaves_the_anchored_face_in_place_is_allowed() {
    let mut d = drawer(AssemblyJointKind::Slider);
    let body = d.carcass_body;
    let far = Sel::Ids(vec![d.s.select(Some(body), Element::Edges, &Sel::EdgesOf(Box::new(facing([-1., 0., 0.])))).unwrap().1[0]]);
    accepted(&mut d, "fillet elsewhere", |s| s.fillet(Some(body), &far, &5.0.into(), None).map(|_| ()));
    let mut d = drawer(AssemblyJointKind::Slider);
    accepted(&mut d, "push_face of the opposite face", |s| s.push_face(Some(body), &facing([-1., 0., 0.]), &20.0.into(), None).map(|_| ()));
    let mut d = drawer(AssemblyJointKind::Slider);
    accepted(&mut d, "hole in another face", |s| {
        s.hole(&Hole {
            body: Some(body),
            face: facing([-1., 0., 0.]),
            at: None,
            offset: None,
            diameter: 10.0.into(),
            depth: Some(20.0.into()),
            kind: HoleKind::Plain,
            dia2: None,
            depth2: None,
            name: None,
        })
        .map(|_| ())
    });
}

#[test]
fn the_same_rules_hold_for_the_anchored_face_of_the_drawer() {
    let mut d = drawer(AssemblyJointKind::Slider);
    let body = d.drawer_body;
    refused(&mut d, "push_face of the drawer's anchored face", |s| {
        s.push_face(Some(body), &facing([-1., 0., 0.]), &20.0.into(), None).map(|_| ())
    });
    let rim = Sel::EdgesOf(Box::new(facing([-1., 0., 0.])));
    let one = Sel::Ids(vec![d.s.select(Some(body), Element::Edges, &rim).unwrap().1[0]]);
    refused(&mut d, "fillet of an edge of the drawer's anchored face", |s| s.fillet(Some(body), &one, &20.0.into(), None).map(|_| ()));
    let far = Sel::Ids(vec![d.s.select(Some(body), Element::Edges, &Sel::EdgesOf(Box::new(facing([1., 0., 0.])))).unwrap().1[0]]);
    accepted(&mut d, "fillet elsewhere on the drawer", |s| s.fillet(Some(body), &far, &5.0.into(), None).map(|_| ()));
}

/// F-160 for faces, native evidence (app data path, no Session): pushing the carcass's +X face moves it to x = 320; the
/// connector still names the consumed body, whose stale face stays at x = 300, and the solver places the drawer on it.
#[test]
fn native_a_pushed_body_keeps_the_face_anchor_on_the_consumed_bodys_old_face_f160() {
    let d = drawer(AssemblyJointKind::Slider);
    let mut s = d.s;
    let path = scratch("h5b-native-pushed.qcad");
    s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    native_assembly_rebuild(&mut m, true);
    let carcass = m.project.components.iter().find(|c| c.name == "carcass").unwrap().id;
    m.project.set_active_component(Some(carcass));
    let f = m.project.regen_faces[&d.carcass_body].iter().find(|f| f.normal[0] > 0.99).unwrap().clone();
    let key = qymcad_core::feature::FaceKey { index: 0, centroid: [f.centroid.x, f.centroid.y, f.centroid.z], normal: f.normal, id: f.id };
    let pushed = m.project.add_push_face(d.carcass_body, key, 20.0);
    native_assembly_rebuild(&mut m, false); // asserts: no feature error, no joint fault, no violation, no conflict
    assert!(m.project.consumed_bodies().contains(&d.carcass_body));
    let x_of = |body: Id| m.project.regen_faces[&body].iter().find(|g| g.id == f.id).unwrap().centroid.x;
    assert_close(x_of(d.carcass_body), 300.0, 1e-6, "the stale face the anchor reads");
    assert_close(x_of(pushed), 320.0, 1e-6, "the face of the current body");
    let a = m.project.connectors.iter().find(|c| c.owner == carcass).unwrap();
    assert!(matches!(a.anchor, qymcad_core::feature::AnchorRef::FaceCenter(b, _) if b == d.carcass_body));
    // The drawer still sits against x = 300 (its −X face at 300 + travel offset origin), not against the face at 320.
    let t = m.project.world_transform(d.drawer);
    assert_close(t[3], world(&s, d.drawer)[3], 1e-6, "the drawer did not follow the pushed face");
}

/// H5c (F-166): face anchors resolve on QymCAD.app's open path too (`regen_faces` comes back from the bodies of the file), for
/// both kinds, with nothing marked for a rebuild.
#[test]
fn native_app_open_resolves_the_face_anchors_of_rigid_and_slider_f166() {
    for kind in [AssemblyJointKind::Slider, AssemblyJointKind::Rigid] {
        let mut d = drawer(kind);
        let path = scratch(&format!("h5c-face-{kind:?}.qcad"));
        d.s.save(Some(&path)).unwrap();
        let m = native_app_open(&path);
        assert!(m.project.joint_faults().is_empty(), "{kind:?}: {:?}", m.project.joint_faults());
        assert!(m.project.mates_violated.is_empty() && !m.project.mates_conflict, "{kind:?}");
        let placed = d.s.project().world_transform(d.drawer);
        for (a, b) in placed.iter().zip(m.project.world_transform(d.drawer)) {
            assert_close(b, *a, 1e-6, "the app places the drawer where the server did");
        }
    }
}
