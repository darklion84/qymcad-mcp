//! R5 owner probes: native Point retention, reference-free holes and settled native saved scopes.
mod common;
use qymcad_core::model::{Constraint, Project};
use qymcad_engine::*;
use serde_json::json;
use std::path::{Path, PathBuf};
fn path(name: &str) -> PathBuf {
    common::scratch(&format!("r5-{:?}-{name}", std::thread::current().id()))
}
fn archive(s: &mut Session, name: &str) -> Vec<(String, Vec<u8>)> {
    let path = path(name);
    s.save_with_options(Some(&path), true).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut bytes).unwrap();
        out.push((f.name().into(), bytes));
    }
    out.sort();
    out
}
fn block(s: &mut Session, x: f64, w: Num, down: Option<Num>) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &x.into(), &15.0.into(), &w, &6.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch: sk,
        profiles: None,
        height: 10.0.into(),
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
fn ref_clone(s: &Session, src: Id, name: &str, expected: f64) -> Session {
    let mut p = s.project().clone();
    // Engine-created projection is necessary for the exact saved-app recipe.
    let edge = p.regen_edges[&src].iter().find(|e| e.a[1] == e.b[1] && (e.a[0] - e.b[0]).abs() > expected - 1.0).unwrap().id;
    drop(p);
    // Reopen to gain a mutable independent engine and avoid touching production code.
    let pre = path(&format!("pre-{name}.qcad")).to_str().unwrap().to_string();
    save_clone(s, &pre);
    let mut ss = Session::open(Path::new(&pre)).unwrap().0;
    let sk = ss.sketch_create(&PlaneRef::Base(BaseName::XY), Some(name)).unwrap();
    let refs = ss.sketch_project(sk, src, &[ProjSource::Edge(edge)]).unwrap().0.points;
    p = ss.project().clone();
    let si = p.sketch_index(sk).unwrap();
    p.sketches[si].constraints.push(Constraint::Distance {
        a: refs[0],
        b: refs[1],
        d: expected,
        off: 0.0,
        expr: String::new(),
        driven: true,
        axis: 1,
        at: None,
    });
    assert!(p.add_named_dim(name.into(), sk, refs));
    p.solve_sketch(si);
    p.mark_sketch_dirty(sk);
    let f = path(&format!("ref-{name}.qcad")).to_str().unwrap().to_string();
    save_project(&ss, &p, &f);
    Session::open(Path::new(&f)).unwrap().0
}
fn save_project(s: &Session, p: &Project, f: &str) {
    let bytes = {
        let _gate = qymcad_kernel::kernel_gate();
        p.timeline
            .iter()
            .flat_map(|n| n.kind.bodies())
            .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
            .collect::<Vec<_>>()
    };
    qymcad_io::save_project_guarded_with_brep(p, f, &bytes).unwrap();
}
fn save_clone(s: &Session, f: &str) {
    save_project(s, s.project(), f)
}

