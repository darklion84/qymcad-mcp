//! E3: metadata changes must leave the recipe and exact geometry alone.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

fn extrusion(sketch: Id) -> Extrude {
    serde_json::from_value(json!({"sketch":sketch,"height":"h","name":"block"})).unwrap()
}
fn stock() -> (Session, Id, Id, Id) {
    let mut s = Session::new_part();
    assert!(s.param_set("h", &4.0.into()).is_ok());
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
    assert!(s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &20.0.into(), false).is_ok());
    let (a, r) = s.extrude(&extrusion(sk)).unwrap();
    volume(&r, 10.0 * 20.0 * 4.0); // Prism = 800 mm³.
    assert_eq!(s.rename(RenameTarget::Body(a), "Housing").unwrap().name, "Housing");
    let (m, r) = s
        .move_body(&Move {
            body: Some(a),
            translation: Some([5.0.into(), 0.0.into(), 0.0.into()]),
            rotation: None,
            name: Some("moved".into()),
        })
        .unwrap();
    volume(&r, 10.0 * 20.0 * 4.0); // Rigid translation keeps 800 mm³.
    (s, sk, a, m.body)
}
fn volume(r: &Rebuild, expected: f64) {
    assert!(r.errors.is_empty(), "{r:?}");
    assert_eq!(r.bodies.len(), 1);
    assert_close(r.bodies[0].volume, expected, 1e-6, "area * height");
}
fn breps(s: &Session) -> Vec<(Id, Vec<u8>)> {
    let _gate = qymcad_kernel::kernel_gate();
    s.project().bodies.iter().filter_map(|b| s.shape(b.id).map(|sh| (b.id, sh.to_brep_bytes().unwrap()))).collect()
}
// ADR0042 runs on native restored copies. Compare the same representation, independently of appearance.
// A Move's first native restore may normalize its serialized B-rep; original handles remain exact for undo.
fn restored_breps(bytes: Vec<(Id, Vec<u8>)>) -> Vec<(Id, Vec<u8>)> {
    let shapes = common::restore_shapes(bytes);
    let _gate = qymcad_kernel::kernel_gate();
    let mut out = shapes.iter().map(|(&id, s)| (id, s.to_brep_bytes().unwrap())).collect::<Vec<_>>();
    out.sort_by_key(|(id, _)| *id);
    out
}
fn snapshot(s: &mut Session, b: Id) -> (String, Vec<(Id, Vec<u8>)>, DocInfo, Topology) {
    let topo = s.topology(Some(b), true).unwrap();
    (ron::to_string(s.project()).unwrap(), breps(s), s.info(), topo)
}
fn result(r: AppearanceResult, a: Id, b: Id, visible: bool, rgb: [u8; 3], source: &str, changed: bool) {
    assert_eq!(
        serde_json::to_value(r).unwrap(),
        json!({"body":{"id":b,"name":"moved","body_name":"Housing"},"lineage_root":a,"visible":visible,"color_rgb":rgb,"color_source":source,"scope":{"visibility":"body","color":"lineage"},"changed":changed})
    );
}
fn appearance(p: &qymcad_core::model::Project, a: Id, b: Id) {
    assert_eq!(p.lineage_root(b), a);
    assert!(!p.bodies[p.mesh_index(b).unwrap()].visible);
    assert!(p.bodies[p.mesh_index(a).unwrap()].visible);
    assert_eq!(p.part_colors.get(&a), Some(&[17, 34, 51]));
    assert!(!p.part_colors.contains_key(&b));
    assert_eq!(p.mesh_color(p.mesh_index(b).unwrap()), [17, 34, 51]);
}

