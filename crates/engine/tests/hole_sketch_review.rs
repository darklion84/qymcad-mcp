//! Independent A4 review: ignored dimensions, through metadata and fixed datum edits.
mod common;
use common::assert_close;
use qymcad_engine::*;
use std::collections::BTreeMap;

fn archive(s: &mut Session, label: &str) -> BTreeMap<String, Vec<u8>> {
    let path = common::scratch(label);
    s.save_with_options(Some(&path), true).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut file = zip.by_index(i).unwrap();
            let name = file.name().to_string();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}

fn fixture(depth: Option<Num>) -> (Session, Id, Id) {
    let mut s = Session::new_part();
    let profile = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(profile, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch: profile,
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
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), None).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    s.sketch_point(sketch, &6.0.into(), &4.0.into(), true).unwrap();
    let (hole, _) = s
        .hole_by_sketch(&HoleSketch {
            body: None,
            sketch,
            points: None,
            flip: false,
            diameter: 4.0.into(),
            depth,
            kind: HoleKind::Plain,
            dia2: None,
            depth2: None,
            name: None,
        })
        .unwrap();
    (s, plane, hole)
}

#[test]
fn explicit_depth_edit_converts_through_to_blind_but_naming_keeps_through() {
    let (mut s, _, hole) = fixture(None);
    let target = DimensionTarget::Feature { feature: hole, key: "depth".into() };
    s.dimension_set(&target, None, Some("drill_depth")).unwrap();
    assert!(s.project().feat_dim(hole, "sketch_through").is_some(), "naming is not a depth edit");
    s.param_set("blind_depth", &3.0.into()).unwrap();
    s.dimension_set(&target, Some(&"blind_depth".into()), None).unwrap();
    assert!(s.project().feat_dim(hole, "sketch_through").is_none(), "explicit depth switches to blind, removing through policy");
    assert_close(s.result_bodies()[0].volume, 12000. - std::f64::consts::PI * 4. * 3., 1e-6, "blind depth cut");
    s.param_set("blind_depth", &5.0.into()).unwrap();
    assert_close(s.result_bodies()[0].volume, 12000. - std::f64::consts::PI * 4. * 5., 1e-6, "blind formula remains parametric");
}

#[test]
fn plain_hole_recess_dimensions_cannot_be_written_or_named() {
    let (mut s, _, hole) = fixture(Some(5.0.into()));
    for key in ["dia2", "depth2"] {
        let target = DimensionTarget::Feature { feature: hole, key: key.into() };
        assert!(s.dimension_set(&target, Some(&2.0.into()), None).is_err(), "plain hole ignores {key} value");
        assert!(s.dimension_set(&target, None, Some("ignored_recess")).is_err(), "plain hole ignores named {key}");
    }
}

#[test]
fn fixed_datum_literal_edit_refuses_atomically_or_rebuilds_blind_hole_origin() {
    let (mut s, plane, hole) = fixture(Some(5.0.into()));
    let before = ron::to_string(s.project()).unwrap();
    let before_volume = s.result_bodies()[0].volume;
    let result = s.dimension_set(&DimensionTarget::Feature { feature: plane, key: "dist".into() }, Some(&12.0.into()), None);
    if result.is_err() {
        assert_eq!(ron::to_string(s.project()).unwrap(), before);
        assert_close(s.result_bodies()[0].volume, before_volume, 1e-9, "atomic refusal preserves original volume");
    } else {
        // A 5 mm cutter starts at z12 and intersects stock z[0,10] only over [7,10].
        assert_close(s.result_bodies()[0].volume, 12000. - std::f64::consts::PI * 4. * 3., 1e-6, "changed marker-plane to stock overlap");
        let face = s.project().regen_faces[&hole]
            .iter()
            .find(|f| s.project().names.get(f.id).is_some_and(|n| n.feature == hole && n.role == qymcad_core::names::Role::Hole))
            .unwrap();
        assert_close(face.centroid.z, 8.5, 1e-4, "bore z follows literal datum edit");
    }
}

#[test]
fn removing_one_of_two_used_points_refuses_without_changing_any_archive_entry() {
    let (mut s, _, hole) = fixture(None);
    let sketch = match s.project().timeline.iter().find(|n| n.id == hole).unwrap().kind {
        qymcad_core::feature::FeatureKind::Hole { sketch, .. } => sketch,
        _ => unreachable!(),
    };
    s.sketch_edit(sketch, |s| s.sketch_point(sketch, &(-8.0).into(), &(-6.0).into(), true)).unwrap();
    let points = s.project().sketch_hole_point_ids(sketch);
    assert_eq!(points.len(), 2);
    let before = archive(&mut s, "a4-review-point-before.qcad");
    let error = s.sketch_edit(sketch, |s| s.sketch_remove_point(sketch, points[0])).unwrap_err();
    assert!(error.to_string().contains(&points[0].to_string()), "{error}");
    let after = archive(&mut s, "a4-review-point-after.qcad");
    assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>());
    for (name, bytes) in &before {
        assert!(after[name] == *bytes, "archive entry {name} changed");
    }
}

#[test]
fn externally_saved_unknown_hole_kind_is_refused_before_native_coercion() {
    let (mut s, _, hole) = fixture(Some(5.0.into()));
    let original = common::scratch("a4-review-kind-source.qcad");
    let invalid = common::scratch("a4-review-kind-invalid.qcad");
    s.save(Some(&original)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(original.to_str().unwrap()).unwrap();
    let node = project.timeline.iter_mut().find(|n| n.id == hole).unwrap();
    match &mut node.kind {
        qymcad_core::feature::FeatureKind::Hole { kind, dia2, depth2, .. } => {
            *kind = 3;
            *dia2 = 8.;
            *depth2 = 2.;
        }
        _ => unreachable!(),
    }
    qymcad_io::save_project_with_brep(&project, invalid.to_str().unwrap(), &breps).unwrap();
    assert!(Session::open(&invalid).is_err(), "unknown native kind must not silently become a supported hole kind");
}

#[test]
fn named_depth_edit_in_saved_app_converts_through_to_blind_on_engine_open() {
    let (mut s, _, hole) = fixture(None);
    s.dimension_set(&DimensionTarget::Feature { feature: hole, key: "depth".into() }, None, Some("drill_depth")).unwrap();
    let path = common::scratch("a4-app-depth.qcad");
    s.save(Some(&path)).unwrap();
    let (v, _, _, project) = common::gui_edit_driver_model(&path, "drill_depth", "3");
    assert_close(v, 12000. - std::f64::consts::PI * 4. * 3., 1e-6, "native SetDriver depth volume");
    // The native app knows nothing of our through marker. Reconcile it when we reopen.
    qymcad_io::save_project(&project, path.to_str().unwrap()).unwrap();
    let (loaded, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    assert!(loaded.project().feat_dim(hole, "sketch_through").is_none(), "explicit native depth must drop the fixed-through policy");
    assert_close(loaded.result_bodies()[0].volume, v, 1e-6, "engine retains app's blind recipe");
}
