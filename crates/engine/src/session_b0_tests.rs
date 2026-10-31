//! B0: the engine array cap must refuse before kernel work, not after a native rebuild of every copy.
use super::*;
use crate::*;

/// A saved array whose count parameter was raised outside the engine (native app edit or crafted file).
fn oversized(count: &str) -> Session {
    let mut s = Session::new_part();
    s.param_set("n", &4.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    let b = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            profiles_at: None,
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
    s.linear_array(Some(b), &ArrayDir { dx: 20.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: "n".into() }, None, None).unwrap();
    s.p.parameters.iter_mut().find(|p| p.name == "n").unwrap().expr = count.into();
    s.p.eval_parameters();
    s.p.mark_all_dirty();
    s
}

/// Kernel work is observable through native regeneration of the array node: a refused rebuild must not
/// have produced a shape for more copies than the cap. Timing is the only outside signal, so the bound is wide.
#[test]
fn standalone_rebuild_refuses_array_over_cap_before_kernel() {
    let mut s = oversized("10000");
    let t = std::time::Instant::now();
    let report = s.rebuild();
    let dt = t.elapsed();
    assert!(report.errors.iter().any(|e| e.message.contains("count") || e.message.contains("copies")), "{report:?}");
    assert!(dt < std::time::Duration::from_secs(3), "rebuilt {dt:?} before refusing: kernel built the oversized array");
}

#[test]
fn open_refuses_array_over_cap_before_kernel() {
    let mut s = oversized("10000");
    let path = std::env::temp_dir().join(format!("b0-array-open-{}-{:?}.qcad", std::process::id(), std::thread::current().id()));
    s.save_with_options(Some(&path), true).unwrap();
    let before = std::fs::read(&path).unwrap();
    let t = std::time::Instant::now();
    let error = match Session::open(&path) {
        Ok(_) => panic!("opened an array over the cap"),
        Err(e) => e.to_string(),
    };
    let dt = t.elapsed();
    assert!(error.contains("count") || error.contains("copies"), "{error}");
    assert!(dt < std::time::Duration::from_secs(3), "open took {dt:?} before refusing: kernel built the oversized array");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_file(path).unwrap();
}

/// B4c: a parameter edit raising a sketch-pattern count over the cap must refuse before the native evaluation
/// regenerates the instances (it imposes pattern counts itself); the document stays exactly as it was.
#[test]
fn param_set_refuses_sketch_pattern_count_over_cap_before_instances() {
    let mut s = Session::new_part();
    s.param_set("n", &3.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let source = s.sketch_rect(sk, &5.0.into(), &5.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    s.sketch_pattern_create(
        sk,
        &source,
        &SketchPatternKind::Linear { count: "n".into(), dx: 15.0.into(), dy: 0.0.into(), count2: None, dx2: None, dy2: None },
    )
    .unwrap();
    let entities = s.p.sketches[s.sketch_si(sk).unwrap()].entities.len();
    let t = std::time::Instant::now();
    let error = s.param_set("n", &100000.0.into()).unwrap_err();
    assert!(t.elapsed() < std::time::Duration::from_secs(60), "refusal took {:?}", t.elapsed());
    assert!(error.to_string().contains("count"), "{error}");
    assert_eq!(s.p.sketches[s.sketch_si(sk).unwrap()].entities.len(), entities, "no instance may be generated");
    assert_eq!(s.p.param_map().get("n"), Some(&3.0));
    // Within the cap the edit still applies.
    s.param_set("n", &5.0.into()).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().contours.len(), 5);
}
