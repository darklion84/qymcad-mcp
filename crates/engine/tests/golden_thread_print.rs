//! Independent dimensional and persistence checks for print threads (ADR 0024).

mod common;
use common::*;
use qymcad_core::{feature::FeatureKind, geom::Mesh};
use qymcad_engine::*;
use std::f64::consts::PI;

fn e(text: &str) -> Num {
    Num::Expr(text.into())
}

fn stock(s: &mut Session, diameter: Num, height: Num, bore: Option<Num>) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &diameter, false).unwrap();
    if let Some(bore) = bore {
        s.sketch_circle(sk, &0.0.into(), &0.0.into(), &bore, false).unwrap();
    }
    let (body, report) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(report.errors.is_empty());
    body
}

fn rim(s: &mut Session, body: Id, diameter: f64, top: bool) -> Sel {
    let topo = s.topology(Some(body), false).unwrap();
    let edge = topo
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Circle && (2.0 * edge.radius.unwrap() - diameter).abs() < 0.001)
        .max_by(|a, b| {
            let ordering = a.center.unwrap()[2].total_cmp(&b.center.unwrap()[2]);
            if top {
                ordering
            } else {
                ordering.reverse()
            }
        })
        .expect("one stock rim");
    Sel::Ids(vec![edge.id])
}

fn alias(size: &str, length: f64) -> ThreadOptions {
    ThreadOptions { size: Some(size.into()), length: length.into(), ..Default::default() }
}

/// Intersect actual output triangles with an axial half-plane. No thread profile/generator supplies expectations.
fn axial_section(mesh: &Mesh, angle: f64) -> Vec<[[f64; 2]; 2]> {
    let (sin, cos) = angle.sin_cos();
    let mut section = Vec::new();
    for triangle in &mesh.tris {
        let points = triangle.map(|index| {
            let p = mesh.verts[index as usize];
            [p.x * cos + p.y * sin, -p.x * sin + p.y * cos, p.z]
        });
        let mut crossings = Vec::new();
        for i in 0..3 {
            let (a, b) = (points[i], points[(i + 1) % 3]);
            if (a[1] <= 0.0 && b[1] > 0.0) || (b[1] <= 0.0 && a[1] > 0.0) {
                let t = -a[1] / (b[1] - a[1]);
                let r = a[0] + t * (b[0] - a[0]);
                if r > 0.0 {
                    crossings.push([a[2] + t * (b[2] - a[2]), r]);
                }
            }
        }
        if crossings.len() == 2 {
            section.push([crossings[0], crossings[1]]);
        }
    }
    section
}

fn radius_at(section: &[[[f64; 2]; 2]], z: f64, internal: bool) -> f64 {
    let radii = section.iter().filter_map(|[a, b]| {
        if (a[0] - b[0]).abs() < 1e-10 || z < a[0].min(b[0]) || z > a[0].max(b[0]) {
            None
        } else {
            Some(a[1] + (z - a[0]) / (b[0] - a[0]) * (b[1] - a[1]))
        }
    });
    let result = if internal { radii.reduce(f64::max) } else { radii.reduce(f64::min) };
    result.expect("a ray crosses the actual result wall")
}

