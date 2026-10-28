use qymcad_core::geom::ProfEdge;
use qymcad_engine::*;

fn stock() -> (Session, Id, Sel) {
    stock_side(false)
}

fn stock_side(internal: bool) -> (Session, Id, Sel) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    if internal {
        s.sketch_circle(sk, &0.0.into(), &0.0.into(), &16.0.into(), false).unwrap();
    }
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &8.0.into(), false).unwrap();
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
        length: 10.0.into(),
        depth: Some(0.4.into()),
        fit: Some(0.0.into()),
        lead_in: Some(0.0.into()),
        lead_out: Some(0.0.into()),
        ..Default::default()
    }
}

fn native_custom_arcs(depth: f64, fit: f64) -> Vec<(f64, f64)> {
    let raw = qymcad_core::thread::ThreadSpec {
        standard: qymcad_core::thread::ThreadStandard::Custom,
        nominal_d: 8.0,
        pitch: 1.25,
        custom_depth: depth,
        fit,
        crest_r: Some(0.05),
        root_r: Some(0.05),
        ..Default::default()
    };
    raw.geometry()
        .groove
        .into_iter()
        .filter_map(|edge| match edge {
            ProfEdge::Arc { a, center, .. } => Some((center.y, (a.x - center.x).hypot(a.y - center.y))),
            _ => None,
        })
        .collect()
}

#[test]
fn clamped_crest_is_refused_even_when_root_has_requested_radius() {
    let arcs = native_custom_arcs(0.4, 0.1);
    assert_eq!(arcs.len(), 2);
    assert!(arcs.iter().all(|(y, r)| *y < -0.4 && (r - 0.05).abs() < 1e-12), "only root arcs remain: {arcs:?}");
    let (mut s, src, edge) = stock();
    let before = s.info();
    let error = s
        .thread(
            Some(src),
            &edge,
            &ThreadOptions { fit: Some(0.1.into()), crest_r: Some(0.05.into()), root_r: Some(0.05.into()), ..options() },
        )
        .unwrap_err();
    assert!(error.to_string().contains("crest_r"), "{error}");
    assert_eq!(s.info(), before);
}

#[test]
fn clamped_root_is_refused_even_when_crest_has_requested_radius() {
    let arcs = native_custom_arcs(0.97, 0.0);
    assert!(arcs.iter().any(|(y, r)| *y > -0.1 && (r - 0.05).abs() < 1e-12), "crest: {arcs:?}");
    assert!(arcs.iter().any(|(y, r)| *y < -0.9 && (r - 0.00427857925749).abs() < 1e-12), "clamped root: {arcs:?}");
    let (mut s, src, edge) = stock();
    let before = s.info();
    let error = s
        .thread(
            Some(src),
            &edge,
            &ThreadOptions { depth: Some(0.97.into()), crest_r: Some(0.05.into()), root_r: Some(0.05.into()), ..options() },
        )
        .unwrap_err();
    assert!(error.to_string().contains("root_r"), "{error}");
    assert_eq!(s.info(), before);
}

#[test]
fn numeric_string_runout_cannot_bypass_refusal() {
    let (mut s, src, edge) = stock();
    let r = s.thread(Some(src), &edge, &options()).unwrap();
    let before = s.info();
    let error = s
        .dimension_set(&DimensionTarget::Feature { feature: r.body, key: "lead_in".into() }, Some(&Num::Expr("0.000001".into())), None)
        .unwrap_err();
    assert!(error.to_string().contains("lead_in"), "{error}");
    assert_eq!(s.info(), before);
}
#[test]
fn numeric_string_length_keeps_every_digit() {
    let (mut s, src, edge) = stock();
    let r = s.thread(Some(src), &edge, &options()).unwrap();
    let edit = s
        .dimension_set(&DimensionTarget::Feature { feature: r.body, key: "length".into() }, Some(&Num::Expr("1.234567891234".into())), None)
        .unwrap();
    assert_eq!(edit.dimension.value, Some(1.234567891234));
    assert_eq!(s.project().feat_dim(r.body, "length"), Some("1.234567891234"));
}