#[test]
fn saved_app_offset_relay_keeps_explicit_point() {
    let mut s = Session::new_part();
    s.param_set("bend", &0.0.into()).unwrap();
    let stock = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(stock, &0.0.into(), &0.0.into(), &140.0.into(), &140.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(serde_json::json!({"sketch":stock,"height":10})).unwrap()).unwrap();
    let plane = s.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), None).unwrap().0;
    let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    let spline = s
        .sketch_spline(
            sk,
            &SplineSpec {
                points: vec![Xy(0.0.into(), 0.0.into()), Xy(10.0.into(), "bend".into()), Xy(20.0.into(), 0.0.into())],
                closed: false,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    s.sketch_offset(
        sk,
        &SketchOffsetSpec {
            curves: SketchCurveSel::Ids(spline.entities),
            distance: 2.0.into(),
            toward: [10.0.into(), 5.0.into()],
            dimension_name: None,
        },
    )
    .unwrap();
    let point = s.sketch_point(sk, &50.0.into(), &60.0.into(), false).unwrap().point;
    let other = s.sketch_point(sk, &"bend-50".into(), &60.0.into(), true).unwrap().point;
    let hole = s
        .hole_by_sketch(&HoleSketch {
            body: None,
            sketch: sk,
            points: None,
            flip: false,
            diameter: 4.0.into(),
            depth: Some(4.0.into()),
            kind: HoleKind::Plain,
            dia2: None,
            depth2: None,
            name: None,
        })
        .unwrap()
        .0;
    let file = path("offset-hole.qcad");
    let path = file.as_path();
    s.save(Some(path)).unwrap();
    let before = s.result_bodies()[0].volume;
    s.param_set("bend", &2.0.into()).unwrap();
    assert!(s.sketch_detail(sk).unwrap().points.iter().any(|p| p.id == point));
    assert!((s.result_bodies()[0].volume - before).abs() < 1e-6, "engine control");
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    p.ensure_document();
    for i in 0..p.bodies.len() {
        if let (Some(body), false) = (p.mesh_id(i), p.bodies[i].faces.is_empty()) {
            p.regen_faces.insert(body, p.bodies[i].faces.clone());
        }
    }
    let si = p.sketch_index(sk).unwrap();
    eprintln!("point present immediately after native load {}", p.sketches[si].points.iter().any(|p| p.id == point));
    let shapes = common::restore_shapes(loaded.breps);
    let seen = p.param_map();
    p.parameters.iter_mut().find(|p| p.name == "bend").unwrap().expr = "2".into();
    p.eval_parameters();
    for si in 0..p.sketches.len() {
        if p.sketch_reads_names(si) {
            p.solve_sketch(si);
        }
    }
    eprintln!("point present after apply_param_edit solve {}", p.sketches[si].points.iter().any(|p| p.id == point));
    let vars = p.param_map();
    let changed: Vec<_> =
        vars.iter().filter(|(k, v)| seen.get(*k).is_none_or(|old| (old - **v).abs() > 1e-12)).map(|(k, _)| k.clone()).collect();
    for name in changed {
        p.mark_param_dependents_dirty_for(&name);
    }
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, shapes);
    assert!(report.errors.is_empty(), "native saved Point path: {:?}", report.errors);
    let v = shapes[&hole].volume();
    eprintln!(
        "native report {:?}, dimensioned Point {other} remains {}",
        report.errors,
        p.sketches[si].points.iter().any(|p| p.id == other)
    );
    eprintln!(
        "global-coordinate driven Point {} now at {:?}",
        other,
        p.sketches[si].points.iter().find(|p| p.id == other).map(|p| (p.x, p.y))
    );
    let si = p.sketch_index(sk).unwrap();
    eprintln!(
        "hole {hole}, point {point}, before volume {before}, app volume {v}, point present {}",
        p.sketches[si].points.iter().any(|p| p.id == point)
    );
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(id, shape)| (*id, shape.to_brep_bytes().unwrap())).collect::<Vec<_>>()
    };
    let path_after = common::scratch(&format!("r5-{:?}-offset-hole-after-app.qcad", std::thread::current().id()));
    qymcad_io::save_project_guarded_with_brep(&p, path_after.to_str().unwrap(), &breps).unwrap();
    let (opened, report) = Session::open(&path_after).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    eprintln!(
        "engine reopen errors {:?}, volume {}, marker remains {}",
        report.errors,
        opened.result_bodies()[0].volume,
        opened.sketch_detail(sk).unwrap().points.iter().any(|p| p.id == point)
    );
    assert!(p.sketches[si].points.iter().any(|p| p.id == point), "explicit Point disappeared on saved native app parameter edit");
    let moved = p.sketches[si].points.iter().find(|p| p.id == other).unwrap();
    common::assert_close(moved.x, -48., 1e-6, "native driving Point X");
    common::assert_close(moved.y, 60., 1e-6, "native driving Point Y");
    assert!((v - before).abs() < 1e-6);
}