#[test]
fn metadata_exact_result_recipe_breps_topology_and_clean_nodes() {
    let (mut s, _, a, b) = stock();
    let before_topo = s.topology(Some(b), true).unwrap();
    let before_breps = breps(&s);
    let expected_breps = restored_breps(before_breps.clone());
    let mut expected = s.project().clone();
    let i = expected.mesh_index(b).unwrap();
    expected.bodies[i].visible = false;
    expected.part_colors.insert(a, [17, 34, 51]);
    result(s.appearance(b, Some(false), Some(Some([17, 34, 51]))).unwrap(), a, b, false, [17, 34, 51], "manual", true);
    appearance(s.project(), a, b);
    assert!(ron::to_string(s.project()).unwrap() == ron::to_string(&expected).unwrap(), "only Body.visible and part_colors change");
    assert!(breps(&s) == expected_breps, "appearance changes no B-rep beyond the existing native snapshot copy");
    assert_eq!(s.topology(Some(b), true).unwrap(), before_topo);
    assert!(s.project().timeline.iter().all(|n| !n.dirty));
    volume(&s.current_rebuild(), 10.0 * 20.0 * 4.0); // 800 mm³ unchanged.
    let body = &s.info().bodies[0];
    assert!(!body.visible);
    assert_eq!(body.color_rgb, [17, 34, 51]);
    assert_eq!(body.color_source, "manual");
    assert_eq!((&body.name, &body.body_name), (&"moved".to_owned(), &"Housing".to_owned()));
    assert_eq!(s.undo().unwrap().call.unwrap().tool, "appearance");
    assert!(breps(&s) == before_breps, "undo restores raw original handles exactly");
}

#[test]
fn persistence_native_app_and_engine_height_edits() {
    let (mut s, _, a, b) = stock();
    result(s.appearance(b, Some(false), Some(Some([17, 34, 51]))).unwrap(), a, b, false, [17, 34, 51], "manual", true);
    let path = scratch("e3-appearance.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    volume(&r, 10.0 * 20.0 * 4.0); // 800.
    appearance(opened.project(), a, b);
    opened.save(Some(&path)).unwrap();
    // Required public app-path helper; independent inspection helper returns that same native edited Project.
    assert_close(gui_edit_param(&path, "h", "7"), 10.0 * 20.0 * 7.0, 1e-6, "app h4→7 = 1400");
    let (v, _, _, p) = gui_edit_param_model(&path, "h", "7");
    assert_close(v, 10.0 * 20.0 * 7.0, 1e-6, "native Project volume = 1400");
    appearance(&p, a, b);
    let r = opened.param_set("h", &7.0.into()).unwrap();
    volume(&r, 10.0 * 20.0 * 7.0); // Same 1400 by engine path.
    appearance(opened.project(), a, b);
    assert_eq!(opened.info().bodies[0].color_source, "manual");
}

#[test]
fn downstream_body_visible_with_lineage_colour() {
    let (mut s, _, a, b) = stock();
    result(s.appearance(b, Some(false), Some(Some([17, 34, 51]))).unwrap(), a, b, false, [17, 34, 51], "manual", true);
    // A direct native add_move creates a new feature: engine move intentionally reuses the current Move.
    s.save(Some(&scratch("e3-downstream.qcad"))).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(s.path().unwrap().to_str().unwrap()).unwrap();
    let shapes = common::restore_shapes(breps);
    let c = project.add_move(b, qymcad_core::feature::PLACE_IDENTITY);
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, shapes);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_ne!(c, b);
    assert!(project.bodies[project.mesh_index(c).unwrap()].visible);
    assert!(!project.bodies[project.mesh_index(b).unwrap()].visible);
    assert_eq!(project.lineage_root(c), a);
    assert_eq!(project.mesh_color(project.mesh_index(c).unwrap()), [17, 34, 51]);
    assert_close(shapes[&c].volume(), 10.0 * 20.0 * 4.0, 1e-6, "native downstream rigid move = 800");
}

