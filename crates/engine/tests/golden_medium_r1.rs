//! Independent R1 regressions: sign safety, named coordinate ownership and numeric inputs.
mod common;
use common::{assert_close, scratch};
use qymcad_engine::*;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

fn stock(s: &mut Session) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch: sk,
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
    .0
}
fn hole_options(sketch: Id) -> HoleSketch {
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
fn coordinate(s: &Session, sk: Id, pt: Id) -> DimensionTarget {
    let ci = s
        .sketch_detail(sk)
        .unwrap()
        .constraints
        .into_iter()
        .find(|c| c.kind == "distance_point_line" && c.points.contains(&pt))
        .unwrap()
        .index;
    DimensionTarget::Sketch { sketch: sk, constraint: ci }
}
fn archive(s: &mut Session) -> BTreeMap<String, Vec<u8>> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = scratch(&format!("r1-archive-{}.qcad", NEXT.fetch_add(1, Ordering::Relaxed)));
    s.save_with_options(Some(&path), true).unwrap();
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut b = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut b).unwrap();
            (name, b)
        })
        .collect()
}
fn xy(s: &Session, sk: Id, pt: Id, x: f64) {
    let p = s.sketch_detail(sk).unwrap().points.into_iter().find(|p| p.id == pt).unwrap();
    assert_close(p.x, x, 1e-6, "signed point x");
}
#[test]
fn point_negative_literal_must_not_be_silently_absolute() {
    let mut s = Session::new_part();
    stock(&mut s);
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let pt = s.sketch_point(sk, &8.0.into(), &6.0.into(), true).unwrap().point;
    s.hole_by_sketch(&hole_options(sk)).unwrap();
    let target = coordinate(&s, sk, pt);
    let before = archive(&mut s);
    for value in [Num::from("-12+0"), (-12.0).into(), " -1.2e1 ".into()] {
        let e = s.dimension_set(&target, Some(&value), None).unwrap_err().to_string();
        assert!(e.contains("sign") || e.contains("does not solve"), "{e}");
        assert_eq!(archive(&mut s), before, "immediate refusal archive");
        xy(&s, sk, pt, 8.);
    }
    assert!(s.rebuild().errors.is_empty());
    assert_eq!(archive(&mut s), before, "separate rebuilt refusal archive");
}
#[test]
fn negative_side_zero_and_reused_points_keep_the_same_literal_policy() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let pt = s.sketch_point(sk, &(-8.0).into(), &6.0.into(), true).unwrap().point;
    s.sketch_line(
        sk,
        &LineSpec { x1: (-8.0).into(), y1: 6.0.into(), x2: (-9.0).into(), y2: 9.0.into(), construction: true, dimensioned: false },
    )
    .unwrap();
    let target = coordinate(&s, sk, pt);
    for (magnitude, x) in [(0., 0.), (12., -12.)] {
        s.dimension_set(&target, Some(&magnitude.into()), None).unwrap();
        xy(&s, sk, pt, x);
        let before = archive(&mut s);
        assert!(s.dimension_set(&target, Some(&(-5.).into()), None).is_err());
        assert_eq!(archive(&mut s), before);
    }
}
#[test]
fn accepted_point_writes_keep_full_precision_and_literal_formula_semantics() {
    let mut s = Session::new_part();
    s.param_set("px", &8.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let pt = s.sketch_point(sk, &"px".into(), &6.0.into(), true).unwrap().point;
    let target = coordinate(&s, sk, pt);
    let r = s.dimension_set(&target, Some(&"px+1".into()), None).unwrap();
    assert_eq!(r.dimension.expression.as_deref(), Some("px+1"));
    s.param_set("px", &10.0.into()).unwrap();
    xy(&s, sk, pt, 11.);
    let r = s.dimension_set(&target, Some(&"1.234567891234e0".into()), None).unwrap();
    assert_eq!(r.dimension.expression, None, "a literal clears the old formula");
    assert_eq!(r.dimension.value, Some(1.234567891234));
    s.param_set("px", &12.0.into()).unwrap();
    xy(&s, sk, pt, 1.234567891234);
}

#[test]
fn open_must_not_silently_absolute_negative_point_formula() {
    let mut s = Session::new_part();
    stock(&mut s);
    s.param_set("px", &8.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let pt = s.sketch_point(sk, &"px".into(), &6.0.into(), true).unwrap().point;
    let path = scratch("r1-point-negative-open.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.parameters.iter_mut().find(|p| p.name == "px").unwrap().expr = "-12".into();
    project.eval_parameters();
    project.mark_param_dependents_dirty_for("px");
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let e = Session::open(&path).err().expect("open must refuse opposite-side expression").to_string();
    assert!(e.contains(&sk.to_string()) && e.contains(&pt.to_string()) && e.contains("sign"), "{e}");
    assert_eq!(std::fs::read(path).unwrap(), bytes, "refusal does not rewrite saved recipe");
}
#[test]
fn exponent_numeric_hole_size_must_be_accepted() {
    for kind in [HoleKind::Plain, HoleKind::Counterbore, HoleKind::Countersink] {
        let mut s = Session::new_part();
        stock(&mut s);
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_point(sk, &6.0.into(), &4.0.into(), true).unwrap();
        let recess = kind != HoleKind::Plain;
        let (id, r) = s
            .hole_by_sketch(&HoleSketch {
                diameter: " 4e0 ".into(),
                depth: Some("+4e0".into()),
                kind,
                dia2: recess.then(|| "8e0".into()),
                depth2: recess.then(|| "1e0".into()),
                ..hole_options(sk)
            })
            .unwrap();
        assert!(r.errors.is_empty());
        for (key, expected) in [("diameter", 4.), ("depth", 4.), ("dia2", 8.), ("depth2", 1.)] {
            if recess || key == "diameter" || key == "depth" {
                let expr = s.project().feat_dim(id, key).unwrap();
                assert_eq!(qymcad_core::expr::eval(expr, &s.project().param_map()).unwrap(), expected);
            }
        }
        let pi = std::f64::consts::PI;
        let removed = match kind {
            HoleKind::Plain => 16. * pi,
            HoleKind::Counterbore => 28. * pi,
            HoleKind::Countersink => (12. + 28. / 3.) * pi,
        };
        assert_close(s.result_bodies()[0].volume, 12000. - removed, 2e-5, "canonical hole dimensions");
        let path = scratch(&format!("r1-numeric-{kind:?}.qcad"));
        s.save(Some(&path)).unwrap();
        let (opened, r) = Session::open(&path).unwrap();
        assert!(r.errors.is_empty());
        assert_close(opened.result_bodies()[0].volume, 12000. - removed, 2e-5, "saved canonical hole");
    }
}
fn named_fixture(with_hole: bool, via_parameter: bool) -> (Session, Id, Id, Option<Id>) {
    let mut s = Session::new_part();
    let a = stock(&mut s);
    let (m, _) =
        s.move_body(&Move { body: Some(a), translation: Some([8.0.into(), 0.0.into(), 0.0.into()]), rotation: None, name: None }).unwrap();
    s.dimension_set(&DimensionTarget::Feature { feature: m.body, key: "dx".into() }, None, Some("marker_x")).unwrap();
    stock(&mut s);
    if via_parameter {
        s.param_set("alias", &"marker_x+0".into()).unwrap();
    }
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("dependent markers")).unwrap();
    s.sketch_point(sk, &if via_parameter { "alias" } else { "marker_x" }.into(), &6.0.into(), true).unwrap();
    let h = with_hole.then(|| s.hole_by_sketch(&HoleSketch { name: Some("dependent bores".into()), ..hole_options(sk) }).unwrap().0);
    (s, m.body, sk, h)
}
fn named_deletion(with_hole: bool) {
    for via_parameter in [false, true] {
        let (mut s, source, sk, h) = named_fixture(with_hole, via_parameter);
        let before = archive(&mut s);
        let e = s.feature_delete(source, false).unwrap_err().to_string();
        assert!(e.contains(&sk.to_string()) && e.contains("dependent markers"), "{e}");
        if let Some(h) = h {
            assert!(e.contains(&h.to_string()) && e.contains("dependent bores"), "{e}");
        }
        assert_eq!(archive(&mut s), before, "immediate delete refusal archive");
        let snapshot = s.begin_tool_edit().unwrap();
        let r = s.feature_delete(source, true).unwrap();
        assert!(r.errors.is_empty());
        s.finish_tool_edit(snapshot, true);
        assert!(s.project().timeline.iter().all(|n| n.id != sk && Some(n.id) != h && n.id != source));
        // Globals remain the user's parameters. An alias still explicitly reading a deleted driver
        // must be removed before another evaluation; direct coordinate consumers leave no dead reads.
        if via_parameter {
            s.param_delete("alias").unwrap();
        }
        s.param_set("unrelated", &1.0.into()).unwrap();
        let deleted = scratch(&format!("r1-deleted-{with_hole}-{via_parameter}.qcad"));
        s.save(Some(&deleted)).unwrap();
        let (_, r) = Session::open(&deleted).unwrap();
        assert!(r.errors.is_empty(), "deleted coordinate consumers must not leave a dead saved driver: {r:?}");
        s.undo().unwrap();
        assert_eq!(archive(&mut s), before, "undo restores deleted source and coordinate consumers");
        let path = scratch(&format!("r1-named-{with_hole}-{via_parameter}.qcad"));
        s.save(Some(&path)).unwrap();
        let (_, r) = Session::open(Path::new(&path)).unwrap();
        assert!(r.errors.is_empty());
    }
}
#[test]
fn removing_named_coordinate_source_must_not_leave_live_hole_with_dead_driver() {
    named_deletion(true);
}
#[test]
fn removing_named_coordinate_source_must_not_leave_plain_point_with_dead_driver() {
    named_deletion(false);
}
