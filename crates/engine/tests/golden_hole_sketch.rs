//! P8's persisted scheduling bridge, native site acceptance and exact rollback.
mod common;
use common::{assert_close, scratch};
use qymcad_engine::*;
use std::{
    f64::consts::PI,
    sync::atomic::{AtomicUsize, Ordering},
};

fn block() -> Session {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    let (_, r) = s
        .extrude(&Extrude {
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
        .unwrap();
    assert!(r.errors.is_empty());
    s
}
fn markers(s: &mut Session, z: f64, xy: &[(f64, f64)]) -> (Id, Vec<Id>) {
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &z.into(), None).unwrap();
    let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    let points = xy.iter().map(|&(x, y)| s.sketch_point(sk, &x.into(), &y.into(), true).unwrap().point).collect();
    (sk, points)
}
fn hole(sketch: Id) -> HoleSketch {
    HoleSketch {
        body: None,
        sketch,
        points: None,
        flip: false,
        diameter: 4.0.into(),
        depth: None,
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: None,
    }
}
fn volume(s: &Session, expected: f64) {
    assert_eq!(s.result_bodies().len(), 1);
    assert_close(s.result_bodies()[0].volume, expected, 2e-5, "analytic stock minus bore volume");
    for (got, want) in s.result_bodies()[0].bbox.into_iter().zip([-20., -15., 0., 20., 15., 10.]) {
        assert_close(got, want, 1e-5, "unaltered stock bbox");
    }
    assert!(s.info().errors.is_empty());
}
fn site(p: &qymcad_core::model::Project, feature: Id, point: Id, xyz: [f64; 3]) -> Vec<u32> {
    site_body(p, feature, feature, point, xyz)
}
fn site_body(p: &qymcad_core::model::Project, body: Id, feature: Id, point: Id, xyz: [f64; 3]) -> Vec<u32> {
    let faces: Vec<_> = p.regen_faces[&body]
        .iter()
        .filter(|f| p.names.get(f.id).is_some_and(|n| n.feature == feature && n.role == qymcad_core::names::Role::Hole && n.src == point))
        .collect();
    assert!(!faces.is_empty(), "point {point} must have its own named bore face");
    for f in &faces {
        assert!(f.area > 0., "point {point} bore has positive area");
        assert_close(f.centroid.x, xyz[0], 0.025, "named bore centroid x");
        assert_close(f.centroid.y, xyz[1], 0.025, "named bore centroid y");
    }
    // Recesses can add named surfaces at other z; callers use this oracle for plain bores.
    assert!(faces.iter().any(|f| (f.centroid.z - xyz[2]).abs() < 0.025), "point {point}: independent bore z");
    faces.iter().map(|f| f.id).collect()
}
fn archive(s: &mut Session) -> Vec<(String, Vec<u8>)> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    // Match feature atomicity: its snapshot is taken after the pending baseline rebuild.
    let report = s.rebuild();
    assert!(report.errors.is_empty(), "archive baseline: {:?}", report.errors);
    let path = scratch(&format!("hole-sketch-archive-{}.qcad", NEXT.fetch_add(1, Ordering::Relaxed)));
    s.save_with_options(Some(&path), true).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut entries = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut bytes).unwrap();
        entries.push((file.name().to_owned(), bytes));
    }
    entries.sort();
    entries
}

#[test]
fn all_isolated_points_and_explicit_full_set_keep_native_site_names() {
    for explicit in [false, true] {
        let mut s = block();
        let (sk, points) = markers(&mut s, 10., &[(-8., 4.), (7., -3.), (0., 0.)]);
        // Construction geometry and its endpoints must never become drill sites.
        s.sketch_line(
            sk,
            &LineSpec { x1: 30.0.into(), y1: 20.0.into(), x2: 35.0.into(), y2: 20.0.into(), construction: true, dimensioned: true },
        )
        .unwrap();
        let (id, r) =
            s.hole_by_sketch(&HoleSketch { points: explicit.then(|| points.iter().rev().copied().collect()), ..hole(sk) }).unwrap();
        assert!(r.errors.is_empty());
        volume(&s, 12000. - 3. * PI * 4. * 10.);
        for (&point, xyz) in points.iter().zip([[-8., 4., 5.], [7., -3., 5.], [0., 0., 5.]]) {
            site(s.project(), id, point, xyz);
        }
    }
}

#[test]
fn directed_flip_and_blind_depth_have_independent_wall_positions() {
    for (z, flip, centre) in [(10., false, 8.5), (0., true, 1.5)] {
        let mut s = block();
        let (sk, points) = markers(&mut s, z, &[(6., 4.)]);
        let (id, r) = s.hole_by_sketch(&HoleSketch { flip, depth: Some(3.0.into()), ..hole(sk) }).unwrap();
        assert!(r.errors.is_empty());
        volume(&s, 12000. - PI * 4. * 3.);
        site(s.project(), id, points[0], [6., 4., centre]);
    }
}

