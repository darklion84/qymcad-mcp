//! Direct pending-recipe tests: a public open would already refuse these documents.
use super::*;
use crate::*;
use qymcad_core::model::{Constraint, ProjSource};

fn extrusion(s: &mut Session, x: f64, width: Num, height: Num, down: Option<Num>) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &x.into(), &15.0.into(), &width, &6.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch: sk,
        profiles: None,
        height,
        down,
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap()
    .0
}
fn measured() -> Session {
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let src = extrusion(&mut s, 20., "w".into(), 10.0.into(), None);
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("measurement")).unwrap();
    let edge = s.p.regen_edges[&src].iter().find(|e| e.a[1] == e.b[1] && (e.a[0] - e.b[0]).abs() > 39.).unwrap().id;
    let refs = s.sketch_project(sk, src, &[ProjSource::Edge(edge)]).unwrap().0.points;
    let si = s.p.sketch_index(sk).unwrap();
    s.p.sketches[si].constraints.push(Constraint::Distance {
        a: refs[0],
        b: refs[1],
        d: 40.,
        off: 0.,
        expr: String::new(),
        driven: true,
        axis: 1,
        at: None,
    });
    assert!(s.p.add_named_dim("measured_w".into(), sk, refs));
    s.p.solve_sketch(si);
    s
}
fn pending(s: &mut Session, value: &str) {
    s.p.parameters.iter_mut().find(|p| p.name == "w").unwrap().expr = value.into();
    s.p.eval_parameters();
    s.p.mark_all_dirty();
}
fn archive(s: &mut Session) -> std::collections::BTreeMap<String, Vec<u8>> {
    let path = std::env::temp_dir().join(format!("r3-unit-{}-{:?}.qcad", std::process::id(), std::thread::current().id()));
    s.save_with_options(Some(&path), true).unwrap();
    let mut z = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let entries = (0..z.len())
        .map(|i| {
            let mut e = z.by_index(i).unwrap();
            let mut b = vec![];
            std::io::Read::read_to_end(&mut e, &mut b).unwrap();
            (e.name().to_string(), b)
        })
        .collect();
    std::fs::remove_file(path).unwrap();
    entries
}
#[test]
fn standalone_late_down_refusal_restores_immediate_archive() {
    let mut s = measured();
    let id = extrusion(&mut s, 100., 10.0.into(), 7.0.into(), Some("50-measured_w".into()));
    pending(&mut s, "60");
    let before = archive(&mut s);
    let r = s.rebuild();
    assert!(r.errors.iter().any(|e| e.node == id && e.message.contains("down")), "{r:?}");
    assert_eq!(archive(&mut s), before);
}
#[test]
fn late_guard_is_not_hidden_by_preexisting_node_error() {
    let mut s = measured();
    let id = extrusion(&mut s, 100., 10.0.into(), 7.0.into(), Some("50-measured_w".into()));
    s.p.regen_errors.insert(id, qymcad_core::errors::CoreError::Kernel("prior failure".into()));
    let before = archive(&mut s);
    let e = s.param_set("w", &60.0.into()).unwrap_err().to_string();
    assert!(e.contains("down") && e.contains(&id.to_string()), "{e}");
    assert_eq!(archive(&mut s), before);
}
#[test]
fn nonconvergent_reference_cycle_refuses_with_exact_rollback() {
    let mut s = measured();
    let si = 0;
    let c = s.p.sketches[si].constraints.iter_mut().find(|c| c.expr() == Some("w")).unwrap();
    *c.expr_mut().unwrap() = "measured_w+1".into();
    s.p.eval_parameters();
    s.p.mark_all_dirty();
    let before = archive(&mut s);
    let r = s.rebuild();
    assert!(r.errors.iter().any(|e| e.message.contains("stabilize") && e.message.contains("4")), "{r:?}");
    assert_eq!(archive(&mut s), before);
}
#[test]
fn standalone_height_and_move_settle_pending_reference_scope() {
    let mut s = measured();
    let body = extrusion(&mut s, 100., 10.0.into(), "measured_w/10".into(), None);
    let m = s
        .move_body(&Move {
            body: Some(body),
            translation: Some(["measured_w/4".into(), 0.0.into(), 0.0.into()]),
            rotation: None,
            name: None,
        })
        .unwrap()
        .0
        .body;
    pending(&mut s, "60");
    let r = s.rebuild();
    assert!(r.errors.is_empty(), "{r:?}");
    let b = r.bodies.iter().find(|b| b.id == m).unwrap();
    assert!((b.bbox[0] - 110.).abs() < 1e-6, "{b:?}");
    assert!((b.volume - 360.).abs() < 1e-5, "{b:?}");
}

