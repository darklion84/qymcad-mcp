//! Round-2 profile guards use independent line/arc measurements and actual output roots.
use qymcad_core::{
    feature::FeatureKind,
    geom::ProfEdge,
    thread::{ThreadSpec, ThreadStandard},
};
use qymcad_engine::*;

fn fixture(internal: bool) -> (Session, Id, Sel) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    if internal {
        s.sketch_circle(sk, &0.0.into(), &0.0.into(), &16.0.into(), false).unwrap();
    }
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
    let t = s.topology(Some(src), false).unwrap();
    let edge = t
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Circle && (e.radius.unwrap() - 4.0).abs() < 1e-5)
        .min_by(|a, b| a.center.unwrap()[2].total_cmp(&b.center.unwrap()[2]))
        .unwrap()
        .id;
    (s, src, Sel::Ids(vec![edge]))
}
fn options() -> ThreadOptions {
    ThreadOptions {
        standard: Some("custom".into()),
        nominal: Some(8.0.into()),
        pitch: Some(1.25.into()),
        length: 20.0.into(),
        depth: Some(0.4.into()),
        fit: Some(0.0.into()),
        lead_in: Some(0.0.into()),
        lead_out: Some(0.0.into()),
        ..Default::default()
    }
}
fn spec(s: &Session, body: Id) -> ThreadSpec {
    let FeatureKind::Thread { spec, .. } = s.project().timeline.iter().find(|n| n.id == body).unwrap().kind else { unreachable!() };
    spec
}
fn angles(spec: ThreadSpec) -> Vec<f64> {
    spec.geometry()
        .groove
        .iter()
        .filter_map(|e| match e {
            ProfEdge::Line { a, b } if (a.x - b.x).abs() > 1e-8 && (a.y - b.y).abs() > 1e-8 => {
                Some(2.0 * ((a.x - b.x) / (a.y - b.y)).abs().atan().to_degrees())
            }
            _ => None,
        })
        .collect()
}
fn root(s: &Session, body: Id, internal: bool) -> f64 {
    let mesh = qymcad_kernel::ask::export_mesh(s.shape(body).unwrap(), 0.001).unwrap().mesh;
    let radii = mesh.verts.iter().filter(|p| p.z > 5.0 && p.z < 15.0 && p.x.hypot(p.y) < 6.0).map(|p| p.x.hypot(p.y));
    if internal {
        radii.reduce(f64::max).unwrap()
    } else {
        radii.reduce(f64::min).unwrap()
    }
}
#[test]
fn exact_upper_angle_boundary_is_native_half_angle_not_180() {
    let maximum = 2.6_f64.to_degrees();
    for angle in [maximum - 1e-9, maximum, maximum + 1e-9] {
        let raw = ThreadSpec {
            standard: ThreadStandard::Custom,
            nominal_d: 8.0,
            pitch: 1.25,
            custom_depth: 0.1,
            custom_angle: angle,
            ..Default::default()
        };
        let measured = angles(raw);
        assert_eq!(measured.len(), 2);
        assert!(measured.iter().all(|v| (*v - angle.min(maximum)).abs() < 1e-11));
        let (mut s, src, edge) = fixture(false);
        let error = s
            .thread(Some(src), &edge, &ThreadOptions { angle: Some(angle.into()), depth: Some(0.1.into()), ..options() })
            .unwrap_err()
            .to_string();
        // At the angle ceiling a feasible profile is too shallow to infer the side correctly.
        assert_eq!(error.contains("half-angle above"), angle > maximum, "{angle}: {error}");
    }
}
#[test]
fn custom_guards_recheck_parameter_and_dimension_edits_atomically() {
    for (key, initial, bad, needle) in [("angle", 60.0, 160.0, "angle"), ("depth", 0.4, 1.5, "maximum depth"), ("depth", 0.4, 0.04, "side")]
    {
        for parameter in [false, true] {
            let (mut s, src, edge) = fixture(false);
            s.param_set("Profile", &initial.into()).unwrap();
            let mut o = options();
            if key == "angle" {
                o.angle = Some(Num::Expr("Profile".into()));
            } else {
                o.depth = Some(Num::Expr("Profile".into()));
            }
            let body = s.thread(Some(src), &edge, &o).unwrap().body;
            let before = s.info();
            let volume = s.shape(body).unwrap().volume();
            let error = if parameter {
                s.param_set("Profile", &bad.into()).unwrap_err()
            } else {
                s.dimension_set(&DimensionTarget::Feature { feature: body, key: key.into() }, Some(&bad.into()), None).unwrap_err()
            };
            assert!(error.to_string().contains(needle), "{key}: {error}");
            assert_eq!(s.info(), before);
            assert_eq!(s.shape(body).unwrap().volume(), volume);
        }
    }
}
#[test]
fn fit_counts_toward_profile_feasibility() {
    // Native public check accepts .96 (<.6125/tan30), yet .96+.12 closes the root.
    let raw =
        ThreadSpec { standard: ThreadStandard::Custom, nominal_d: 8.0, pitch: 1.25, custom_depth: 0.96, fit: 0.12, ..Default::default() };
    assert!(raw.profile_overflow().is_none());
    assert!(angles(raw).iter().any(|a| (*a - 60.0).abs() > 0.1));
    let (mut s, src, edge) = fixture(false);
    let result = s.thread(Some(src), &edge, &ThreadOptions { depth: Some(0.96.into()), fit: Some(0.12.into()), ..options() });
    assert!(result.is_err(), "native changed the angle even though public feasibility passed");
}
#[test]
fn both_sides_of_shallow_boundary_have_correct_measured_roots() {
    for internal in [false, true] {
        for delta in [-1e-5, 0.0, 1e-5] {
            let depth = 0.15 * 1.25 - 0.1 + delta;
            let (mut s, src, edge) = fixture(internal);
            let result = s.thread(Some(src), &edge, &ThreadOptions { depth: Some(depth.into()), fit: Some(0.1.into()), ..options() });
            let accepted = delta > 0.0 || (!internal && delta == 0.0);
            if !accepted {
                assert!(result.unwrap_err().to_string().contains("side"));
                continue;
            }
            let r = result.unwrap();
            let relief = (0.1 - 0.04 * 1.25) / 30_f64.to_radians().tan();
            let expected = 4.0 + if internal { depth + 0.1 + relief } else { -depth - 0.1 - relief };
            assert!((root(&s, r.body, internal) - expected).abs() < 0.003, "internal={internal} delta={delta}");
        }
    }
}
#[test]
fn iso_internal_stock_adjusted_profile_is_checked() {
    let (mut s, src, _) = fixture(true);
    // Bore diameter is independently changed before creating the thread.
    let sk = s.project().sketches[0].id;
    let c = s.project().sketches[0]
        .constraints
        .iter()
        .position(|c| matches!(c,qymcad_core::model::Constraint::Diameter{d,..} if (*d-8.0).abs()<1e-8))
        .unwrap();
    s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: c }, Some(&7.9.into()), None).unwrap();
    let t = s.topology(Some(src), false).unwrap();
    let e = t
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Circle && (e.radius.unwrap() - 3.95).abs() < 1e-4)
        .min_by(|a, b| a.center.unwrap()[2].total_cmp(&b.center.unwrap()[2]))
        .unwrap()
        .id;
    let result = s.thread(Some(src), &Sel::Ids(vec![e]), &ThreadOptions { standard: Some("metric_iso".into()), depth: None, ..options() });
    assert!(result.unwrap_err().to_string().contains("side"));
}
#[test]
fn audited_explicit_inputs_are_preserved_or_refused() {
    let cases = [("pitch", 1e-10), ("crest_r", 1.0), ("root_r", 1.0), ("crest_r", 5e-10), ("root_r", 5e-10), ("depth", 5e-10)];
    let mut accepted = Vec::new();
    for (key, v) in cases {
        let (mut s, src, edge) = fixture(false);
        let mut o = options();
        match key {
            "pitch" => o.pitch = Some(v.into()),
            "crest_r" => o.crest_r = Some(v.into()),
            "root_r" => o.root_r = Some(v.into()),
            "depth" => o.depth = Some(v.into()),
            _ => unreachable!(),
        }
        match s.thread(Some(src), &edge, &o) {
            Err(e) => println!("{key}={v} refused: {e}"),
            Ok(r) => {
                let actual = spec(&s, r.body).geometry();
                println!("{key}={v} accepted groove={:?}", actual.groove);
                accepted.push((key, v));
            }
        }
    }
    assert!(accepted.is_empty(), "silently changed explicit inputs: {accepted:?}");
}
#[test]
fn ignored_standard_overrides_and_false_buttress_are_refused() {
    let raw = ThreadSpec { standard: ThreadStandard::Buttress, pitch: 1.25, ..Default::default() };
    assert!(angles(raw).iter().all(|a| (*a - 52.0).abs() < 1e-9), "native symmetric26/26, not7/45");
    for (standard, angle, depth) in [("metric_iso", 75.0, 0.0), ("metric_iso", 60.0, 0.4), ("buttress", 52.0, 0.0)] {
        let (mut s, src, edge) = fixture(false);
        let result = s.thread(
            Some(src),
            &edge,
            &ThreadOptions { standard: Some(standard.into()), angle: Some(angle.into()), depth: Some(depth.into()), ..options() },
        );
        if let Ok(r) = &result {
            println!("{standard} requested angle={angle} depth={depth} actual={:?}", spec(&s, r.body).geometry());
        }
        assert!(result.is_err(), "ignored/false native profile {standard}/{angle}/{depth}");
    }
}
#[test]
fn radius_near_full_rounding_must_not_snap() {
    // Custom .4 deep: root half-width=.45P-.4tan30; full radius=wb/(sec30-tan30).
    let full = (0.45 * 1.25 - 0.4 * 30_f64.to_radians().tan()) / (1.0 / 30_f64.to_radians().cos() - 30_f64.to_radians().tan());
    // h*.45 binds at .18, so use metric root near its full radius instead.
    assert!(full > 0.18);
    let requested = 0.1804225;
    let (mut s, src, edge) = fixture(false);
    let result = s.thread(
        Some(src),
        &edge,
        &ThreadOptions { standard: Some("metric_iso".into()), depth: Some(0.0.into()), root_r: Some(requested.into()), ..options() },
    );
    if let Ok(r) = &result {
        let measured: Vec<_> = spec(&s, r.body)
            .geometry()
            .groove
            .iter()
            .filter_map(|e| if let ProfEdge::Arc { a, center, .. } = e { Some((a.x - center.x).hypot(a.y - center.y)) } else { None })
            .collect();
        println!("requested root={requested} measured={measured:?}");
    }
    assert!(result.is_err(), "explicit root radius snapped upward");
}
