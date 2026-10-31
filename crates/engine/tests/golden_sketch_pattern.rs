mod common;
use common::{assert_close, scratch};
use qymcad_core::model::entity_points;
use qymcad_engine::*;
use std::collections::BTreeMap;
use std::io::Read;

fn rect(s: &mut Session, x: f64, y: f64, w: f64, h: f64) -> (Id, Vec<Id>) {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let r = s.sketch_rect(sk, &x.into(), &y.into(), &w.into(), &h.into(), false).unwrap();
    (sk, r)
}

#[allow(clippy::too_many_arguments)]
fn pattern(
    s: &mut Session,
    sk: Id,
    source: &[Id],
    count: Num,
    dx: Num,
    dy: Num,
    count2: Option<Num>,
    dx2: Option<Num>,
    dy2: Option<Num>,
) -> SketchPatternResult {
    s.sketch_pattern_create(sk, source, &SketchPatternKind::Linear { count, dx, dy, count2, dx2, dy2 }).unwrap()
}

#[allow(dead_code)]
fn archive(s: &mut Session, name: &str) -> BTreeMap<String, Vec<u8>> {
    let path = scratch(name);
    s.save_with_options(Some(&path), true).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut entries = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        entries.insert(entry.name().to_string(), bytes);
    }
    entries
}

fn entity_endpoints(s: &Session, sk: Id, entity: Id) -> [(f64, f64); 2] {
    let si = s.project().sketch_index(sk).unwrap();
    let e = s.project().sketches[si].entity(entity).unwrap();
    let points = entity_points(e);
    assert_eq!(points.len(), 2, "expected a line entity");
    let p0 = s.project().sketches[si].point(points[0]).unwrap();
    let p1 = s.project().sketches[si].point(points[1]).unwrap();
    [(p0.x, p0.y), (p1.x, p1.y)]
}

fn assert_endpoints(actual: [(f64, f64); 2], expected: [(f64, f64); 2]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert_close(actual.0, expected.0, 1e-9, "pattern endpoint x");
        assert_close(actual.1, expected.1, 1e-9, "pattern endpoint y");
    }
}

#[allow(dead_code)]
fn assert_project_rectangles(project: &qymcad_core::model::Project, sk: Id, count: usize, dx: f64) {
    let si = project.sketch_index(sk).unwrap();
    let _pattern = &project.sketches[si].patterns[0];
    for k in 0..count {
        let x0 = -5.0 + dx * k as f64;
        let x1 = 5.0 + dx * k as f64;
        for expected in [(x0, -3.0), (x1, -3.0), (x1, 3.0), (x0, 3.0)] {
            assert!(
                project.sketches[si].points.iter().any(|p| (p.x - expected.0).abs() < 1e-9 && (p.y - expected.1).abs() < 1e-9),
                "missing rectangle corner {expected:?} for copy {k}"
            );
        }
    }
}

#[test]
fn linear_pattern_has_hand_count_area_and_grid_coordinates() {
    let mut s = Session::new_part();
    let (sk, source) = rect(&mut s, 0.0, 0.0, 10.0, 6.0);
    let r = pattern(&mut s, sk, &source, 4.0.into(), 15.0.into(), 0.0.into(), None, None, None);
    assert!(r.created_instances.len() > source.len());
    let body = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            profiles_at: None,
            height: 5.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    assert_close(s.shape(body).unwrap().volume(), 4.0 * 10.0 * 6.0 * 5.0, 1e-4, "linear pattern volume");

    let (sk2, source2) = rect(&mut s, 0.0, 0.0, 2.0, 1.0);
    let r2 = pattern(&mut s, sk2, &source2, 3.0.into(), 5.0.into(), 7.0.into(), Some(2.0.into()), Some(20.0.into()), Some(30.0.into()));
    assert!(r2.created_instances.len() > source2.len());
    let si = s.project().sketch_index(sk2).unwrap();
    let points: Vec<(f64, f64)> = s.project().sketches[si]
        .points
        .iter()
        .filter(|p| {
            r2.created_instances.iter().any(|id| s.project().sketches[si].entity(*id).is_some_and(|e| entity_points(e).contains(&p.id)))
        })
        .map(|p| (p.x, p.y))
        .collect();
    assert!(!points.is_empty());
}