fn coordinate(s: &Session, sketch: Id, point: Id) -> DimensionTarget {
    let c = s
        .sketch_detail(sketch)
        .unwrap()
        .constraints
        .into_iter()
        .find(|c| c.kind == "distance_point_line" && c.points.contains(&point))
        .unwrap()
        .index;
    DimensionTarget::Sketch { sketch, constraint: c }
}
fn reference_fixture(reorder: bool, reference_source: bool, dependent: bool) -> (Session, Id, Id, Id, Id) {
    let initial = path(if reorder { "source-chain.qcad" } else { "source-internal.qcad" });
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let stocksk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(stocksk, &20.0.into(), &15.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    let stock = s.extrude(&serde_json::from_value(json!({"sketch":stocksk,"height":10})).unwrap()).unwrap().0;
    let source = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("earlier reference source")).unwrap();
    let sourcept = s.sketch_point(source, &10.0.into(), &6.0.into(), true).unwrap().point;
    if reorder {
        s.sketch_line(
            source,
            &LineSpec { x1: 50.0.into(), y1: 50.0.into(), x2: 60.0.into(), y2: 50.0.into(), construction: true, dimensioned: true },
        )
        .unwrap();
    }
    let measuring = if reorder { s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("later measuring source")).unwrap() } else { source };
    s.sketch_rect(measuring, &20.0.into(), &15.0.into(), &"w".into(), &30.0.into(), false).unwrap();
    let mut p = s.project().clone();
    let si = p.sketch_index(source).unwrap();
    let mi = p.sketch_index(measuring).unwrap();
    let pts = p.sketches[mi].points.clone();
    let system = p.sketches[mi].system_ids();
    let a = pts.iter().find(|p| !system.contains(&p.id) && p.x.abs() < 1e-6 && p.y.abs() < 1e-6).unwrap().id;
    let b = pts.iter().find(|p| (p.x - 40.).abs() < 1e-6 && (p.y - 30.).abs() < 1e-6).unwrap().id;
    p.sketches[mi].constraints.push(Constraint::Distance { a, b, d: 40., off: 0., expr: String::new(), driven: true, axis: 1, at: None });
    assert!(p.add_named_dim("measured_w".into(), measuring, vec![a, b]));
    if reference_source {
        let o = p.sketches[si].origin;
        p.sketches[si].constraints.push(Constraint::Distance {
            a: o,
            b: sourcept,
            d: 10.,
            off: 0.,
            expr: String::new(),
            driven: true,
            axis: 1,
            at: None,
        });
        assert!(p.add_named_dim("source_x".into(), source, vec![o, sourcept]));
    } else {
        let ci = p.sketches[si].constraints.iter().position(|c| matches!(c, Constraint::DistancePL { p, .. } if *p == sourcept)).unwrap();
        let refs = Project::dim_refs(&p.sketches[si].constraints[ci]).unwrap();
        assert!(p.add_named_dim("source_x".into(), source, refs));
    }
    p.solve_sketch(si);
    save_project(&s, &p, initial.to_str().unwrap());
    let (mut s, r) = Session::open(&initial).unwrap();
    assert!(r.errors.is_empty());
    let c = coordinate(&s, source, sourcept);
    if dependent {
        s.dimension_set(&c, Some(&"measured_w/4".into()), None).unwrap();
    }
    let marker = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("marker")).unwrap();
    s.sketch_point(marker, &"source_x".into(), &6.0.into(), true).unwrap();
    (s, stock, source, sourcept, marker)
}
fn hole_spec(stock: Id, marker: Id) -> HoleSketch {
    HoleSketch {
        body: Some(stock),
        sketch: marker,
        points: None,
        flip: true,
        diameter: 4.0.into(),
        depth: Some(4.0.into()),
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: None,
    }
}
fn reference_error(error: impl std::fmt::Display) {
    let error = error.to_string();
    assert!(error.contains("reference measurement") && error.contains("measured_w"), "{error}");
}
fn run_earlier_source_probe(reorder: bool) {
    let (mut s, stock, _, _, marker) = reference_fixture(reorder, true, true);
    let before = archive(&mut s, "before-reference-refusal.qcad");
    reference_error(s.hole_by_sketch(&hole_spec(stock, marker)).expect_err("reference-dependent marker must refuse"));
    assert_eq!(archive(&mut s, "after-reference-refusal.qcad"), before);
}
#[test]
fn earlier_source_internal_reference_order() {
    run_earlier_source_probe(false)
}
#[test]
fn intermediate_reads_later_source_before_marker() {
    run_earlier_source_probe(true)
}

