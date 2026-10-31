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
    let path = std::env::temp_dir().join(format!("r4-unit-{}-{:?}.qcad", std::process::id(), std::thread::current().id()));
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

fn array_fixture() -> (Session, Id) {
    let mut s = measured();
    let b = extrusion(&mut s, 100., 10.0.into(), 10.0.into(), None);
    let array = s
        .linear_array(Some(b), &ArrayDir { dx: 20.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: "measured_w/10".into() }, None, None)
        .unwrap()
        .0;
    (s, array)
}
#[test]
fn late_array_limits_refuse_standalone_rebuild_with_exact_archive() {
    for value in ["45", "10010"] {
        let (mut s, _) = array_fixture();
        pending(&mut s, value);
        let before = archive(&mut s);
        let report = s.rebuild();
        assert!(report.errors.iter().any(|e| e.message.contains("count") || e.message.contains("copies")), "{report:?}");
        assert_eq!(archive(&mut s), before);
    }
}
#[test]
fn late_array_limits_refuse_dimension_edits_with_exact_archive() {
    for value in [45., 10010.] {
        let (mut s, _) = array_fixture();
        let sk = s.p.sketches[0].id;
        let ci = s.p.sketches[0].constraints.iter().position(|c| c.expr() == Some("w")).unwrap();
        let before = archive(&mut s);
        let error =
            s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: ci }, Some(&value.into()), None).unwrap_err().to_string();
        assert!(error.contains("count") || error.contains("copies"), "{error}");
        assert_eq!(archive(&mut s), before);
    }
}
#[test]
fn late_array_limits_refuse_open_without_rewriting() {
    for value in ["45", "10010"] {
        let (mut s, _) = array_fixture();
        pending(&mut s, value);
        let path = std::env::temp_dir().join(format!("r4-array-open-{}-{value}.qcad", std::process::id()));
        s.save_with_options(Some(&path), true).unwrap();
        let before = std::fs::read(&path).unwrap();
        let error = match Session::open(&path) {
            Ok(_) => panic!("opened invalid array {value}"),
            Err(e) => e.to_string(),
        };
        assert!(error.contains("count") || error.contains("copies"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn same_marker_reference_rebuild_refuses_exact_archive() {
    let mut s = Session::new_part();
    s.param_set("w", &40.0.into()).unwrap();
    let stock = extrusion(&mut s, 20., 40.0.into(), 10.0.into(), None);
    let marker = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(marker, &20.0.into(), &15.0.into(), &"w".into(), &6.0.into(), false).unwrap();
    let point = s.sketch_point(marker, &10.0.into(), &15.0.into(), true).unwrap().point;
    let si = s.p.sketch_index(marker).unwrap();
    let sk = &s.p.sketches[si];
    let a = sk.points.iter().find(|p| p.x.abs() < 1e-6 && (p.y - 12.).abs() < 1e-6).unwrap().id;
    let b = sk.points.iter().find(|p| (p.x - 40.).abs() < 1e-6 && (p.y - 18.).abs() < 1e-6).unwrap().id;
    s.p.sketches[si].constraints.push(Constraint::Distance { a, b, d: 40., off: 0., expr: String::new(), driven: true, axis: 1, at: None });
    assert!(s.p.add_named_dim("measured_w".into(), marker, vec![a, b]));
    s.p.solve_sketch(si);
    s.hole_by_sketch(&HoleSketch {
        body: Some(stock),
        sketch: marker,
        points: None,
        flip: true,
        diameter: 2.0.into(),
        depth: Some(4.0.into()),
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: None,
    })
    .unwrap();
    let ci = s.p.sketches[si].constraints.iter().position(|c| matches!(c,Constraint::DistancePL{p,..} if *p==point)).unwrap();
    *s.p.sketches[si].constraints[ci].expr_mut().unwrap() = "measured_w/4".into();
    s.p.mark_all_dirty();
    let before = archive(&mut s);
    let report = s.rebuild();
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.message.contains("measured_w") && e.message.contains("solved after") && e.message.contains("coordinates")),
        "{report:?}"
    );
    assert_eq!(archive(&mut s), before);
}

#[test]
fn native_midpoint_helper_is_not_promoted_to_explicit_point() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let line = s
        .sketch_line(
            sk,
            &LineSpec { x1: 10.0.into(), y1: 10.0.into(), x2: 20.0.into(), y2: 10.0.into(), construction: true, dimensioned: true },
        )
        .unwrap();
    let si = s.p.sketch_index(sk).unwrap();
    let helper = s.p.sketch_point_at(si, 15., 10., 1e-6);
    s.p.sketches[si].constraints.push(Constraint::Midpoint { p: helper, a: line.points[0], b: line.points[1] });
    s.p.solve_sketch(si);
    s.sketch_remove_entity(sk, line.entities[0]).unwrap();
    assert!(!s.p.sketches[si].points.iter().any(|p| p.id == helper), "native helper resurrected as a hole marker");
}

#[test]
fn native_offset_regeneration_keeps_free_explicit_point() {
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
    s.p.parameters.iter_mut().find(|p| p.name == "bend").unwrap().expr = "2".into();
    s.p.eval_parameters();
    s.p.mark_all_dirty();
    let report = s.rebuild();
    assert!(report.errors.is_empty(), "{report:?}");
    assert!(s.sketch_detail(sk).unwrap().points.iter().any(|p| p.id == point));
}