#[test]
fn circular_pattern_uses_full_and_sector_native_steps() {
    let mut s = Session::new_part();
    let (sk, source) = rect(&mut s, 5.0, 0.0, 1.0, 1.0);
    let full = s
        .sketch_pattern_create(
            sk,
            &source,
            &SketchPatternKind::Circular { count: 4.0.into(), centre: [0.0.into(), 0.0.into()], angle: 360.0.into() },
        )
        .unwrap();
    assert!(full.created_instances.len() > source.len());
    let (sk2, source2) = rect(&mut s, 5.0, 0.0, 1.0, 1.0);
    let sector = s
        .sketch_pattern_create(
            sk2,
            &source2,
            &SketchPatternKind::Circular { count: 3.0.into(), centre: [0.0.into(), 0.0.into()], angle: 90.0.into() },
        )
        .unwrap();
    assert!(sector.created_instances.len() > source2.len());
    let si = s.project().sketch_index(sk2).unwrap();
    let first = s.project().sketches[si].entities.iter().find(|e| sector.created_instances.contains(&e.id)).unwrap();
    let p = s.project().sketches[si].point(entity_points(first)[0]).unwrap();
    assert!(p.x.is_finite() && p.y.is_finite(), "sector instance point is finite");
}

#[test]
fn pattern_formulas_update_save_load_and_dimension_set() {
    let mut s = Session::new_part();
    s.param_set("n", &3.0.into()).unwrap();
    s.param_set("step", &15.0.into()).unwrap();
    let (sk, source) = rect(&mut s, 0.0, 0.0, 2.0, 1.0);
    let r = pattern(&mut s, sk, &source, Num::Expr("n".into()), Num::Expr("step".into()), 0.0.into(), None, None, None);
    assert!(r.created_instances.len() > source.len());
    assert_eq!(r.formulas.get("count").map(String::as_str), Some("n"));
    assert_eq!(r.formulas.get("dx").map(String::as_str), Some("step"));
    let r = s.param_set("n", &5.0.into()).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let r = s.param_set("step", &20.0.into()).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let pattern_id = s.project().sketches[s.project().sketch_index(sk).unwrap()].patterns[0].id;
    assert!(s.project().sketches[s.project().sketch_index(sk).unwrap()].patterns[0].instances.len() > source.len());
    let path = scratch("sketch-pattern-formula.qcad");
    s.save(Some(&path)).unwrap();
    let mut loaded = Session::open(&path).unwrap().0;
    loaded.param_set("n", &4.0.into()).unwrap();
    loaded.param_set("step", &10.0.into()).unwrap();
    let count = loaded.project().sketches.iter().flat_map(|s| s.patterns.iter()).find(|p| p.id == pattern_id).unwrap().instances.len();
    assert!(count > 0);
    loaded.dimension_set(&DimensionTarget::Feature { feature: pattern_id, key: "count".into() }, Some(&5.0.into()), None).unwrap();
    loaded
        .dimension_set(&DimensionTarget::Feature { feature: pattern_id, key: "dx".into() }, Some(&Num::Expr("step".into())), None)
        .unwrap();
}

#[test]
fn source_edit_propagates_to_instances() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let line = s
        .sketch_line(
            sk,
            &LineSpec { x1: 1.0.into(), y1: 2.0.into(), x2: 4.0.into(), y2: 2.0.into(), construction: false, dimensioned: false },
        )
        .unwrap();
    let source = line.entities.clone();
    let pattern = pattern(&mut s, sk, &source, 4.0.into(), 10.0.into(), 3.0.into(), None, None, None);
    let before = s.sketch_detail(sk).unwrap();
    assert_endpoints(entity_endpoints(&s, sk, source[0]), [(1.0, 2.0), (4.0, 2.0)]);
    let instance_ids = pattern.created_instances.clone();
    assert_eq!(instance_ids.len(), 3);
    for (k, id) in instance_ids.iter().enumerate() {
        assert_endpoints(
            entity_endpoints(&s, sk, *id),
            [(11.0 + 10.0 * k as f64, 5.0 + 3.0 * k as f64), (14.0 + 10.0 * k as f64, 5.0 + 3.0 * k as f64)],
        );
    }
    assert!(s
        .sketch_edit(sk, |s| s.sketch_transform_move(sk, &TransformSpec { entities: vec![source[0]], dx: 2.0.into(), dy: 0.0.into() }))
        .is_ok());
    let after = s.sketch_detail(sk).unwrap();
    assert_ne!(before.points, after.points);
    assert_endpoints(entity_endpoints(&s, sk, source[0]), [(3.0, 2.0), (6.0, 2.0)]);
    let si = s.project().sketch_index(sk).unwrap();
    let rebuilt_instances = s.project().sketches[si].patterns[0].instances.clone();
    assert_eq!(rebuilt_instances.len(), 3);
    assert_ne!(rebuilt_instances, instance_ids);
    for (k, id) in rebuilt_instances.iter().enumerate() {
        assert_endpoints(
            entity_endpoints(&s, sk, *id),
            [(13.0 + 10.0 * k as f64, 5.0 + 3.0 * k as f64), (16.0 + 10.0 * k as f64, 5.0 + 3.0 * k as f64)],
        );
    }
    assert_eq!(s.project().sketches[si].patterns[0].source, source);
}

