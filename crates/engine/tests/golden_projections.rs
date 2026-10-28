//! Projection-only dependencies: A drives a pocket in independent stock B.
mod common;
use common::*;
use qymcad_core::{
    feature::{Extent, FeatureKind, Purpose, Reach},
    geom::Point2,
    model::{CombineSpan, Constraint, ProjSource, Project},
};
use qymcad_engine::*;
use qymcad_kernel::{OcctKernel, Shape};
use std::{cell::RefCell, collections::HashMap};

struct Fixture {
    s: Session,
    source: Id,
    base: Id,
    sketch: Id,
    cut: Id,
    fail: Id,
    width: usize,
    ends: [Id; 2],
    centers: Vec<Id>,
}

fn bytes(s: &Session) -> Vec<(Id, Vec<u8>)> {
    let _gate = qymcad_kernel::kernel_gate();
    s.project()
        .timeline
        .iter()
        .flat_map(|n| n.kind.bodies())
        .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
        .collect()
}

fn distance(a: Id, b: Id, d: f64, axis: u8) -> Constraint {
    Constraint::Distance { a, b, d, off: 0.0, expr: String::new(), driven: false, axis, at: None }
}

fn fixture(label: &str, broken: bool) -> Fixture {
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let base = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("source profile")).unwrap();
    s.sketch_rect(base, &0.0.into(), &0.0.into(), &Num::Expr("w".into()), &30.0.into(), false).unwrap();
    let (source, _) = s
        .extrude(&Extrude {
            sketch: base,
            profiles: None,
            height: 10.0.into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("A".into()),
        })
        .unwrap();
    let stock_sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("independent stock")).unwrap();
    s.sketch_rect(stock_sketch, &0.0.into(), &0.0.into(), &80.0.into(), &30.0.into(), false).unwrap();
    let (stock, _) = s
        .extrude(&Extrude {
            sketch: stock_sketch,
            profiles: None,
            height: 10.0.into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("B".into()),
        })
        .unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), None).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Plane(plane), Some("projected pocket")).unwrap();
    let edge = s.project().regen_edges[&source]
        .iter()
        .find(|e| {
            (e.a[0] - 20.0).abs() < 1e-6 && (e.b[0] - 20.0).abs() < 1e-6 && (e.a[2] - 10.0).abs() < 1e-6 && (e.b[2] - 10.0).abs() < 1e-6
        })
        .expect("top edge along Y")
        .id;
    let projection = s.sketch_project(sketch, source, &[ProjSource::Edge(edge)]).unwrap().0;
    assert_eq!(projection.entities.len(), 1);
    let mut p = s.project().clone();
    let si = p.sketch_index(sketch).unwrap();
    let kernel = OcctKernel { shapes: RefCell::new(restore_shapes(bytes(&s))), ..Default::default() };
    let corner = *p.sketches[si].projections[0]
        .points
        .iter()
        .filter_map(|id| p.sketches[si].points.iter().find(|q| q.id == *id))
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    for e in &mut p.sketches[si].entities {
        e.construction = true;
    }
    let start = p.sketches[si].entities.len();
    p.add_slot_entity(si, Point2::new(corner.x - 3.0, corner.y + 5.0), Point2::new(corner.x - 3.0, corner.y + 15.0), 1.0, Purpose::Real);
    let centers: Vec<_> = p.sketches[si].entities[start..].iter().filter_map(|e| e.kind.as_arc().map(|a| a.center)).collect();
    assert_eq!(centers.len(), 2);
    for &center in &centers {
        let dy = (p.sketches[si].points.iter().find(|q| q.id == center).unwrap().y - corner.y).abs();
        p.sketches[si].constraints.push(distance(corner.id, center, 3.0, 1));
        p.sketches[si].constraints.push(distance(corner.id, center, dy, 2));
    }
    p.sketches[si].constraints.push(Constraint::Diameter {
        c: centers[0],
        d: 1.0,
        off: 0.0,
        expr: String::new(),
        driven: false,
        diam: false,
        at: None,
    });
    assert!(p.solve_sketch(si) <= 1e-6);
    assert_eq!(p.sketch_dof(si), (0, 0));
    let profile = p.sketches[si].contour_ids[0];
    let cut = p.add_combine_on(stock, sketch, profile, CombineSpan { height: 2.0, down: 2.0, extent: Extent::default(), fill: &[] }, 0);
    // Valid cached downstream feature, later poisoned without dirtying it: only projection movement reaches it.
    let fail = p.add_extrude_on(sketch, profile, 1.0, Reach::Forward, 0.0);
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, kernel.shapes.into_inner());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    for (id, faces) in report.built {
        p.set_body_faces(id, faces);
    }
    assert_close(shapes[&cut].volume(), 24000.0 - 2.0 * (20.0 + std::f64::consts::PI), 1e-6, "pocket volume");
    if broken {
        if let FeatureKind::Extrude { height, .. } = &mut p.timeline.iter_mut().find(|n| n.id == fail).unwrap().kind {
            *height = 0.0;
        }
    }
    let base_si = p.sketch_index(base).unwrap();
    let width = p.sketches[base_si].constraints.iter().position(|c| c.expr() == Some("w")).unwrap();
    let ends = match p.sketches[base_si].constraints[width] {
        Constraint::Distance { a, b, .. } => [a, b],
        _ => panic!("width"),
    };
    let path = scratch(&format!("projection-{label}.qcad"));
    save(&p, &shapes, &path);
    let (s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    Fixture { s, source, base, sketch, cut, fail, width, ends, centers }
}