#[test]
fn standalone_late_tilt_refusal_restores_immediate_archive() {
    let mut s = measured();
    let id =
        s.plane_offset_tilt(&PlaneRef::Base(BaseName::XY), &7.0.into(), &"8*measured_w".into(), &0.0.into(), &0.0.into(), None).unwrap().0;
    pending(&mut s, "60");
    let before = archive(&mut s);
    let r = s.rebuild();
    assert!(r.errors.iter().any(|e| e.node == id && e.message.contains("tilt_x")), "{r:?}");
    assert_eq!(archive(&mut s), before);
}
#[test]
fn standalone_late_chamfer_refusal_restores_immediate_archive() {
    let mut s = measured();
    let b = extrusion(&mut s, 100., 40.0.into(), 10.0.into(), None);
    let t = s.topology(Some(b), true).unwrap();
    let x = t.faces.iter().find(|f| f.normal.unwrap()[0] > 0.99).unwrap().id;
    let y = t.faces.iter().find(|f| f.normal.unwrap()[1] > 0.99).unwrap().id;
    let e = t.edges.iter().find(|e| e.faces.unwrap().contains(&x) && e.faces.unwrap().contains(&y)).unwrap().id;
    let id = s
        .chamfer_ex(&Chamfer {
            body: Some(b),
            edges: Sel::Ids(vec![e]),
            dist: 1.0.into(),
            d2: None,
            angle: Some("measured_w/2".into()),
            reference_face: Some(Sel::Ids(vec![y])),
            flip: false,
            name: None,
        })
        .unwrap()
        .0;
    pending(&mut s, "200");
    let before = archive(&mut s);
    let r = s.rebuild();
    assert!(r.errors.iter().any(|e| e.node == id && e.message.contains("angle")), "{r:?}");
    assert_eq!(archive(&mut s), before);
}
#[test]
fn evaluated_rebuild_refuses_later_world_source_before_kernel() {
    use qymcad_core::model::{DimTarget, HoleTool, NamedDim};
    let mut s = Session::new_part();
    let marker = s.p.add_sketch("marker", vec![], None);
    s.p.add_sketch_node(marker, "marker");
    let source = s.p.add_sketch("late world source", vec![], None);
    s.p.add_sketch_node(source, "late world source");
    s.p.set_feat_dim(source, "measurement", "40".into());
    s.p.named_dims.push(NamedDim { name: "measured_w".into(), target: DimTarget::Feature { node: source, key: "measurement".into() } });
    s.p.set_feat_dim(marker, "coordinate", "measured_w/4".into());
    let hole = s.p.add_hole_from_sketch(0, marker, HoleTool { kind: 0, diameter: 4., depth: 4., dia2: 0., depth2: 0. }, true);
    s.reconcile_hole_dependencies();
    let before = archive(&mut s);
    let r = s.rebuild();
    assert!(
        r.errors.iter().any(|e| e.node == hole && e.message.contains("late world source") && e.message.contains("solved after the marker")),
        "{r:?}"
    );
    assert!(s.shapes.is_empty());
    assert_eq!(archive(&mut s), before);
}
#[test]
fn chained_projected_measurements_settle_in_more_than_two_passes() {
    let mut s = measured();
    let src = extrusion(&mut s, 200., "measured_w/2".into(), 10.0.into(), None);
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("second reference")).unwrap();
    let edge = s.p.regen_edges[&src].iter().find(|e| e.a[1] == e.b[1] && (e.a[0] - e.b[0]).abs() > 19.).unwrap().id;
    let refs = s.sketch_project(sk, src, &[ProjSource::Edge(edge)]).unwrap().0.points;
    let si = s.p.sketch_index(sk).unwrap();
    s.p.sketches[si].constraints.push(Constraint::Distance {
        a: refs[0],
        b: refs[1],
        d: 20.,
        off: 0.,
        expr: String::new(),
        driven: true,
        axis: 1,
        at: None,
    });
    s.p.add_named_dim("second_w".into(), sk, refs);
    s.p.solve_sketch(si);
    s.param_set("alias1", &"second_w/4".into()).unwrap();
    s.param_set("alias2", &"alias1+0".into()).unwrap();
    let body = extrusion(&mut s, 100., 10.0.into(), 10.0.into(), None);
    let m = s
        .move_body(&Move { body: Some(body), translation: Some(["alias2".into(), 0.0.into(), 0.0.into()]), rotation: None, name: None })
        .unwrap()
        .0
        .body;
    pending(&mut s, "60");
    let r = s.rebuild();
    assert!(r.errors.is_empty(), "{r:?}");
    let b = r.bodies.iter().find(|b| b.id == m).unwrap();
    assert!((b.bbox[0] - 102.5).abs() < 1e-6, "{b:?}");
    assert!((s.p.param_map()["second_w"] - 30.).abs() < 1e-6);
}