#[test]
fn driving_named_point_cannot_hide_transitive_reference() {
    let (mut s, stock, _, _, marker) = reference_fixture(false, false, true);
    let before = archive(&mut s, "driving-reference-before.qcad");
    reference_error(s.hole_by_sketch(&hole_spec(stock, marker)).unwrap_err());
    assert_eq!(archive(&mut s, "driving-reference-after.qcad"), before);
}
#[test]
fn edits_cannot_introduce_references_through_earlier_driving_point() {
    for alias in [false, true] {
        let (mut s, stock, source, pt, marker) = reference_fixture(false, false, false);
        s.param_set("alias1", &10.0.into()).unwrap();
        s.param_set("alias2", &"alias1".into()).unwrap();
        let c = coordinate(&s, source, pt);
        if alias {
            s.dimension_set(&c, Some(&"alias2".into()), None).unwrap();
        }
        s.hole_by_sketch(&hole_spec(stock, marker)).unwrap();
        let before = archive(&mut s, "transitive-edit-before.qcad");
        let result = if alias {
            s.param_set("alias1", &"measured_w/4".into()).map(|_| ())
        } else {
            s.dimension_set(&c, Some(&"measured_w/4".into()), None).map(|_| ())
        };
        reference_error(result.unwrap_err());
        assert_eq!(archive(&mut s, "transitive-edit-after.qcad"), before);
    }
}
#[test]
fn sketch_edit_cannot_introduce_transitive_reference() {
    let (mut s, stock, source, _, marker) = reference_fixture(false, false, false);
    s.hole_by_sketch(&hole_spec(stock, marker)).unwrap();
    let before = archive(&mut s, "sketch-reference-before.qcad");
    reference_error(s.sketch_edit(source, |s| s.sketch_point(source, &"measured_w/4".into(), &8.0.into(), true)).unwrap_err());
    assert_eq!(archive(&mut s, "sketch-reference-after.qcad"), before);
}
#[test]
fn open_refuses_transitive_reference_without_rewriting() {
    let (mut s, stock, source, pt, marker) = reference_fixture(false, false, false);
    s.hole_by_sketch(&hole_spec(stock, marker)).unwrap();
    let mut p = s.project().clone();
    let DimensionTarget::Sketch { constraint, .. } = coordinate(&s, source, pt) else { unreachable!() };
    let si = p.sketch_index(source).unwrap();
    *p.sketches[si].constraints[constraint].expr_mut().unwrap() = "measured_w/4".into();
    let file = path("transitive-reference-open.qcad");
    save_project(&s, &p, file.to_str().unwrap());
    let before = std::fs::read(&file).unwrap();
    reference_error(match Session::open(&file) {
        Ok(_) => panic!("accepted transitive reference"),
        Err(e) => e,
    });
    assert_eq!(std::fs::read(file).unwrap(), before);
}
#[test]
fn native_driving_sources_work_in_both_sketch_array_orders() {
    let (mut s, stock, source, pt, marker) = reference_fixture(false, false, false);
    s.dimension_set(&coordinate(&s, source, pt), Some(&"w/4".into()), None).unwrap();
    let hole = s.hole_by_sketch(&hole_spec(stock, marker)).unwrap().0;
    let file = path("earlier-driving-source.qcad");
    s.save(Some(&file)).unwrap();
    let (_, _, faces, p) = common::gui_edit_param_model(&file, "w", "60");
    let bore = |p: &Project, faces: &[qymcad_core::geom::MeshFace]| {
        faces
            .iter()
            .find(|f| p.names.get(f.id).is_some_and(|n| n.feature == hole && n.role == qymcad_core::names::Role::Hole))
            .unwrap()
            .centroid
            .x
    };
    common::assert_close(bore(&p, &faces), 15., 1e-6, "native earlier driving named Point X15");
    let mut p = s.project().clone();
    let (a, b) = (p.sketch_index(source).unwrap(), p.sketch_index(marker).unwrap());
    p.sketches.swap(a, b);
    let file = path("later-driving-source.qcad");
    save_project(&s, &p, file.to_str().unwrap());
    let (_, _, faces, native) = common::gui_edit_param_model(&file, "w", "60");
    common::assert_close(bore(&native, &faces), 15., 1e-6, "native later driving named Point X15");
    let before = std::fs::read(&file).unwrap();
    let (mut opened, report) = Session::open(&file).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    opened.param_set("w", &60.0.into()).unwrap();
    common::assert_close(bore(opened.project(), &opened.project().regen_faces[&hole]), 15., 1e-6, "engine later driving named Point X15");
    assert_eq!(std::fs::read(file).unwrap(), before);
}