#[test]
fn reset_palette_and_palette_equal_manual_are_distinct() {
    let (mut s, _, a, b) = stock();
    result(s.appearance(b, None, Some(Some([17, 34, 51]))).unwrap(), a, b, true, [17, 34, 51], "manual", true);
    // Retained native probe tasks/research-medE/appearance/rustc-run.log: root index 0 palette [150,170,190].
    let reset = s.appearance(b, None, Some(None)).unwrap();
    assert_eq!(s.project().mesh_color(s.project().mesh_index(b).unwrap()), [150, 170, 190]);
    assert!(!s.project().part_colors.contains_key(&a));
    result(reset, a, b, true, [150, 170, 190], "palette", true);
    result(s.appearance(b, None, Some(None)).unwrap(), a, b, true, [150, 170, 190], "palette", false);
    result(s.appearance(b, None, Some(Some([150, 170, 190]))).unwrap(), a, b, true, [150, 170, 190], "manual", true);
    assert_eq!(s.project().part_colors.get(&a), Some(&[150, 170, 190]));
    assert_eq!(s.info().bodies[0].color_source, "manual");
}

#[test]
fn refusals_snapshot_everything_and_preserve_history() {
    let (mut s, sk, a, b) = stock();
    // Establish exactly one engine metadata undo entry; creation APIs don't add tool history.
    let before_change = snapshot(&mut s, b);
    result(s.appearance(b, Some(false), Some(Some([17, 34, 51]))).unwrap(), a, b, false, [17, 34, 51], "manual", true);
    for (id, visible, color) in [(b, None, None), (a, Some(true), None), (sk, Some(true), None), (u64::MAX, Some(true), None)] {
        let before = snapshot(&mut s, b);
        assert!(s.appearance(id, visible, color).is_err());
        assert_eq!(snapshot(&mut s, b), before);
    }
    let undone = s.undo().unwrap();
    assert_eq!(undone.call.unwrap().tool, "appearance");
    assert_eq!(snapshot(&mut s, b), before_change, "refusals did not add undo entries");
    assert_eq!(s.undo().unwrap().call.unwrap().tool, "rename");
    assert!(s.undo().is_err(), "only setup rename and appearance had history entries");
}

#[test]
fn noop_has_no_history_and_undo_restores_exact_maps_and_flag() {
    let (mut s, _, a, b) = stock();
    let before = snapshot(&mut s, b);
    result(s.appearance(b, Some(false), Some(Some([17, 34, 51]))).unwrap(), a, b, false, [17, 34, 51], "manual", true);
    let changed = snapshot(&mut s, b);
    result(s.appearance(b, Some(false), Some(Some([17, 34, 51]))).unwrap(), a, b, false, [17, 34, 51], "manual", false);
    assert_eq!(snapshot(&mut s, b), changed);
    assert_eq!(s.undo().unwrap().call.unwrap().tool, "appearance");
    assert_eq!(snapshot(&mut s, b), before);
    assert_eq!(s.undo().unwrap().call.unwrap().tool, "rename");
    assert!(s.undo().is_err());
}