#[test]
fn reopen_repairs_clean_native_cache_after_reference_refresh() {
    let mut s = measured();
    let b = extrusion(&mut s, 100., 10.0.into(), "measured_w/10".into(), None);
    let m = s
        .move_body(&Move { body: Some(b), translation: Some(["measured_w/4".into(), 0.0.into(), 0.0.into()]), rotation: None, name: None })
        .unwrap()
        .0
        .body;
    pending(&mut s, "60");
    // One unmodified native pass: refreshed measured_w=60, old feature scope=40, clean timeline.
    let shapes = std::mem::take(&mut s.shapes);
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut s.p, shapes);
    s.shapes = shapes;
    assert!(r.errors.is_empty(), "{r:?}");
    for (id, faces) in r.built {
        s.p.set_body_faces(id, faces);
    }
    assert_eq!(s.p.param_map()["measured_w"], 60.);
    assert!(s.p.timeline.iter().all(|n| !n.dirty));
    assert!((s.shapes[&m].bbox().unwrap()[0] - 105.).abs() < 1e-6);
    let file = std::env::temp_dir().join(format!("r3-clean-native-{}.qcad", std::process::id()));
    s.save_with_options(Some(&file), true).unwrap();
    let (opened, r) = Session::open(&file).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    let b = opened.result_bodies().into_iter().find(|b| b.id == m).unwrap();
    assert!((b.bbox[0] - 110.).abs() < 1e-6, "{b:?}");
    assert!((b.volume - 360.).abs() < 1e-5, "{b:?}");
    std::fs::remove_file(file).unwrap();
}

#[test]
fn nonconvergence_on_parameter_edit_bypasses_prior_errors_and_restores_archive() {
    let mut s = measured();
    let source = s.p.sketches[0].id;
    let c = s.p.sketches[0].constraints.iter_mut().find(|c| c.expr() == Some("w")).unwrap();
    *c.expr_mut().unwrap() = "w+(measured_w-40)/2".into();
    s.p.regen_errors.insert(source, qymcad_core::errors::CoreError::Kernel("prior failure".into()));
    let before = archive(&mut s);
    let e = s.param_set("w", &60.0.into()).unwrap_err().to_string();
    assert!(e.contains("stabilize") && e.contains("4"), "{e}");
    assert_eq!(archive(&mut s), before);
}