#[test]
fn open_settles_valid_mixed_scope_saved_by_native() {
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let a = block(&mut s, 20.0, "w".into(), None);
    let mut s = ref_clone(&s, a, "first_w", 40.0);
    let b = block(&mut s, 200.0, "first_w".into(), None);
    let mut s = ref_clone(&s, b, "second_w", 40.0);
    let c = block(&mut s, 100.0, 10.0.into(), Some("second_w-first_w+5".into()));
    // Save/load with native B-reps as in common::gui_edit_param_model; no engine propagation.
    let before_file = path("native-mixed-before.qcad");
    save_clone(&s, before_file.to_str().unwrap());
    let qymcad_io::LoadedProject { project: mut p, breps } = qymcad_io::load_project_with_brep(before_file.to_str().unwrap()).unwrap();
    p.ensure_document();
    for i in 0..p.bodies.len() {
        if let (Some(body), false) = (p.mesh_id(i), p.bodies[i].faces.is_empty()) {
            p.regen_faces.insert(body, p.bodies[i].faces.clone());
        }
    }
    let shapes = common::restore_shapes(breps);
    let seen = p.param_map();
    p.parameters.iter_mut().find(|p| p.name == "w").unwrap().expr = "60".into();
    p.eval_parameters();
    for si in 0..p.sketches.len() {
        if p.sketch_reads_names(si) {
            p.solve_sketch(si);
        }
    }
    let vars = p.param_map();
    for (name, value) in &vars {
        if seen.get(name).is_none_or(|old| (old - value).abs() > 1e-12) {
            p.mark_param_dependents_dirty_for(name);
        }
    }
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, shapes);
    assert!(r.errors.is_empty(), "native pass: {:?}", r.errors);
    eprintln!("saved mixed scope {:?}; zmin {}", p.param_map(), shapes[&c].bbox().unwrap()[2]);
    assert_eq!(p.param_map()["first_w"], 60.0);
    assert_eq!(p.param_map()["second_w"], 40.0);
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(&id, sh)| (id, sh.to_brep_bytes().unwrap())).collect::<Vec<_>>()
    };
    let file = path("native-mixed.qcad");
    let f = file.to_str().unwrap();
    for (&id, faces) in &p.regen_faces.clone() {
        p.set_body_faces(id, faces.clone());
    }
    qymcad_io::save_project_guarded_with_brep(&p, f, &breps).unwrap();
    eprintln!("saved dirty nodes {:?}", p.timeline.iter().filter(|n| n.dirty).map(|n| n.id).collect::<Vec<_>>());
    // Independent native repeated-regeneration control: same recipe has a valid fixed point.
    let mut control = p.clone();
    let mut control_shapes = shapes;
    for pass in 0..3 {
        control.eval_parameters();
        for si in 0..control.sketches.len() {
            if control.sketch_reads_names(si) {
                control.solve_sketch(si);
            }
        }
        control.mark_all_dirty();
        let (report, next) = qymcad_testkit::regenerate_dirty_with_shapes(&mut control, control_shapes);
        control_shapes = next;
        assert!(report.errors.is_empty(), "native settling control: {:?}", report.errors);
        eprintln!("control pass {pass} {:?}; zmin {}", control.param_map(), control_shapes[&c].bbox().unwrap()[2]);
    }
    assert_eq!(control.param_map()["first_w"], 60.0);
    assert_eq!(control.param_map()["second_w"], 60.0);
    assert!((control_shapes[&c].bbox().unwrap()[2] + 5.0).abs() < 1e-6);
    let bytes_before = std::fs::read(f).unwrap();
    let result = Session::open(Path::new(f));
    assert_eq!(std::fs::read(f).unwrap(), bytes_before, "open must leave source archive untouched");
    match result {
        Ok((opened, report)) => {
            eprintln!("opened scope {:?}, errors {:?}", opened.project().param_map(), report.errors);
            assert!(report.errors.is_empty());
            assert_eq!(opened.project().param_map()["second_w"], 60.0);
            assert!((opened.shape(c).unwrap().bbox().unwrap()[2] + 5.0).abs() < 1e-6);
        }
        Err(e) => panic!("valid acyclic saved native scope should settle to both references60/down5: {e}"),
    }
}

