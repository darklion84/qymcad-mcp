//! Independent P6 angle/setback geometry and native saved-app driver paths.
mod common;
use common::*;
use qymcad_engine::*;

fn block() -> (Session, Id, u32, u32, u32) {
    let mut s = Session::new_part();
    s.param_set("leg", &2.0.into()).unwrap();
    s.param_set("other", &30.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    let src = s.extrude(&extrusion(sk)).unwrap().0;
    let t = s.topology(None, true).unwrap();
    let x = t.faces.iter().find(|f| f.normal.unwrap()[0] > 0.99).unwrap().id;
    let y = t.faces.iter().find(|f| f.normal.unwrap()[1] > 0.99).unwrap().id;
    let e = t.edges.iter().find(|e| e.faces.unwrap().contains(&x) && e.faces.unwrap().contains(&y)).unwrap().id;
    (s, src, e, x, y)
}
fn extrusion(sketch: Id) -> Extrude {
    Extrude {
        sketch,
        profiles: None,
        height: 10.0.into(),
        down: None,
        op: Op::Add,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    }
}
fn args(src: Id, edge: u32, reference: Option<u32>, angle: bool) -> Chamfer {
    Chamfer {
        body: Some(src),
        edges: Sel::Ids(vec![edge]),
        dist: "leg".into(),
        d2: (!angle).then(|| "other".into()),
        angle: angle.then(|| "other".into()),
        reference_face: reference.map(|f| Sel::Ids(vec![f])),
        flip: false,
        name: None,
    }
}
fn area(faces: &[qymcad_core::geom::MeshFace], id: u32, expected: f64) {
    let f = faces.iter().find(|f| f.id == id).expect("retained support face");
    assert_close(f.area, expected, 2e-4, "support face area establishes first/second setback");
}
fn check(s: &mut Session, x: u32, y: u32, reference: u32, leg: f64, second: f64) {
    assert_close(s.result_bodies()[0].volume, 12000.0 - 5.0 * leg * second, 2e-5, "triangular-prism removal");
    let t = s.topology(None, false).unwrap();
    for (id, initial, setback) in
        [(x, 300.0, if reference == x { leg } else { second }), (y, 400.0, if reference == y { leg } else { second })]
    {
        let f = t.faces.iter().find(|f| f.id == id).unwrap();
        assert_close(f.area, initial - 10.0 * setback, 2e-4, "support area");
        let expected = if id == x { [20.0, -setback / 2.0, 5.0] } else { [-setback / 2.0, 15.0, 5.0] };
        for (a, e) in f.centroid.into_iter().zip(expected) {
            assert_close(a, e, 2e-5, "trimmed support centroid");
        }
    }
}

#[test]
fn angle_reference_faces_and_flip_have_independent_setback_oracles() {
    for angle in [false, true] {
        for ref_y in [false, true] {
            for flip in [false, true] {
                let (mut s, src, e, x, y) = block();
                if !angle {
                    s.param_set("other", &4.0.into()).unwrap();
                }
                let reference = if ref_y { y } else { x };
                let mut a = args(src, e, Some(reference), angle);
                a.flip = flip;
                let (id, r) = s.chamfer_ex(&a).unwrap();
                assert!(r.errors.is_empty());
                let meta = s.chamfer_info(id).unwrap();
                assert_eq!(meta.mode, if angle { "dist_angle" } else { "two_dist" });
                assert_eq!(meta.reference_face, Some(reference));
                assert_eq!(meta.flip, flip);
                check(&mut s, x, y, reference, 2.0, if angle { 2.0 * 30f64.to_radians().tan() } else { 4.0 });
            }
        }
    }
}

#[test]
fn automatic_flip_keeps_native_side_choice() {
    let mut areas = Vec::new();
    for flip in [false, true] {
        let (mut s, src, e, x, _) = block();
        let mut a = args(src, e, None, true);
        a.flip = flip;
        s.chamfer_ex(&a).unwrap();
        let t = s.topology(None, false).unwrap();
        areas.push(t.faces.iter().find(|f| f.id == x).unwrap().area);
    }
    assert_close(areas[0] + areas[1], 600.0 - 20.0 - 20.0 * 30f64.to_radians().tan(), 2e-4, "flip exchanges two setbacks");
    assert!((areas[0] - areas[1]).abs() > 1.0, "flip must change reference side");
}

#[test]
fn saved_global_and_named_native_d2_drivers_rebuild_angle_and_distance() {
    for angle in [false, true] {
        let (mut s, src, e, x, y) = block();
        if !angle {
            s.param_set("other", &4.0.into()).unwrap();
        }
        let (id, _) = s.chamfer_ex(&args(src, e, Some(y), angle)).unwrap();
        for key in ["dist", "d2"] {
            s.dimension_set(
                &DimensionTarget::Feature { feature: id, key: key.into() },
                None,
                Some(if key == "dist" { "bevel_leg" } else { "bevel_other" }),
            )
            .unwrap();
        }
        let path = scratch(&format!("p6-drivers-{angle}.qcad"));
        s.save(Some(&path)).unwrap();
        let (mut loaded, r) = Session::open(&path).unwrap();
        assert!(r.errors.is_empty());
        check(&mut loaded, x, y, y, 2.0, if angle { 2.0 * 30f64.to_radians().tan() } else { 4.0 });
        for (driver, text, leg, other, named) in [
            ("leg", "3", 3.0, if angle { 30.0 } else { 4.0 }, false),
            ("other", if angle { "45" } else { "5" }, 2.0, if angle { 45.0 } else { 5.0 }, false),
            ("bevel_leg", "3", 3.0, if angle { 30.0 } else { 4.0 }, true),
            ("bevel_other", if angle { "45" } else { "5" }, 2.0, if angle { 45.0 } else { 5.0 }, true),
        ] {
            let (v, bb, faces, _) =
                if named { gui_edit_driver_model(&path, driver, text) } else { gui_edit_param_model(&path, driver, text) };
            let second = if angle { leg * f64::to_radians(other).tan() } else { other };
            assert_close(v, 12000.0 - 5.0 * leg * second, 2e-5, "native driver triangular volume");
            area(&faces, y, 400.0 - 10.0 * leg);
            area(&faces, x, 300.0 - 10.0 * second);
            for (a, e) in bb.into_iter().zip([-20.0, -15.0, 0.0, 20.0, 15.0, 10.0]) {
                assert_close(a, e, 0.001, "native app bounds");
            }
        }
    }
}

#[test]
fn oblique_135_degree_corner_uses_law_of_sines() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_polyline(
        sk,
        &PolylineSpec {
            points: [[-20.0, -15.0], [15.0, -15.0], [20.0, -10.0], [20.0, 15.0], [-20.0, 15.0]]
                .into_iter()
                .map(|p| Xy(p[0].into(), p[1].into()))
                .collect(),
            closed: true,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    let src = s.extrude(&extrusion(sk)).unwrap().0;
    let t = s.topology(None, true).unwrap();
    let x = t.faces.iter().find(|f| f.normal.unwrap()[0] > 0.99).unwrap().id;
    let d = t
        .faces
        .iter()
        .find(|f| {
            let n = f.normal.unwrap();
            n[0] > 0.5 && n[1] < -0.5
        })
        .unwrap()
        .id;
    let e = t.edges.iter().find(|e| e.faces.unwrap().contains(&x) && e.faces.unwrap().contains(&d)).unwrap().id;
    s.param_set("leg", &2.0.into()).unwrap();
    s.param_set("other", &30.0.into()).unwrap();
    s.chamfer_ex(&args(src, e, Some(x), true)).unwrap();
    // phi=135°, theta=30°: b=a*sin(theta)/sin(180°-phi-theta), removed A=a*b*sin(phi)/2=1+sqrt(3).
    assert_close(s.result_bodies()[0].volume, 11875.0 - 10.0 * (1.0 + 3f64.sqrt()), 2e-5, "oblique law-of-sines prism");
    let t = s.topology(None, false).unwrap();
    let face = t.faces.iter().find(|f| f.id == x).unwrap();
    assert_close(face.area, 230.0, 2e-4, "reference wall loses leg*height");
    assert_close(face.centroid[1], 3.5, 2e-5, "reference wall centroid after setback");
}

#[test]
fn creation_validation_is_exactly_atomic() {
    let (mut s, src, e, x, y) = block();
    let path = scratch("p6-create-atomic.qcad");
    s.save(Some(&path)).unwrap();
    let before = std::fs::read(&path).unwrap();
    let mut bad = Vec::new();
    for angle in [Num::Value(-30.0), 0.0.into(), 90.0.into(), 181.0.into(), "NaN".into(), "1e400".into()] {
        let mut a = args(src, e, Some(x), true);
        a.angle = Some(angle);
        bad.push(a);
    }
    let mut a = args(src, e, Some(x), true);
    a.d2 = Some(4.0.into());
    bad.push(a);
    let mut a = args(src, e, Some(x), true);
    a.reference_face = Some(Sel::Ids(vec![x, y]));
    bad.push(a);
    let mut a = args(src, e, Some(x), true);
    a.reference_face = Some(Sel::Ids(vec![123456]));
    bad.push(a);
    let mut a = args(src, e, Some(x), true);
    a.angle = None;
    bad.push(a);
    let mut a = args(src, e, None, true);
    a.angle = None;
    a.flip = true;
    bad.push(a);
    let t = s.topology(None, true).unwrap();
    let unrelated = t.faces.iter().find(|f| f.normal.unwrap()[2] > 0.99).unwrap().id;
    let mut a = args(src, e, Some(unrelated), true);
    bad.push(a.clone());
    a.reference_face = Some(Sel::Ids(vec![]));
    bad.push(a);
    let other_edge = t.edges.iter().find(|edge| !edge.faces.unwrap().contains(&x)).unwrap().id;
    let mut a = args(src, e, Some(x), true);
    a.edges = Sel::Ids(vec![e, other_edge]);
    bad.push(a);
    for a in bad {
        assert!(s.chamfer_ex(&a).is_err(), "must refuse {a:?}");
        s.save(Some(&path)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before, "refused creation exact archive");
    }
}

#[test]
fn evaluated_angle_guards_are_exactly_atomic() {
    let (mut s, src, e, x, _) = block();
    let (id, _) = s.chamfer_ex(&args(src, e, Some(x), true)).unwrap();
    let path = scratch("p6-edit-atomic.qcad");
    s.save(Some(&path)).unwrap();
    let before = std::fs::read(&path).unwrap();
    for a in [-30.0, 0.0, 90.0, 181.0] {
        assert!(s.param_set("other", &a.into()).is_err(), "global angle {a}");
        s.save(Some(&path)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            s.dimension_set(&DimensionTarget::Feature { feature: id, key: "d2".into() }, Some(&a.into()), None).is_err(),
            "native d2 angle {a}"
        );
        s.save(Some(&path)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}

#[test]
fn upstream_reference_face_loss_refuses_atomically() {
    let (mut s, a, _, _, _) = block();
    s.param_set("width", &42.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &"width".into(), &32.0.into(), false).unwrap();
    let mut ex = extrusion(sk);
    ex.op = Op::NewBody;
    let b = s.extrude(&ex).unwrap().0;
    let src = s.body_boolean(&BodyBooleanOptions { a, b, op: BooleanOp::Intersect, name: None, piece_names: None }).unwrap().body;
    let t = s.topology(Some(src), true).unwrap();
    let x = t.faces.iter().find(|f| f.normal.unwrap()[0] > 0.99).unwrap().id;
    let y = t.faces.iter().find(|f| f.normal.unwrap()[1] > 0.99).unwrap().id;
    let e = t.edges.iter().find(|e| e.faces.unwrap().contains(&x) && e.faces.unwrap().contains(&y)).unwrap().id;
    s.chamfer_ex(&args(src, e, Some(x), true)).unwrap();
    let path = scratch("p6-reference-loss.qcad");
    s.save(Some(&path)).unwrap();
    let before = std::fs::read(&path).unwrap();
    let error =
        s.param_set("width", &38.0.into()).expect_err("changed support identity must refuse even when native kernel picks a fallback");
    assert!(
        error.to_string().contains("reference face") || error.to_string().contains("named edges"),
        "support/edge identity loss must refuse: {error}"
    );
    s.save(Some(&path)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn native_tangent_contours_extend_beyond_explicit_picks() {
    let (mut s, _, _, _, _) = block();
    let t = s.topology(None, false).unwrap();
    let edges = t
        .edges
        .iter()
        .filter(|e| (e.a[2] - 10.0).abs() < 1e-5 && (e.b[2] - 10.0).abs() < 1e-5 && (e.a[0] - e.b[0]).abs() > 39.0)
        .map(|e| e.id)
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 2);
    s.fillet(None, &Sel::Ids(edges), &1.0.into(), None).unwrap();
    s.hole(&Hole {
        body: None,
        face: Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 },
        at: None,
        offset: None,
        diameter: 8.0.into(),
        depth: None,
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: None,
    })
    .unwrap();
    let source = s.topology(None, true).unwrap();
    let cylinders = source.faces.iter().filter(|f| f.kind == FaceKind::Cylinder).map(|f| f.id).collect::<Vec<_>>();
    let picks = source.edges.iter().filter(|e| e.faces.unwrap().iter().any(|f| cylinders.contains(f))).map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(picks.len(), 11);
    let before = s.result_bodies()[0].volume;
    s.chamfer(None, &Sel::Ids(picks), &0.5.into(), None, None).unwrap();
    let (h, l, r, b, c) = (10.0, 30.0, 1.0, 4.0, 0.5_f64);
    let removed = (4.0 * (h - r) + 2.0 * (l - 2.0 * r)) * c * c / 2.0
        + std::f64::consts::PI * (r * c * c - c.powi(3) / 3.0)
        + 2.0 * std::f64::consts::PI * (b * c * c + c.powi(3) / 3.0);
    assert_close(before - s.result_bodies()[0].volume, removed, 1e-6, "F-064 propagated prism and bore-rim removal");
    let t = s.topology(None, false).unwrap();
    let d = 1.0 / 2f64.sqrt();
    for sx in [-1.0, 1.0] {
        for n in [[sx * d, d, 0.0], [sx * d, -d, 0.0], [sx * d, 0.0, d]] {
            assert!(
                t.faces.iter().any(|f| f.normal.is_some_and(|v| v.into_iter().zip(n).all(|(a, b)| (a - b).abs() < 5e-5))),
                "unselected contour adds plane {n:?}"
            );
        }
    }
}

#[test]
fn precise_numeric_strings_use_the_formula_door_without_rounding() {
    let (mut s, src, e, x, y) = block();
    let mut a = args(src, e, Some(x), true);
    a.dist = "1.234567891234".into();
    a.angle = Some("3e1".into());
    let (id, _) = s.chamfer_ex(&a).unwrap();
    check(&mut s, x, y, x, 1.234567891234, 1.234567891234 * 30f64.to_radians().tan());
    assert_close(
        s.project().feat_dim_value(id, "dist", 0.0, &s.project().param_map()),
        1.234567891234,
        1e-13,
        "full-precision leg formula",
    );
    assert_close(s.project().feat_dim_value(id, "d2", 0.0, &s.project().param_map()), 30.0, 1e-13, "numeric exponent angle");
    let path = scratch("p6-precise-literals.qcad");
    s.save(Some(&path)).unwrap();
    let (mut loaded, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    check(&mut loaded, x, y, x, 1.234567891234, 1.234567891234 * 30f64.to_radians().tan());
}

#[test]
fn corrupt_open_does_not_bypass_angle_or_reference_guards() {
    let (mut s, src, e, x, _) = block();
    let (id, _) = s.chamfer_ex(&args(src, e, Some(x), true)).unwrap();
    let path = scratch("p6-invalid-open.qcad");
    s.save(Some(&path)).unwrap();
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    for text in ["0", "90", "181", "-30", "1e400"] {
        let mut p = loaded.project.clone();
        p.set_feat_dim(id, "d2", text.into());
        qymcad_io::save_project_with_brep(&p, path.to_str().unwrap(), &loaded.breps).unwrap();
        assert!(Session::open(&path).is_err(), "invalid persisted angle {text}");
    }
    let mut p = loaded.project.clone();
    if let qymcad_core::feature::FeatureKind::Chamfer { ref mut ref_face, .. } = p.timeline.iter_mut().find(|n| n.id == id).unwrap().kind {
        *ref_face = 123456;
    }
    qymcad_io::save_project_with_brep(&p, path.to_str().unwrap(), &loaded.breps).unwrap();
    assert!(Session::open(&path).is_err(), "persisted unrelated reference face must not silently use auto side");
}

#[test]
fn native_unrelated_reference_face_silently_uses_automatic_side() {
    use qymcad_core::{feature::ChamferMode, model::ChamferShape};
    let (s, src, e, _, _) = block();
    let mut volumes = Vec::new();
    let mut areas = Vec::new();
    for reference in [0, 123456] {
        let mut p = s.project().clone();
        let id =
            p.add_chamfer_ex(src, 2.0, ChamferShape { mode: ChamferMode::DistAngle, d2: 30.0, flip: false, ref_face: reference }, vec![e]);
        let (r, shapes) = qymcad_testkit::regenerate(&mut p);
        assert!(r.errors.is_empty(), "native fallback has no error: {:?}", r.errors);
        volumes.push(shapes[&id].volume());
        let mut a = p.regen_faces[&id].iter().map(|f| f.area).collect::<Vec<_>>();
        a.sort_by(f64::total_cmp);
        areas.push(a);
    }
    assert_close(volumes[0], volumes[1], 1e-9, "native unrelated reference silently equals automatic");
    for (a, b) in areas[0].iter().zip(&areas[1]) {
        assert_close(*a, *b, 1e-9, "native fallback support areas");
    }
}

#[test]
fn curved_concave_rim_angle_drops_seam_and_keeps_reference_setback() {
    let (mut s, _, _, _, _) = block();
    s.hole(&Hole {
        body: None,
        face: Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 },
        at: None,
        offset: None,
        diameter: 8.0.into(),
        depth: None,
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: None,
    })
    .unwrap();
    let t = s.topology(None, true).unwrap();
    let top = t.faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    let rim = t.edges.iter().find(|e| e.kind == EdgeKind::Circle && e.mid[2] > 9.99).unwrap().id;
    let seam = t.edges.iter().find(|e| e.seam).unwrap().id;
    let before = s.result_bodies()[0].volume;
    let a = Chamfer {
        body: None,
        edges: Sel::Ids(vec![rim, seam]),
        dist: 1.0.into(),
        d2: None,
        angle: Some(30.0.into()),
        reference_face: Some(Sel::Ids(vec![top])),
        flip: false,
        name: None,
    };
    let (_, r) = s.chamfer_ex(&a).unwrap();
    assert!(r.notes.iter().any(|n| n.contains("dropped 1 seam")));
    let second = 30f64.to_radians().tan();
    let removed = std::f64::consts::PI * second * (4.0 + 1.0 / 3.0);
    assert_close(before - s.result_bodies()[0].volume, removed, 2e-5, "concave rim frustum outside original bore");
    let t = s.topology(None, false).unwrap();
    let wall = t.faces.iter().find(|f| f.kind == FaceKind::Cylinder).unwrap();
    assert_close(wall.centroid[2], (10.0 - second) / 2.0, 2e-5, "retained bore wall centroid");
}

#[test]
fn evaluated_dist_and_two_distance_domains_are_atomic() {
    let (mut s, src, e, x, _) = block();
    s.param_set("other", &4.0.into()).unwrap();
    let (id, _) = s.chamfer_ex(&args(src, e, Some(x), false)).unwrap();
    let path = scratch("p6-distance-atomic.qcad");
    s.save(Some(&path)).unwrap();
    let before = std::fs::read(&path).unwrap();
    for (param, key) in [("leg", "dist"), ("other", "d2")] {
        for value in [-1.0, 0.0] {
            assert!(s.param_set(param, &value.into()).is_err());
            s.save(Some(&path)).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert!(s.dimension_set(&DimensionTarget::Feature { feature: id, key: key.into() }, Some(&value.into()), None).is_err());
            s.save(Some(&path)).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }
}

#[test]
fn sheet_boundary_cannot_silently_become_symmetric() {
    let mut s = Session::new_part();
    let mut sections = Vec::new();
    for z in [0.0, 10.0] {
        let plane = s.plane_offset(&PlaneRef::Base(BaseName::XY), &z.into(), None).unwrap().0;
        let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
        s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &16.0.into(), false).unwrap();
        sections.push(LoftSection { sketch: sk, profile: None });
    }
    let src = s.loft(&Loft { sections, ruled: true, op: Op::NewBody, target: None, surface: true, name: None }).unwrap().0;
    let t = s.topology(None, true).unwrap();
    let boundary = t.edges.iter().find(|e| e.faces.is_none_or(|faces| faces.contains(&0))).expect("sheet has one-sided boundary").id;
    let before = ron::ser::to_string(s.project()).unwrap();
    let a = Chamfer {
        body: Some(src),
        edges: Sel::Ids(vec![boundary]),
        dist: 1.0.into(),
        d2: None,
        angle: Some(30.0.into()),
        reference_face: None,
        flip: false,
        name: None,
    };
    let error = s.chamfer_ex(&a).unwrap_err();
    assert!(error.to_string().contains("two distinct adjacent faces"), "asymmetric boundary guard: {error}");
    assert_eq!(ron::ser::to_string(s.project()).unwrap(), before);
}

#[test]
fn corrupt_open_checks_both_two_distance_lengths() {
    let (mut s, src, e, x, _) = block();
    s.param_set("other", &4.0.into()).unwrap();
    let (id, _) = s.chamfer_ex(&args(src, e, Some(x), false)).unwrap();
    let path = scratch("p6-invalid-distances.qcad");
    s.save(Some(&path)).unwrap();
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    for key in ["dist", "d2"] {
        for text in ["0", "-1", "1e400"] {
            let mut p = loaded.project.clone();
            p.set_feat_dim(id, key, text.into());
            qymcad_io::save_project_with_brep(&p, path.to_str().unwrap(), &loaded.breps).unwrap();
            assert!(Session::open(&path).is_err(), "persisted {key}={text}");
        }
    }
}

#[test]
fn one_reference_face_can_support_every_edge_in_a_multi_pick() {
    let (mut s, src, _, x, _) = block();
    let t = s.topology(None, true).unwrap();
    let picks =
        t.edges.iter().filter(|e| e.faces.unwrap().contains(&x) && (e.a[2] - e.b[2]).abs() > 9.99).map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(picks.len(), 2);
    let a = Chamfer {
        body: Some(src),
        edges: Sel::Ids(picks),
        dist: 2.0.into(),
        d2: None,
        angle: Some(30.0.into()),
        reference_face: Some(Sel::Ids(vec![x])),
        flip: false,
        name: None,
    };
    s.chamfer_ex(&a).unwrap();
    let second = 2.0 * 30f64.to_radians().tan();
    assert_close(s.result_bodies()[0].volume, 12000.0 - 2.0 * 5.0 * 2.0 * second, 2e-5, "two disjoint triangular prisms");
    let t = s.topology(None, false).unwrap();
    let f = t.faces.iter().find(|f| f.id == x).unwrap();
    assert_close(f.area, 260.0, 2e-4, "reference face loses both setbacks");
    assert_close(f.centroid[1], 0.0, 2e-5, "symmetric disjoint support trimming centroid");
}