/// Expected diameters derive from ISO's fundamental triangle H=√3 P/2 and ISO 2901 Tr16×4 clearance ac=.25.
/// Fit widens each half-groove to min((P-flat)/2+fit,.49P); its overflow is radial / tan(half-angle).
fn check_sections(s: &Session, body: Id, nominal: f64, pitch: f64, fit: f64, tr: bool, internal: bool) {
    let (depth, crest_flat, half_angle) = if tr {
        (pitch / 2.0 + 0.25, 0.366 * pitch, 15_f64.to_radians())
    } else if internal {
        (5.0 * 3_f64.sqrt() * pitch / 16.0, pitch / 4.0, 30_f64.to_radians())
    } else {
        (17.0 * 3_f64.sqrt() * pitch / 48.0, pitch / 8.0, 30_f64.to_radians())
    };
    let room = 0.49 * pitch - (pitch - crest_flat) / 2.0;
    let relief = (fit - room).max(0.0) / half_angle.tan();
    let (root, crest) =
        if internal { (nominal + 2.0 * (fit + relief), nominal - 2.0 * depth) } else { (nominal - 2.0 * (depth + fit + relief), nominal) };
    // export_mesh owns the kernel gate: deliberately no outer gate (F-073).
    let built = qymcad_kernel::ask::export_mesh(s.shape(body).unwrap(), 0.003).unwrap();
    for angle in [0.173, 0.913] {
        // Keep both exposed and enclosed groove walls. Native relief translates the groove; when relief
        // exceeds its overshoot it leaves an outer skin. The minimum exterior crossing still measures the
        // actual groove root. For a nut, exclude only the unrelated Ø16 outside stock wall.
        let section: Vec<_> = axial_section(&built.mesh, angle)
            .into_iter()
            .filter(|[a, b]| !internal || (a[1] < nominal * 0.75 && b[1] < nominal * 0.75))
            .collect();
        let samples: Vec<_> = (0..1000).map(|i| radius_at(&section, 4.0 * pitch + i as f64 * pitch / 250.0, internal)).collect();
        let low = 2.0 * samples.iter().copied().reduce(f64::min).unwrap();
        let high = 2.0 * samples.iter().copied().reduce(f64::max).unwrap();
        assert_close(low, root.min(crest), 0.035, "measured section minimum diameter");
        assert_close(high, root.max(crest), 0.035, "measured section maximum diameter");
        if !tr {
            // At the ISO pitch cylinder the groove occupies half an axial pitch. The median radius
            // therefore measures that cylinder without consulting ThreadSpec or its profile. Native fit
            // widens the half-groove until its cap, then translates the entire groove by the relief.
            let basic_pitch_diameter = nominal - 3.0 * 3_f64.sqrt() * pitch / 8.0;
            let allowance = 2.0 * (relief + fit.min(room) / half_angle.tan());
            let expected = basic_pitch_diameter + if internal { allowance } else { -allowance };
            let mut ordered = samples.clone();
            ordered.sort_by(f64::total_cmp);
            assert_close(2.0 * ordered[ordered.len() / 2], expected, 0.045, "section half-pitch cylinder diameter");
        }
        let pitch_error = (0..500).map(|i| (samples[i] - samples[i + 250]).powi(2)).sum::<f64>() / 500.0;
        assert!(pitch_error.sqrt() < 0.015, "section repeats at hand expected pitch {pitch}: RMS {}", pitch_error.sqrt());
        let half_pitch_error = (0..500).map(|i| (samples[i] - samples[i + 125]).abs()).sum::<f64>() / 500.0;
        assert!(half_pitch_error > depth * 0.2, "half-pitch differs from one pitch: {half_pitch_error}");
    }
}