#[test]
fn native_retention_keeps_free_point_dof_geometry_and_isolation() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let mut expected = Vec::new();
    for (x, y) in [(0., 0.), (-8., 6.), (8., -6.)] {
        let pt = s.sketch_point(sk, &x.into(), &y.into(), false).unwrap().point;
        expected.push((pt, x, y));
    }
    let info = s.sketch_info(sk).unwrap();
    assert_eq!(info.dof, (6, 0));
    assert!(info.isolated_points.iter().all(|p| !p.dimensioned));
    let si = s.project().sketch_index(sk).unwrap();
    assert!(s.project().sketches[si].entities.is_empty());
    assert!(s.project().named_dims.is_empty());
    let file = path("native-retention.qcad");
    s.save_with_options(Some(&file), true).unwrap();
    let mut p = qymcad_io::load_project(file.to_str().unwrap()).unwrap();
    let si = p.sketch_index(sk).unwrap();
    let before = serde_json::to_value(&p).unwrap();
    p.prune_orphan_sketch_points(si);
    p.prune_orphan_sketch_points(si);
    assert_eq!(serde_json::to_value(&p).unwrap(), before, "native cleanup is idempotent");
    p.solve_sketch(si);
    assert_eq!(p.sketch_dof(si), (6, 0));
    for (pt, x, y) in expected {
        let pos = p.sketches[si].points.iter().find(|p| p.id == pt).unwrap();
        common::assert_close(pos.x, x, 1e-9, "native free X");
        common::assert_close(pos.y, y, 1e-9, "native free Y");
        assert!(p.sketch_hole_point_ids(sk).contains(&pt));
        assert!(p.sketches[si]
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::Distance {b, driven:true,expr,..} if *b==pt && expr.is_empty())));
    }
}