#[test]
fn three_default_splits_have_unique_resolvable_names() {
    let (mut s, src, _) = stock();
    let first = s
        .split(&SplitOptions { body: Some(src), plane: PlaneRef::Base(BaseName::XY), offset: 10.0.into(), name: None, piece_names: None })
        .unwrap();
    let second = s
        .split(&SplitOptions {
            body: Some(first.pieces[0].id),
            plane: PlaneRef::Base(BaseName::XY),
            offset: 5.0.into(),
            name: None,
            piece_names: None,
        })
        .unwrap();
    let third = s
        .split(&SplitOptions {
            body: Some(second.pieces[0].id),
            plane: PlaneRef::Base(BaseName::XY),
            offset: 2.5.into(),
            name: None,
            piece_names: None,
        })
        .unwrap();
    let mut names = std::collections::HashSet::new();
    for piece in first.pieces.iter().chain(&second.pieces).chain(&third.pieces) {
        assert!(names.insert(piece.name.clone()), "duplicate {}", piece.name);
        assert_eq!(s.resolve(&piece.name).unwrap(), piece.id);
        assert!((s.shape(piece.id).unwrap().volume() - 16.0 * std::f64::consts::PI * (piece.bbox[5] - piece.bbox[2])).abs() < 0.02);
    }
}

