//! Independent standard design-profile formulas, rather than native cutter diameters.
//! Sources and distinction from fit/cutter dimensions: FINDINGS F-105.
use qymcad_engine::*;

#[test]
fn every_accepted_standard_reports_basic_diameters_for_both_sides() {
    let mut failures = Vec::new();
    let (d, p) = (16.0, 2.0);
    for standard in ["metric_iso", "trapezoidal_tr", "acme", "round_rd", "custom"] {
        for internal in [false, true] {
            let (major, pitch, minor) = match standard {
                "metric_iso" => (
                    d,
                    d - 3.0 * 3.0_f64.sqrt() * p / 8.0,
                    d - if internal { 5.0 * 3.0_f64.sqrt() / 8.0 } else { 17.0 * 3.0_f64.sqrt() / 24.0 } * p,
                ),
                "trapezoidal_tr" => (d + if internal { 0.5 } else { 0.0 }, d - p / 2.0, d - p - if internal { 0.0 } else { 0.5 }),
                "acme" => (d, d - p / 2.0, d - p),
                "round_rd" => (d + if internal { 0.1 * p } else { 0.0 }, d - p / 2.0, d - if internal { 0.9 } else { 1.0 } * p),
                "custom" => (d, d - 0.4 * p, d - 0.8 * p),
                _ => unreachable!(),
            };
            let mut s = Session::new_part();
            let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
            if internal {
                s.sketch_circle(sk, &0.0.into(), &0.0.into(), &24.0.into(), false).unwrap();
            }
            let rim_d = if internal { minor } else { d };
            s.sketch_circle(sk, &0.0.into(), &0.0.into(), &rim_d.into(), false).unwrap();
            let src = s
                .extrude(&Extrude {
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
                .filter(|e| e.kind == EdgeKind::Circle && (2.0 * e.radius.unwrap() - rim_d).abs() < 1e-5)
                .min_by(|a, b| a.center.unwrap()[2].total_cmp(&b.center.unwrap()[2]))
                .unwrap()
                .id;
            let r = s
                .thread(
                    Some(src),
                    &Sel::Ids(vec![edge]),
                    &ThreadOptions {
                        standard: Some(standard.into()),
                        nominal: Some(d.into()),
                        pitch: Some(p.into()),
                        length: 8.0.into(),
                        fit: Some(0.0.into()),
                        lead_in: Some(0.0.into()),
                        lead_out: Some(0.0.into()),
                        depth: (standard == "custom").then_some((0.4 * p).into()),
                        ..Default::default()
                    },
                )
                .unwrap();
            for (field, got, expected) in [
                ("major", r.basic_major_diameter, major),
                ("pitch", r.basic_pitch_diameter, pitch),
                ("minor", r.basic_minor_diameter, minor),
            ] {
                if (got - expected).abs() >= 2e-6 {
                    failures.push(format!("{standard} internal={internal} {field}: {got} != {expected}"));
                }
            }
            assert_eq!(r.kind, if internal { "internal" } else { "external" });
            assert!(r.rebuild.errors.is_empty());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