#[test]
fn imported_triangle_and_face_colours_survive_set_and_reset() {
    let path = scratch("e3-coloured.ply");
    // Outward tetrahedron: volume = |det((2,0,0),(0,3,0),(0,0,4))| / 6 = 4 mm³.
    std::fs::write(&path,"ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 4\nproperty list uchar int vertex_indices\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nend_header\n0 0 0\n2 0 0\n0 3 0\n0 0 4\n3 0 2 1 10 20 30\n3 0 1 3 10 20 30\n3 0 3 2 40 50 60\n3 1 2 3 40 50 60\n").unwrap();
    let mut s = Session::new_part();
    let imported = s.import_mesh(&path, None, None, 1.0, Some("coloured")).unwrap();
    assert!(imported.rebuild.errors.is_empty());
    assert_eq!(imported.pieces.len(), 1);
    let b = imported.pieces[0].body;
    assert_close(s.info().bodies[0].volume, 2.0 * 3.0 * 4.0 / 6.0, 1e-6, "tetrahedron = 4");
    assert_eq!(s.project().tri_colors[&b], (vec![[10, 20, 30], [40, 50, 60]], vec![0, 0, 1, 1]));
    preserve_import_maps(&mut s, b);

    // Real STEP colour import: emit one author-coloured native face using the public tree writer.
    let (source, _, _, body) = stock();
    let mut tree = source.project().export_tree(source.project().root, |id| id == body);
    let node = tree.iter_mut().find(|n| n.body == Some(body)).unwrap();
    node.face_colors = vec![(source.project().bodies[source.project().mesh_index(body).unwrap()].faces[0].id, [71, 82, 93])];
    let step = scratch("e3-face-coloured.step");
    {
        let _g = qymcad_kernel::kernel_gate();
        qymcad_kernel::write_step_tree(&tree, &[(body, source.shape(body).unwrap())], step.to_str().unwrap()).unwrap();
    }
    let mut imported = Session::new_part();
    let r = imported.import(&step, 1.0, None, None).unwrap();
    assert!(r.rebuild.errors.is_empty());
    assert_eq!(r.bodies.len(), 1);
    let b = r.bodies[0].body;
    assert_close(r.bodies[0].volume, 10.0 * 20.0 * 4.0, 1e-6, "imported rigid prism = 800");
    let faces = &imported.project().face_colors[&imported.project().lineage_root(b)];
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].1, [71, 82, 93]);
    preserve_import_maps(&mut imported, b);
}
fn preserve_import_maps(s: &mut Session, b: Id) {
    let before = (ron::to_string(&s.project().face_colors).unwrap(), ron::to_string(&s.project().tri_colors).unwrap());
    let root = s.project().lineage_root(b);
    let set = s.appearance(b, None, Some(Some([17, 34, 51]))).unwrap();
    assert_eq!((set.color_rgb, set.color_source, set.changed), ([17, 34, 51], "manual", true));
    assert_eq!(s.project().part_colors.get(&root), Some(&[17, 34, 51]));
    assert_eq!((ron::to_string(&s.project().face_colors).unwrap(), ron::to_string(&s.project().tri_colors).unwrap()), before);
    let reset = s.appearance(b, None, Some(None)).unwrap();
    assert_eq!((reset.color_rgb, reset.color_source, reset.changed), ([150, 170, 190], "palette", true)); // First lineage palette, retained native probe.
    assert!(!s.project().part_colors.contains_key(&root));
    assert_eq!((ron::to_string(&s.project().face_colors).unwrap(), ron::to_string(&s.project().tri_colors).unwrap()), before);
}

#[test]
fn hidden_and_coloured_bodies_keep_default_selection_and_export_colour() {
    let (mut s, _, a, b) = stock();
    let before = s.render(View::Iso, 64, 64, None).unwrap();
    let path = scratch("e3-coloured.3mf");
    let old_report = s.export(ExportFormat::ThreeMf, &path, Quality::Standard, None).unwrap();
    let old_meshes = qymcad_io::import_3mf(path.to_str().unwrap()).unwrap();
    result(s.appearance(b, Some(false), Some(Some([17, 34, 51]))).unwrap(), a, b, false, [17, 34, 51], "manual", true);
    let after = s.render(View::Iso, 64, 64, None).unwrap();
    assert_eq!(after.bodies, vec![b]);
    assert_eq!(after.png, before.png);
    let report = s.export(ExportFormat::ThreeMf, &path, Quality::Standard, None).unwrap();
    assert_eq!(report.bodies[0].id, b);
    assert_eq!(report.triangles, old_report.triangles);
    let meshes = qymcad_io::import_3mf(path.to_str().unwrap()).unwrap();
    assert_eq!(meshes.len(), 1);
    assert_eq!(meshes[0].mesh, old_meshes[0].mesh);
    assert_close(meshes[0].mesh.volume(), 10.0 * 20.0 * 4.0, 1e-6, "rigid prism volume = 800");
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("3D/3dmodel.model").unwrap(), &mut xml).unwrap();
    assert!(xml.contains("displaycolor=\"#112233\""), "E4 body colour reaches 3MF");
}

#[test]
fn history_copy_normalization_control_retains_exact_originals() {
    let (mut s, _, _, b) = stock();
    s.topology(Some(b), true).unwrap();
    let before = breps(&s);
    let expected = restored_breps(before.clone());
    let snapshot = s.begin_tool_edit().unwrap();
    assert!(breps(&s) == expected, "boundary equals an independent native restore without any appearance edit");
    s.finish_tool_edit(snapshot, false);
    assert!(breps(&s) == before, "rollback restores exact raw originals");
}
