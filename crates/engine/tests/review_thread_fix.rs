use qymcad_engine::*;

fn cylinder(s: &mut Session, diameter: f64, bore: Option<f64>) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &diameter.into(), false).unwrap();
    if let Some(bore) = bore {
        s.sketch_circle(sk, &0.0.into(), &0.0.into(), &bore.into(), false).unwrap();
    }
    s.extrude(&Extrude {
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

fn bottom_rim(s: &mut Session, body: Id, diameter: f64) -> Sel {
    let topo = s.topology(Some(body), false).unwrap();
    let edge = topo
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Circle && (e.radius.unwrap() * 2.0 - diameter).abs() < 0.001)
        .min_by(|a, b| a.center.unwrap()[2].total_cmp(&b.center.unwrap()[2]))
        .unwrap();
    Sel::Ids(vec![edge.id])
}

#[test]
fn review_undersized_bore_skin() {
    let mut s = Session::new_part();
    let src = cylinder(&mut s, 16.0, Some(6.1));
    let rim = bottom_rim(&mut s, src, 6.1);
    let result = s.thread(Some(src), &rim, &ThreadOptions { size: Some("M8".into()), length: 20.0.into(), ..Default::default() });
    println!("undersized bore result={result:?}");
    if result.is_err() {
        return;
    }
    let result = result.unwrap();
    let built = qymcad_kernel::ask::export_mesh(s.shape(result.body).unwrap(), 0.003).unwrap();
    let z = 6.0;
    let (sn, cs) = 0.173_f64.sin_cos();
    let mut radii = Vec::new();
    for tri in &built.mesh.tris {
        let p = tri.map(|i| {
            let v = built.mesh.verts[i as usize];
            [v.x * cs + v.y * sn, -v.x * sn + v.y * cs, v.z]
        });
        let mut hits = Vec::new();
        for i in 0..3 {
            let (a, b) = (p[i], p[(i + 1) % 3]);
            if (a[1] <= 0.0 && b[1] > 0.0) || (b[1] <= 0.0 && a[1] > 0.0) {
                let t = -a[1] / (b[1] - a[1]);
                let r = a[0] + t * (b[0] - a[0]);
                if r > 0.0 {
                    hits.push([a[2] + t * (b[2] - a[2]), r]);
                }
            }
        }
        if let [a, b] = hits.as_slice() {
            if (a[0] - b[0]).abs() > 1e-10 && z >= a[0].min(b[0]) && z <= a[0].max(b[0]) {
                radii.push(a[1] + (z - a[0]) / (b[0] - a[0]) * (b[1] - a[1]));
            }
        }
    }
    radii.sort_by(f64::total_cmp);
    radii.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
    println!("undersized bore radial crossings at z={z}: {radii:?}");
    assert!(radii.len() <= 2, "thread groove enclosed behind intact bore wall");
}

#[test]
fn review_custom_angle_is_kept() {
    let mut s = Session::new_part();
    let src = cylinder(&mut s, 8.0, None);
    let rim = bottom_rim(&mut s, src, 8.0);
    let result = s.thread(
        Some(src),
        &rim,
        &ThreadOptions {
            standard: Some("custom".into()),
            nominal: Some(8.0.into()),
            pitch: Some(1.25.into()),
            angle: Some(0.5.into()),
            fit: Some(0.0.into()),
            length: 20.0.into(),
            ..Default::default()
        },
    );
    println!("custom angle result={result:?}");
    if result.is_err() {
        return;
    }
    let result = result.unwrap();
    let qymcad_core::feature::FeatureKind::Thread { spec, .. } = s.project().timeline.iter().find(|n| n.id == result.body).unwrap().kind
    else {
        unreachable!()
    };
    println!("stored angle={} effective angle={}", spec.custom_angle, spec.geometry().angle_deg);
    assert_eq!(spec.geometry().angle_deg, 0.5, "accepted custom angle silently ignored");
}

#[test]
fn refused_thread_edit_names_its_feature() {
    let mut s = Session::new_part();
    let src = cylinder(&mut s, 8.0, None);
    let edge = bottom_rim(&mut s, src, 8.0);
    let result = s
        .thread(
            Some(src),
            &edge,
            &ThreadOptions { size: Some("M8".into()), length: 20.0.into(), name: Some("mount thread".into()), ..Default::default() },
        )
        .unwrap();
    let err = s
        .dimension_set(&DimensionTarget::Feature { feature: result.body, key: "nominal".into() }, Some(&5.0.into()), None)
        .unwrap_err()
        .to_string();
    assert!(err.contains(&format!("mount thread ({})", result.body)), "thread diagnostic lacks feature name/id: {err}");
}

// ISO D1 is D−1.082532P in pinned native geometry. Internal fit room is .115P;
// excess fit translates the groove by excess/tan30°. Retain .02P of .15P overshoot.
fn minimum_m8_bore() -> f64 {
    let p = 1.25;
    let relief = (0.2 - 0.115 * p) / 30_f64.to_radians().tan();
    8.0 - 1.082_532 * p + 2.0 * (relief - 0.13 * p)
}

fn assert_open_section(s: &Session, body: Id, internal: bool) {
    let built = qymcad_kernel::ask::export_mesh(s.shape(body).unwrap(), 0.003).unwrap();
    for angle in [0.173_f64, 0.913] {
        let (sn, cs) = angle.sin_cos();
        let mut section = Vec::new();
        for tri in &built.mesh.tris {
            let p = tri.map(|i| {
                let v = built.mesh.verts[i as usize];
                [v.x * cs + v.y * sn, -v.x * sn + v.y * cs, v.z]
            });
            let mut hits = Vec::new();
            for i in 0..3 {
                let (a, b) = (p[i], p[(i + 1) % 3]);
                if (a[1] <= 0.0 && b[1] > 0.0) || (b[1] <= 0.0 && a[1] > 0.0) {
                    let t = -a[1] / (b[1] - a[1]);
                    let r = a[0] + t * (b[0] - a[0]);
                    if r > 0.0 {
                        hits.push([a[2] + t * (b[2] - a[2]), r]);
                    }
                }
            }
            if let [a, b] = hits.as_slice() {
                section.push([*a, *b]);
            }
        }
        for i in 0..40 {
            let z = 6.0 + i as f64 * 1.25 / 40.0;
            let mut radii = section
                .iter()
                .filter_map(|[a, b]| {
                    if (a[0] - b[0]).abs() > 1e-10 && z >= a[0].min(b[0]) && z <= a[0].max(b[0]) {
                        Some(a[1] + (z - a[0]) / (b[0] - a[0]) * (b[1] - a[1]))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            radii.sort_by(f64::total_cmp);
            radii.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
            assert_eq!(radii.len(), if internal { 2 } else { 1 }, "enclosed stock skin at angle={angle},z={z}: {radii:?}");
        }
    }
}

#[test]
fn thread_stock_intervals_keep_both_boundary_sides_and_open_sections() {
    let low_internal = minimum_m8_bore();
    // External native range is D ± max(.05D,.5P); internal maximum is 1.05D.
    // External stock's actual radius anchors its cutter: both oversize and undersize remain open.
    for (internal, diameter, accepted) in [
        (true, low_internal - 0.001, false),
        (true, low_internal + 0.001, true),
        (true, 8.4 - 0.001, true),
        (true, 8.4 + 0.001, false),
        (false, 7.375 - 0.001, false),
        (false, 7.375 + 0.001, true),
        (false, 8.625 - 0.001, true),
        (false, 8.625 + 0.001, false),
    ] {
        let mut s = Session::new_part();
        let src = cylinder(&mut s, if internal { 16.0 } else { diameter }, internal.then_some(diameter));
        let edge = bottom_rim(&mut s, src, diameter);
        let before = s.info();
        let options = ThreadOptions { size: Some("M8".into()), length: 20.0.into(), ..Default::default() };
        let result = s.thread(Some(src), &edge, &options);
        if accepted {
            let result = result.unwrap_or_else(|e| panic!("internal={internal}, diameter={diameter}: {e}"));
            assert_open_section(&s, result.body, internal);
        } else {
            assert!(result.is_err(), "internal={internal}, diameter={diameter} outside safe interval accepted");
            assert_eq!(s.info(), before, "refused creation is atomic");
        }
    }
}

#[test]
fn thread_stock_and_profile_edits_recheck_open_margin_atomically() {
    let mut s = Session::new_part();
    let diameter = minimum_m8_bore() + 0.01;
    s.param_set("bore", &diameter.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &16.0.into(), false).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &"bore".into(), false).unwrap();
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
    let rim = bottom_rim(&mut s, src, diameter);
    let result = s.thread(Some(src), &rim, &ThreadOptions { size: Some("M8".into()), length: 20.0.into(), ..Default::default() }).unwrap();
    let before = s.info();
    let bytes = {
        let _gate = qymcad_kernel::kernel_gate();
        s.shape(result.body).unwrap().to_brep_bytes()
    };
    let err = s.param_set("bore", &(minimum_m8_bore() - 0.01).into()).unwrap_err().to_string();
    assert!(err.contains("open-groove limit"), "{err}");
    assert_eq!(s.info(), before);
    for (key, value) in [("nominal", 8.15), ("pitch", 1.1), ("fit", 0.21)] {
        let err = s
            .dimension_set(&DimensionTarget::Feature { feature: result.body, key: key.into() }, Some(&value.into()), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("open-groove limit"), "{key}: {err}");
        assert_eq!(s.info(), before);
    }
    let after = {
        let _gate = qymcad_kernel::kernel_gate();
        s.shape(result.body).unwrap().to_brep_bytes()
    };
    assert_eq!(after, bytes, "rejected stock/profile edits preserve exact B-rep");
    s.param_set("bore", &(diameter + 0.05).into()).unwrap();
    assert_open_section(&s, result.body, true);
}

#[test]
fn custom_angle_exact_threshold_is_refused_at_creation_and_on_edits() {
    for angle in [0.5, 1.0, 1.000_001] {
        let mut s = Session::new_part();
        s.param_set("angle", &angle.into()).unwrap();
        let src = cylinder(&mut s, 8.0, None);
        let edge = bottom_rim(&mut s, src, 8.0);
        let options = ThreadOptions {
            standard: Some("custom".into()),
            nominal: Some(8.0.into()),
            pitch: Some(1.25.into()),
            fit: Some(0.0.into()),
            length: 20.0.into(),
            angle: Some("angle".into()),
            ..Default::default()
        };
        let before = s.info();
        let result = s.thread(Some(src), &edge, &options);
        if angle <= 1.0 {
            let err = result.unwrap_err().to_string();
            assert!(err.contains("greater than 1 degree"), "{err}");
            assert_eq!(s.info(), before);
            continue;
        }
        let result = result.unwrap();
        let before = s.info();
        let original = {
            let _gate = qymcad_kernel::kernel_gate();
            s.shape(result.body).unwrap().to_brep_bytes()
        };
        for bad in [0.5, 1.0] {
            let err = s.param_set("angle", &bad.into()).unwrap_err().to_string();
            assert!(err.contains("greater than 1 degree"), "{err}");
            assert_eq!(s.info(), before);
            let err = s
                .dimension_set(&DimensionTarget::Feature { feature: result.body, key: "angle".into() }, Some(&bad.into()), None)
                .unwrap_err()
                .to_string();
            assert!(err.contains("greater than 1 degree"), "{err}");
            assert_eq!(s.info(), before);
        }
        let after = {
            let _gate = qymcad_kernel::kernel_gate();
            s.shape(result.body).unwrap().to_brep_bytes()
        };
        assert_eq!(after, original, "refused angle edits preserve exact B-rep");
    }
}

#[test]
fn a_preexisting_thread_error_cannot_exempt_new_safety_refusals() {
    let mut s = Session::new_part();
    let src = cylinder(&mut s, 8.0, None);
    let edge = bottom_rim(&mut s, src, 8.0);
    let thread = s
        .thread(
            Some(src),
            &edge,
            &ThreadOptions {
                standard: Some("custom".into()),
                nominal: Some(8.0.into()),
                pitch: Some(1.25.into()),
                length: 20.0.into(),
                fit: Some(0.0.into()),
                ..Default::default()
            },
        )
        .unwrap()
        .body;
    let path = std::env::temp_dir().join(format!("qymcad-error-thread-{}.qcad", std::process::id()));
    s.save(Some(&path)).unwrap();
    let mut loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    loaded.project.feat_dims.entry(thread).or_default().insert("length".into(), "21".into());
    loaded.project.timeline.iter_mut().find(|n| n.id == thread).unwrap().dirty = true;
    qymcad_io::save_project_with_brep(&loaded.project, path.to_str().unwrap(), &loaded.breps).unwrap();
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.iter().any(|e| e.node == thread), "fixture must have an existing thread span error");
    let before = s.info();
    let edited = s.dimension_set(&DimensionTarget::Feature { feature: thread, key: "angle".into() }, Some(&0.5.into()), None);
    assert!(edited.is_err(), "preexisting thread error hid a new ignored-angle refusal");
    assert_eq!(s.info(), before);
    s.dimension_set(&DimensionTarget::Feature { feature: thread, key: "length".into() }, Some(&20.0.into()), None).unwrap();
    assert!(s.current_rebuild().errors.is_empty(), "safe repair must remain possible");
    std::fs::remove_file(path).unwrap();
}
