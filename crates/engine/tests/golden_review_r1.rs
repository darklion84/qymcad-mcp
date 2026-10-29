//! Independent regressions for the review r1 scenarios.
mod common;
use common::*;
use qymcad_core::model::ProjSource;
use qymcad_engine::*;

fn exact_state(s: &Session) -> (Vec<u8>, Vec<(Id, Vec<u8>)>) {
    let _gate = qymcad_kernel::kernel_gate();
    let bodies = s
        .project()
        .timeline
        .iter()
        .flat_map(|n| n.kind.bodies())
        .filter_map(|id| s.shape(id).map(|shape| (id, shape.to_brep_bytes().unwrap())))
        .collect();
    (serde_json::to_vec(s.project()).unwrap(), bodies)
}

fn box_body(s: &mut Session, width: Num, depth: f64, height: f64, cx: f64) -> Id {
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sketch, &cx.into(), &0.0.into(), &width, &depth.into(), false).unwrap();
    extrude(s, sketch, height)
}

fn extrude(s: &mut Session, sketch: Id, height: f64) -> Id {
    s.extrude(&Extrude {
        down: None,
        sketch,
        profiles: None,
        height: height.into(),
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap()
    .0
}

fn top_edges(s: &mut Session, body: Id, z: f64) -> Vec<ProjSource> {
    s.topology(Some(body), false)
        .unwrap()
        .edges
        .iter()
        .filter(|e| (e.a[2] - z).abs() < 1e-6 && (e.b[2] - z).abs() < 1e-6)
        .map(|e| ProjSource::Edge(e.id))
        .collect()
}

fn options(a: Id, b: Id, op: BooleanOp, name: &str) -> BodyBooleanOptions {
    BodyBooleanOptions { a, b, op, name: Some(name.into()), piece_names: None }
}

fn assert_rejected<T: std::fmt::Debug>(result: qymcad_engine::Result<T>, context: &str) {
    match result {
        Err(error) => assert!(error.to_string().contains(context), "{error}"),
        Ok(_) => panic!("expected refusal: {context}"),
    }
}

fn imported(s: &mut Session) -> Id {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/server-box-10x10x5-inch.igs");
    s.import(&fixture, 1.0, None, Some("Imported")).unwrap().bodies[0].body
}

#[test]
fn r1_1_forward_edge_projection_is_refused_atomically() {
    assert_forward_projection_refused(false);
}

#[test]
fn r1_1_forward_face_projection_is_refused_atomically() {
    assert_forward_projection_refused(true);
}

fn assert_forward_projection_refused(face: bool) {
    let mut s = Session::new_part();
    let early = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("early")).unwrap();
    let body = box_body(&mut s, 20.0.into(), 10.0, 10.0, 0.0);
    let topo = s.topology(Some(body), false).unwrap();
    let source = if face {
        ProjSource::Face(topo.faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id)
    } else {
        let edges = top_edges(&mut s, body, 10.0);
        assert_eq!(edges.len(), 4);
        edges[0]
    };
    let before = exact_state(&s);
    let error = s.sketch_project(early, body, &[source]).unwrap_err().to_string();
    assert!(error.contains("body must be built above the sketch"), "{error}");
    assert_eq!(exact_state(&s), before, "forward {source:?} changed recipe or B-reps");
}

#[test]
fn r1_1_persisted_forward_projection_is_rejected_on_open() {
    let mut s = Session::new_part();
    s.param_set("w", &20.0.into()).unwrap();
    let body = box_body(&mut s, Num::Expr("w".into()), 10.0, 10.0, 0.0);
    let early = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("early")).unwrap();
    let mut reordered = s.project().clone();
    let sketch_node = reordered.timeline.iter().find(|node| node.id == early).unwrap().clone();
    reordered.timeline.retain(|node| node.id != early);
    reordered.timeline.insert(0, sketch_node);
    let path = scratch("r1-forward-projection.qcad");
    let (_, breps) = exact_state(&s);
    qymcad_io::save_project_guarded_with_brep(&reordered, path.to_str().unwrap(), &breps).unwrap();
    let edges = top_edges(&mut s, body, 10.0);
    assert_eq!(edges.len(), 4);
    let mut forward = reordered.clone();
    let si = forward.sketch_index(early).unwrap();
    let projection_id = forward.alloc_id();
    forward.sketches[si].projections.push(qymcad_core::model::SketchProjection {
        id: projection_id,
        body,
        src: edges[0],
        entities: Vec::new(),
        points: Vec::new(),
        edges: Vec::new(),
    });
    qymcad_io::save_project_guarded_with_brep(&forward, path.to_str().unwrap(), &breps).unwrap();
    assert_rejected(Session::open(&path).map(|_| ()), "body must be built above the sketch");
}

#[test]
fn r1_1_externally_reordered_projection_is_diagnosed_on_open() {
    let mut s = Session::new_part();
    let body = box_body(&mut s, 20.0.into(), 10.0, 10.0, 0.0);
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let edges = top_edges(&mut s, body, 10.0);
    let mut p = s.project().clone();
    let si = p.sketch_index(sketch).unwrap();
    let projection_id = p.alloc_id();
    p.sketches[si].projections.push(qymcad_core::model::SketchProjection {
        id: projection_id,
        body,
        src: edges[0],
        entities: Vec::new(),
        points: Vec::new(),
        edges: Vec::new(),
    });
    let sketch_node = p.timeline.iter().find(|node| node.id == sketch).unwrap().clone();
    p.timeline.retain(|node| node.id != sketch);
    p.timeline.push(sketch_node);
    let at = p.timeline.iter().position(|n| n.id == sketch).unwrap();
    let node = p.timeline.remove(at);
    p.timeline.insert(0, node);
    let path = scratch("r1-reordered-projection.qcad");
    let (_, breps) = exact_state(&s);
    qymcad_io::save_project_guarded_with_brep(&p, path.to_str().unwrap(), &breps).unwrap();
    assert_rejected(Session::open(&path).map(|_| ()), "body must be built above the sketch");
}

