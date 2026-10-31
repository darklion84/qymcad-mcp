//! Standalone guards and migration of pre-R5 native recipes.
use super::*;
fn fixture(name: &str) -> Session {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(file.to_str().unwrap()).unwrap();
    project.ensure_document();
    for i in 0..project.bodies.len() {
        if let (Some(body), false) = (project.mesh_id(i), project.bodies[i].faces.is_empty()) {
            project.regen_faces.insert(body, project.bodies[i].faces.clone());
        }
    }
    let mut s = Session::new_part();
    s.p = project;
    s.shapes =
        qymcad_kernel::imports::restore(breps.into_iter().map(|(body, bytes)| qymcad_kernel::imports::SavedBody { body, bytes }).collect())
            .into_iter()
            .map(|r| (r.body, r.shape))
            .collect();
    s.restore_edges().unwrap();
    s
}
fn archive(s: &mut Session) -> std::collections::BTreeMap<String, Vec<u8>> {
    let file = std::env::temp_dir().join(format!("qymcad-r5-unit-{}-{:?}.qcad", std::process::id(), std::thread::current().id()));
    s.save_with_options(Some(&file), true).unwrap();
    let mut z = zip::ZipArchive::new(std::fs::File::open(&file).unwrap()).unwrap();
    let entries = (0..z.len())
        .map(|i| {
            let mut e = z.by_index(i).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut e, &mut bytes).unwrap();
            (e.name().into(), bytes)
        })
        .collect();
    std::fs::remove_file(file).unwrap();
    entries
}
#[test]
fn standalone_rebuild_settles_already_mixed_native_scope() {
    let mut s = fixture("r5-native-mixed.qcad");
    assert_eq!(s.p.param_map()["first_w"], 60.);
    assert_eq!(s.p.param_map()["second_w"], 40.);
    s.p.mark_all_dirty();
    let report = s.rebuild();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(s.p.param_map()["second_w"], 60.);
    assert!((s.shapes[&65].bbox().unwrap()[2] + 5.).abs() < 1e-6);
}
#[test]
fn standalone_transitive_reference_refusal_restores_immediate_archive() {
    let mut s = fixture("r5-transitive-reference.qcad");
    s.p.mark_all_dirty();
    let before = archive(&mut s);
    let report = s.rebuild();
    assert!(report.errors.iter().any(|e| e.message.contains("reference measurement measured_w")), "{report:?}");
    assert_eq!(archive(&mut s), before);
}
#[test]
fn opening_legacy_offset_recipe_persists_native_point_retention() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/r5-native-offset.qcad");
    let (mut s, report) = Session::open(&file).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    // This pre-R5 fixture contains the free Point at (50,60), alongside its driving Point.
    let si = s.p.sketches.iter().position(|sk| sk.points.iter().any(|pt| (pt.x - 50.).abs() < 1e-9 && (pt.y - 60.).abs() < 1e-9)).unwrap();
    let point = s.p.sketches[si].points.iter().find(|pt| (pt.x - 50.).abs() < 1e-9 && (pt.y - 60.).abs() < 1e-9).unwrap().id;
    let output = std::env::temp_dir().join(format!("qymcad-r5-legacy-offset-{}.qcad", std::process::id()));
    s.save(Some(&output)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(output.to_str().unwrap()).unwrap();
    project.parameters.iter_mut().find(|p| p.name == "bend").unwrap().expr = "2".into();
    project.eval_parameters();
    for si in 0..project.sketches.len() {
        if project.sketch_reads_names(si) {
            project.solve_sketch(si);
        }
    }
    project.mark_all_dirty();
    let shapes =
        qymcad_kernel::imports::restore(breps.into_iter().map(|(body, bytes)| qymcad_kernel::imports::SavedBody { body, bytes }).collect())
            .into_iter()
            .map(|r| (r.body, r.shape))
            .collect();
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, shapes);
    assert!(report.errors.is_empty(), "{report:?}");
    assert!(project.sketches[si].points.iter().any(|p| p.id == point));
    // Native cut body remains two bores (the source body is larger).
    let hole =
        project.timeline.iter().find(|n| matches!(n.kind,qymcad_core::feature::FeatureKind::Hole{sketch,..} if sketch!=0)).unwrap().id;
    assert!((shapes[&hole].volume() - (196000. - 32. * std::f64::consts::PI)).abs() < 1e-6);
    std::fs::remove_file(output).unwrap();
}

#[test]
fn native_joint_readings_are_references_but_specifications_are_driving() {
    use qymcad_core::{
        feature::JointKind,
        model::{DimTarget, HoleTool, NamedDim},
    };
    for (slot, key) in ["angle", "offset", "offset2"].into_iter().enumerate() {
        let mut s = Session::new_part();
        let joint = s.p.alloc_id();
        s.p.joints.push(serde_json::from_value(serde_json::json!({"id":joint,"name":"joint readings","a":0,"b":0,"kind":JointKind::Cylindrical,"angle":40,"offset":40,"offset2":40})).unwrap());
        s.p.named_dims.push(NamedDim { name: "joint_value".into(), target: DimTarget::Feature { node: joint, key: key.into() } });
        let marker = s.p.add_sketch("marker", vec![], None);
        s.p.add_sketch_node(marker, "marker");
        s.p.set_feat_dim(marker, "coordinate", "joint_value/4".into());
        s.p.add_hole_from_sketch(0, marker, HoleTool { kind: 0, diameter: 4., depth: 4., dia2: 0., depth2: 0. }, true);
        let before = archive(&mut s);
        let (_, error) = s.check_hole_supports().expect_err("free joint field is a solver reading");
        assert!(error.to_string().contains("reference measurement joint_value"), "{error}");
        assert_eq!(archive(&mut s), before);
        s.p.joints[0].drive[slot] = Some(40.);
        assert!(s.check_hole_supports().is_ok(), "explicit specification remains driving");
        s.p.joints[0].drive[slot] = None;
        s.p.set_feat_dim(joint, key, "40".into());
        assert!(s.check_hole_supports().is_ok(), "own formula replaces the reading");
    }
}
