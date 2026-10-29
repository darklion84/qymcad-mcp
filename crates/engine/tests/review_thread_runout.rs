//! Native runout fallbacks must never silently replace accepted creation/evaluated inputs.
use qymcad_core::thread::ThreadSpec;
use qymcad_engine::*;

fn stock() -> (Session, Id, Sel) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &8.0.into(), false).unwrap();
    let src = s
        .extrude(&Extrude {
            down: None,
            sketch: sk,
            profiles: None,
            height: 20.0.into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    let edge = s
        .topology(Some(src), false)
        .unwrap()
        .edges
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Circle)
        .min_by(|a, b| a.center.unwrap()[2].total_cmp(&b.center.unwrap()[2]))
        .unwrap()
        .id;
    (s, src, Sel::Ids(vec![edge]))
}

fn options(lead_in: Num, lead_out: Num, length: Num) -> ThreadOptions {
    ThreadOptions {
        size: Some("M8".into()),
        fit: Some(0.1.into()),
        lead_in: Some(lead_in),
        lead_out: Some(lead_out),
        length,
        name: Some("runout thread".into()),
        ..Default::default()
    }
}

fn breps(s: &Session) -> Vec<(Id, Vec<u8>)> {
    let _gate = qymcad_kernel::kernel_gate();
    s.project().timeline.iter().flat_map(|n| n.kind.bodies()).map(|id| (id, s.shape(id).unwrap().to_brep_bytes().unwrap())).collect()
}

/// Fresh full native regeneration, independently bypassing the server's validation.
fn raw_metrics(s: &Session, source: Id, rim: &Sel, lead_in: f64, lead_out: f64) -> (f64, [f64; 6]) {
    let mut project = s.project().clone();
    let Sel::Ids(edges) = rim else { unreachable!() };
    let body = project.add_thread(source, edges[0], ThreadSpec { nominal_d: 8.0, fit: 0.1, ..Default::default() }, 20.0, lead_in, lead_out);
    // Testkit owns the kernel gate and forces all body-producing nodes to rebuild from scratch.
    let (report, shapes) = qymcad_testkit::regenerate_with_shapes(&mut project, Default::default());
    assert!(report.errors.is_empty(), "native fixture errors: {:?}", report.errors);
    let _gate = qymcad_kernel::kernel_gate();
    (shapes[&body].volume(), shapes[&body].bbox().unwrap())
}

fn assert_same_geometry(a: (f64, [f64; 6]), b: (f64, [f64; 6])) {
    assert!((a.0 - b.0).abs() < 1e-8, "measured fresh native volumes differ: {a:?} vs {b:?}");
    for (a, b) in a.1.into_iter().zip(b.1) {
        assert!((a - b).abs() < 1e-9);
    }
}

#[test]
fn runout_at_or_below_native_epsilon_is_refused_at_creation() {
    let (mut s, src, edge) = stock();
    let zero = raw_metrics(&s, src, &edge, 0.0, 0.0);
    for (entry, exit) in [(1e-6, 0.0), (0.0, 1e-6), (5e-7, 5e-7)] {
        let actual = raw_metrics(&s, src, &edge, entry, exit);
        assert_same_geometry(actual, zero);
        println!("native dropped entry={entry},exit={exit}: measured volume={}", actual.0);
        let before = s.info();
        let bytes = breps(&s);
        let error = s
            .thread(Some(src), &edge, &options(entry.into(), exit.into(), 20.0.into()))
            .expect_err("tiny positive runout silently became zero");
        assert!(error.to_string().contains("runout"), "{error}");
        assert_eq!(s.info(), before);
        assert_eq!(breps(&s), bytes);
    }
    let allowed = s.thread(Some(src), &edge, &options(0.0.into(), 0.0.into(), 20.0.into())).unwrap();
    assert_same_geometry((s.shape(allowed.body).unwrap().volume(), s.shape(allowed.body).unwrap().bbox().unwrap()), zero);
}

