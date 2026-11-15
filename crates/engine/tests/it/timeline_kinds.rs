//! `timeline[].kind` is a public word per node; a document with many kinds of node reports each as before.
use qymcad_engine::*;

fn rect_extrude(s: &mut Session, op: Op, x: f64) {
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sketch, &x.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let spec = Extrude {
        down: None,
        sketch,
        profiles: None,
        profiles_at: None,
        height: 10.0.into(),
        op,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    };
    let (_, report) = s.extrude(&spec).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
}

#[test]
fn timeline_kinds_are_the_lowercased_variant_words() {
    let mut s = Session::new_part();
    rect_extrude(&mut s, Op::Add, 0.0);
    rect_extrude(&mut s, Op::Cut, 5.0);
    rect_extrude(&mut s, Op::Intersect, 2.0);
    s.plane_offset(&PlaneRef::Base(BaseName::XY), &15.0.into(), None).unwrap();
    s.primitive(&PrimitiveSpec::Cylinder { r: 3.0.into(), h: 4.0.into() }, &Placement::Origin, None).unwrap();
    s.primitive(&PrimitiveSpec::Sphere { r: 2.0.into() }, &Placement::Origin, None).unwrap();
    s.primitive(&PrimitiveSpec::Box { dx: 1.0.into(), dy: 1.0.into(), dz: 1.0.into() }, &Placement::Origin, None).unwrap();
    s.primitive(&PrimitiveSpec::Cone { r1: 2.0.into(), r2: 1.0.into(), h: 3.0.into() }, &Placement::Origin, None).unwrap();
    s.primitive(&PrimitiveSpec::Torus { major: 5.0.into(), minor: 1.0.into() }, &Placement::Origin, None).unwrap();
    s.primitive(&PrimitiveSpec::Prism { r: 3.0.into(), h: 2.0.into(), n: 6.0.into() }, &Placement::Origin, None).unwrap();

    let timeline = s.timeline();
    let old: Vec<String> = s
        .project()
        .timeline
        .iter()
        .map(|n| {
            let debug = format!("{:?}", n.kind);
            let word = debug.split(|c: char| !c.is_alphanumeric()).next().unwrap().to_lowercase();
            match word.as_str() {
                "combine" => match &n.kind {
                    qymcad_core::feature::FeatureKind::Combine { op: 0, .. } => "extrude (cut)".into(),
                    qymcad_core::feature::FeatureKind::Combine { op: 1, .. } => "extrude (add)".into(),
                    _ => "extrude (intersect)".into(),
                },
                _ => word,
            }
        })
        .collect();
    let new: Vec<String> = timeline.iter().map(|n| n.kind.clone()).collect();
    assert_eq!(new, old);
    for want in
        ["sketch", "extrude", "plane", "cylinder", "sphere", "box3", "cone", "torus", "prism", "extrude (cut)", "extrude (intersect)"]
    {
        assert!(new.iter().any(|k| k == want), "no `{want}` in {new:?}");
    }
}