fn save(p: &Project, shapes: &HashMap<Id, Shape>, path: &std::path::Path) {
    let breps: Vec<_> = {
        let _gate = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(&id, sh)| (id, sh.to_brep_bytes().unwrap())).collect()
    };
    qymcad_io::save_project_guarded_with_brep(p, path.to_str().unwrap(), &breps).unwrap();
}

fn assert_pocket(faces: &[qymcad_core::geom::MeshFace], x: f64) {
    let floor = faces.iter().find(|f| (f.centroid.z - 8.0).abs() < 1e-6).expect("pocket floor");
    assert_close(floor.centroid.x, x, 1e-5, "pocket moved with projected edge");
}

#[test]
fn engine_projection_api_projects_face_outline_and_is_idempotent() {
    use qymcad_core::model::ProjSource;
    let mut s = Session::new_part();
    let profile = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(profile, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    let (body, _) = s
        .extrude(&Extrude {
            sketch: profile,
            profiles: None,
            height: 10.0.into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    let top = s.topology(Some(body), false).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    let face_sketch = s.sketch_create(&PlaneRef::Face { body, face: top }, None).unwrap();
    let result = s.sketch_project(face_sketch, body, &[ProjSource::Face(top)]).unwrap().0;
    assert_eq!(result.projections.len(), 1);
    assert_eq!(result.entities.len(), 4);
    let detail = s.sketch_detail(face_sketch).unwrap();
    let mut lengths: Vec<f64> = detail
        .entities
        .iter()
        .filter(|e| result.entities.contains(&e.id))
        .map(|e| {
            let [a, b] = e.points.as_slice() else { panic!("outline edge should be a line") };
            let a = detail.points.iter().find(|p| p.id == *a).unwrap();
            let b = detail.points.iter().find(|p| p.id == *b).unwrap();
            (a.x - b.x).hypot(a.y - b.y)
        })
        .collect();
    lengths.sort_by(f64::total_cmp);
    assert_eq!(lengths, vec![30.0, 30.0, 40.0, 40.0]);
    let before_repeat = serde_json::to_vec(s.project()).unwrap();
    let repeated = s.sketch_project(face_sketch, body, &[ProjSource::Face(top)]).unwrap().0;
    assert_eq!(serde_json::to_vec(s.project()).unwrap(), before_repeat, "duplicate does not change recipe");
    let mut previous_entities = result.entities.clone();
    let mut repeated_entities = repeated.entities.clone();
    previous_entities.sort_unstable();
    repeated_entities.sort_unstable();
    assert_eq!(repeated.projections, result.projections);
    assert_eq!(repeated_entities, previous_entities);
    assert!(detail.entities.iter().filter(|e| result.entities.contains(&e.id)).all(|e| e.driven && e.projection_state == Some("attached")));
    assert!(detail.entities.iter().filter(|e| result.entities.contains(&e.id)).all(|e| e.projection.is_some()));
}

#[test]
fn engine_projection_api_projects_a_circular_edge_at_the_hole_radius() {
    use qymcad_core::model::ProjSource;
    let mut s = Session::new_part();
    let profile = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(profile, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    s.sketch_circle(profile, &0.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
    let (body, _) = s
        .extrude(&Extrude {
            sketch: profile,
            profiles: None,
            height: 10.0.into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    let topology = s.topology(Some(body), false).unwrap();
    let hole = topology
        .edges
        .iter()
        .find(|edge| edge.kind == EdgeKind::Circle && edge.radius.is_some_and(|radius| (radius - 5.0).abs() < 1e-6))
        .unwrap();
    let plane = s.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), None).unwrap().0;
    let sketch = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    let projection = s.sketch_project(sketch, body, &[ProjSource::Edge(hole.id)]).unwrap().0;
    let entity = s.sketch_detail(sketch).unwrap().entities.into_iter().find(|e| projection.entities.contains(&e.id)).unwrap();
    assert_eq!(entity.kind, "circle");
    assert_close(entity.r.unwrap(), 5.0, 1e-6, "projected hole radius");
}

#[test]
fn parameter_moves_projection_and_independent_cut_in_server_and_app() {
    let mut f = fixture("movement", false);
    assert!(!f.s.project().dependents(f.source).contains(&f.cut), "characterize native missing projection dependency");
    let before = f.s.project().sketches.iter().find(|s| s.id == f.sketch).unwrap().clone();
    let path = scratch("projection-app.qcad");
    f.s.save(Some(&path)).unwrap();
    f.s.param_set("w", &60.0.into()).unwrap();
    let after = f.s.project().sketches.iter().find(|s| s.id == f.sketch).unwrap();
    assert_eq!(before.projections[0].points, after.projections[0].points);
    assert_eq!(before.projections[0].entities, after.projections[0].entities);
    for id in &before.projections[0].points {
        let a = before.points.iter().find(|q| q.id == *id).unwrap();
        let b = after.points.iter().find(|q| q.id == *id).unwrap();
        assert_close(b.x - a.x, 10.0, 1e-6, "projected point moves");
    }
    for &id in &f.centers {
        let a = before.points.iter().find(|q| q.id == id).unwrap();
        let b = after.points.iter().find(|q| q.id == id).unwrap();
        assert_close(b.x - a.x, 10.0, 1e-6, "constrained slot follows");
    }
    assert_pocket(&f.s.project().regen_faces[&f.cut], 27.0);
    assert_close(f.s.shape(f.cut).unwrap().volume(), 24000.0 - 2.0 * (20.0 + std::f64::consts::PI), 1e-6, "moved pocket volume");
    // Common app-path helper returns the final body's faces. Remove auxiliary extrusion in saved fixture.
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    p.delete_feature_op(f.fail);
    save(&p, &restore_shapes(loaded.breps), &path);
    let (v, _, faces, app) = gui_edit_param_model(&path, "w", "60");
    let app_sketch = &app.sketches[app.sketch_index(f.sketch).unwrap()];
    assert_eq!(app_sketch.projections[0].entities, before.projections[0].entities);
    for id in &before.projections[0].points {
        let point = app_sketch.points.iter().find(|p| p.id == *id).unwrap();
        assert_close(point.x, 30.0, 1e-6, "app projected edge at w/2");
    }
    for id in &f.centers {
        let point = app_sketch.points.iter().find(|p| p.id == *id).unwrap();
        assert_close(point.x, 27.0, 1e-6, "app slot centre at w/2-3");
    }
    assert_close(v, 24000.0 - 2.0 * (20.0 + std::f64::consts::PI), 1e-6, "app pocket volume");
    assert_pocket(&faces, 27.0);
}

#[test]
fn parameter_failure_below_projection_restores_project_and_every_brep() {
    let mut f = fixture("param-failure", true);
    let before = serde_json::to_vec(f.s.project()).unwrap();
    let shapes = bytes(&f.s);
    let error = f.s.param_set("w", &60.0.into()).unwrap_err();
    assert!(error.to_string().contains(&f.fail.to_string()), "{error}");
    assert!(serde_json::to_vec(f.s.project()).unwrap() == before, "Project changed after refusal");
    assert!(bytes(&f.s) == shapes, "B-rep changed after projection failure");
}

#[test]
fn sketch_failure_below_projection_restores_project_and_every_brep() {
    let mut f = fixture("sketch-failure", true);
    let before = serde_json::to_vec(f.s.project()).unwrap();
    let shapes = bytes(&f.s);
    let error =
        f.s.sketch_edit(f.base, |s| {
            s.sketch_remove_constraint(f.base, f.width)?;
            s.sketch_constrain(
                f.base,
                &ConstrainSpec {
                    kind: ConstraintKind::Distance,
                    refs: f.ends.map(SketchRef::Id).to_vec(),
                    value: Some(60.0.into()),
                    axis: DistAxis::X,
                    reference: false,
                },
            )
        })
        .unwrap_err();
    assert!(error.to_string().contains(&f.fail.to_string()), "{error}");
    assert!(serde_json::to_vec(f.s.project()).unwrap() == before, "Project changed after sketch refusal");
    assert!(bytes(&f.s) == shapes, "B-rep changed after projection sketch failure");
}

#[test]
fn dimension_failure_below_projection_restores_project_and_every_brep() {
    let mut f = fixture("dimension-failure", true);
    let before = serde_json::to_vec(f.s.project()).unwrap();
    let shapes = bytes(&f.s);
    let error = f.s.dimension_set(&DimensionTarget::Sketch { sketch: f.base, constraint: f.width }, Some(&60.0.into()), None).unwrap_err();
    assert!(error.to_string().contains(&f.fail.to_string()), "{error}");
    assert!(serde_json::to_vec(f.s.project()).unwrap() == before, "Project changed after dimension refusal");
    assert!(bytes(&f.s) == shapes, "B-rep changed after projection dimension failure");
}

#[test]
fn deleting_projection_source_is_refused_even_with_cascade() {
    for cascade in [false, true] {
        let mut f = fixture(if cascade { "delete-cascade" } else { "delete" }, false);
        let before = serde_json::to_vec(f.s.project()).unwrap();
        let shapes = bytes(&f.s);
        let error = f.s.feature_delete(f.source, cascade).unwrap_err();
        assert!(error.to_string().contains("projection"), "{error}");
        assert!(error.to_string().contains(&f.sketch.to_string()), "{error}");
        assert!(serde_json::to_vec(f.s.project()).unwrap() == before, "deletion refusal changed Project");
        assert!(bytes(&f.s) == shapes, "deletion refusal changed B-reps");
        f.s.feature_delete(f.sketch, true).unwrap();
        f.s.feature_delete(f.source, cascade).unwrap();
        assert_eq!(f.s.result_bodies().len(), 1, "independent stock survives explicit sketch/source deletion");
        assert_close(f.s.result_bodies()[0].volume, 24000.0, 1e-6, "uncut independent stock");
    }
}

#[test]
fn native_missing_edge_detaches_but_absent_body_retains_projection() {
    let f = fixture("native-delete", false);
    let mut p = f.s.project().clone();
    let si = p.sketch_index(f.sketch).unwrap();
    let geometry = serde_json::to_vec(&p.sketches[si].entities).unwrap();
    let constraints = serde_json::to_vec(&p.sketches[si].constraints).unwrap();
    let mut shapes = restore_shapes(bytes(&f.s));
    p.delete_feature_op(f.source);
    let source_shape = shapes.remove(&f.source).unwrap();
    let kernel = OcctKernel { shapes: RefCell::new(shapes), ..Default::default() };
    {
        let _gate = qymcad_kernel::kernel_gate();
        p.resolve_sketch_projections(si, &kernel);
    }
    assert_eq!(p.sketches[si].projections.len(), 1, "absent B-rep is Unknown, even after source deletion");
    let path = scratch("projection-detached-info.qcad");
    save(&p, &kernel.shapes.borrow(), &path);
    let (session, _) = Session::open(&path).unwrap();
    let unknown = session.sketch_detail(f.sketch).unwrap();
    let projected: Vec<_> = unknown.entities.iter().filter(|e| e.projection.is_some()).collect();
    assert_eq!(projected.len(), 1);
    assert!(projected.iter().all(|e| e.driven && e.projection_state == Some("unknown")));
    kernel.shapes.borrow_mut().insert(f.source, source_shape);
    p.sketches[si].projections[0].src = ProjSource::Edge(u32::MAX);
    {
        let _gate = qymcad_kernel::kernel_gate();
        p.resolve_sketch_projections(si, &kernel);
    }
    assert!(p.sketches[si].projections.is_empty(), "missing edge on a live source detaches");
    assert_eq!(serde_json::to_vec(&p.sketches[si].entities).unwrap(), geometry);
    assert_eq!(serde_json::to_vec(&p.sketches[si].constraints).unwrap(), constraints);
    let detached_path = scratch("projection-detached-info.qcad");
    save(&p, &kernel.shapes.borrow(), &detached_path);
    let (detached_session, _) = Session::open(&detached_path).unwrap();
    let detail = detached_session.sketch_detail(f.sketch).unwrap();
    // Native release deliberately serializes ordinary geometry only. Session provenance cannot
    // be recovered after opening a file already detached outside this Session.
    assert_eq!(detail.entities.len(), p.sketches[si].entities.len());
    assert!(detail.entities.iter().all(|e| !e.driven && e.projection.is_none()));
}

#[test]
fn session_projection_api_refuses_invalid_selection_without_changes() {
    let mut f = fixture("projection-api-invalid", false);
    let before = serde_json::to_vec(f.s.project()).unwrap();
    let missing = f.s.sketch_project(f.sketch, f.source, &[ProjSource::Edge(u32::MAX)]).unwrap_err();
    assert!(missing.to_string().contains("edge"), "{missing}");
    assert_eq!(serde_json::to_vec(f.s.project()).unwrap(), before);
    let (plane, _) = f.s.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), None).unwrap();
    let wrong_plane = f.s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    let top = f.s.topology(Some(f.source), false).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    let wrong_host = f.s.sketch_project(wrong_plane, f.source, &[ProjSource::Face(top)]).unwrap_err();
    assert!(wrong_host.to_string().contains("hosted"), "{wrong_host}");
}

#[test]
fn stored_query_preflight_reaches_projection_only_ancestor() {
    use qymcad_core::refs::{Query, Ref};
    let mut f = fixture("query", false);
    let mut p = f.s.project().clone();
    p.delete_feature_op(f.fail);
    let fillet = p.add_fillet_ref(f.cut, 0.3, Ref::many(Query::Oriented { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }));
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, restore_shapes(bytes(&f.s)));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    for (id, faces) in report.built {
        p.set_body_faces(id, faces);
    }
    let path = scratch("projection-query-unnamed.qcad");
    save(&p, &shapes, &path);
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut breps = loaded.breps;
    native::rename_names(&mut breps.iter_mut().find(|(id, _)| *id == f.source).unwrap().1, &[], true);
    qymcad_io::save_project_guarded_with_brep(&loaded.project, path.to_str().unwrap(), &breps).unwrap();
    f.s = Session::open(&path).unwrap().0;
    let before = serde_json::to_vec(f.s.project()).unwrap();
    let shapes = bytes(&f.s);
    let error = f.s.param_set("w", &60.0.into()).unwrap_err();
    assert!(error.to_string().contains("stored edge query"), "{error}");
    assert!(error.to_string().contains(&fillet.to_string()), "{error}");
    assert!(serde_json::to_vec(f.s.project()).unwrap() == before, "query refusal changed Project");
    assert!(bytes(&f.s) == shapes, "query refusal changed B-reps");
}

#[test]
fn edit_of_consuming_source_above_projection_rolls_back_exactly() {
    let mut f = fixture("lineage", false);
    let mut p = f.s.project().clone();
    let face = p.regen_faces[&f.source].iter().find(|face| face.normal[0] > 0.99).unwrap();
    let key = qymcad_core::feature::FaceKey {
        id: face.id,
        centroid: [face.centroid.x, face.centroid.y, face.centroid.z],
        normal: face.normal,
        index: 0,
    };
    let push = p.add_push_face(f.source, key, 1.0);
    let node = p.timeline.pop().unwrap();
    let at = p.timeline.iter().position(|n| n.id == f.sketch).unwrap();
    p.timeline.insert(at, node);
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, restore_shapes(bytes(&f.s)));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    for (id, faces) in report.built {
        p.set_body_faces(id, faces);
    }
    assert_pocket(&p.regen_faces[&f.cut], 18.0);
    assert!(!p.dependents(push).contains(&f.cut));
    if let FeatureKind::Extrude { height, .. } = &mut p.timeline.iter_mut().find(|n| n.id == f.fail).unwrap().kind {
        *height = 0.0;
    }
    let path = scratch("projection-lineage.qcad");
    save(&p, &shapes, &path);
    f.s = Session::open(&path).unwrap().0;
    let before = serde_json::to_vec(f.s.project()).unwrap();
    let shapes = bytes(&f.s);
    let error = f.s.dimension_set(&DimensionTarget::Feature { feature: push, key: "dist".into() }, Some(&3.0.into()), None).unwrap_err();
    assert!(error.to_string().contains(&f.fail.to_string()), "{error}");
    assert!(serde_json::to_vec(f.s.project()).unwrap() == before, "lineage refusal changed Project");
    assert!(bytes(&f.s) == shapes, "lineage refusal changed B-reps");
}

#[test]
fn projection_rebuild_failure_restores_recipe_shapes_and_inspection() {
    let mut f = fixture("project-rebuild-failure", true);
    let source = f.s.project().sketches[f.s.project().sketch_index(f.sketch).unwrap()].projections[0].src;
    let project = serde_json::to_vec(f.s.project()).unwrap();
    let shapes = bytes(&f.s);
    let detail = f.s.sketch_detail(f.sketch).unwrap();
    let error = f.s.sketch_project(f.sketch, f.source, &[source]).unwrap_err();
    assert!(error.to_string().contains(&f.fail.to_string()), "{error}");
    assert_eq!(serde_json::to_vec(f.s.project()).unwrap(), project);
    assert!(bytes(&f.s) == shapes, "projection refusal changed B-reps");
    assert_eq!(f.s.sketch_detail(f.sketch).unwrap(), detail);
}
