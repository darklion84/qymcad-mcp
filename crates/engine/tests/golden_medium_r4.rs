//! R4 owner review regressions: explicit Points, marker solve order, settled guards and array limits.
mod common;
use qymcad_core::model::{Constraint, Project};
use qymcad_engine::*;
use serde_json::json;
use std::path::{Path, PathBuf};
fn path(name: &str) -> PathBuf {
    common::scratch(&format!("r4-{:?}-{name}", std::thread::current().id()))
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
#[test]
fn deleting_unrelated_line_must_preserve_native_points() {
    let mut deleted = Vec::new();
    for dimensioned in [false, true] {
        let mut s = Session::new_part();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let point = s.sketch_point(sk, &8.0.into(), &6.0.into(), dimensioned).unwrap().point;
        s.sketch_line(
            sk,
            &LineSpec { x1: 20.0.into(), y1: 20.0.into(), x2: 30.0.into(), y2: 20.0.into(), construction: true, dimensioned: true },
        )
        .unwrap();
        let entity = s.project().sketches[s.project().sketch_index(sk).unwrap()].entities[0].id;
        let result = s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, entity));
        eprintln!(
            "dimensioned={dimensioned}, point={point}, remove={result:?}, remaining={:?}",
            s.sketch_info(sk).unwrap().isolated_points
        );
        assert!(result.is_ok());
        if !s.project().sketches[s.project().sketch_index(sk).unwrap()].points.iter().any(|p| p.id == point) {
            deleted.push(dimensioned);
        }
    }
    assert!(deleted.is_empty(), "deleting unrelated construction line erased explicitly-created Points: dimensioned={deleted:?}");
}

#[test]
fn deleting_unrelated_line_must_not_silently_remove_a_bore() {
    let mut s = Session::new_part();
    let stock = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(stock, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":stock,"height":10})).unwrap()).unwrap();
    let plane = s.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), None).unwrap().0;
    let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    let p1 = s.sketch_point(sk, &(-8.0).into(), &6.0.into(), true).unwrap().point;
    let p2 = s.sketch_point(sk, &8.0.into(), &6.0.into(), true).unwrap().point;
    let keep = s
        .sketch_line(
            sk,
            &LineSpec { x1: 25.0.into(), y1: 6.0.into(), x2: 30.0.into(), y2: 6.0.into(), construction: true, dimensioned: true },
        )
        .unwrap();
    s.sketch_constrain(
        sk,
        &ConstrainSpec {
            kind: ConstraintKind::Distance,
            refs: vec![SketchRef::Id(p1), SketchRef::Id(keep.points[0])],
            value: None,
            axis: DistAxis::Aligned,
            reference: true,
        },
    )
    .unwrap();
    s.sketch_line(
        sk,
        &LineSpec { x1: 25.0.into(), y1: 20.0.into(), x2: 30.0.into(), y2: 20.0.into(), construction: true, dimensioned: true },
    )
    .unwrap();
    let entity = s.project().sketches[s.project().sketch_index(sk).unwrap()].entities[1].id;
    let (hole, _) = s
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
        .unwrap();
    let before = s.result_bodies()[0].volume;
    assert!((before - (12000. - 32. * std::f64::consts::PI)).abs() < 1e-5);
    s.save(Some(&path("two-bores-before.qcad"))).unwrap();
    let result = s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, entity));
    let after = s.result_bodies()[0].volume;
    eprintln!(
        "hole={hole}, p1={p1}, p2={p2}, result={result:?}, volume {before} -> {after}, points={:?}",
        s.sketch_info(sk).unwrap().isolated_points
    );
    assert!(result.is_ok());
    s.save(Some(&path("two-bores-after.qcad"))).unwrap();
    let (opened, report) = Session::open(&path("two-bores-after.qcad")).unwrap();
    eprintln!("reopened errors={:?}, volume={}", report.errors, opened.result_bodies()[0].volume);
    assert!(report.errors.is_empty());
    assert!((opened.result_bodies()[0].volume - after).abs() < 1e-5);
    assert!((after - before).abs() < 1e-5, "unrelated line deletion removed a bore");
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
fn transient_guard_before_reference_settling() {
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let a = block(&mut s, 20.0, "w".into(), None);
    let mut s = ref_clone(&s, a, "first_w", 40.0);
    let b = block(&mut s, 200.0, "first_w".into(), None);
    let mut s = ref_clone(&s, b, "second_w", 40.0);
    let c = block(&mut s, 100.0, 10.0.into(), Some("second_w-first_w+5".into()));
    eprintln!("initial scope {:?}", s.project().param_map());
    save_clone(&s, path("transient-before.qcad").to_str().unwrap());
    // Raw kernel control: the acyclic reference chain settles after three passes.
    let mut p = s.project().clone();
    let raw = collect_shapes(&s);
    let mut shapes =
        qymcad_kernel::imports::restore(raw.into_iter().map(|(body, bytes)| qymcad_kernel::imports::SavedBody { body, bytes }).collect())
            .into_iter()
            .map(|r| (r.body, r.shape))
            .collect();
    p.parameters.iter_mut().find(|p| p.name == "w").unwrap().expr = "60".into();
    for pass in 0..3 {
        p.eval_parameters();
        for si in 0..p.sketches.len() {
            if p.sketch_reads_names(si) {
                p.solve_sketch(si);
            }
        }
        p.mark_all_dirty();
        let (r, next) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, shapes);
        shapes = next;
        assert!(r.errors.is_empty(), "native pass {pass}: {:?}", r.errors);
        eprintln!("native pass {pass}: {:?}, zmin {}", p.param_map(), shapes[&c].bbox().unwrap()[2]);
    }
    assert_eq!(p.param_map()["first_w"], 60.0);
    assert_eq!(p.param_map()["second_w"], 60.0);
    assert!((shapes[&c].bbox().unwrap()[2] + 5.0).abs() < 1e-6);
    let before = archive(&mut s, "refusal-before");
    let before_breps = collect_shapes(&s);
    let result = s.param_set("w", &60.0.into());
    if result.is_err() {
        assert_eq!(archive(&mut s, "refusal-after"), before);
        assert_eq!(collect_shapes(&s), before_breps);
    }
    save_clone(&s, path("transient-after.qcad").to_str().unwrap());
    eprintln!("edit result {result:?}, scope {:?}", s.project().param_map());
    assert!(result.is_ok(), "both references settle to 60, down remains 5: {result:?}");
    let bb = s.result_bodies().into_iter().find(|b| b.id == c).unwrap();
    assert!((bb.bbox[2] + 5.0).abs() < 1e-6);
}