#[test]
fn r1_2_placed_cross_part_projection_is_refused_atomically() {
    let mut s = Session::new_part();
    let body = imported(&mut s);
    let mut p = s.project().clone();
    p.move_component(p.body_owner(body).unwrap(), [100.0, 0.0, 0.0]);
    let path = scratch("r1-placed-import.qcad");
    let (_, breps) = exact_state(&s);
    qymcad_io::save_project_guarded_with_brep(&p, path.to_str().unwrap(), &breps).unwrap();
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    let obj = scratch("r1-placed-import.obj");
    s.export(ExportFormat::Obj, &obj, Quality::Standard, None).unwrap();
    let vertices: Vec<f64> = std::fs::read_to_string(obj)
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("v "))
        .map(|line| line.split_whitespace().next().unwrap().parse::<f64>().unwrap())
        .collect();
    assert_close(vertices.iter().copied().fold(f64::INFINITY, f64::min), 95.0, 0.001, "placed source min x");
    assert_close(vertices.iter().copied().fold(f64::NEG_INFINITY, f64::max), 105.0, 0.001, "placed source max x");
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let edges = top_edges(&mut s, body, 5.0);
    assert_eq!(edges.len(), 4);
    let before = exact_state(&s);
    let error = s.sketch_project(sketch, body, &edges).unwrap_err().to_string();
    assert!(error.contains("project only bodies of the sketch's own part"), "{error}");
    assert_eq!(exact_state(&s), before);
}

#[test]
fn r1_4_imported_same_part_union_is_508_and_restores_active_part() {
    let mut s = Session::new_part();
    let active = s.project().active_component;
    let a = imported(&mut s);
    let face = s.topology(Some(a), false).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    let sketch = s.sketch_create(&PlaneRef::Face { body: a, face }, None).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    let b = extrude(&mut s, sketch, 2.0);
    assert_eq!(s.project().body_owner(a), s.project().body_owner(b));
    assert_ne!(s.project().body_owner(a), active);
    let result = s.body_boolean(&options(a, b, BooleanOp::Union, "Joined")).unwrap();
    assert_close(result.pieces[0].volume, 508.0, 1e-5, "import + 2x2x2 boss");
    assert_eq!(s.project().body_owner(result.body), s.project().body_owner(a));
    assert_eq!(s.project().active_component, active);
    let fresh = box_body(&mut s, 1.0.into(), 1.0, 1.0, 20.0);
    assert_eq!(s.project().body_owner(fresh), active);
}

#[test]
fn r1_5_default_cut_piece_names_use_first_available_suffixes() {
    for occupied in [false, true] {
        let mut s = Session::new_part();
        if occupied {
            s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("Cut 1")).unwrap();
        }
        let a = box_body(&mut s, 30.0.into(), 10.0, 20.0, 0.0);
        let b = box_body(&mut s, 2.0.into(), 12.0, 20.0, 0.0);
        let result = s.body_boolean(&options(a, b, BooleanOp::Cut, "Cut")).unwrap();
        let names: Vec<_> = result.pieces.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, if occupied { vec!["Cut 2", "Cut 3"] } else { vec!["Cut 1", "Cut 2"] });
        for piece in result.pieces {
            assert_eq!(s.resolve(&piece.name).unwrap(), piece.id);
        }
    }
}

#[test]
fn r1_6_explicit_union_piece_name_is_reported_and_survives_open() {
    let mut s = Session::new_part();
    let a = box_body(&mut s, 10.0.into(), 10.0, 10.0, 0.0);
    let b = box_body(&mut s, 10.0.into(), 10.0, 10.0, 5.0);
    let mut args = options(a, b, BooleanOp::Union, "Unite");
    args.piece_names = Some(vec!["Named union".into()]);
    let result = s.body_boolean(&args).unwrap();
    assert_eq!(result.pieces[0].name, "Named union");
    assert_eq!(s.result_bodies()[0].name, "Named union");
    assert_eq!(s.resolve("Named union").unwrap(), result.body);
    let path = scratch("r1-union-name.qcad");
    s.save(Some(&path)).unwrap();
    let (s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    assert_eq!(s.result_bodies()[0].name, "Named union");
}

#[test]
fn r1_8_native_noop_errors_restore_exact_recipe_and_breps() {
    let mut s = Session::new_part();
    let a = box_body(&mut s, 10.0.into(), 10.0, 20.0, 0.0);
    let b = box_body(&mut s, 10.0.into(), 10.0, 20.0, 30.0);
    for op in [BooleanOp::Cut, BooleanOp::Intersect] {
        let before = exact_state(&s);
        let error = s.body_boolean(&options(a, b, op, "Noop")).unwrap_err();
        assert!(matches!(error, Error::Rebuild(_)), "native rebuild must reject the no-op: {error}");
        assert_eq!(exact_state(&s), before, "exact state after {op:?}");
    }
}