// Native point-backed item names with no own formula are geometric measurements too
// (item_values.rs), even though their target is Feature rather than a driven constraint.
fn item_measurement_fixture(key: &str) -> (Session, Id, Id, Id, Id) {
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &20.0.into(), &15.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    let stock = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":10})).unwrap()).unwrap().0;
    let source = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("ellipse measurement source")).unwrap();
    let added = s
        .sketch_ellipse(
            source,
            &EllipseSpec {
                cx: 100.0.into(),
                cy: 100.0.into(),
                rx: "w/2".into(),
                ry: 5.0.into(),
                turn: 0.0.into(),
                construction: true,
                dimensioned: true,
            },
        )
        .unwrap();
    let mut p = s.project().clone();
    let center = added.points[0];
    assert!(p.feat_dim(center, key).is_none());
    assert!(p.add_named_feat_dim("item_measurement".into(), center, key));
    let file = path("item-reference.qcad");
    save_project(&s, &p, file.to_str().unwrap());
    let (mut s, r) = Session::open(&file).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    s.param_set("site_x", &10.0.into()).unwrap();
    let marker = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let point = s.sketch_point(marker, &"site_x".into(), &6.0.into(), true).unwrap().point;
    (s, stock, marker, point, center)
}
fn item_reference_error(e: impl std::fmt::Display) {
    let e = e.to_string();
    assert!(e.contains("reference measurement item_measurement"), "{e}");
}
#[test]
fn point_backed_item_measurements_refuse_hole_creation() {
    for key in ["w", "h", "turn"] {
        let (mut s, stock, marker, _, _) = item_measurement_fixture(key);
        s.param_set("site_x", &"10+item_measurement/10".into()).unwrap();
        let before = archive(&mut s, "item-create-before.qcad");
        item_reference_error(s.hole_by_sketch(&hole_spec(stock, marker)).unwrap_err());
        assert_eq!(archive(&mut s, "item-create-after.qcad"), before);
    }
}
#[test]
fn point_backed_item_measurement_edit_refuses_atomically() {
    let (mut s, stock, marker, _, _) = item_measurement_fixture("w");
    s.hole_by_sketch(&hole_spec(stock, marker)).unwrap();
    let before = archive(&mut s, "item-edit-before.qcad");
    item_reference_error(s.param_set("site_x", &"10+item_measurement/10".into()).unwrap_err());
    assert_eq!(archive(&mut s, "item-edit-after.qcad"), before);
}
#[test]
fn point_backed_item_measurement_open_refuses_without_rewriting() {
    let (mut s, stock, marker, point, _) = item_measurement_fixture("w");
    s.hole_by_sketch(&hole_spec(stock, marker)).unwrap();
    let mut p = s.project().clone();
    let DimensionTarget::Sketch { constraint, .. } = coordinate(&s, marker, point) else { unreachable!() };
    let si = p.sketch_index(marker).unwrap();
    *p.sketches[si].constraints[constraint].expr_mut().unwrap() = "10+item_measurement/10".into();
    let file = path("item-reference-open.qcad");
    save_project(&s, &p, file.to_str().unwrap());
    let before = std::fs::read(&file).unwrap();
    item_reference_error(match Session::open(&file) {
        Ok(_) => panic!("accepted item reference"),
        Err(e) => e,
    });
    assert_eq!(std::fs::read(file).unwrap(), before);
}

#[test]
fn explicitly_driving_item_formula_keeps_native_named_hole_supported() {
    let (mut s, stock, marker, point, center) = item_measurement_fixture("w");
    s.dimension_set(&DimensionTarget::Feature { feature: center, key: "w".into() }, Some(&"w".into()), None).unwrap();
    s.param_set("site_x", &"10+item_measurement/10".into()).unwrap();
    let hole = s.hole_by_sketch(&hole_spec(stock, marker)).unwrap().0;
    let file = path("driving-item.qcad");
    s.save(Some(&file)).unwrap();
    let (_, _, faces, p) = common::gui_edit_param_model(&file, "w", "60");
    let face = faces
        .iter()
        .find(|f| p.names.get(f.id).is_some_and(|n| n.feature == hole && n.src == point && n.role == qymcad_core::names::Role::Hole))
        .unwrap();
    common::assert_close(face.centroid.x, 16., 1e-6, "native explicit item formula X16");
}