#[test]
fn native_counterbore_and_countersink_obey_hand_formulas() {
    for kind in [HoleKind::Counterbore, HoleKind::Countersink] {
        let mut s = block();
        let (sk, points) = markers(&mut s, 10., &[(-8., 4.), (7., -3.)]);
        let removal = match kind {
            HoleKind::Counterbore => PI * 4. * 7. + PI * (16. - 4.) * 2.,
            HoleKind::Countersink => PI * 4. * (7. - 2.) + PI * 2. * (16. + 8. + 4.) / 3.,
            _ => unreachable!(),
        };
        let (id, r) = s
            .hole_by_sketch(&HoleSketch { depth: Some(7.0.into()), kind, dia2: Some(8.0.into()), depth2: Some(2.0.into()), ..hole(sk) })
            .unwrap();
        assert!(r.errors.is_empty());
        volume(&s, 12000. - 2. * removal);
        for (&point, xyz) in points.iter().zip([[-8., 4., 5.5], [7., -3., 5.5]]) {
            site(s.project(), id, point, xyz);
        }
    }
}

fn driven() -> (Session, Id, Id, Id, [usize; 2]) {
    let mut s = block();
    s.param_set("px", &6.0.into()).unwrap();
    s.param_set("py", &4.0.into()).unwrap();
    let (sk, _) = markers(&mut s, 10., &[]);
    let point = s.sketch_point(sk, &"px".into(), &"py".into(), true).unwrap().point;
    let dims: Vec<_> = s
        .sketch_detail(sk)
        .unwrap()
        .constraints
        .into_iter()
        .filter(|c| c.kind == "distance_point_line" && c.points.contains(&point))
        .map(|c| c.index)
        .collect();
    assert_eq!(dims.len(), 2);
    for (&constraint, name) in dims.iter().zip(["point_x", "point_y"]) {
        s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint }, None, Some(name)).unwrap();
    }
    let (id, _) = s.hole_by_sketch(&hole(sk)).unwrap();
    (s, sk, point, id, [dims[0], dims[1]])
}

#[test]
fn global_and_named_coordinates_rebuild_engine_and_saved_app_paths() {
    let (mut s, sk, point, id, dims) = driven();
    let path = scratch("hole-sketch-app-drivers.qcad");
    s.save(Some(&path)).unwrap();
    let (mut loaded, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    for (name, value, xyz) in [("px", 9., [9., 4., 5.]), ("py", 6., [9., 6., 5.])] {
        loaded.param_set(name, &value.into()).unwrap();
        volume(&loaded, 12000. - PI * 4. * 10.);
        site(loaded.project(), id, point, xyz);
    }
    for (constraint, value, xyz) in [(dims[0], 11., [11., 6., 5.]), (dims[1], 7., [11., 7., 5.])] {
        loaded.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint }, Some(&value.into()), None).unwrap();
        site(loaded.project(), id, point, xyz);
    }
    for (name, value, xyz) in [("px", "9", [9., 4., 5.]), ("py", "6", [6., 6., 5.])] {
        let (v, _, _, p) = common::gui_edit_param_model(&path, name, value);
        assert_close(v, 12000. - PI * 4. * 10., 2e-5, "native app global volume");
        site(&p, id, point, xyz);
    }
    for (name, value, xyz) in [("point_x", "11", [11., 4., 5.]), ("point_y", "7", [6., 7., 5.])] {
        let (v, _, _, p) = common::gui_edit_driver_model(&path, name, value);
        assert_close(v, 12000. - PI * 4. * 10., 2e-5, "native app named volume");
        site(&p, id, point, xyz);
    }
}

#[test]
fn unnamed_engine_coordinate_edit_and_new_marker_rebuild_preserving_old_face_id() {
    let mut s = block();
    let (sk, points) = markers(&mut s, 10., &[(6., 4.)]);
    let point = points[0];
    let ci = s
        .sketch_detail(sk)
        .unwrap()
        .constraints
        .iter()
        .find(|c| c.kind == "distance_point_line" && c.points.contains(&point))
        .unwrap()
        .index;
    let (id, _) = s.hole_by_sketch(&hole(sk)).unwrap();
    let old = site(s.project(), id, point, [6., 4., 5.]);
    s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: ci }, Some(&9.0.into()), None).unwrap();
    assert_eq!(site(s.project(), id, point, [9., 4., 5.]), old);
    let added = s.sketch_edit(sk, |s| s.sketch_point(sk, &(-8.0).into(), &(-3.0).into(), true)).unwrap().0.point;
    volume(&s, 12000. - 2. * PI * 4. * 10.);
    assert_eq!(site(s.project(), id, point, [9., 4., 5.]), old);
    site(s.project(), id, added, [-8., -3., 5.]);
}

#[test]
fn explicit_subsets_duplicates_and_foreign_ids_are_archive_atomic() {
    let mut s = block();
    let (sk, points) = markers(&mut s, 10., &[(6., 4.), (-8., -3.)]);
    let before = archive(&mut s);
    for ids in [vec![], vec![points[0]], vec![points[0], points[0]], vec![points[0], 999_999]] {
        assert!(s.hole_by_sketch(&HoleSketch { points: Some(ids), ..hole(sk) }).is_err());
        assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
    }
}