#[test]
fn oversized_runouts_are_refused_at_creation() {
    let (mut s, src, edge) = stock();
    let requested = raw_metrics(&s, src, &edge, 12.0, 12.0);
    // Native scales by .95*length/(entry+exit): each12 becomes9.5, despite stored12.
    let scaled = raw_metrics(&s, src, &edge, 9.5, 9.5);
    let unscaled_control = raw_metrics(&s, src, &edge, 2.0, 2.0);
    assert_same_geometry(requested, scaled);
    assert!((requested.0 - unscaled_control.0).abs() > 1.0, "runout changes measurable material removal");
    println!("native requested12+12 volume={} equals explicit9.5+9.5 volume={}; 2+2 volume={}", requested.0, scaled.0, unscaled_control.0);
    let before = s.info();
    let bytes = breps(&s);
    let error = s
        .thread(Some(src), &edge, &options(12.0.into(), 12.0.into(), 20.0.into()))
        .expect_err("native silently scaled runouts to95% of length");
    assert!(error.to_string().contains("runout"), "{error}");
    assert_eq!(s.info(), before);
    assert_eq!(breps(&s), bytes);
    let allowed = s.thread(Some(src), &edge, &options(9.5.into(), 9.5.into(), 20.0.into())).unwrap();
    assert_same_geometry((s.shape(allowed.body).unwrap().volume(), s.shape(allowed.body).unwrap().bbox().unwrap()), scaled);
}

#[test]
fn evaluated_runout_dimensions_refuse_native_changes_and_restore_geometry() {
    let (mut s, src, edge) = stock();
    let fresh = raw_metrics(&s, src, &edge, 2.0, 2.0);
    let thread = s.thread(Some(src), &edge, &options(2.0.into(), 2.0.into(), 20.0.into())).unwrap();
    assert!((s.shape(thread.body).unwrap().volume() - fresh.0).abs() < 1e-8, "safe baseline matches fresh native build");
    for (key, value) in [
        ("lead_in", Num::Value(1e-6)),
        ("lead_out", Num::Value(5e-7)),
        ("lead_in", "1/1000000".into()),
        ("lead_out", "1/2000000".into()),
        ("lead_in", 20.0.into()),
        ("lead_out", 20.0.into()),
        ("length", 3.0.into()),
    ] {
        let before = s.info();
        let bytes = breps(&s);
        let error = s
            .dimension_set(&DimensionTarget::Feature { feature: thread.body, key: key.into() }, Some(&value), None)
            .expect_err("evaluated dimension accepted a native runout fallback");
        let text = error.to_string();
        assert!(text.contains("runout") && text.contains(&thread.body.to_string()), "{text}");
        assert_eq!(s.info(), before);
        assert_eq!(breps(&s), bytes);
    }
}

#[test]
fn evaluated_runout_parameters_refuse_native_changes_and_restore_geometry() {
    let (mut s, src, edge) = stock();
    for (name, value) in [("entry", 2.0), ("exit", 2.0), ("span", 20.0)] {
        s.param_set(name, &value.into()).unwrap();
    }
    let thread = s.thread(Some(src), &edge, &options("entry".into(), "exit".into(), "span".into())).unwrap();
    for (key, value) in [("entry", 1e-6), ("exit", 5e-7), ("entry", 20.0), ("exit", 20.0), ("span", 3.0)] {
        let before = s.info();
        let bytes = breps(&s);
        let error = s.param_set(key, &value.into()).expect_err("evaluated parameter accepted a native runout fallback");
        let text = error.to_string();
        assert!(text.contains("runout") && text.contains(&thread.body.to_string()), "{text}");
        assert_eq!(s.info(), before);
        assert_eq!(breps(&s), bytes);
    }
}

#[test]
fn numeric_runout_dimension_keeps_precision_and_matches_fresh_native_geometry() {
    let (mut s, src, edge) = stock();
    let requested = 1.234567891234;
    let fresh = raw_metrics(&s, src, &edge, requested, 2.0);
    let thread = s.thread(Some(src), &edge, &options(2.0.into(), 2.0.into(), 20.0.into())).unwrap();
    let edited =
        s.dimension_set(&DimensionTarget::Feature { feature: thread.body, key: "lead_in".into() }, Some(&requested.into()), None).unwrap();
    assert_eq!(edited.dimension.value, Some(requested), "accepted runout numeric edit must preserve every requested digit");
    assert_same_geometry((s.shape(thread.body).unwrap().volume(), s.shape(thread.body).unwrap().bbox().unwrap()), fresh);
}