fn collect_shapes(s: &Session) -> std::collections::BTreeMap<Id, Vec<u8>> {
    let _gate = qymcad_kernel::kernel_gate();
    s.project()
        .timeline
        .iter()
        .flat_map(|n| n.kind.bodies())
        .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
        .collect()
}

#[test]
fn late_reference_array_count_must_not_round() {
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let a = block(&mut s, 20.0, "w".into(), None);
    let mut s = ref_clone(&s, a, "array_w", 40.0);
    let b = block(&mut s, 100.0, 10.0.into(), None);
    let (array, _) = s
        .linear_array(Some(b), &ArrayDir { dx: 20.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: "array_w/10".into() }, None, None)
        .unwrap();
    let before = archive(&mut s, "refusal-before");
    let result = s.param_set("w", &45.0.into());
    eprintln!("late array result {result:?}, scope {:?}", s.project().param_map());
    let body = s.result_bodies().into_iter().find(|b| b.id == array).unwrap();
    eprintln!("array body {body:?}");
    assert!(result.is_err(), "native round/clamp accepted count4.5: V{} => {} copies", body.volume, body.volume / 600.0);
    assert_eq!(archive(&mut s, "refusal-after"), before);
}

#[test]
fn reference_repeat_must_enforce_engine_array_cap() {
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let a = block(&mut s, 20.0, "w".into(), None);
    let mut s = ref_clone(&s, a, "cap_w", 40.0);
    let b = block(&mut s, 100.0, 10.0.into(), None);
    let (array, _) = s
        .linear_array(Some(b), &ArrayDir { dx: 20.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: "cap_w/10".into() }, None, None)
        .unwrap();
    // Single native pass control follows the former scheduler: it builds four copies
    // against the old feature scope, then refreshes cap_w. No oversized job is needed.
    let mut p = s.project().clone();
    let raw = collect_shapes(&s);
    let shapes =
        qymcad_kernel::imports::restore(raw.into_iter().map(|(body, bytes)| qymcad_kernel::imports::SavedBody { body, bytes }).collect())
            .into_iter()
            .map(|r| (r.body, r.shape))
            .collect();
    p.parameters.iter_mut().find(|p| p.name == "w").unwrap().expr = "10010".into();
    p.eval_parameters();
    for si in 0..p.sketches.len() {
        if p.sketch_reads_names(si) {
            p.solve_sketch(si);
        }
    }
    p.mark_all_dirty();
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, shapes);
    assert!(r.errors.is_empty(), "native control: {:?}", r.errors);
    assert_eq!(p.param_map()["cap_w"], 10010.0);
    assert!((shapes[&array].volume() - 2400.0).abs() < 1e-5);
    eprintln!("one native pass: measured {}, copies {}", p.param_map()["cap_w"], shapes[&array].volume() / 600.0);
    let before = archive(&mut s, "refusal-before");
    let result = s.param_set("w", &10010.0.into());
    eprintln!("engine result {result:?}, scope {:?}", s.project().param_map());
    if result.is_ok() {
        let body = s.result_bodies().into_iter().find(|b| b.id == array).unwrap();
        eprintln!("actual array volume {}, copies {}", body.volume, body.volume / 600.0);
        assert!((body.volume - 600600.0).abs() < 1e-3);
    }
    assert!(result.is_err(), "reference-settling repeat built 1001 copies despite engine cap 1000");
    assert_eq!(archive(&mut s, "refusal-after"), before);
}