#[test]
fn every_site_must_cut_stock_outside_and_void_refuse_atomically() {
    for xy in [[(0., 0.), (30., 0.)], [(0., 0.), (6., 4.)]] {
        let mut s = block();
        if xy[1] == (6., 4.) {
            let (sk, _) = markers(&mut s, 10., &[(6., 4.)]);
            s.hole_by_sketch(&HoleSketch { diameter: 8.0.into(), ..hole(sk) }).unwrap();
        }
        let (sk, points) = markers(&mut s, 10., &xy);
        let before = archive(&mut s);
        let err = s.hole_by_sketch(&hole(sk)).expect_err("a lost requested site must be refused").to_string();
        assert!(err.contains(&points[1].to_string()), "refusal identifies the missing point: {err}");
        assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
    }
}

#[test]
fn overlapping_holes_retain_each_named_site_and_match_the_analytic_lens() {
    let mut s = block();
    let (sk, points) = markers(&mut s, 10., &[(0., 0.), (1., 0.)]);
    let (id, r) = s.hole_by_sketch(&hole(sk)).unwrap();
    assert!(r.errors.is_empty());
    let radius = 2_f64;
    let separation = 1_f64;
    let alpha = (separation / (2. * radius)).acos();
    let lens = 2. * radius * radius * alpha - separation / 2. * (4. * radius * radius - separation * separation).sqrt();
    volume(&s, 12000. - (2. * PI * radius * radius - lens) * 10.);
    let arc_centre_offset = radius * alpha.sin() / (PI - alpha);
    let expected_area = radius * (2. * PI - 2. * alpha) * 10.;
    for (&point, x) in points.iter().zip([-arc_centre_offset, separation + arc_centre_offset]) {
        let faces: Vec<_> = s.project().regen_faces[&id]
            .iter()
            .filter(|f| {
                s.project().names.get(f.id).is_some_and(|n| n.feature == id && n.role == qymcad_core::names::Role::Hole && n.src == point)
            })
            .collect();
        assert!(!faces.is_empty() && faces.iter().all(|f| f.area > 0.), "surviving bore arcs for point {point}");
        // The periodic cylinder seam can split the surviving arc into two faces.
        let area: f64 = faces.iter().map(|f| f.area).sum();
        assert_close(area, expected_area, 0.1, "surviving cylindrical arc area");
        assert_close(faces.iter().map(|f| f.centroid.x * f.area).sum::<f64>() / area, x, 0.025, "surviving bore arc centroid x");
        assert_close(faces.iter().map(|f| f.centroid.y * f.area).sum::<f64>() / area, 0., 0.025, "surviving bore arc centroid y");
        assert_close(faces.iter().map(|f| f.centroid.z * f.area).sum::<f64>() / area, 5., 0.025, "surviving bore arc centroid z");
    }
}

#[test]
fn through_preflight_checks_directed_span_from_a_displaced_marker_plane() {
    let mut s = block();
    // Stock diagonal < 10000, but the plane-to-farthest-stock span is 10005.
    let (sk, _) = markers(&mut s, 10005., &[(6., 4.)]);
    let before = archive(&mut s);
    let err = s.hole_by_sketch(&hole(sk)).unwrap_err().to_string();
    assert!(err.contains("10000"), "{err}");
    assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
}

#[test]
fn used_point_and_sketch_deletion_and_ignored_offsets_obey_atomic_guards() {
    let (mut s, sk, point, id, _) = driven();
    let before = archive(&mut s);
    assert!(s.sketch_edit(sk, |s| s.sketch_remove_point(sk, point)).is_err());
    assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
    assert!(s.feature_delete(sk, false).is_err());
    assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
    for key in ["off_u", "off_v"] {
        for (value, name) in [(Some(2.0.into()), None), (None, Some("ignored_shift"))] {
            assert!(s.dimension_set(&DimensionTarget::Feature { feature: id, key: key.into() }, value.as_ref(), name).is_err());
            assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
        }
    }
    s.feature_delete(sk, true).unwrap();
    volume(&s, 12000.);
    assert!(!s.project().timeline.iter().any(|n| n.id == id || n.id == sk));
}

#[test]
fn evaluated_coordinate_and_feature_failures_restore_every_archive_entry() {
    let (mut s, _, _, id, _) = driven();
    let before = archive(&mut s);
    assert!(s.param_set("px", &30.0.into()).is_err());
    assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
    for key in ["diameter", "depth"] {
        assert!(s.dimension_set(&DimensionTarget::Feature { feature: id, key: key.into() }, Some(&(-1.0).into()), None).is_err());
        assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
    }
    let mut s = block();
    s.param_set("recess", &8.0.into()).unwrap();
    let (sk, _) = markers(&mut s, 10., &[(6., 4.)]);
    let (id, _) = s
        .hole_by_sketch(&HoleSketch {
            depth: Some(7.0.into()),
            kind: HoleKind::Counterbore,
            dia2: Some("recess".into()),
            depth2: Some(2.0.into()),
            ..hole(sk)
        })
        .unwrap();
    let before = archive(&mut s);
    assert!(s.param_set("recess", &3.0.into()).is_err());
    assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
    for (key, value) in [("dia2", 3.), ("depth2", 8.)] {
        assert!(s.dimension_set(&DimensionTarget::Feature { feature: id, key: key.into() }, Some(&value.into()), None).is_err());
        assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
    }
}