#[test]
fn metric_thread_sections_match_hand_dimensions() {
    for (size, nominal, pitch, length, fit) in
        [("M8", 8.0, 1.25, 20.0, 0.15), ("M3", 3.0, 0.5, 10.0, 0.06), ("M8x1.25", 8.0, 1.25, 20.0, 0.0)]
    {
        let mut s = Session::new_part();
        let src = stock(&mut s, nominal.into(), length.into(), None);
        let edge = rim(&mut s, src, nominal, false);
        let result = s.thread(Some(src), &edge, &ThreadOptions { fit: Some(fit.into()), ..alias(size, length) }).unwrap();
        assert_eq!(result.kind, "external");
        match &s.project().timeline.iter().find(|node| node.id == result.body).unwrap().kind {
            FeatureKind::Thread { lead_in, lead_out, .. } => {
                assert_close(*lead_in, 1.5, 1e-12, "app lead-in default");
                assert_close(*lead_out, 1.5, 1e-12, "app lead-out default");
            }
            _ => panic!("native Thread recipe"),
        }
        assert_close(result.pitch, pitch, 1e-9, "coarse pitch");
        assert_close(result.lead, pitch, 1e-9, "single-start lead");
        assert_close(result.basic_major_diameter, nominal, 1e-9, "basic major diameter");
        assert_close(result.basic_pitch_diameter, nominal - 3.0 * 3_f64.sqrt() * pitch / 8.0, 2e-6, "ISO basic pitch diameter");
        assert_close(result.basic_minor_diameter, nominal - 17.0 * 3_f64.sqrt() * pitch / 24.0, 2e-6, "ISO basic external minor");
        let relief = (fit - 0.0525 * pitch).max(0.0) / 30_f64.to_radians().tan();
        assert_close(result.radial_relief, relief, 1e-9, "reported radial relief");
        check_sections(&s, result.body, nominal, pitch, fit, false, false);
        let volume = s.shape(result.body).unwrap().volume();
        let root_radius = nominal / 2.0 - 17.0 * 3_f64.sqrt() * pitch / 48.0 - fit - relief;
        assert!(volume > PI * root_radius.powi(2) * length, "material exceeds the compensated core cylinder");
        assert!(volume < PI * (nominal / 2.0).powi(2) * length * 0.95, "a meaningful thread removes stock");
    }
}

#[test]
fn trapezoidal_thread_sections_match_hand_dimensions() {
    let mut s = Session::new_part();
    let src = stock(&mut s, 16.0.into(), 40.0.into(), None);
    let edge = rim(&mut s, src, 16.0, false);
    let result = s.thread(None, &edge, &alias("Tr16x4", 40.0)).unwrap();
    assert_close(result.pitch, 4.0, 1e-9, "Tr pitch");
    assert_close(result.basic_pitch_diameter, 14.0, 1e-9, "Tr basic pitch diameter");
    assert_close(result.basic_minor_diameter, 11.5, 1e-9, "Tr basic minor diameter");
    check_sections(&s, result.body, 16.0, 4.0, 0.2, true, false);
    let volume = s.shape(result.body).unwrap().volume();
    assert!(volume > PI * 5.55_f64.powi(2) * 40.0 && volume < PI * 8_f64.powi(2) * 40.0 * 0.95);
}

#[test]
fn internal_thread_detects_side_and_cuts_the_bore() {
    let mut s = Session::new_part();
    let minor = 8.0 - 5.0 * 3_f64.sqrt() * 1.25 / 8.0;
    let src = stock(&mut s, 16.0.into(), 20.0.into(), Some(minor.into()));
    let edge = rim(&mut s, src, minor, true);
    let initial = s.shape(src).unwrap().volume();
    let result = s.thread(None, &edge, &alias("M8", 20.0)).unwrap();
    assert_eq!(result.kind, "internal");
    match &s.project().timeline.iter().find(|n| n.id == result.body).unwrap().kind {
        FeatureKind::Thread { spec, .. } => assert!(spec.internal, "persist the detected side as the app does"),
        _ => panic!("native Thread recipe"),
    }
    check_sections(&s, result.body, 8.0, 1.25, 0.2, false, true);
    let volume = s.shape(result.body).unwrap().volume();
    assert!(volume < initial - 50.0 && volume > PI * (8_f64.powi(2) - 4.4_f64.powi(2)) * 20.0);
}