fn same_sketch_fixture() -> (Session, Id, Id, Id, usize) {
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let stocksk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(stocksk, &20.0.into(), &15.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    let stock = s
        .extrude(&Extrude {
            sketch: stocksk,
            profiles: None,
            height: 10.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    let marker = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("marker and width reference")).unwrap();
    s.sketch_rect(marker, &20.0.into(), &15.0.into(), &"w".into(), &30.0.into(), false).unwrap();
    let point = s.sketch_point(marker, &10.0.into(), &6.0.into(), true).unwrap().point;
    let mut p = s.project().clone();
    let si = p.sketch_index(marker).unwrap();
    let pts: Vec<_> = p.sketches[si].points.iter().filter(|pt| !p.sketches[si].system_ids().contains(&pt.id)).collect();
    let a = pts.iter().find(|pt| pt.x.abs() < 1e-6 && pt.y.abs() < 1e-6).unwrap().id;
    let b = pts.iter().find(|pt| (pt.x - 40.).abs() < 1e-6 && (pt.y - 30.).abs() < 1e-6).unwrap().id;
    p.sketches[si].constraints.push(Constraint::Distance { a, b, d: 40., off: 0., expr: String::new(), driven: true, axis: 1, at: None });
    assert!(p.add_named_dim("measured_w".into(), marker, vec![a, b]));
    p.solve_sketch(si);
    let breps = {
        let _g = qymcad_kernel::kernel_gate();
        p.timeline
            .iter()
            .flat_map(|n| n.kind.bodies())
            .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
            .collect::<Vec<_>>()
    };
    let file_path = path("same-sketch.qcad");
    let file = file_path.as_path();
    qymcad_io::save_project_guarded_with_brep(&p, file.to_str().unwrap(), &breps).unwrap();
    let (s, r) = Session::open(file).unwrap();
    assert!(r.errors.is_empty());
    let ci = s
        .sketch_detail(marker)
        .unwrap()
        .constraints
        .into_iter()
        .find(|c| c.kind == "distance_point_line" && c.points.contains(&point))
        .unwrap()
        .index;
    (s, stock, marker, point, ci)
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
fn late_measurement(message: &str) {
    assert!(message.contains("measured_w") && message.contains("solved after") && message.contains("coordinates"), "{message}");
}
#[test]
fn same_sketch_reference_creation_refuses_atomically() {
    let (mut s, stock, marker, _, ci) = same_sketch_fixture();
    s.param_set("alias1", &"measured_w".into()).unwrap();
    s.param_set("alias2", &"alias1".into()).unwrap();
    s.dimension_set(&DimensionTarget::Sketch { sketch: marker, constraint: ci }, Some(&"alias2/4".into()), None).unwrap();
    let before = archive(&mut s, "same-create-before");
    let result = s.hole_by_sketch(&hole_spec(stock, marker));
    late_measurement(&result.unwrap_err().to_string());
    assert_eq!(archive(&mut s, "same-create-after"), before);
}
#[test]
fn same_sketch_reference_edits_refuse_atomically() {
    for through_alias in [false, true] {
        let (mut s, stock, marker, _, ci) = same_sketch_fixture();
        s.param_set("alias1", &10.0.into()).unwrap();
        s.param_set("alias2", &"alias1".into()).unwrap();
        if through_alias {
            s.dimension_set(&DimensionTarget::Sketch { sketch: marker, constraint: ci }, Some(&"alias2".into()), None).unwrap();
        }
        s.hole_by_sketch(&hole_spec(stock, marker)).unwrap();
        let before = archive(&mut s, "same-edit-before");
        let result = if through_alias {
            s.param_set("alias1", &"measured_w/4".into()).map(|_| ())
        } else {
            s.dimension_set(&DimensionTarget::Sketch { sketch: marker, constraint: ci }, Some(&"measured_w/4".into()), None).map(|_| ())
        };
        late_measurement(&result.unwrap_err().to_string());
        assert_eq!(archive(&mut s, "same-edit-after"), before);
    }
}
#[test]
fn same_sketch_reference_saved_open_refuses_without_rewriting() {
    let (mut s, stock, marker, _, ci) = same_sketch_fixture();
    s.hole_by_sketch(&hole_spec(stock, marker)).unwrap();
    let mut p = s.project().clone();
    let si = p.sketch_index(marker).unwrap();
    if let Constraint::DistancePL { expr, .. } = &mut p.sketches[si].constraints[ci] {
        *expr = "measured_w/4".into();
    } else {
        panic!("Point coordinate driver");
    }
    p.mark_sketch_dirty(marker);
    let file = path("same-open-invalid.qcad");
    save_project(&s, &p, file.to_str().unwrap());
    let before = std::fs::read(&file).unwrap();
    let error = match Session::open(&file) {
        Ok(_) => panic!("opened same-marker reference coordinate"),
        Err(e) => e,
    };
    late_measurement(&error.to_string());
    assert_eq!(std::fs::read(file).unwrap(), before);
}

#[test]
fn explicit_points_survive_saved_native_curve_cleanup() {
    for operation in ["trim", "break", "offset", "corner", "mirror", "constraint", "adopted"] {
        for dimensioned in [false, true] {
            let mut s = Session::new_part();
            let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
            let point = s.sketch_point(sk, &50.0.into(), &60.0.into(), dimensioned).unwrap().point;
            let a = s
                .sketch_line(
                    sk,
                    &LineSpec { x1: 0.0.into(), y1: 0.0.into(), x2: 10.0.into(), y2: 0.0.into(), construction: false, dimensioned: true },
                )
                .unwrap();
            let b = s
                .sketch_line(
                    sk,
                    &LineSpec { x1: 10.0.into(), y1: 0.0.into(), x2: 10.0.into(), y2: 10.0.into(), construction: false, dimensioned: true },
                )
                .unwrap();
            let file = path(&format!("cleanup-{operation}-{dimensioned}.qcad"));
            s.save_with_options(Some(&file), true).unwrap();
            let mut s = Session::open(&file).unwrap().0;
            let before: Vec<_> = s.project().sketches[s.project().sketch_index(sk).unwrap()]
                .constraints
                .iter()
                .filter(|c| c.points().contains(&point))
                .cloned()
                .collect();
            match operation {
                "trim" => {
                    s.sketch_trim(sk, &CurvePickSpec { curve: EntityRef::Id(a.entities[0]), at: [5.0.into(), 0.0.into()] }).unwrap();
                }
                "break" => {
                    s.sketch_break(
                        sk,
                        &BreakSpec { curve: EntityRef::Id(a.entities[0]), at: Some([5.0.into(), 0.0.into()]), intersections: false },
                    )
                    .unwrap();
                }
                "offset" => {
                    s.sketch_offset(
                        sk,
                        &SketchOffsetSpec {
                            curves: SketchCurveSel::Ids(vec![a.entities[0]]),
                            distance: 2.0.into(),
                            toward: [5.0.into(), 2.0.into()],
                            dimension_name: None,
                        },
                    )
                    .unwrap();
                }
                "corner" => {
                    s.sketch_corner(
                        sk,
                        &CornerSpec {
                            curves: [EntityRef::Id(a.entities[0]), EntityRef::Id(b.entities[0])],
                            point: PointSel::Id(a.points[1]),
                            size: CornerSize::Radius { value: 2.0.into() },
                        },
                    )
                    .unwrap();
                }
                "mirror" => {
                    s.sketch_mirror_point(
                        sk,
                        &MirrorPointSpec { curves: SketchCurveSel::Ids(vec![a.entities[0]]), about: PointSel::Id(point) },
                    )
                    .unwrap();
                }
                "constraint" => {
                    let ci = s
                        .sketch_detail(sk)
                        .unwrap()
                        .constraints
                        .into_iter()
                        .find(|c| c.value.is_some() && !c.points.contains(&point))
                        .unwrap()
                        .index;
                    s.sketch_remove_constraint(sk, ci).unwrap();
                }
                "adopted" => {
                    let line = s
                        .sketch_line(
                            sk,
                            &LineSpec {
                                x1: 50.0.into(),
                                y1: 60.0.into(),
                                x2: 70.0.into(),
                                y2: 60.0.into(),
                                construction: true,
                                dimensioned: false,
                            },
                        )
                        .unwrap();
                    s.sketch_remove_entity(sk, line.entities[0]).unwrap();
                }
                _ => unreachable!(),
            }
            let sket = &s.project().sketches[s.project().sketch_index(sk).unwrap()];
            assert!(sket.points.iter().any(|p| p.id == point), "{operation}/{dimensioned}: explicit Point lost");
            for c in before {
                assert!(
                    sket.constraints.iter().any(|kept| serde_json::to_value(kept).unwrap() == serde_json::to_value(&c).unwrap()),
                    "{operation}/{dimensioned}: coordinate driver lost: {c:?}"
                );
            }
            s.save_with_options(Some(&file), true).unwrap();
            let s = Session::open(&file).unwrap().0;
            assert!(
                s.project().sketches[s.project().sketch_index(sk).unwrap()].points.iter().any(|p| p.id == point),
                "{operation}/{dimensioned}: reopened Point lost"
            );
        }
    }
}

#[test]
fn reviewer_legacy_two_bore_archive_keeps_both_points_and_drivers() {
    let file = path("legacy-two-bores.qcad");
    std::fs::write(&file, include_bytes!("fixtures/r4-two-bores.qcad")).unwrap();
    let (mut s, r) = Session::open(&file).unwrap();
    assert!(r.errors.is_empty());
    let sk = s.project().sketches.iter().find(|sk| s.project().sketch_hole_point_ids(sk.id).len() == 2).unwrap().id;
    let points = s.project().sketch_hole_point_ids(sk);
    let constraints = s.project().sketches[s.project().sketch_index(sk).unwrap()]
        .constraints
        .iter()
        .filter(|c| c.points().iter().any(|p| points.contains(p)))
        .map(|c| serde_json::to_value(c).unwrap())
        .collect::<Vec<_>>();
    let trash = s.project().sketches[s.project().sketch_index(sk).unwrap()].entities[1].id;
    s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, trash)).unwrap();
    assert_eq!(s.project().sketch_hole_point_ids(sk), points);
    for c in constraints {
        assert!(s.project().sketches[s.project().sketch_index(sk).unwrap()]
            .constraints
            .iter()
            .any(|row| serde_json::to_value(row).unwrap() == c));
    }
    common::assert_close(s.result_bodies()[0].volume, 12000. - 32. * std::f64::consts::PI, 1e-5, "both legacy bores");
    s.save(Some(&file)).unwrap();
    let (s, r) = Session::open(&file).unwrap();
    assert!(r.errors.is_empty());
    assert_eq!(s.project().sketch_hole_point_ids(sk), points);
    common::assert_close(s.result_bodies()[0].volume, 12000. - 32. * std::f64::consts::PI, 1e-5, "both saved bores");
}
#[test]
fn reviewer_same_sketch_archive_is_refused() {
    let file = path("legacy-same-sketch.qcad");
    let bytes = include_bytes!("fixtures/r4-same-sketch-reference.qcad");
    std::fs::write(&file, bytes).unwrap();
    let error = match Session::open(&file) {
        Ok(_) => panic!("accepted reviewer same-sketch reference"),
        Err(e) => e,
    };
    late_measurement(&error.to_string());
    assert_eq!(std::fs::read(file).unwrap(), bytes);
}
#[test]
fn explicit_point_used_as_angle_helper_survives_constraint_cleanup_after_reopen() {
    let mut s = Session::new_part();
    s.param_set("rot", &30.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let point = s.sketch_point(sk, &10.0.into(), &0.0.into(), false).unwrap();
    s.sketch_polygon(
        sk,
        &PolygonSpec {
            cx: 0.0.into(),
            cy: 0.0.into(),
            r: Some(10.0.into()),
            vertex: None,
            sides: 6,
            angle: Some("rot".into()),
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    let detail = s.sketch_detail(sk).unwrap();
    let helper = detail.points.iter().find(|p| p.role == Some("angle_reference")).unwrap();
    let helper = helper.id;
    // An external/native edit reuses an existing explicit Point as the direction reference.
    let mut p = s.project().clone();
    let si = p.sketch_index(sk).unwrap();
    for c in &mut p.sketches[si].constraints {
        match c {
            Constraint::ArcLength { a, .. } if *a == helper => *a = point.point,
            Constraint::Horizontal { b, .. } if *b == helper => *b = point.point,
            Constraint::PointOnCircle { p, .. } if *p == helper => *p = point.point,
            _ => {}
        }
    }
    p.sketches[si].points.retain(|p| p.id != helper);
    p.solve_sketch(si);
    let file = path("point-used-as-helper.qcad");
    save_project(&s, &p, file.to_str().unwrap());
    let mut s = Session::open(&file).unwrap().0;
    let ci = s
        .sketch_detail(sk)
        .unwrap()
        .constraints
        .into_iter()
        .find(|c| c.kind == "arc_length" && c.points.contains(&point.point))
        .unwrap()
        .index;
    s.sketch_remove_constraint(sk, ci).unwrap();
    assert!(s.sketch_detail(sk).unwrap().points.iter().any(|p| p.id == point.point));
}

fn offset_fixture() -> (Session, Id, Id) {
    let mut s = Session::new_part();
    s.param_set("bend", &0.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
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
    (s, sk, point)
}

#[test]
fn offset_relay_during_parameter_edit_keeps_free_explicit_point() {
    let (mut s, sk, point) = offset_fixture();
    for bend in [2., 5., 0.] {
        s.param_set("bend", &bend.into()).unwrap();
        assert!(s.sketch_detail(sk).unwrap().points.iter().any(|p| p.id == point), "offset relay lost free Point at bend={bend}");
    }
}

#[test]
fn offset_relay_during_open_keeps_free_explicit_point() {
    let (s, sk, point) = offset_fixture();
    let mut p = s.project().clone();
    p.parameters.iter_mut().find(|p| p.name == "bend").unwrap().expr = "2".into();
    p.eval_parameters();
    p.mark_all_dirty();
    let file = path("offset-relay-open.qcad");
    save_project(&s, &p, file.to_str().unwrap());
    let (s, r) = Session::open(&file).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    assert!(s.sketch_detail(sk).unwrap().points.iter().any(|p| p.id == point), "native regeneration pruned free Point");
}

#[test]
fn same_sketch_driving_dimensions_and_named_point_coordinates_remain_supported() {
    let (mut s, stock, marker, point, ci) = same_sketch_fixture();
    let width = s.sketch_detail(marker).unwrap().constraints.into_iter().find(|c| c.expr.as_deref() == Some("w")).unwrap().index;
    s.dimension_set(&DimensionTarget::Sketch { sketch: marker, constraint: width }, None, Some("driving_w")).unwrap();
    s.dimension_set(&DimensionTarget::Sketch { sketch: marker, constraint: ci }, Some(&"driving_w/4".into()), Some("marker_x")).unwrap();
    let y = s
        .sketch_detail(marker)
        .unwrap()
        .constraints
        .into_iter()
        .find(|c| c.kind == "distance_point_line" && c.points.contains(&point) && c.index != ci)
        .unwrap()
        .index;
    s.dimension_set(&DimensionTarget::Sketch { sketch: marker, constraint: y }, Some(&"marker_x/2".into()), None).unwrap();
    let hole = s.hole_by_sketch(&hole_spec(stock, marker)).unwrap().0;
    let file = path("same-driving.qcad");
    s.save(Some(&file)).unwrap();
    let (_, _, faces, p) = common::gui_edit_param_model(&file, "w", "60");
    let bore = faces
        .iter()
        .find(|f| p.names.get(f.id).is_some_and(|n| n.feature == hole && n.src == point && n.role == qymcad_core::names::Role::Hole))
        .unwrap();
    common::assert_close(bore.centroid.x, 15., 1e-6, "native same-sketch driving X");
    common::assert_close(bore.centroid.y, 7.5, 1e-6, "native named Point coordinate Y");
    s.param_set("w", &60.0.into()).unwrap();
    let bore = s.project().regen_faces[&hole]
        .iter()
        .find(|f| {
            s.project().names.get(f.id).is_some_and(|n| n.feature == hole && n.src == point && n.role == qymcad_core::names::Role::Hole)
        })
        .unwrap();
    common::assert_close(bore.centroid.x, 15., 1e-6, "engine same-sketch driving X");
    common::assert_close(bore.centroid.y, 7.5, 1e-6, "engine named Point coordinate Y");
}
