use qymcad_core::{feature::FeatureKind, geom::ProfEdge};
use qymcad_engine::*;

fn cylinder(s: &mut Session) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &8.0.into(), false).unwrap();
    s.extrude(&Extrude {
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
    .0
}

fn rim(s: &mut Session, src: Id) -> Sel {
    let t = s.topology(Some(src), false).unwrap();
    Sel::Ids(vec![
        t.edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Circle)
            .min_by(|a, b| a.center.unwrap()[2].total_cmp(&b.center.unwrap()[2]))
            .unwrap()
            .id,
    ])
}

#[test]
fn review_large_custom_angle() {
    let mut s = Session::new_part();
    let src = cylinder(&mut s);
    let edge = rim(&mut s, src);
    let r = s.thread(
        Some(src),
        &edge,
        &ThreadOptions {
            standard: Some("custom".into()),
            nominal: Some(8.0.into()),
            pitch: Some(1.25.into()),
            length: 10.0.into(),
            angle: Some(160.0.into()),
            depth: Some(0.1.into()),
            fit: Some(0.0.into()),
            ..Default::default()
        },
    );
    if let Err(error) = &r {
        assert!(error.to_string().contains("angle"), "{error}");
        return;
    }
    let r = r.unwrap();
    let FeatureKind::Thread { spec, .. } = s.project().timeline.iter().find(|n| n.id == r.body).unwrap().kind else { panic!() };
    let angles: Vec<_> = spec
        .geometry()
        .groove
        .iter()
        .filter_map(|e| match e {
            ProfEdge::Line { a, b } if (a.x - b.x).abs() > 1e-5 && (a.y - b.y).abs() > 1e-5 => {
                Some(2.0 * ((a.x - b.x) / (a.y - b.y)).abs().atan().to_degrees())
            }
            _ => None,
        })
        .collect();
    println!("requested=160 reported={} actual_flank_angles={angles:?} errors={:?}", spec.geometry().angle_deg, r.rebuild.errors);
    assert_eq!(angles.len(), 2);
    assert!(angles.iter().all(|a| (a - 160.0).abs() < 0.01), "accepted angle was clamped");
}

#[test]
fn review_custom_depth_changes_flank_angle() {
    let mut s = Session::new_part();
    let src = cylinder(&mut s);
    let edge = rim(&mut s, src);
    let r = s.thread(
        Some(src),
        &edge,
        &ThreadOptions {
            standard: Some("custom".into()),
            nominal: Some(8.0.into()),
            pitch: Some(1.25.into()),
            length: 10.0.into(),
            angle: Some(60.0.into()),
            depth: Some(1.5.into()),
            fit: Some(0.0.into()),
            ..Default::default()
        },
    );
    if let Err(error) = &r {
        assert!(error.to_string().contains("maximum depth"), "{error}");
        return;
    }
    let r = r.unwrap();
    let FeatureKind::Thread { spec, .. } = s.project().timeline.iter().find(|n| n.id == r.body).unwrap().kind else { panic!() };
    let angles: Vec<_> = spec
        .geometry()
        .groove
        .iter()
        .filter_map(|e| match e {
            ProfEdge::Line { a, b } if (a.x - b.x).abs() > 1e-5 && (a.y - b.y).abs() > 1e-5 => {
                Some(2.0 * ((a.x - b.x) / (a.y - b.y)).abs().atan().to_degrees())
            }
            _ => None,
        })
        .collect();
    println!(
        "requested=60 depth=1.5 reported={} actual_flank_angles={angles:?} overflow={:?} errors={:?}",
        spec.geometry().angle_deg,
        spec.profile_overflow(),
        r.rebuild.errors
    );
    assert_eq!(angles.len(), 2);
    assert!(angles.iter().all(|a| (a - 60.0).abs() < 0.01), "accepted depth silently changed angle");
}

#[test]
fn review_rim_becomes_arc() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let circle = s.sketch_circle(sk, &0.0.into(), &0.0.into(), &8.0.into(), false).unwrap();
    s.sketch_line(
        sk,
        &LineSpec { x1: (-4.0).into(), y1: 0.0.into(), x2: 4.0.into(), y2: 0.0.into(), construction: false, dimensioned: true },
    )
    .unwrap();
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
    let t = s.topology(Some(src), false).unwrap();
    println!("initial topology edges: {:?}", t.edges.iter().map(|e| (e.id, e.kind)).collect::<Vec<_>>());
    let edge = rim(&mut s, src);
    let threaded =
        s.thread(Some(src), &edge, &ThreadOptions { size: Some("M8".into()), length: 20.0.into(), ..Default::default() }).unwrap();
    let edit = s.sketch_edit(sk, |s| s.sketch_trim(sk, &CurvePickSpec { curve: EntityRef::Id(circle), at: [0.0.into(), (-4.0).into()] }));
    println!("trim result={edit:?}");
    println!(
        "source edges now: {:?}; thread={}",
        s.topology(Some(src), false).unwrap().edges.iter().map(|e| (e.id, e.kind)).collect::<Vec<_>>(),
        threaded.body
    );
    assert!(edit.is_err(), "full rim became arc without refusing thread");
}

