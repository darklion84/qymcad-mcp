//! Explicit body edits stay in their source Part while another Part is active.
use crate::*;

fn extrude(s: &mut Session, circle: bool) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    if circle {
        s.sketch_circle(sk, &0.0.into(), &0.0.into(), &8.0.into(), false).unwrap();
    } else {
        s.sketch_rect(sk, &30.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    }
    s.extrude(&Extrude {
        sketch: sk,
        profiles: None,
        profiles_at: None,
        height: 20.0.into(),
        down: None,
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap()
    .0
}

fn foreign_active(circle: bool, edit: impl FnOnce(&mut Session, Id) -> Id) {
    let mut s = Session::new_part();
    let carcass = s.p.active_component.unwrap();
    let source = extrude(&mut s, circle);
    s.p.set_active_component(Some(s.p.root));
    let drawer = s.p.add_part("drawer");
    s.p.set_active_component(Some(drawer));
    let other = extrude(&mut s, false);
    let before = {
        let _gate = qymcad_kernel::kernel_gate();
        s.shapes[&other].to_brep_bytes().unwrap()
    };
    let id = edit(&mut s, source);
    assert_eq!(s.p.active_component, Some(drawer), "body edit must restore the active drawer");
    assert_eq!(s.p.timeline.iter().find(|n| n.id == id).unwrap().parent, Some(carcass), "new feature must belong to source carcass");
    assert_eq!(s.p.body_owner(id), Some(carcass));
    let after = {
        let _gate = qymcad_kernel::kernel_gate();
        s.shapes[&other].to_brep_bytes().unwrap()
    };
    assert_eq!(before, after, "other Part's original B-rep must remain byte-identical");
    assert!(s.p.regen_errors.is_empty());
}

fn top() -> Sel {
    Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }
}

#[test]
fn owner_routing_shell() {
    foreign_active(false, |s, body| {
        let (id, r) = s.shell(Some(body), &top(), &1.0.into(), Side::Inward, None).unwrap();
        // 20³ stock minus 18×18×19 cavity = 1844 mm³.
        assert!((r.bodies.iter().find(|b| b.id == id).unwrap().volume - 1844.0).abs() < 1e-5);
        id
    });
}

#[test]
fn owner_routing_face_hole() {
    foreign_active(false, |s, body| {
        let (id, r) = s
            .hole(&Hole {
                body: Some(body),
                face: top(),
                at: None,
                offset: None,
                diameter: 4.0.into(),
                depth: Some(5.0.into()),
                kind: HoleKind::Plain,
                dia2: None,
                depth2: None,
                name: None,
            })
            .unwrap();
        // Ø4 × 5 blind bore removes π×2²×5 = 20π mm³ from 20³.
        assert!((r.bodies.iter().find(|b| b.id == id).unwrap().volume - (8000.0 - 20.0 * std::f64::consts::PI)).abs() < 1e-5);
        id
    });
}

#[test]
fn owner_routing_linear_array() {
    foreign_active(false, |s, body| {
        let (id, r) = s
            .linear_array(Some(body), &ArrayDir { dx: 30.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: 2.0.into() }, None, None)
            .unwrap();
        // Two disjoint 20³ boxes = 16000 mm³.
        assert!((r.bodies.iter().find(|b| b.id == id).unwrap().volume - 16000.0).abs() < 1e-5);
        id
    });
}

#[test]
fn owner_routing_circular_array() {
    foreign_active(false, |s, body| {
        let (id, r) = s.circular_array(Some(body), &2.0.into(), &360.0.into(), None, None).unwrap();
        // Box centres x=±30; two disjoint 20³ boxes = 16000 mm³.
        assert!((r.bodies.iter().find(|b| b.id == id).unwrap().volume - 16000.0).abs() < 1e-5);
        id
    });
}

#[test]
fn owner_routing_mirror() {
    foreign_active(false, |s, body| {
        let (id, r) = s.mirror(Some(body), &PlaneRef::Base(BaseName::YZ), false, None).unwrap();
        // Reflection retains a single 20³ body = 8000 mm³.
        assert!((r.bodies.iter().find(|b| b.id == id).unwrap().volume - 8000.0).abs() < 1e-5);
        id
    });
}

#[test]
fn owner_routing_split() {
    foreign_active(false, |s, body| {
        let r = s
            .split(&SplitOptions {
                body: Some(body),
                plane: PlaneRef::Base(BaseName::YZ),
                offset: 30.0.into(),
                name: None,
                piece_names: None,
            })
            .unwrap();
        // X=30 bisects the stock's X=[20,40]: two 10×20×20 pieces = 4000 mm³ each.
        assert_eq!(r.pieces.len(), 2);
        for b in &r.pieces {
            assert!((b.volume - 4000.0).abs() < 1e-5);
        }
        r.feature
    });
}

#[test]
fn owner_routing_thread() {
    foreign_active(true, |s, body| {
        let topo = s.topology(Some(body), false).unwrap();
        let rim = topo
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Circle)
            .max_by(|a, b| a.center.unwrap()[2].total_cmp(&b.center.unwrap()[2]))
            .unwrap()
            .id;
        let r = s
            .thread(Some(body), &Sel::Ids(vec![rim]), &ThreadOptions { size: Some("M8".into()), length: 10.0.into(), ..Default::default() })
            .unwrap();
        // A cut thread removes material from Ø8×20 stock (π×4²×20 = 320π).
        let v = r.rebuild.bodies.iter().find(|b| b.id == r.body).unwrap().volume;
        assert!(v > 0.0 && v < 320.0 * std::f64::consts::PI);
        r.body
    });
}

/// A sketch hosted on another Part's datum or face while a Part is active would silently put new bodies in
/// that other Part (live tumba run: a shared datum moved drawer fronts into the carcass). Refuse instead.
#[test]
fn sketch_on_foreign_part_datum_refuses_while_a_part_is_active() {
    let mut s = Session::new_part();
    let carcass = s.p.active_component.unwrap();
    let (datum, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &5.0.into(), Some("carcass_z5")).unwrap();
    let body = extrude(&mut s, false);
    s.p.set_active_component(Some(s.p.root));
    let drawer = s.p.add_part("drawer");
    s.p.set_active_component(Some(drawer));
    let nodes = s.p.timeline.len();
    let err = s.sketch_create(&PlaneRef::Plane(datum), None).unwrap_err().to_string();
    assert!(err.contains("belongs to part") && err.contains("part_select"), "{err}");
    assert_eq!(s.p.timeline.len(), nodes, "refusal changes nothing");
    assert_eq!(s.p.active_component, Some(drawer));
    // A face is an explicit body pick: it keeps routing to the body's Part (review R1) and restores the active Part.
    let face = s.p.regen_faces[&body].iter().find(|f| f.normal[2] > 0.5).unwrap().id;
    let sk = s.sketch_create(&PlaneRef::Face { body, face }, None).unwrap();
    assert_eq!(s.p.sketch_owner(sk), Some(carcass));
    assert_eq!(s.p.active_component, Some(drawer));
    // Selecting the owner makes the same hosts legal again.
    s.p.set_active_component(Some(carcass));
    let sk = s.sketch_create(&PlaneRef::Plane(datum), None).unwrap();
    assert_eq!(s.p.sketch_owner(sk), Some(carcass));
    // With the root Assembly active the documented ADR 0048 routing still applies.
    s.p.set_active_component(Some(s.p.root));
    let sk = s.sketch_create(&PlaneRef::Plane(datum), None).unwrap();
    assert_eq!(s.p.sketch_owner(sk), Some(carcass));
}