#[test]
fn unnamed_native_app_edit_is_stale_but_engine_open_rebuilds_the_saved_recipe() {
    let mut s = block();
    let (sk, points) = markers(&mut s, 10., &[(6., 4.)]);
    let point = points[0];
    let ci = s
        .sketch_detail(sk)
        .unwrap()
        .constraints
        .iter()
        .find(|c| c.kind == "distance_point_line" && c.points.contains(&point))
        .unwrap()
        .index;
    let (id, _) = s.hole_by_sketch(&hole(sk)).unwrap();
    let path = scratch("hole-sketch-unnamed-native.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.ensure_document();
    for i in 0..project.bodies.len() {
        if let Some(body) = project.mesh_id(i) {
            project.regen_faces.insert(body, project.bodies[i].faces.clone());
        }
    }
    let shapes = common::restore_shapes(breps.clone());
    let refs = qymcad_core::model::Project::dim_refs(&project.sketches[project.sketch_index(sk).unwrap()].constraints[ci]).unwrap();
    let target = qymcad_core::model::DimTarget::Sketch { sketch: sk, refs };
    assert!(project.set_dim_target_text(&target, "9").unwrap());
    assert!(project.solve_sketch(project.sketch_index(sk).unwrap()) <= 1e-6);
    project.mark_sketch_dirty(sk);
    let (r, _shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, shapes);
    assert!(r.errors.is_empty());
    let moved = project.sketches[project.sketch_index(sk).unwrap()].points.iter().find(|p| p.id == point).unwrap();
    assert_close(moved.x, 9., 1e-6, "native unnamed marker moved");
    site(&project, id, point, [6., 4., 5.]);
    // Save the changed recipe with the deliberately stale cached bodies, then let our engine repair it.
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (loaded, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    volume(&loaded, 12000. - PI * 4. * 10.);
    site(loaded.project(), id, point, [9., 4., 5.]);
}

#[test]
fn through_uses_full_plane_to_stock_span_even_for_compact_stock() {
    let mut s = block();
    let (sk, points) = markers(&mut s, 9999., &[(6., 4.)]);
    let (id, r) = s.hole_by_sketch(&hole(sk)).unwrap();
    assert!(r.errors.is_empty());
    volume(&s, 12000. - PI * 4. * 10.);
    site(s.project(), id, points[0], [6., 4., 5.]);
}

#[test]
fn empty_markers_wrong_direction_and_bad_names_refuse_atomically() {
    let mut s = block();
    let (empty, _) = markers(&mut s, 10., &[]);
    let before = archive(&mut s);
    assert!(s.hole_by_sketch(&hole(empty)).is_err());
    assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
    let (sk, _) = markers(&mut s, 10., &[(6., 4.)]);
    let before = archive(&mut s);
    assert!(s.hole_by_sketch(&HoleSketch { flip: true, ..hole(sk) }).is_err());
    assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
    for name in ["123", " Hole "] {
        let before = archive(&mut s);
        assert!(s.hole_by_sketch(&HoleSketch { name: Some(name.into()), ..hole(sk) }).is_err());
        assert!(archive(&mut s) == before, "refusal must preserve every saved archive entry byte for byte");
    }
}

#[test]
fn saved_global_bridge_does_not_depend_on_named_coordinate_dimensions() {
    let mut s = block();
    s.param_set("px", &6.0.into()).unwrap();
    s.param_set("py", &4.0.into()).unwrap();
    let (sk, _) = markers(&mut s, 10., &[]);
    let point = s.sketch_point(sk, &"px".into(), &"py".into(), true).unwrap().point;
    let (id, _) = s.hole_by_sketch(&hole(sk)).unwrap();
    assert!(s.project().named_dims.is_empty());
    let path = scratch("hole-sketch-global-only.qcad");
    s.save(Some(&path)).unwrap();
    for (name, value, xyz) in [("px", "9", [9., 4., 5.]), ("py", "6", [6., 6., 5.])] {
        let (v, _, _, p) = common::gui_edit_param_model(&path, name, value);
        assert_close(v, 12000. - PI * 4. * 10., 2e-5, "global-only native app volume");
        site(&p, id, point, xyz);
    }
    let dims: Vec<_> = s
        .sketch_detail(sk)
        .unwrap()
        .constraints
        .into_iter()
        .filter(|c| c.kind == "distance_point_line" && c.points.contains(&point))
        .collect();
    for (dim, name) in dims.iter().zip(["late_point_x", "late_point_y"]) {
        s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: dim.index }, None, Some(name)).unwrap();
    }
    s.save(Some(&path)).unwrap();
    for (name, value, xyz) in [("late_point_x", "11", [11., 4., 5.]), ("late_point_y", "7", [6., 7., 5.])] {
        let (_, _, _, p) = common::gui_edit_driver_model(&path, name, value);
        site(&p, id, point, xyz);
    }
    // A source-only archive has no stored B-reps: reopening must rebuild the same site names.
    let path = scratch("hole-sketch-source-only.qcad");
    qymcad_io::save_project(s.project(), path.to_str().unwrap()).unwrap();
    let (loaded, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    volume(&loaded, 12000. - PI * 4. * 10.);
    site(loaded.project(), id, point, [6., 4., 5.]);
}

#[test]
fn dynamic_datum_supports_refuse_until_native_hole_frame_scheduling_is_fixed() {
    for named in [false, true] {
        let mut s = block();
        s.param_set("height", &10.0.into()).unwrap();
        let dist = if named { 10.0.into() } else { "height".into() };
        let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &dist, None).unwrap();
        if named {
            s.dimension_set(&DimensionTarget::Feature { feature: plane, key: "dist".into() }, None, Some("support_height")).unwrap();
        }
        let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
        s.sketch_point(sk, &6.0.into(), &4.0.into(), true).unwrap();
        let before = archive(&mut s);
        assert!(s.hole_by_sketch(&hole(sk)).is_err(), "dynamic datum support cannot promise app propagation");
        assert!(archive(&mut s) == before, "dynamic support refusal preserves every archive entry");
    }
}