#[test]
fn review_split_small_piece_false_refusal() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &250.0.into(), &250.0.into(), &500.0.into(), &500.0.into(), false).unwrap();
    s.sketch_rect(sk, &600.5.into(), &0.5.into(), &1.0.into(), &1.0.into(), false).unwrap();
    let src = s
        .extrude(&Extrude {
            down: None,
            sketch: sk,
            profiles: None,
            height: 500.0.into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    let cap = s
        .topology(Some(src), false)
        .unwrap()
        .faces
        .iter()
        .find(|f| f.centroid[0] > 600.0 && f.normal.is_some_and(|n| n[2] > 0.9))
        .unwrap()
        .id;
    let (src, _) = s.push_face(Some(src), &Sel::Ids(vec![cap]), &(-499.0).into(), None).unwrap();
    let split = s
        .split(&SplitOptions {
            body: Some(src),
            plane: PlaneRef::Base(BaseName::XY),
            offset: 250.0.into(),
            name: Some("split".into()),
            piece_names: None,
        })
        .unwrap();
    let small = split.pieces.iter().find(|piece| piece.bbox[0] > 599.0).unwrap();
    let small_id = small.id;
    let small_bounds = small.bbox;
    assert!((small.volume - 1.0).abs() < 1e-7);
    let result = s.dimension_set(&DimensionTarget::Feature { feature: split.feature, key: "offset".into() }, Some(&251.0.into()), None);
    result.expect("unchanged 1mm cube has unique full overlap but is refused");
    let after = s.result_bodies();
    let small = after.iter().find(|piece| piece.id == small_id).unwrap();
    assert_eq!(small.bbox, small_bounds, "unchanged small piece keeps its native id and bounds");
    assert!((small.volume - 1.0).abs() < 1e-7);
    let lower = after.iter().find(|piece| piece.id != small_id && piece.bbox[2] < 1.0).unwrap();
    let upper = after.iter().find(|piece| piece.id != small_id && piece.bbox[2] > 250.0).unwrap();
    // Independent box formulas after moving the XY plane by 1mm.
    assert!((lower.volume - 500.0 * 500.0 * 251.0).abs() < 1e-5);
    assert!((upper.volume - 500.0 * 500.0 * 249.0).abs() < 1e-5);
    assert!((after.iter().map(|piece| piece.volume).sum::<f64>() - (500.0_f64.powi(3) + 1.0)).abs() < 1e-5);
}

#[test]
fn review_shallow_profile_relief_moves_wrong_way() {
    let mut s = Session::new_part();
    let src = cylinder(&mut s);
    let edge = rim(&mut s, src);
    let r = s.thread(
        Some(src),
        &edge,
        &ThreadOptions {
            standard: Some("custom".into()),
            nominal: Some(8.0.into()),
            pitch: Some(1.25.into()),
            length: 20.0.into(),
            angle: Some(60.0.into()),
            depth: Some(0.04.into()),
            fit: Some(0.1.into()),
            ..Default::default()
        },
    );
    if let Err(error) = &r {
        assert!(error.to_string().contains("side"), "{error}");
        return;
    }
    let r = r.unwrap();
    let mesh = qymcad_kernel::ask::export_mesh(s.shape(r.body).unwrap(), 0.001).unwrap().mesh;
    let minimum = mesh.verts.iter().filter(|p| p.z > 5.0 && p.z < 15.0).map(|p| p.x.hypot(p.y)).reduce(f64::min).unwrap();
    let relief = (0.1 - 0.04 * 1.25) / 30_f64.to_radians().tan();
    let expected = 4.0 - 0.04 - 0.1 - relief;
    println!("shallow fit=.1: radial_relief={} min_radius={minimum} expected={expected}; errors={:?}", r.radial_relief, r.rebuild.errors);
    assert!((minimum - expected).abs() < 0.005, "external relief moved outward instead of inward");
}