#[test]
fn thread_formulas_persist_and_follow_server_and_app_edits() {
    let mut s = Session::new_part();
    for (name, value) in [("Diameter", 8.0), ("StockLength", 20.0), ("ThreadLength", 20.0), ("Clearance", 0.15), ("Starts", 1.0)] {
        s.param_set(name, &value.into()).unwrap();
    }
    let src = stock(&mut s, e("Diameter"), e("StockLength"), None);
    let edge = rim(&mut s, src, 8.0, false);
    let options = ThreadOptions {
        nominal: Some(e("Diameter")),
        length: e("ThreadLength"),
        fit: Some(e("Clearance")),
        starts: Some(e("Starts")),
        ..Default::default()
    };
    let result = s.thread(None, &edge, &options).unwrap();
    assert_close(result.fit, 0.15, 1e-12, "explicit safe fit expression");
    let initial = s.shape(result.body).unwrap().volume();
    let path = scratch("print_thread_formula.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    assert_close(opened.shape(result.body).unwrap().volume(), initial, 1e-8, "saved native B-rep");
    for (name, value) in [("ThreadLength", 16.0), ("Clearance", 0.1), ("Diameter", 8.2), ("StockLength", 24.0), ("Starts", 2.0)] {
        let (mut changed, _) = Session::open(&path).unwrap();
        changed.param_set(name, &value.into()).unwrap();
        let server_volume = changed.shape(result.body).unwrap().volume();
        if name != "Starts" {
            assert!((server_volume - initial).abs() > 1.0, "{name} must dirty/rebuild the thread");
        }
        let (app_volume, _, faces) = gui_edit_param_faces(&path, name, &value.to_string());
        assert!(!faces.is_empty());
        assert_close(app_volume, server_volume, 1e-7, "unmodified native app parameter path agrees");
        if name == "Diameter" {
            check_sections(&changed, result.body, 8.2, 1.25, 0.15, false, false);
        }
    }
    let before = s.info();
    let original_brep = {
        let _gate = qymcad_kernel::kernel_gate();
        s.shape(result.body).unwrap().to_brep_bytes()
    };
    for starts in [0.0, 1.5, 65.0] {
        let error = s.param_set("Starts", &starts.into()).unwrap_err().to_string();
        assert!(error.contains("starts") && error.contains("integer"), "{error}");
        assert_eq!(s.info(), before);
        let brep = {
            let _gate = qymcad_kernel::kernel_gate();
            s.shape(result.body).unwrap().to_brep_bytes()
        };
        assert_eq!(brep, original_brep, "a refused edit preserves exact geometry");
    }
}

#[test]
fn every_thread_key_is_reachable_by_dimension_set() {
    let mut s = Session::new_part();
    let src = stock(&mut s, 8.0.into(), 20.0.into(), None);
    let edge = rim(&mut s, src, 8.0, false);
    let result = s.thread(None, &edge, &alias("M8", 20.0)).unwrap();
    for (key, value) in [
        ("length", 20.0),
        ("lead_in", 1.5),
        ("lead_out", 1.5),
        ("nominal", 8.0),
        ("pitch", 1.25),
        ("fit", 0.15),
        ("crest_r", 0.0),
        ("root_r", 0.0),
        ("angle", 60.0),
        ("depth", 0.0),
        ("starts", 1.0),
    ] {
        assert!(s.project().feat_dims[&result.body].contains_key(key), "creation writes native {key} as a formula");
        let edited =
            s.dimension_set(&DimensionTarget::Feature { feature: result.body, key: key.into() }, Some(&value.into()), None).unwrap();
        assert!(edited.rebuild.errors.is_empty(), "{key}");
        assert_close(edited.dimension.value.unwrap(), value, 1e-9, key);
    }
    let error =
        s.dimension_set(&DimensionTarget::Feature { feature: result.body, key: "starts".into() }, Some(&1.5.into()), None).unwrap_err();
    assert!(error.to_string().contains("integer"), "{error}");
}

#[test]
fn thread_refuses_wrong_rims_side_stock_and_span_atomically() {
    let mut s = Session::new_part();
    let src = stock(&mut s, 8.0.into(), 20.0.into(), None);
    let edge = rim(&mut s, src, 8.0, false);
    let other = stock(&mut s, 10.0.into(), 20.0.into(), None);
    let foreign_edge = rim(&mut s, other, 10.0, false);
    let before = s.info();
    let straight = s.topology(Some(src), false).unwrap().edges.into_iter().find(|edge| edge.kind == EdgeKind::Line).unwrap().id;
    for (selection, options, word) in [
        (Sel::Ids(vec![straight]), alias("M8", 20.0), "circular"),
        (Sel::Kind(SelectionKind::Circle), alias("M8", 20.0), "one"),
        (Sel::Ids(vec![u32::MAX]), alias("M8", 20.0), "edge"),
        (foreign_edge, alias("M8", 20.0), "edge"),
        (edge.clone(), ThreadOptions { kind: "internal".into(), ..alias("M8", 20.0) }, "internal"),
        (edge.clone(), alias("M3", 20.0), "fit"),
        (edge.clone(), alias("M8", 21.0), "long"),
        (edge.clone(), ThreadOptions { starts: Some(1.5.into()), ..alias("M8", 20.0) }, "integer"),
        (edge.clone(), ThreadOptions { nominal: Some(8.0.into()), ..alias("M8", 20.0) }, "size"),
    ] {
        let error = s.thread(Some(src), &selection, &options).unwrap_err().to_string();
        assert!(error.to_lowercase().contains(word), "expected {word} in {error}");
        assert_eq!(s.info(), before, "refused creation changes nothing");
    }
}

#[test]
fn fillet_beside_a_threaded_rim_is_checked_before_and_after_thread() {
    for before in [true, false] {
        let mut s = Session::new_part();
        s.param_set("StockLength", &30.0.into()).unwrap();
        let mut src = stock(&mut s, 8.0.into(), e("StockLength"), None);
        if before {
            let top = rim(&mut s, src, 8.0, true);
            src = s.fillet(Some(src), &top, &0.4.into(), None).unwrap().0;
        }
        let bottom = rim(&mut s, src, 8.0, false);
        let threaded = s.thread(Some(src), &bottom, &alias("M8", 15.0)).unwrap().body;
        let final_body = if before {
            threaded
        } else {
            let top = rim(&mut s, threaded, 8.0, true);
            s.fillet(Some(threaded), &top, &0.4.into(), None).unwrap().0
        };
        assert!(s.info().errors.is_empty());
        let path = scratch(&format!("thread_fillet_before_{before}.qcad"));
        s.save(Some(&path)).unwrap();
        let (opened, report) = Session::open(&path).unwrap();
        assert!(report.errors.is_empty());
        assert_close(opened.shape(final_body).unwrap().volume(), s.shape(final_body).unwrap().volume(), 1e-8, "fillet/thread reopen");
        s.param_set("StockLength", &34.0.into()).unwrap();
        let (volume, _, faces) = gui_edit_param_faces(&path, "StockLength", "34");
        assert!(!faces.is_empty());
        assert_close(volume, s.shape(final_body).unwrap().volume(), 1e-7, "fillet/thread native app rebuild");
    }
}

#[test]
fn a_circular_arc_is_not_a_full_thread_rim() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_line(
        sk,
        &LineSpec { x1: (-4.0).into(), y1: 0.0.into(), x2: 4.0.into(), y2: 0.0.into(), construction: false, dimensioned: true },
    )
    .unwrap();
    s.sketch_arc(
        sk,
        &ArcSpec {
            cx: 0.0.into(),
            cy: 0.0.into(),
            r: Some(4.0.into()),
            start_angle: Some(0.0.into()),
            end_angle: Some(180.0.into()),
            start: None,
            end: None,
            ccw: true,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    let (src, _) = s
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
        .unwrap();
    let arc = s.topology(Some(src), false).unwrap().edges.into_iter().find(|edge| edge.kind == EdgeKind::Arc).unwrap().id;
    let before = s.info();
    let error = s.thread(Some(src), &Sel::Ids(vec![arc]), &alias("M8", 20.0)).unwrap_err().to_string();
    assert!(error.contains("circular rim"), "{error}");
    assert_eq!(s.info(), before);
}

/// Native fit relief can exceed the cutter's radial overshoot and enclose the cut beneath the stock skin.
/// This is upstream geometry, not an ISO clearance/tolerance guarantee (ADR 0024 item 6).
#[test]
fn upstream_metric_fit_relief_leaves_a_stock_skin_over_the_groove() {
    let mut s = Session::new_part();
    let src = stock(&mut s, 8.0.into(), 20.0.into(), None);
    let edge = rim(&mut s, src, 8.0, false);
    // Bypass server safety deliberately: reproduce the pinned app's native recipe.
    let mut project = s.project().clone();
    let Sel::Ids(edges) = edge else { unreachable!() };
    let body = project.add_thread(
        src,
        edges[0],
        qymcad_core::thread::ThreadSpec { nominal_d: 8.0, fit: 0.2, ..Default::default() },
        20.0,
        1.5,
        1.5,
    );
    let seed = scratch("native_buried_thread_stock.qcad");
    s.save(Some(&seed)).unwrap();
    let loaded = qymcad_io::load_project_with_brep(seed.to_str().unwrap()).unwrap();
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, restore_shapes(loaded.breps));
    assert!(report.errors.is_empty());
    let built = qymcad_kernel::ask::export_mesh(&shapes[&body], 0.003).unwrap();
    let section = axial_section(&built.mesh, 0.173);
    let z = 6.0;
    let mut radii: Vec<_> = section
        .iter()
        .filter_map(|[a, b]| {
            if (a[0] - b[0]).abs() < 1e-10 || z < a[0].min(b[0]) || z > a[0].max(b[0]) {
                None
            } else {
                Some(a[1] + (z - a[0]) / (b[0] - a[0]) * (b[1] - a[1]))
            }
        })
        .collect();
    radii.sort_by(f64::total_cmp);
    radii.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
    assert_eq!(radii.len(), 3, "stock exterior and both walls of the enclosed groove: {radii:?}");
    assert_close(2.0 * radii[2], 8.0, 0.01, "the outside stock skin remains");
    assert!(radii[1] < radii[2] - 0.02 && radii[0] < radii[1] - 0.5, "a genuine enclosed groove: {radii:?}");
}

#[test]
fn a_thread_that_removes_nothing_is_refused_atomically() {
    let mut s = Session::new_part();
    let src = stock(&mut s, 8.0.into(), 10.0.into(), None);
    let edge = rim(&mut s, src, 8.0, false);
    let before = s.info();
    let result = s.thread(
        None,
        &edge,
        &ThreadOptions {
            standard: Some("custom".into()),
            nominal: Some(8.0.into()),
            pitch: Some(1.0.into()),
            length: 10.0.into(),
            depth: Some(0.000001.into()),
            fit: Some(0.0.into()),
            lead_in: Some(0.0.into()),
            lead_out: Some(0.0.into()),
            ..Default::default()
        },
    );
    let error = result.unwrap_err().to_string();
    // A depth this shallow is now refused before native builds it: native would infer the wrong cutter side.
    assert!(error.contains("The thread did not build") || error.contains("nothing") || error.contains("side"), "{error}");
    assert_eq!(s.info(), before);
}

#[test]
fn a_downstream_fillet_failure_rolls_back_the_thread_and_its_stock() {
    let mut s = Session::new_part();
    s.param_set("Diameter", &8.0.into()).unwrap();
    let src = stock(&mut s, e("Diameter"), 30.0.into(), None);
    let bottom = rim(&mut s, src, 8.0, false);
    let thread = s
        .thread(
            None,
            &bottom,
            &ThreadOptions { nominal: Some(e("Diameter")), length: 15.0.into(), fit: Some(0.1.into()), ..Default::default() },
        )
        .unwrap()
        .body;
    let top = rim(&mut s, thread, 8.0, true);
    let final_body = s.fillet(Some(thread), &top, &3.0.into(), None).unwrap().0;
    let before = s.info();
    let brep = {
        let _gate = qymcad_kernel::kernel_gate();
        s.shape(final_body).unwrap().to_brep_bytes()
    };
    let error = s.param_set("Diameter", &5.0.into()).unwrap_err().to_string();
    assert!(error.contains("Fillet") || error.contains("fillet"), "{error}");
    assert_eq!(s.info(), before);
    let after = {
        let _gate = qymcad_kernel::kernel_gate();
        s.shape(final_body).unwrap().to_brep_bytes()
    };
    assert_eq!(after, brep, "the downstream failure preserves the exact original final body");
}

#[test]
fn default_fit_keeps_the_m8_groove_open_to_the_surface() {
    let mut s = Session::new_part();
    let src = stock(&mut s, 8.0.into(), 20.0.into(), None);
    let edge = rim(&mut s, src, 8.0, false);
    let result = s.thread(None, &edge, &alias("M8", 20.0)).unwrap();
    let expected_limit = 0.0525 * 1.25 + 0.13 * 1.25 * 30_f64.to_radians().tan();
    assert_close(result.fit, expected_limit, 1e-10, "largest fit below relief limit");
    assert_close(result.radial_relief, 0.13 * 1.25, 1e-10, "overshoot minus safety margin");
    let built = qymcad_kernel::ask::export_mesh(s.shape(result.body).unwrap(), 0.003).unwrap();
    let section = axial_section(&built.mesh, 0.173);
    let exterior = radius_at(&section, 6.0, true); // furthest crossing, including any stock skin
    assert!(exterior < 3.9, "groove must reach the outside; furthest crossing {exterior}");
}

#[test]
fn unsafe_fit_creation_and_later_edits_are_refused_atomically() {
    let mut s = Session::new_part();
    s.param_set("Clearance", &0.1.into()).unwrap();
    let src = stock(&mut s, 8.0.into(), 20.0.into(), None);
    let edge = rim(&mut s, src, 8.0, false);
    let before = s.info();
    let error = s.thread(None, &edge, &ThreadOptions { fit: Some(0.2.into()), ..alias("M8", 20.0) }).unwrap_err().to_string();
    assert!(error.contains("fit") && error.contains("limit") && error.contains("0.15944"), "{error}");
    assert_eq!(s.info(), before);
    let result = s.thread(None, &edge, &ThreadOptions { fit: Some(e("Clearance")), ..alias("M8", 20.0) }).unwrap();
    let before = s.info();
    let original = {
        let _gate = qymcad_kernel::kernel_gate();
        s.shape(result.body).unwrap().to_brep_bytes()
    };
    let error = s.param_set("Clearance", &0.2.into()).unwrap_err().to_string();
    assert!(error.contains("limit"), "{error}");
    assert_eq!(s.info(), before);
    let error = s
        .dimension_set(&DimensionTarget::Feature { feature: result.body, key: "fit".into() }, Some(&0.2.into()), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("limit"), "{error}");
    assert_eq!(s.info(), before);
    let error = s
        .dimension_set(&DimensionTarget::Feature { feature: result.body, key: "pitch".into() }, Some(&0.5.into()), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("limit"), "a pitch edit recomputes the fit ceiling: {error}");
    assert_eq!(s.info(), before);
    let after = {
        let _gate = qymcad_kernel::kernel_gate();
        s.shape(result.body).unwrap().to_brep_bytes()
    };
    assert_eq!(after, original);

    // The app accepts the unsafe edit. Persist its real native parameter/rebuild path, then ensure
    // the server refuses that document on open without changing the file or our live session.
    let path = scratch("app_unsafe_thread_fit.qcad");
    s.save(Some(&path)).unwrap();
    let before = s.info();
    gui_edit_param_faces(&path, "Clearance", "0.2");
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.parameters.iter_mut().find(|p| p.name == "Clearance").unwrap().expr = "0.2".into();
    project.eval_parameters();
    project.mark_param_dependents_dirty_for("Clearance");
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, restore_shapes(breps));
    assert!(report.errors.is_empty(), "native app permits the defect");
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(id, shape)| (*id, shape.to_brep_bytes().unwrap())).collect::<Vec<_>>()
    };
    qymcad_io::save_project_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let original_file = std::fs::read(&path).unwrap();
    let error = match Session::open(&path) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("unsafe native app edit must be refused on open"),
    };
    assert!(error.contains("limit"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), original_file);
    assert_eq!(s.info(), before);
}
