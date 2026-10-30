//! R2 reviewer regressions: all named recipe reads and transitive sketch-hole support.
mod common;
use common::{assert_close, scratch};
use qymcad_core::model::{Constraint, ProjSource};
use qymcad_engine::*;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicUsize, Ordering},
};

fn extrude(s: &mut Session, sketch: Id, height: Num, name: Option<&str>) -> Id {
    s.extrude(&Extrude {
        sketch,
        profiles: None,
        height,
        down: None,
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: name.map(str::to_string),
    })
    .unwrap()
    .0
}
fn profile(s: &mut Session) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &20.0.into(), &15.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    sk
}
fn stock(s: &mut Session) -> Id {
    let sk = profile(s);
    extrude(s, sk, 10.0.into(), None)
}
fn shift(s: &mut Session, body: Id, dx: Num, name: Option<&str>) -> Id {
    s.move_body(&Move { body: Some(body), translation: Some([dx, 0.0.into(), 0.0.into()]), rotation: None, name: name.map(str::to_string) })
        .unwrap()
        .0
        .body
}
fn path(label: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    scratch(&format!("r2-{label}-{}.qcad", NEXT.fetch_add(1, Ordering::Relaxed)))
}
// Observe every saved entry immediately; this helper must never rebuild to hide a rollback defect.
fn archive(s: &mut Session) -> BTreeMap<String, Vec<u8>> {
    let file = path("archive");
    s.save_with_options(Some(&file), true).unwrap();
    let mut z = zip::ZipArchive::new(std::fs::File::open(file).unwrap()).unwrap();
    (0..z.len())
        .map(|i| {
            let mut e = z.by_index(i).unwrap();
            let name = e.name().to_string();
            let mut bytes = vec![];
            std::io::Read::read_to_end(&mut e, &mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}
fn named_deletion(consumer_move: bool, alias: bool) {
    let mut s = Session::new_part();
    s.param_set("p", &1.0.into()).unwrap();
    let a = stock(&mut s);
    let source = shift(&mut s, a, "1/p".into(), None);
    s.dimension_set(&DimensionTarget::Feature { feature: source, key: "dx".into() }, Some(&8.0.into()), Some("ShiftFromA")).unwrap();
    let expr = if alias {
        s.param_set("alias1", &"ShiftFromA+1".into()).unwrap();
        s.param_set("alias2", &"alias1-1".into()).unwrap();
        "alias2"
    } else {
        "ShiftFromA"
    };
    let sk = profile(&mut s);
    let consumer = if consumer_move {
        let b = extrude(&mut s, sk, 10.0.into(), None);
        shift(&mut s, b, expr.into(), Some("dependent move"))
    } else {
        extrude(&mut s, sk, expr.into(), Some("dependent extrusion"))
    };
    let body = s.result_bodies().into_iter().find(|b| b.id == consumer).unwrap();
    assert_close(body.volume, if consumer_move { 12000. } else { 9600. }, 1e-6, "named recipe consumer");
    assert_close(if consumer_move { body.bbox[0] } else { body.bbox[5] }, 8., 1e-6, "evaluated named driver");
    let before = archive(&mut s);
    let err = s.feature_delete(source, false).unwrap_err().to_string();
    eprintln!("non-cascade: {err}");
    assert!(
        err.contains(&consumer.to_string()) && err.contains(if consumer_move { "dependent move" } else { "dependent extrusion" }),
        "{err}"
    );
    assert_eq!(archive(&mut s), before, "noncascade refusal must preserve every entry immediately");
    let snapshot = s.begin_tool_edit().unwrap();
    let r = s.feature_delete(source, true).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    s.finish_tool_edit(snapshot, true);
    assert!(s.project().timeline.iter().all(|n| n.id != source && n.id != consumer));
    assert_eq!(s.project().parameters.iter().find(|p| p.name == "p").unwrap().value, 1.);
    if alias {
        // Global parameters have no feature owner and retain the R1 documented cleanup contract.
        assert!(s.project().parameters.iter().any(|p| p.name == "alias1"));
        let retained = archive(&mut s);
        let err = s.param_set("unrelated", &1.0.into()).unwrap_err().to_string();
        assert!(err.contains("ShiftFromA"), "retained alias limit: {err}");
        assert_eq!(archive(&mut s), retained, "dead global alias refusal is atomic too");
        s.param_delete("alias2").unwrap();
        s.param_delete("alias1").unwrap();
    }
    s.param_set("unrelated", &1.0.into()).unwrap();
    let file = path("deleted");
    s.save(Some(&file)).unwrap();
    assert!(Session::open(&file).unwrap().1.errors.is_empty());
    s.undo().unwrap();
    assert_eq!(archive(&mut s), before, "cascade history restores recipe and original B-reps");
}
#[test]
fn deleting_named_move_shift_source_must_not_leave_other_move_dead() {
    named_deletion(true, false);
}
#[test]
fn deleting_named_source_must_not_leave_extrude_distance_dead() {
    named_deletion(false, false);
}
#[test]
fn feature_expression_aliases_cascade_consumers_but_retain_globals() {
    named_deletion(false, true);
}

fn hole(sketch: Id) -> HoleSketch {
    HoleSketch {
        body: None,
        sketch,
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
fn coordinate(s: &Session, sketch: Id, point: Id) -> DimensionTarget {
    DimensionTarget::Sketch {
        sketch,
        constraint: s
            .sketch_detail(sketch)
            .unwrap()
            .constraints
            .into_iter()
            .find(|c| c.kind == "distance_point_line" && c.points.contains(&point))
            .unwrap()
            .index,
    }
}

// Reviewer's native fixture: reference measurements are imported via the archive API because the
// engine correctly refuses dimension_set on reference targets. No self-gating call under the gate.
fn projected_measurement() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let base = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(base, &20.0.into(), &15.0.into(), &"w".into(), &30.0.into(), false).unwrap();
    let source = extrude(&mut s, base, 10.0.into(), None);
    stock(&mut s);
    let projected = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("projected measurement")).unwrap();
    let edge = s.project().regen_edges[&source]
        .iter()
        .find(|e| e.a[1].abs() < 1e-6 && e.b[1].abs() < 1e-6 && (e.a[2] - 10.).abs() < 1e-6 && (e.b[2] - 10.).abs() < 1e-6)
        .unwrap()
        .id;
    let refs = s.sketch_project(projected, source, &[ProjSource::Edge(edge)]).unwrap().0.points;
    let mut p = s.project().clone();
    let si = p.sketch_index(projected).unwrap();
    p.sketches[si].constraints.push(Constraint::Distance {
        a: refs[0],
        b: refs[1],
        d: 40.,
        off: 0.,
        expr: String::new(),
        driven: true,
        axis: 1,
        at: None,
    });
    assert!(p.add_named_dim("measured_w".into(), projected, refs));
    p.solve_sketch(si);
    p.mark_sketch_dirty(projected);
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        p.timeline
            .iter()
            .flat_map(|n| n.kind.bodies())
            .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
            .collect::<Vec<_>>()
    };
    let file = path("projected-reference");
    qymcad_io::save_project_guarded_with_brep(&p, file.to_str().unwrap(), &breps).unwrap();
    let (s, r) = Session::open(&file).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    assert_close(s.project().param_map()["measured_w"], 40., 1e-6, "native named projected measurement");
    (s, projected)
}
fn support_error(err: impl std::fmt::Display, projected: Id) {
    let err = err.to_string();
    eprintln!("support refusal: {err}");
    assert!(err.contains("projected") && err.contains(&projected.to_string()) && err.contains("ordering"), "{err}");
}
#[test]
fn projected_named_measurement_source_is_refused_atomically() {
    for indirect in [false, true] {
        let (mut s, projected) = projected_measurement();
        let expr = if indirect {
            s.param_set("alias1", &"measured_w/4".into()).unwrap();
            s.param_set("alias2", &"alias1+0".into()).unwrap();
            let middle = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("intermediate source")).unwrap();
            let pt = s.sketch_point(middle, &"alias2".into(), &6.0.into(), true).unwrap().point;
            s.dimension_set(&coordinate(&s, middle, pt), None, Some("middle_x")).unwrap();
            "middle_x"
        } else {
            "measured_w/4"
        };
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_point(sk, &expr.into(), &6.0.into(), true).unwrap();
        let before = archive(&mut s);
        support_error(s.hole_by_sketch(&hole(sk)).unwrap_err(), projected);
        assert_eq!(archive(&mut s), before, "hole creation refusal must be exact before any rebuild");
    }
}
#[test]
fn edits_introducing_projected_named_sources_are_refused_atomically() {
    for edit in ["coordinate", "parameter", "upstream_coordinate"] {
        let (mut s, projected) = projected_measurement();
        s.param_set("marker_x", &10.0.into()).unwrap();
        let middle = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let mp = s.sketch_point(middle, &10.0.into(), &6.0.into(), true).unwrap().point;
        s.dimension_set(&coordinate(&s, middle, mp), None, Some("middle_x")).unwrap();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let expr = if edit == "upstream_coordinate" { "middle_x" } else { "marker_x" };
        let pt = s.sketch_point(sk, &expr.into(), &6.0.into(), true).unwrap().point;
        let (h, r) = s.hole_by_sketch(&hole(sk)).unwrap();
        assert!(r.errors.is_empty());
        assert_close(
            s.result_bodies().iter().find(|b| b.id == h).unwrap().volume,
            12000. - 16. * std::f64::consts::PI,
            1e-6,
            "supported control bore",
        );
        let before = archive(&mut s);
        let error = match edit {
            "parameter" => s.param_set("marker_x", &"measured_w/4".into()).unwrap_err(),
            "upstream_coordinate" => s.dimension_set(&coordinate(&s, middle, mp), Some(&"measured_w/4".into()), None).unwrap_err(),
            _ => s.dimension_set(&coordinate(&s, sk, pt), Some(&"measured_w/4".into()), None).unwrap_err(),
        };
        support_error(error, projected);
        assert_eq!(archive(&mut s), before, "{edit} refusal archive immediately");
    }
}
#[test]
fn open_refuses_saved_projected_named_measurement_source_without_rewriting() {
    let (mut s, projected) = projected_measurement();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let pt = s.sketch_point(sk, &10.0.into(), &6.0.into(), true).unwrap().point;
    let (h, _) = s.hole_by_sketch(&hole(sk)).unwrap();
    let file = path("unsupported-open");
    s.save(Some(&file)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(file.to_str().unwrap()).unwrap();
    let ci = match coordinate(&s, sk, pt) {
        DimensionTarget::Sketch { constraint, .. } => constraint,
        _ => unreachable!(),
    };
    let si = project.sketch_index(sk).unwrap();
    *project.sketches[si].constraints[ci].expr_mut().unwrap() = "measured_w/4".into();
    project.eval_parameters();
    project.solve_sketch(si);
    project.mark_sketch_dirty(sk);
    qymcad_io::save_project_guarded_with_brep(&project, file.to_str().unwrap(), &breps).unwrap();
    let bytes = std::fs::read(&file).unwrap();
    let err = Session::open(&file).err().expect("open must reject unsupported named source");
    assert!(err.to_string().contains(&h.to_string()), "{err}");
    support_error(err, projected);
    assert_eq!(std::fs::read(file).unwrap(), bytes);
}