#[test]
fn direct_source_face_host_is_allowed_but_independent_face_host_is_refused() {
    for independent in [false, true] {
        let mut s = block();
        let source = s.result_bodies()[0].id;
        let host = if independent {
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
        } else {
            source
        };
        let face = s.topology(Some(host), false).unwrap().faces.into_iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.999)).unwrap().id;
        let sk = s.sketch_create(&PlaneRef::Face { body: host, face }, None).unwrap();
        let point = s.sketch_point(sk, &6.0.into(), &4.0.into(), true).unwrap().point;
        let before = archive(&mut s);
        let result = s.hole_by_sketch(&HoleSketch { body: Some(source), ..hole(sk) });
        if independent {
            assert!(result.is_err(), "independent face host cannot promise native app propagation");
            assert!(archive(&mut s) == before, "independent host refusal preserves every archive entry");
        } else {
            let (id, r) = result.unwrap();
            assert!(r.errors.is_empty());
            volume(&s, 12000. - PI * 4. * 10.);
            site(s.project(), id, point, [6., 4., 5.]);
        }
    }
}

#[test]
fn projected_marker_sketch_is_refused_with_exact_archive_rollback() {
    let mut s = block();
    let source = s.result_bodies()[0].id;
    let edge = s.topology(Some(source), true).unwrap().edges.into_iter().find(|e| e.a[2] > 9.99 && e.b[2] > 9.99).unwrap().id;
    let (sk, _) = markers(&mut s, 10., &[(6., 4.)]);
    s.sketch_project(sk, source, &[ProjSource::Edge(edge)]).unwrap();
    let before = archive(&mut s);
    assert!(s.hole_by_sketch(&hole(sk)).is_err(), "projected markers cannot promise native app propagation");
    assert!(archive(&mut s) == before, "projected marker refusal preserves every archive entry");
}

#[test]
fn point_motion_rebuilds_a_downstream_push_and_refusal_restores_its_exact_brep() {
    let (mut s, _, point, id, _) = driven();
    let (push, r) = s.push_face(Some(id), &Sel::Facing { dir: [1., 0., 0.], tol_deg: 1. }, &2.0.into(), None).unwrap();
    assert!(r.errors.is_empty());
    s.param_set("px", &9.0.into()).unwrap();
    assert_close(s.result_bodies()[0].volume, 12000. + 2. * 30. * 10. - PI * 4. * 10., 2e-5, "downstream push volume");
    site_body(s.project(), push, id, point, [9., 4., 5.]);
    let before = archive(&mut s);
    assert!(s.param_set("px", &30.0.into()).is_err());
    assert!(archive(&mut s) == before, "failed point movement restores downstream push B-rep as well");
}