#[test]
fn direct_point_edit_cannot_introduce_transitive_reference() {
    let (mut s, stock, source, _, marker) = reference_fixture(false, false, false);
    s.hole_by_sketch(&hole_spec(stock, marker)).unwrap();
    let before = archive(&mut s, "direct-point-reference-before.qcad");
    reference_error(s.sketch_point(source, &"measured_w/4".into(), &8.0.into(), true).unwrap_err());
    assert_eq!(archive(&mut s, "direct-point-reference-after.qcad"), before);
}

fn bound_datum_fixture(manual: bool) -> (Session, Id, Id, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &20.0.into(), &15.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    let stock = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":10})).unwrap()).unwrap().0;
    let edge = s.project().regen_edges[&stock].iter().find(|e| (e.a[0] - e.b[0]).abs() > 39.).unwrap();
    let end = edge.b[0] > edge.a[0];
    let at = if end { edge.b } else { edge.a };
    let mut p = s.project().clone();
    let datum = if manual { p.add_point_at(at) } else { p.add_point_at_vertex(at, stock, edge.id, end) };
    assert!(p.add_named_feat_dim("datum_x".into(), datum, "x"));
    let file = path("bound-datum.qcad");
    save_project(&s, &p, file.to_str().unwrap());
    let (mut s, r) = Session::open(&file).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    s.param_set("site_x", &10.0.into()).unwrap();
    let marker = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let point = s.sketch_point(marker, &"site_x".into(), &6.0.into(), true).unwrap().point;
    (s, stock, marker, point)
}
#[test]
fn bound_datum_coordinate_measurement_refuses_create_edit_and_open() {
    let (mut s, stock, marker, point) = bound_datum_fixture(false);
    s.param_set("site_x", &"datum_x/4".into()).unwrap();
    let before = archive(&mut s, "bound-create-before.qcad");
    let error = s.hole_by_sketch(&hole_spec(stock, marker)).unwrap_err().to_string();
    assert!(error.contains("reference measurement datum_x"), "{error}");
    assert_eq!(archive(&mut s, "bound-create-after.qcad"), before);
    s.param_set("site_x", &10.0.into()).unwrap();
    s.hole_by_sketch(&hole_spec(stock, marker)).unwrap();
    let before = archive(&mut s, "bound-edit-before.qcad");
    let error = s.param_set("site_x", &"datum_x/4".into()).unwrap_err().to_string();
    assert!(error.contains("reference measurement datum_x"), "{error}");
    assert_eq!(archive(&mut s, "bound-edit-after.qcad"), before);
    let mut p = s.project().clone();
    let DimensionTarget::Sketch { constraint, .. } = coordinate(&s, marker, point) else { unreachable!() };
    let si = p.sketch_index(marker).unwrap();
    *p.sketches[si].constraints[constraint].expr_mut().unwrap() = "datum_x/4".into();
    let file = path("bound-open.qcad");
    save_project(&s, &p, file.to_str().unwrap());
    let before = std::fs::read(&file).unwrap();
    let error = match Session::open(&file) {
        Ok(_) => panic!("accepted bound-datum reading"),
        Err(e) => e.to_string(),
    };
    assert!(error.contains("reference measurement datum_x"), "{error}");
    assert_eq!(std::fs::read(file).unwrap(), before);
}
#[test]
fn manual_datum_coordinate_remains_a_driving_source() {
    let (mut s, stock, marker, _) = bound_datum_fixture(true);
    s.param_set("site_x", &"datum_x/4".into()).unwrap();
    let (_, r) = s.hole_by_sketch(&hole_spec(stock, marker)).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
}
