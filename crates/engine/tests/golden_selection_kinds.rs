//! Kind-selected persistent edges keep their intended geometry on server and native GUI parameter edits.

mod common;
use common::*;
use qymcad_engine::*;
use std::f64::consts::PI;

fn volume(height: f64, radius: f64) -> f64 {
    // Each of four vertical corners replaces r² of square cross-section with a quarter disc πr²/4.
    (20.0 * 16.0 - 4.0 * (1.0 - PI / 4.0) * radius.powi(2)) * height
}

#[test]
fn kind_selected_fillet_survives_server_and_gui_parameter_edits() {
    let mut s = Session::new_part();
    s.param_set("h", &10.0.into()).unwrap();
    s.param_set("r", &2.0.into()).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &20.0.into(), &16.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch,
        profiles: None,
        height: "h".into(),
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap();
    let edges = Sel::And(Box::new(Sel::Kind(SelectionKind::Line)), Box::new(Sel::Along { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }));
    assert_eq!(
        s.select(None, Element::Edges, &edges).unwrap().1.len(),
        4,
        "four rectangle vertices extrude to four straight vertical corners"
    );
    s.fillet(None, &edges, &"r".into(), None).unwrap();
    assert_close(s.result_bodies()[0].volume, volume(10.0, 2.0), 1e-6, "kind-selected initial fillet");
    let path = scratch("kind_selected_fillet.qcad");
    s.save(Some(&path)).unwrap();
    for (name, value, height, radius) in [("h", "14", 14.0, 2.0), ("r", "3", 10.0, 3.0)] {
        let (mut edited, _) = Session::open(&path).unwrap();
        let rebuilt = edited.param_set(name, &Num::Expr(value.into())).unwrap();
        assert!(rebuilt.errors.is_empty(), "{name}: {:?}", rebuilt.errors);
        assert_close(edited.result_bodies()[0].volume, volume(height, radius), 1e-6, "server kind-selected fillet edit");
        assert_close(gui_edit_param(&path, name, value), volume(height, radius), 1e-6, "GUI kind-selected fillet edit");
        let arcs = edited.select(None, Element::Edges, &Sel::Kind(SelectionKind::Arc)).unwrap().1;
        let curves = edited.select(None, Element::Edges, &Sel::Kind(SelectionKind::Curve)).unwrap().1;
        assert_eq!(arcs.len(), 2 * 4, "four quarter arcs on each of two caps");
        assert_eq!(curves, arcs, "curve aliases the arc/other topology kinds");
        let topo = edited.topology(None, false).unwrap();
        for edge in topo.edges.iter().filter(|e| arcs.contains(&e.id)) {
            assert_close(edge.radius.unwrap(), radius, 1e-6, "quarter-arc radius");
            assert_close(edge.length, PI * radius / 2.0, 1e-6, "quarter-arc length");
        }
    }
}

fn kind_top() -> Sel {
    Sel::And(Box::new(Sel::Kind(SelectionKind::Plane)), Box::new(Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 5.0 }))
}

fn kind_face_modifier(tool: &str) {
    use qymcad_core::{feature::FeatureKind, refs::Query};
    let mut s = Session::new_part();
    s.param_set("h", &10.0.into()).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &20.0.into(), &16.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch,
        profiles: None,
        height: "h".into(),
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap();
    let picks = s.select(None, Element::Faces, &kind_top()).unwrap().1;
    let (id, _) = match tool {
        "hole" => s.hole(&Hole {
            body: None,
            face: kind_top(),
            at: None,
            diameter: 2.0.into(),
            depth: Some(3.0.into()),
            kind: HoleKind::Plain,
            dia2: None,
            depth2: None,
            name: None,
        }),
        "shell" => s.shell(None, &kind_top(), &1.0.into(), Side::Inward, None),
        "push_face" => s.push_face(None, &kind_top(), &2.0.into(), None),
        _ => unreachable!(),
    }
    .unwrap_or_else(|e| panic!("{tool} must persist kind-selected faces: {e}"));
    let query = match &s.project().timeline.iter().find(|n| n.id == id).unwrap().kind {
        FeatureKind::Hole { face, .. } | FeatureKind::PushFace { face, .. } => &face.query,
        FeatureKind::Shell { faces, .. } => &faces.query,
        _ => unreachable!(),
    };
    assert_eq!(*query, Query::Ids(picks), "freeze the whole kind-containing face selection");
    assert!(query.is_pick_list(), "retain the native asked-versus-opened guard");
    // Stock V=20*16*h. Hole removes a radius-1 depth-3 cylinder; shell removes an
    // 18*14*(h-1) rectangular cavity; pushing the top by 2 adds 20*16*2.
    let expected = |h: f64| match tool {
        "hole" => 20.0 * 16.0 * h - PI * 3.0,
        "shell" => 20.0 * 16.0 * h - 18.0 * 14.0 * (h - 1.0),
        "push_face" => 20.0 * 16.0 * (h + 2.0),
        _ => unreachable!(),
    };
    assert_close(s.result_bodies()[0].volume, expected(10.0), 1e-6, "kind face modifier creation");
    let path = scratch(&format!("kind_selected_{tool}.qcad"));
    s.save(Some(&path)).unwrap();
    let rebuilt = s.param_set("h", &14.0.into()).unwrap();
    assert!(rebuilt.errors.is_empty(), "{tool}: {:?}", rebuilt.errors);
    assert_close(s.result_bodies()[0].volume, expected(14.0), 1e-6, "kind face modifier parameter edit");
    let (mut reopened, _) = Session::open(&path).unwrap();
    reopened.param_set("h", &14.0.into()).unwrap();
    assert_close(reopened.result_bodies()[0].volume, expected(14.0), 1e-6, "reopened kind face modifier edit");
    assert_close(gui_edit_param(&path, "h", "14"), expected(14.0), 1e-6, "GUI kind face modifier edit");
}

#[test]
fn kind_selected_hole_survives_server_and_gui_parameter_edits() {
    kind_face_modifier("hole");
}

#[test]
fn kind_selected_shell_survives_server_and_gui_parameter_edits() {
    kind_face_modifier("shell");
}

#[test]
fn kind_selected_push_face_survives_server_and_gui_parameter_edits() {
    kind_face_modifier("push_face");
}