#[test]
fn through_guard_is_rechecked_after_the_source_stock_grows() {
    let mut s = Session::new_part();
    s.param_set("stock_height", &10.0.into()).unwrap();
    let stock = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(stock, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch: stock,
        profiles: None,
        height: "stock_height".into(),
        down: None,
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let point = s.sketch_point(sk, &6.0.into(), &4.0.into(), true).unwrap().point;
    let (id, _) = s.hole_by_sketch(&HoleSketch { flip: true, ..hole(sk) }).unwrap();
    s.param_set("stock_height", &100.0.into()).unwrap();
    assert_close(s.result_bodies()[0].volume, (40. * 30. - PI * 4.) * 100., 2e-5, "through source grew to 100");
    site(s.project(), id, point, [6., 4., 50.]);
    let before = archive(&mut s);
    let err = s.param_set("stock_height", &10001.0.into()).unwrap_err().to_string();
    assert!(err.contains("10000"), "directed-span refusal: {err}");
    assert!(archive(&mut s) == before, "stock growth beyond fixed depth restores every B-rep and recipe");
}

#[test]
fn every_formula_size_is_revalidated_on_later_parameter_edits() {
    let mut s = block();
    for (name, value) in [("bore_d", 4.), ("bore_h", 7.), ("recess_d", 8.), ("recess_h", 2.)] {
        s.param_set(name, &value.into()).unwrap();
    }
    let (sk, _) = markers(&mut s, 10., &[(6., 4.)]);
    s.hole_by_sketch(&HoleSketch {
        diameter: "bore_d".into(),
        depth: Some("bore_h".into()),
        kind: HoleKind::Counterbore,
        dia2: Some("recess_d".into()),
        depth2: Some("recess_h".into()),
        ..hole(sk)
    })
    .unwrap();
    let before = archive(&mut s);
    for (name, value) in [("bore_d", -4.), ("bore_h", -7.), ("recess_d", 3.), ("recess_h", 8.)] {
        assert!(s.param_set(name, &value.into()).is_err(), "invalid evaluated size {name}={value}");
        assert!(archive(&mut s) == before, "{name} refusal restores every B-rep and recipe");
    }
}

#[test]
fn downstream_legacy_query_without_edge_names_never_runs_through_hidden_sketch_dependency() {
    use qymcad_core::refs::{Query, Ref};
    let (mut s, sk, _, id, dims) = driven();
    let path = scratch("hole-sketch-legacy-query.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let fillet = project.add_fillet_ref(id, 0.3, Ref::many(Query::Oriented { dir: [0., 0., 1.], tol_deg: 1. }));
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, common::restore_shapes(breps));
    assert!(report.errors.is_empty(), "native legacy fixture: {:?}", report.errors);
    for (body, faces) in report.built {
        project.set_body_faces(body, faces);
    }
    let mut breps: Vec<_> = {
        let _gate = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(&body, shape)| (body, shape.to_brep_bytes().unwrap())).collect()
    };
    common::native::rename_names(&mut breps.iter_mut().find(|(body, _)| *body == id).unwrap().1, &[], true);
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    match Session::open(&path) {
        Err(error) => {
            assert!(error.to_string().contains("stored edge query"), "safe-open refusal must explain the legacy query: {error}");
        }
        Ok((mut loaded, r)) => {
            if !r.errors.is_empty() {
                assert!(
                    r.errors.iter().any(|e| e.node == fillet && e.message.contains("stored edge query")),
                    "safe open query refusal: {:?}",
                    r.errors
                );
                return;
            }
            let before = archive(&mut loaded);
            let error =
                loaded.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: dims[0] }, Some(&9.0.into()), None).unwrap_err();
            assert!(error.to_string().contains("stored edge query"), "query preflight: {error}");
            assert!(error.to_string().contains(&fillet.to_string()), "query refusal identifies the downstream feature: {error}");
            assert!(archive(&mut loaded) == before, "query preflight leaves every saved archive entry unchanged");
        }
    }
}

#[test]
fn unnamed_fixed_datum_edit_never_reports_success_with_stale_hole_frame() {
    let mut s = block();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), None).unwrap();
    let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    let point = s.sketch_point(sk, &6.0.into(), &4.0.into(), true).unwrap().point;
    let (id, _) = s.hole_by_sketch(&HoleSketch { depth: Some(4.0.into()), ..hole(sk) }).unwrap();
    volume(&s, 12000. - PI * 4. * 4.);
    site(s.project(), id, point, [6., 4., 8.]);
    let before = archive(&mut s);
    match s.dimension_set(&DimensionTarget::Feature { feature: plane, key: "dist".into() }, Some(&11.0.into()), None) {
        Ok(_) => {
            volume(&s, 12000. - PI * 4. * 3.);
            site(s.project(), id, point, [6., 4., 8.5]);
        }
        Err(_) => assert!(archive(&mut s) == before, "fixed datum refusal restores every archive entry"),
    }
}

