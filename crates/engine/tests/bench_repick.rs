//! Timing of a fillet re-pick on a document with many unrelated bodies (review item P2). Run with
//! `cargo test -p qymcad-engine --release --test bench_repick -- --ignored --nocapture`.
use qymcad_engine::*;

fn extrusion(sketch: Id, op: Op) -> Extrude {
    Extrude {
        sketch,
        profiles: None,
        profiles_at: None,
        height: 10.0.into(),
        down: None,
        op,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    }
}

#[test]
#[ignore]
fn fillet_repick_among_unrelated_bodies() {
    let mut s = Session::new_part();
    let mut first = None;
    for i in 0..13 {
        let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        let x = 40.0 * i as f64;
        s.sketch_rect(sketch, &x.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
        let (body, _) = s.extrude(&extrusion(sketch, if i == 0 { Op::Add } else { Op::NewBody })).unwrap();
        let vertical: Vec<u32> =
            s.topology(Some(body), true).unwrap().edges.iter().filter(|e| (e.a[2] - e.b[2]).abs() > 9.9).map(|e| e.id).collect();
        let (feature, _) = s.fillet(Some(body), &Sel::Ids(vec![vertical[0]]), &2.0.into(), None).unwrap();
        first.get_or_insert((feature, s.topology(Some(body), true).unwrap()));
    }
    let (feature, _) = first.unwrap();
    let src = match &s.project().timeline.iter().find(|n| n.id == feature).unwrap().kind {
        qymcad_core::feature::FeatureKind::Fillet { src, .. } => *src,
        _ => unreachable!(),
    };
    let edges: Vec<u32> =
        s.topology(Some(src), true).unwrap().edges.iter().filter(|e| (e.a[2] - e.b[2]).abs() > 9.9).map(|e| e.id).collect();
    let mut times = Vec::new();
    for round in 0..6 {
        let t = std::time::Instant::now();
        s.repick_fillet(feature, &Sel::Ids(vec![edges[1 + round % 3]])).unwrap();
        times.push(t.elapsed().as_millis());
    }
    eprintln!("BENCH repick_fillet ms: {times:?}");
}