#[test]
fn stock_specific_valid_crest_radius_matches_fresh_native_build() {
    let mut s = Session::new_part();
    s.param_set("Bore", &6.75.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &16.0.into(), false).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &"Bore".into(), false).unwrap();
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
        .filter(|e| e.kind == EdgeKind::Circle && (e.radius.unwrap() - 3.375).abs() < 1e-5)
        .min_by(|a, b| a.center.unwrap()[2].total_cmp(&b.center.unwrap()[2]))
        .unwrap()
        .id;
    let raw = qymcad_core::thread::ThreadSpec { nominal_d: 8.0, pitch: 1.25, internal: true, crest_r: Some(0.26), ..Default::default() };
    let radii: Vec<_> = raw
        .geometry_in(Some(6.75))
        .groove
        .into_iter()
        .filter_map(|e| if let ProfEdge::Arc { a, center, .. } = e { Some((a.x - center.x).hypot(a.y - center.y)) } else { None })
        .collect();
    assert!(radii.iter().any(|r| (r - 0.26).abs() < 1e-12));
    let mut p = s.project().clone();
    let body = p.add_thread(src, edge, raw, 10.0, 0.0, 0.0);
    let (report, shapes) = qymcad_testkit::regenerate_with_shapes(&mut p, Default::default());
    assert!(report.errors.is_empty(), "native errors: {:?}", report.errors);
    assert!(shapes[&body].volume() < s.shape(src).unwrap().volume());
    let result = s
        .thread(
            Some(src),
            &Sel::Ids(vec![edge]),
            &ThreadOptions {
                size: Some("M8".into()),
                length: 10.0.into(),
                fit: Some(0.0.into()),
                crest_r: Some(0.26.into()),
                lead_in: Some(0.0.into()),
                lead_out: Some(0.0.into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(result.rebuild.errors.is_empty());
    assert!((s.shape(result.body).unwrap().volume() - shapes[&body].volume()).abs() < 1e-6);
    let before = s.info();
    let volume = s.shape(result.body).unwrap().volume();
    let error =
        s.dimension_set(&DimensionTarget::Feature { feature: result.body, key: "crest_r".into() }, Some(&0.3.into()), None).unwrap_err();
    assert!(error.to_string().contains("crest_r"), "{error}");
    assert_eq!(s.info(), before);
    assert_eq!(s.shape(result.body).unwrap().volume(), volume);
    let error = s.param_set("Bore", &6.6.into()).unwrap_err();
    assert!(error.to_string().contains("crest_r"), "{error}");
    assert_eq!(s.info(), before);
    // A valid dependent edit must still use this bore, and match a clean native build.
    s.dimension_set(&DimensionTarget::Feature { feature: result.body, key: "length".into() }, Some(&9.0.into()), None).unwrap();
    p.set_feat_dim(body, "length", "9".into());
    let (report, fresh) = qymcad_testkit::regenerate_with_shapes(&mut p, Default::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!((s.shape(result.body).unwrap().volume() - fresh[&body].volume()).abs() < 1e-6);
}
#[test]
fn internal_tr_reports_iso2904_basic_diameters() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &24.0.into(), false).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &12.0.into(), false).unwrap();
    let src = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: 40.0.into(),
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
        .filter(|e| e.kind == EdgeKind::Circle && (e.radius.unwrap() - 6.0).abs() < 1e-5)
        .min_by(|a, b| a.center.unwrap()[2].total_cmp(&b.center.unwrap()[2]))
        .unwrap()
        .id;
    let r = s
        .thread(
            Some(src),
            &Sel::Ids(vec![edge]),
            &ThreadOptions {
                size: Some("Tr16x4".into()),
                length: 40.0.into(),
                fit: Some(0.0.into()),
                lead_in: Some(0.0.into()),
                lead_out: Some(0.0.into()),
                ..Default::default()
            },
        )
        .unwrap();
    let mesh = qymcad_kernel::ask::export_mesh(s.shape(r.body).unwrap(), 0.002).unwrap().mesh;
    let max_radius =
        mesh.verts.iter().filter(|p| p.z > 8.0 && p.z < 32.0 && p.x.hypot(p.y) < 10.0).map(|p| p.x.hypot(p.y)).reduce(f64::max).unwrap();
    println!(
        "Tr16x4 internal fit=0 bore=12: basic major={},minor={}, root measured diameter={}",
        r.basic_major_diameter,
        r.basic_minor_diameter,
        2.0 * max_radius
    );
    assert_eq!(r.basic_major_diameter, 16.5);
    assert_eq!(r.basic_minor_diameter, 12.0);
    assert!((2.0 * max_radius - 16.5).abs() < 0.02);
}
#[test]
fn mapped_bitmap_only_glyph_is_refused_atomically() {
    let font_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/glyph-gap.ttf");
    let bytes = std::fs::read(&font_path).unwrap();
    assert_eq!(qymcad_core::text::family_name(&bytes, 0).unwrap(), "Review Glyph Gap");
    assert!(qymcad_core::text::can_write(&bytes, 0, "AB"));
    assert_eq!(
        qymcad_core::text::text_outline_contours(&bytes, 0, "A", 10.0, 0.0, 0.0),
        qymcad_core::text::text_outline_contours(&bytes, 0, "AB", 10.0, 0.0, 0.0)
    );
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let before = s.sketch_info(sk).unwrap();
    let error = s
        .sketch_text(
            sk,
            &TextSpec {
                text: "AB".into(),
                at: [0.0, 0.0],
                height: 10.0,
                angle: 0.0,
                font: Some(TextFont { path: font_path.to_string_lossy().into(), index: 0 }),
                construction: false,
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains('B'), "{error}");
    assert_eq!(s.sketch_info(sk).unwrap(), before);
}

#[test]
fn equal_crest_and_root_radii_are_measured_on_their_own_arcs_and_edits_refuse_clamps() {
    use qymcad_core::{feature::FeatureKind, thread::ThreadSpec};
    for internal in [false, true] {
        let raw = ThreadSpec {
            standard: qymcad_core::thread::ThreadStandard::Custom,
            nominal_d: 8.0,
            pitch: 1.25,
            internal,
            custom_depth: 0.4,
            crest_r: Some(0.05),
            root_r: Some(0.05),
            ..Default::default()
        };
        let mut crest = Vec::new();
        let mut root = Vec::new();
        // Independent oracle uses measured radial positions, not native arc winding.
        for e in raw.geometry().groove {
            if let ProfEdge::Arc { a, center, .. } = e {
                let radius = (a.x - center.x).hypot(a.y - center.y);
                let y = if internal { center.y } else { -center.y };
                if y < 0.2 {
                    crest.push(radius);
                } else {
                    root.push(radius);
                }
            }
        }
        assert_eq!((crest.len(), root.len()), (2, 2));
        assert!(crest.iter().chain(&root).all(|r| (r - 0.05).abs() < 1e-12));
    }
    for internal in [false, true] {
        let (mut s, src, edge) = stock_side(internal);
        let r = s.thread(Some(src), &edge, &ThreadOptions { crest_r: Some(0.05.into()), root_r: Some(0.05.into()), ..options() }).unwrap();
        let FeatureKind::Thread { spec, .. } = s.project().timeline.iter().find(|n| n.id == r.body).unwrap().kind else { panic!() };
        assert_eq!(spec.crest_r, spec.root_r);
        for (key, value) in [("depth", 0.97), ("fit", 0.1)] {
            let before = s.info();
            let error =
                s.dimension_set(&DimensionTarget::Feature { feature: r.body, key: key.into() }, Some(&value.into()), None).unwrap_err();
            assert!(error.to_string().contains(if key == "depth" { "root_r" } else { "crest_r" }), "{error}");
            assert_eq!(s.info(), before);
        }
    }
}

#[test]
fn numeric_strings_and_formulas_keep_their_native_meaning() {
    let (mut s, src, edge) = stock();
    let r = s.thread(Some(src), &edge, &options()).unwrap();
    for text in [" 1.234567891234 ", "+1.234567891234", "1.234567891234e0"] {
        let result =
            s.dimension_set(&DimensionTarget::Feature { feature: r.body, key: "length".into() }, Some(&text.into()), None).unwrap();
        assert_eq!(result.dimension.value, Some(1.234567891234));
    }
    // A true formula remains stored as a formula, including at the native runout threshold.
    let length = s
        .dimension_set(
            &DimensionTarget::Feature { feature: r.body, key: "length".into() },
            Some(&"1234567891234/1000000000000".into()),
            None,
        )
        .unwrap();
    assert_eq!(length.dimension.expression.as_deref(), Some("1234567891234/1000000000000"));
    let fit = s.dimension_set(&DimensionTarget::Feature { feature: r.body, key: "fit".into() }, Some(&"1/1000000".into()), None).unwrap();
    assert_eq!(fit.dimension.expression.as_deref(), Some("1/1000000"));
    assert_eq!(fit.dimension.value, Some(0.000001));
    let before = s.info();
    let error =
        s.dimension_set(&DimensionTarget::Feature { feature: r.body, key: "lead_in".into() }, Some(&"1/1000000".into()), None).unwrap_err();
    assert!(error.to_string().contains("lead_in"), "{error}");
    assert_eq!(s.info(), before);
}

#[test]
fn explicit_split_piece_collision_is_still_refused() {
    let (mut s, src, _) = stock();
    let split = s
        .split(&SplitOptions {
            body: Some(src),
            plane: PlaneRef::Base(BaseName::XY),
            offset: 10.0.into(),
            name: None,
            piece_names: Some(vec!["lower".into(), "upper".into()]),
        })
        .unwrap();
    let before = s.info();
    let error = s
        .split(&SplitOptions {
            body: Some(split.pieces[0].id),
            plane: PlaneRef::Base(BaseName::XY),
            offset: 5.0.into(),
            name: None,
            piece_names: Some(vec!["lower".into(), "new upper".into()]),
        })
        .unwrap_err();
    assert!(error.to_string().contains("already names an object"), "{error}");
    assert_eq!(s.info(), before);
}

#[test]
fn vector_glyphs_with_spaces_and_normal_arial_text_still_bake() {
    for font in [
        Some(TextFont {
            path: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/glyph-gap.ttf").to_string_lossy().into(),
            index: 0,
        }),
        None,
    ] {
        let mut s = Session::new_part();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_text(sk, &TextSpec { text: "A A".into(), at: [0.0, 0.0], height: 10.0, angle: 0.0, font, construction: false }).unwrap();
        assert_eq!(s.sketch_info(sk).unwrap().texts.len(), 1);
    }
}