#[test]
fn cross_part_marker_is_refused_without_changing_the_archive() {
    let mut s = block();
    let source = s.result_bodies()[0].id;
    let (sk, _) = markers(&mut s, 10., &[(6., 4.)]);
    let path = scratch("hole-sketch-cross-part.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let foreign = project.add_part("Foreign markers");
    project.timeline.iter_mut().find(|n| n.id == sk).unwrap().parent = Some(foreign);
    assert_ne!(project.sketch_owner(sk), project.body_owner(source));
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (mut loaded, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    let before = archive(&mut loaded);
    let error = loaded.hole_by_sketch(&HoleSketch { body: Some(source), ..hole(sk) }).unwrap_err().to_string();
    assert!(error.contains("same part"), "cross-part refusal: {error}");
    assert!(archive(&mut loaded) == before, "cross-part refusal preserves every archive entry");
}

#[test]
fn open_refuses_evaluated_bad_sizes_even_with_clean_cached_breps() {
    let mut s = block();
    let (sk, _) = markers(&mut s, 10., &[(6., 4.)]);
    let (id, _) = s
        .hole_by_sketch(&HoleSketch {
            depth: Some(7.0.into()),
            kind: HoleKind::Counterbore,
            dia2: Some(8.0.into()),
            depth2: Some(2.0.into()),
            ..hole(sk)
        })
        .unwrap();
    let path = scratch("hole-sketch-clean-invalid.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    for (key, value) in [("diameter", "-4"), ("depth", "-7"), ("dia2", "3"), ("depth2", "8")] {
        let mut invalid = project.clone();
        invalid.feat_dims.entry(id).or_default().insert(key.into(), value.into());
        assert!(invalid.timeline.iter().all(|n| !n.dirty), "fixture's stored bodies are clean");
        qymcad_io::save_project_guarded_with_brep(&invalid, path.to_str().unwrap(), &breps).unwrap();
        assert!(Session::open(&path).is_err(), "open refuses {key}={value} despite clean caches");
    }
}

#[test]
fn metadata_reconciles_driver_rename_removal_and_formula_replacement_preserving_other_fields() {
    let (mut s, sk, point, id, dims) = driven();
    s.dimension_set(&DimensionTarget::Feature { feature: id, key: "diameter".into() }, Some(&"4+0".into()), None).unwrap();
    let path = scratch("a4-metadata-reconcile.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.feat_dims.entry(id).or_default().insert("vendor_note".into(), "42".into());
    project.feat_dims.entry(id).or_default().insert("sketch_dep_obsolete".into(), "px".into());
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (mut loaded, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    assert_eq!(loaded.project().feat_dim(id, "vendor_note"), Some("42"));
    assert!(loaded.project().feat_dim(id, "sketch_dep_obsolete").is_none());
    loaded.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: dims[0] }, Some(&9.0.into()), Some("renamed_x")).unwrap();
    loaded.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: dims[1] }, Some(&5.0.into()), Some("")).unwrap();
    let metadata: Vec<_> =
        loaded.project().feat_dims[&id].iter().filter(|(k, _)| k.starts_with("sketch_dep_")).map(|(_, v)| v.as_str()).collect();
    assert!(metadata.contains(&"renamed_x"));
    for obsolete in ["point_x", "point_y", "px", "py"] {
        assert!(!metadata.contains(&obsolete), "obsolete bridge read {obsolete}");
    }
    loaded.param_delete("px").unwrap();
    loaded.param_delete("py").unwrap();
    assert_eq!(loaded.project().feat_dim(id, "diameter"), Some("4+0"));
    assert_eq!(loaded.project().feat_dim(id, "vendor_note"), Some("42"));
    loaded.save(Some(&path)).unwrap();
    let (v, _, _, p) = common::gui_edit_driver_model(&path, "renamed_x", "11");
    assert_close(v, 12000. - PI * 4. * 10., 1e-5, "renamed saved driver volume");
    site(&p, id, point, [11., 5., 5.]);
}

#[test]
fn downstream_fillet_safety_is_retained_after_named_point_motion() {
    let (mut s, sk, point, id, dims) = driven();
    s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: dims[0] }, Some(&2.0.into()), None).unwrap();
    let edges = Sel::Along { dir: [0., 0., 1.], tol_deg: 1. };
    let (fillet, r) = s.fillet(Some(id), &edges, &"point_x*3".into(), None).unwrap();
    assert!(r.errors.is_empty());
    assert_close(s.result_bodies()[0].volume, 12000. - PI * 4. * 10. - (4. - PI) * 36. * 10., 1e-4, "four R6 corners plus bore");
    site_body(s.project(), fillet, id, point, [2., 4., 5.]);
    let before = archive(&mut s);
    let error =
        s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: dims[0] }, Some(&6.0.into()), None).unwrap_err().to_string();
    assert!(
        error.contains("does not fit") && error.contains(&fillet.to_string()),
        "F-074 must identify overlapping downstream fillet: {error}"
    );
    assert!(archive(&mut s) == before, "F-074 refusal restores hole and fillet archives exactly");
}