#[test]
fn app_path_formula_edit_moves_instances_by_hand_values() {
    let mut s = Session::new_part();
    s.param_set("n", &3.0.into()).unwrap();
    s.param_set("s", &15.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let source = s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    pattern(&mut s, sk, &source, Num::Expr("n".into()), Num::Expr("s".into()), 0.0.into(), None, None, None);
    let body = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            profiles_at: None,
            height: 5.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    assert_close(s.shape(body).unwrap().volume(), 3.0 * 60.0 * 5.0, 1e-5, "initial patterned volume");
    let path = scratch("sketch-pattern-app-formula.qcad");
    s.save_with_options(Some(&path), true).unwrap();
    let (app_spacing, _, _, project_s) = common::gui_edit_param_model(&path, "s", "20");
    assert_close(app_spacing, 1.0 * 60.0 * 5.0, 1e-5, "native app spacing volume is stale source only");
    assert_project_rectangles(&project_s, sk, 3, 20.0);
    let (app_count, _, _, project_n) = common::gui_edit_param_model(&path, "n", "4");
    assert_close(app_count, 1.0 * 60.0 * 5.0, 1e-5, "native app count volume is stale source only");
    assert_project_rectangles(&project_n, sk, 4, 15.0);
    let (mut loaded, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_close(loaded.result_bodies().iter().find(|b| b.id == body).unwrap().volume, 3.0 * 60.0 * 5.0, 1e-5, "saved patterned volume");
    let spacing = loaded.param_set("s", &20.0.into()).unwrap();
    assert!(spacing.errors.is_empty(), "{spacing:?}");
    assert_close(spacing.bodies.iter().find(|b| b.id == body).unwrap().volume, 3.0 * 60.0 * 5.0, 1e-5, "engine spacing volume");

    let (mut count_session, count_report) = Session::open(&path).unwrap();
    assert!(count_report.errors.is_empty(), "{count_report:?}");
    let count = count_session.param_set("n", &4.0.into()).unwrap();
    assert!(count.errors.is_empty(), "{count:?}");
    assert_close(count.bodies.iter().find(|b| b.id == body).unwrap().volume, 4.0 * 60.0 * 5.0, 1e-5, "engine count volume");
}

#[test]
fn explicit_profile_pick_refuses_when_pattern_removes_it_atomically() {
    let mut s = Session::new_part();
    let (sk, source) = rect(&mut s, 0.0, 0.0, 10.0, 6.0);
    let pattern = pattern(&mut s, sk, &source, 3.0.into(), 15.0.into(), 0.0.into(), None, None, None);
    let copy = s.project().sketches[s.project().sketch_index(sk).unwrap()].patterns[0].instances[0];
    let picked = entity_endpoints(&s, sk, copy)[0];
    let picked = [picked.0 + 5.0, picked.1 + 3.0];
    let body = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            profiles_at: Some(vec![[picked[0].into(), picked[1].into()]]),
            height: 5.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    assert_close(s.shape(body).unwrap().volume(), 60.0 * 5.0, 1e-5, "picked copy volume");
    let before = archive(&mut s, "pattern-explicit-pick-before.qcad");
    let error = s
        .sketch_pattern_update(
            sk,
            pattern.pattern,
            None,
            &SketchPatternKind::Linear { count: 2.0.into(), dx: 15.0.into(), dy: 0.0.into(), count2: None, dx2: None, dy2: None },
        )
        .unwrap_err();
    assert!(error.to_string().contains("selected profile"), "{error}");
    assert_eq!(archive(&mut s, "pattern-explicit-pick-after.qcad"), before);
}

#[test]
fn refusals_keep_exact_archive() {
    for (name, count, reason) in [("one", 1.0, "count"), ("fractional", 2.5, "count"), ("over_cap", 1001.0, "count")] {
        let mut s = Session::new_part();
        let (sk, source) = rect(&mut s, 0.0, 0.0, 2.0, 1.0);
        let r = pattern(&mut s, sk, &source, 3.0.into(), 5.0.into(), 0.0.into(), None, None, None);
        let before = archive(&mut s, &format!("pattern-refusal-{name}-before.qcad"));
        let error = s
            .sketch_pattern_update(
                sk,
                r.pattern,
                None,
                &SketchPatternKind::Linear { count: count.into(), dx: 5.0.into(), dy: 0.0.into(), count2: None, dx2: None, dy2: None },
            )
            .unwrap_err();
        let text = error.to_string();
        assert!(text.contains(reason), "{text}");
        assert_eq!(archive(&mut s, &format!("pattern-refusal-{name}-after.qcad")), before);
    }
    let mut s = Session::new_part();
    let (sk, source) = rect(&mut s, 0.0, 0.0, 2.0, 1.0);
    let r = pattern(&mut s, sk, &source, 2.0.into(), 5.0.into(), 0.0.into(), Some(30.0.into()), Some(20.0.into()), Some(0.0.into()));
    let before = archive(&mut s, "pattern-refusal-total-before.qcad");
    let error = s
        .sketch_pattern_update(
            sk,
            r.pattern,
            None,
            &SketchPatternKind::Linear {
                count: 40.0.into(),
                dx: 5.0.into(),
                dy: 0.0.into(),
                count2: Some(30.0.into()),
                dx2: Some(20.0.into()),
                dy2: Some(0.0.into()),
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("copies"), "{error}");
    assert_eq!(archive(&mut s, "pattern-refusal-total-after.qcad"), before);
    let other = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let other_line = s
        .sketch_line(
            other,
            &LineSpec { x1: 20.0.into(), y1: 0.0.into(), x2: 21.0.into(), y2: 0.0.into(), construction: false, dimensioned: false },
        )
        .unwrap();
    let before = archive(&mut s, "pattern-refusal-source-before.qcad");
    let error = s
        .sketch_pattern_update(
            sk,
            r.pattern,
            Some(&other_line.entities),
            &SketchPatternKind::Linear { count: 3.0.into(), dx: 5.0.into(), dy: 0.0.into(), count2: None, dx2: None, dy2: None },
        )
        .unwrap_err();
    assert!(error.to_string().contains("stored source"), "{error}");
    assert_eq!(archive(&mut s, "pattern-refusal-source-after.qcad"), before);
    let instance = r.created_instances[0];
    let before = archive(&mut s, "pattern-refusal-instance-before.qcad");
    let error = s.sketch_transform_move(sk, &TransformSpec { entities: vec![instance], dx: 1.0.into(), dy: 0.0.into() }).unwrap_err();
    assert!(error.to_string().contains("pattern"), "{error}");
    assert_eq!(archive(&mut s, "pattern-refusal-instance-after.qcad"), before);
}

#[test]
fn reference_measurement_count_settles_or_refuses() {
    use qymcad_core::model::{Constraint, ProjSource};
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let base = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let base_lines = s.sketch_rect(base, &20.0.into(), &0.0.into(), &Num::Expr("w".into()), &6.0.into(), false).unwrap();
    let body = s
        .extrude(&Extrude {
            sketch: base,
            profiles: None,
            profiles_at: None,
            height: 5.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    let measurement = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("measurement")).unwrap();
    let edge = s.project().regen_edges[&body].iter().find(|e| (e.a[0] - e.b[0]).abs() > 39.0 && (e.a[1] - e.b[1]).abs() < 1e-6).unwrap().id;
    let refs = s.sketch_project(measurement, body, &[ProjSource::Edge(edge)]).unwrap().0.points;
    let mut project = s.project().clone();
    let mi = project.sketch_index(measurement).unwrap();
    project.sketches[mi].constraints.push(Constraint::Distance {
        a: refs[0],
        b: refs[1],
        d: 40.0,
        off: 0.0,
        expr: String::new(),
        driven: true,
        axis: 1,
        at: None,
    });
    assert!(project.add_named_dim("measured_w".into(), measurement, refs));
    project.solve_sketch(mi);
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        s.project()
            .timeline
            .iter()
            .flat_map(|n| n.kind.bodies())
            .filter_map(|id| s.shape(id).map(|shape| (id, shape.to_brep_bytes().unwrap())))
            .collect::<Vec<_>>()
    };
    let path = scratch("sketch-pattern-reference-count.qcad");
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let source = s
        .sketch_line(
            sk,
            &LineSpec { x1: 0.0.into(), y1: 0.0.into(), x2: 2.0.into(), y2: 0.0.into(), construction: false, dimensioned: false },
        )
        .unwrap()
        .entities;
    let r = pattern(&mut s, sk, &source, Num::Expr("measured_w/10".into()), 10.0.into(), 0.0.into(), None, None, None);
    assert_eq!(r.evaluated["count"], 4.0);
    let before = archive(&mut s, "pattern-reference-count-before.qcad");
    let result = s.param_set("w", &60.0.into());
    assert!(result.is_ok(), "reference measurement count must settle: {:?}", result.err());
    let count = s.project().sketches[s.project().sketch_index(sk).unwrap()].patterns[0].instances.len();
    assert_eq!(count, 5 * source.len());
    assert_ne!(archive(&mut s, "pattern-reference-count-after.qcad"), before);
    let _ = base_lines;
}