#[test]
fn externally_reordered_marker_sketch_cannot_be_a_forward_hole_dependency() {
    let (mut s, sketch, _, hole, _) = driven();
    let path = scratch("a4-forward-marker.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let index = project.timeline.iter().position(|n| n.id == sketch).unwrap();
    let marker = project.timeline.remove(index);
    let index = project.timeline.iter().position(|n| n.id == hole).unwrap();
    project.timeline.insert(index + 1, marker);
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    assert!(Session::open(&path).is_err(), "holes require preceding marker sketches even when cached frames exist");
}

#[test]
fn tilted_fixed_markers_use_eight_corner_span_and_oblique_bore_positions() {
    for (distance, refuse) in [(9980., false), (9995., true)] {
        let mut s = block();
        // Tilt X45: n=(0,-1/sqrt2,+1/sqrt2), projected frame origin has n·o=distance.
        // Max directed span over the box is distance+15/sqrt2, at y15,z0.
        // Checking only bbox.min and bbox.max would miss that corner.
        let (plane, _) = s
            .plane_offset_tilt(
                &PlaneRef::Base(BaseName::XY),
                &(distance * 2_f64.sqrt()).into(),
                &45.0.into(),
                &0.0.into(),
                &0.0.into(),
                None,
            )
            .unwrap();
        let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
        let point = s.sketch_point(sk, &0.0.into(), &0.0.into(), true).unwrap().point;
        let before = archive(&mut s);
        let result = s.hole_by_sketch(&hole(sk));
        if refuse {
            assert!(result.unwrap_err().to_string().contains("10000"));
            assert!(archive(&mut s) == before, "oblique through-span refusal restores every archive entry");
        } else {
            let (id, r) = result.unwrap();
            assert!(r.errors.is_empty());
            volume(&s, 12000. - 40. * PI * 2_f64.sqrt());
            site(s.project(), id, point, [0., -5., 5.]);
        }
    }
}

#[test]
fn failed_marker_addition_restores_hole_and_downstream_fillet_archives() {
    let (mut s, sk, _, hole, _) = driven();
    s.fillet(Some(hole), &Sel::Along { dir: [0., 0., 1.], tol_deg: 1. }, &1.0.into(), None).unwrap();
    let before = archive(&mut s);
    let error = s.sketch_edit(sk, |s| s.sketch_point(sk, &40.0.into(), &4.0.into(), true)).unwrap_err().to_string();
    assert!(error.contains("bore face"), "missing marker must refuse: {error}");
    assert!(archive(&mut s) == before, "failed marker addition restores every hole/fillet archive byte");
}

#[test]
fn stock_face_host_native_global_and_named_growth_preserve_blind_bore_origin() {
    let mut s = block();
    let source = s.result_bodies()[0].id;
    s.param_set("stock_h", &10.0.into()).unwrap();
    s.dimension_set(&DimensionTarget::Feature { feature: source, key: "height".into() }, Some(&"stock_h".into()), Some("stock_size"))
        .unwrap();
    let face = s.topology(Some(source), false).unwrap().faces.into_iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.999)).unwrap().id;
    let sk = s.sketch_create(&PlaneRef::Face { body: source, face }, None).unwrap();
    let point = s.sketch_point(sk, &6.0.into(), &4.0.into(), true).unwrap().point;
    let (id, _) = s.hole_by_sketch(&HoleSketch { depth: Some(3.0.into()), ..hole(sk) }).unwrap();
    let path = scratch("a4-face-host-native-growth.qcad");
    s.save(Some(&path)).unwrap();
    for named in [false, true] {
        let (v, _, _, p) = if named {
            common::gui_edit_driver_model(&path, "stock_size", "12")
        } else {
            common::gui_edit_param_model(&path, "stock_h", "12")
        };
        assert_close(v, 40. * 30. * 12. - PI * 4. * 3., 1e-5, "native raised stock minus three-mm blind bore");
        site(&p, id, point, [6., 4., 10.5]);
    }
    s.param_set("stock_h", &11.0.into()).unwrap();
    assert_close(s.result_bodies()[0].volume, 40. * 30. * 11. - PI * 4. * 3., 1e-5, "engine raised stock blind bore");
    site(s.project(), id, point, [6., 4., 9.5]);
}

#[test]
fn existing_hole_error_never_exempts_new_missing_site_edits_and_can_be_repaired() {
    let (mut s, sk, point, hole, dims) = driven();
    let path = scratch("a4-prior-hole-error.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.parameters.iter_mut().find(|p| p.name == "px").unwrap().expr = "30".into();
    assert!(project.eval_parameters().is_empty());
    assert!(project.solve_sketch(project.sketch_index(sk).unwrap()) < 1e-6);
    project.mark_sketch_dirty(sk);
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (mut loaded, report) = Session::open(&path).unwrap();
    assert!(report.errors.iter().any(|e| e.node == hole));
    // Unlike archive(), this snapshot must preserve an intentionally broken baseline without rebuilding it.
    let current_archive = |s: &mut Session| {
        s.save_with_options(Some(&path), true).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut entries = Vec::new();
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut bytes).unwrap();
            entries.push((file.name().to_owned(), bytes));
        }
        entries.sort();
        entries
    };
    let before = current_archive(&mut loaded);
    assert!(loaded.param_set("px", &31.0.into()).is_err(), "prior error cannot exempt a newly missing bore site");
    assert!(current_archive(&mut loaded) == before, "same-node parameter refusal is archive-atomic");
    assert!(loaded.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: dims[0] }, Some(&32.0.into()), None).is_err());
    assert!(current_archive(&mut loaded) == before, "same-node dimension refusal is archive-atomic");
    loaded.param_set("px", &9.0.into()).unwrap();
    volume(&loaded, 12000. - PI * 4. * 10.);
    site(loaded.project(), hole, point, [9., 4., 5.]);
}
