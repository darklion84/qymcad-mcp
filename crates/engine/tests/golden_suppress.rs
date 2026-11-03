//! E5/E6 native active result bodies and atomic suppression.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

fn block(w: f64, l: f64, h: f64) -> (Session, Id, Id) {
    let mut s = Session::new_part();
    assert!(s.param_set("h", &h.into()).is_ok());
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
    assert!(s.sketch_rect(sk, &0.0.into(), &0.0.into(), &w.into(), &l.into(), false).is_ok());
    let (base, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":"h","name":"stock"})).unwrap()).unwrap();
    volume(&r, base, w * l * h); // Centred rectangle area times extrusion height.
    (s, sk, base)
}
fn volume(r: &Rebuild, body: Id, expected: f64) {
    assert!(r.errors.is_empty(), "{r:?}");
    assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![body]);
    assert_close(r.bodies[0].volume, expected, 1e-5, "analytic solid volume");
}
fn bounds(s: &Session, body: Id, expected: [f64; 6]) {
    let bodies = s.result_bodies();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].id, body);
    for (a, e) in bodies[0].bbox.into_iter().zip(expected) {
        assert_close(a, e, 2e-6, "centred sketch and native translation bounds");
    }
}
fn shift(s: &mut Session, source: Id, xyz: [f64; 3]) -> Id {
    let (m, r) = s
        .move_body(&Move { body: Some(source), translation: Some(xyz.map(Num::Value)), rotation: None, name: Some("shift".into()) })
        .unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    assert_eq!(m.source_body, source);
    assert_ne!(m.body, source);
    m.body
}

#[test]
fn suppressed_final_modifier_is_the_pass_through_result() {
    let (mut s, _, base) = block(10., 20., 20.);
    let moved = shift(&mut s, base, [5., 0., 0.]);
    bounds(&s, moved, [0., -10., 0., 10., 10., 20.]); // x = ±10/2 + 5.
    let path = scratch("e56-native-suppressed-final.qcad");
    assert!(s.save(Some(&path)).is_ok());
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    let shapes = restore_shapes(loaded.breps);
    let ti = p.timeline.iter().position(|n| n.id == moved).unwrap();
    assert!(p.set_feature_suppressed(ti, true));
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, shapes);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    for b in &mut p.bodies {
        if let Some(f) = p.regen_faces.get(&b.id) {
            b.faces = f.clone();
        }
    }
    let breps = {
        let _g = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(&id, sh)| (id, sh.to_brep_bytes().unwrap())).collect::<Vec<_>>()
    };
    assert!(qymcad_io::save_project_with_brep(&p, path.to_str().unwrap(), &breps).is_ok());
    let (opened, r) = Session::open(&path).unwrap();
    volume(&r, moved, 10. * 20. * 20.); // 4000, suppressed move is native identity.
    bounds(&opened, moved, [-5., -10., 0., 5., 10., 20.]);
    assert_eq!(opened.info().bodies[0].id, moved);
}

fn cut(s: &mut Session, source: Id, center: [f64; 2], size: [f64; 2], name: &str) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some(name)).unwrap();
    assert!(s.sketch_rect(sk, &center[0].into(), &center[1].into(), &size[0].into(), &size[1].into(), false).is_ok());
    let (id, r) = s
        .extrude(
            &serde_json::from_value(json!({"sketch":sk,"height":"h","op":"cut","target":source,"name":format!("{name}_cut")})).unwrap(),
        )
        .unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    assert_ne!(id, source);
    id
}
fn flag(s: &Session, id: Id) -> bool {
    s.info().timeline.iter().find(|n| n.id == id).unwrap().suppressed
}

#[test]
fn rectangular_cut_suppression_unsuppression_and_dirty_suffix() {
    let (mut s, _, base) = block(20., 20., 10.);
    let first = cut(&mut s, base, [-4., 0.], [5., 5.], "first");
    volume(&s.current_rebuild(), first, 20. * 20. * 10. - 5. * 5. * 10.); // 3750.
    let r = s.feature_suppress(first, true).unwrap();
    assert_eq!(r.feature.id, first);
    assert_eq!(r.feature.name, "first_cut");
    assert!(r.suppressed && r.changed);
    assert!(r.unbuilt.is_empty());
    volume(&r.rebuild, first, 20. * 20. * 10.); // 4000, native identity output retains first's id.
    let r = s.feature_suppress(first, false).unwrap();
    assert_eq!(r.feature.id, first);
    assert!(!r.suppressed && r.changed);
    volume(&r.rebuild, first, 4000. - 5. * 5. * 10.); // 3750.
    let second = cut(&mut s, first, [5., 0.], [2., 2.], "second");
    volume(&s.current_rebuild(), second, 4000. - 250. - 2. * 2. * 10.); // 3710.
    let r = s.feature_suppress(first, true).unwrap();
    assert_eq!(r.feature.id, first);
    assert!(r.suppressed && r.changed);
    assert!(r.unbuilt.is_empty());
    volume(&r.rebuild, second, 4000. - 2. * 2. * 10.); // 3960; second must rebuild too.
    assert!(flag(&s, first));
    assert!(!flag(&s, second));
    volume(&s.feature_suppress(first, false).unwrap().rebuild, second, 4000. - 250. - 40.);
    // 3710.
}

#[test]
fn base_suppression_cascade_is_unbuilt_without_an_error() {
    let (mut s, _, base) = block(20., 20., 10.);
    let first = cut(&mut s, base, [-4., 0.], [5., 5.], "first");
    let second = cut(&mut s, first, [5., 0.], [2., 2.], "second");
    volume(&s.current_rebuild(), second, 20. * 20. * 10. - 5. * 5. * 10. - 2. * 2. * 10.); // 3710.
    let r = s.feature_suppress(base, true).unwrap();
    assert_eq!(r.feature.id, base);
    assert!(r.suppressed && r.changed);
    let mut unbuilt = r.unbuilt.clone();
    unbuilt.sort_unstable();
    let mut expected = vec![base, first, second];
    expected.sort_unstable();
    assert_eq!(unbuilt, expected);
    assert!(r.rebuild.errors.is_empty());
    assert!(r.rebuild.bodies.is_empty());
    assert!(s.info().bodies.is_empty());
    assert!(s.info().errors.is_empty());
    let before = s.info();
    assert!(s.topology(None, false).is_err(), "no current body");
    assert_eq!(s.info(), before);
    assert!(s.measure(MeasureOperand::Face { body: second, selection: Sel::Largest }, None).is_err());
    assert_eq!(s.info(), before);
    let path = scratch("e56-base-cascade.qcad");
    assert!(s.save_with_options(Some(&path), true).is_ok());
    let (mut opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    assert!(report.bodies.is_empty());
    assert!(flag(&opened, base));
    volume(&opened.feature_suppress(base, false).unwrap().rebuild, second, 4000. - 250. - 40.);
    // 3710.
}

use std::{collections::BTreeMap, io::Read, path::Path};
fn archive(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).unwrap();
            if name == "document.ron" {
                // ADR 0020 permits the monotonic allocator to advance; no other entry is normalized.
                bytes = String::from_utf8(bytes)
                    .unwrap()
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("next_id:"))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into_bytes();
            }
            (name, bytes)
        })
        .collect()
}
type ExactState = (String, Vec<(Id, Vec<u8>)>, DocInfo, Option<Topology>, BTreeMap<String, Vec<u8>>);
fn snapshot(s: &mut Session, body: Id, path: &Path) -> ExactState {
    let topology = if s.project().mesh_born(body) && s.shape(body).is_none() { None } else { Some(s.topology(Some(body), true).unwrap()) };
    assert!(s.save(Some(path)).is_ok());
    let breps = {
        let _g = qymcad_kernel::kernel_gate();
        s.project().bodies.iter().filter_map(|b| s.shape(b.id).map(|sh| (b.id, sh.to_brep_bytes().unwrap()))).collect()
    };
    (ron::to_string(s.project()).unwrap(), breps, s.info(), topology, archive(path))
}

#[test]
fn lost_cylindrical_cut_rim_refuses_and_restores_every_archive_entry() {
    let (mut s, _, base) = block(10., 20., 20.);
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("bore_profile")).unwrap();
    assert!(s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).is_ok());
    let (cut, r) =
        s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":"h","op":"cut","target":base,"name":"bore"})).unwrap()).unwrap();
    let bore = 10. * 20. * 20. - std::f64::consts::PI * 2_f64.powi(2) * 20.; // 4000 - 80π.
    volume(&r, cut, bore);
    let rim = s.project().regen_edges[&cut]
        .iter()
        .find(|e| (e.radius - 2.).abs() < 1e-7 && (e.center[2] - 20.).abs() < 1e-7)
        .expect("radius 2 circular top rim")
        .id;
    let (fillet, r) = s.fillet(Some(cut), &Sel::Ids(vec![rim]), &0.5.into(), Some("rim_round")).unwrap();
    // Integrate π[(R+f-sqrt(f²-u²))²-R²], u=0..f, R=2, f=.5.
    let (big_r, f) = (2., 0.5_f64);
    let loss = std::f64::consts::PI * (2. * big_r * f * f + 5. / 3. * f * f * f - std::f64::consts::PI / 2. * (big_r + f) * f * f);
    volume(&r, fillet, bore - loss);
    assert_ne!(fillet, cut);
    assert!(s.undo().is_err(), "direct fixture setup has no history");
    let path = scratch("e56-lost-rim-exact.qcad");
    let before = snapshot(&mut s, fillet, &path);
    let error = s.feature_suppress(cut, true).unwrap_err().to_string();
    assert!(error.contains("edge") || error.contains("Edge"), "native lost edge reason: {error}");
    assert_eq!(snapshot(&mut s, fillet, &path), before);
    assert!(!flag(&s, cut));
    assert!(s.undo().is_err(), "refusal adds no history");
}

fn cube_stl() -> Vec<u8> {
    let vertices =
        [[0., 0., 0.], [10., 0., 0.], [10., 10., 0.], [0., 10., 0.], [0., 0., 10.], [10., 0., 10.], [10., 10., 10.], [0., 10., 10.]];
    let tris = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    let mut bytes = vec![0; 84 + tris.len() * 50];
    bytes[80..84].copy_from_slice(&(tris.len() as u32).to_le_bytes());
    for (k, t) in tris.into_iter().enumerate() {
        for (j, v) in t.into_iter().map(|i| vertices[i]).enumerate() {
            for (axis, value) in v.into_iter().enumerate() {
                let offset = 84 + k * 50 + 12 + j * 12 + axis * 4;
                bytes[offset..offset + 4].copy_from_slice(&(value as f32).to_le_bytes());
            }
        }
    }
    bytes
}

#[test]
fn mesh_born_move_refuses_before_mutation() {
    let path = scratch("e56-authored-cube.stl");
    assert!(std::fs::write(&path, cube_stl()).is_ok());
    let mut s = Session::new_part();
    let imported = s.import_mesh(&path, None, Some("mm"), 1., Some("mesh_cube")).unwrap();
    assert_eq!(imported.pieces.len(), 1);
    let source = imported.pieces[0].body;
    let moved = shift(&mut s, source, [5., 0., 0.]);
    volume(&s.current_rebuild(), moved, 10. * 10. * 10.); // Closed authored cube, 1000.
    bounds(&s, moved, [5., 0., 0., 15., 10., 10.]);
    let path = scratch("e56-mesh-move-refusal.qcad");
    let before = snapshot(&mut s, moved, &path);
    let error = s.feature_suppress(moved, true).unwrap_err().to_string();
    assert!(error.contains("mesh"), "F-128 refusal: {error}");
    assert_eq!(snapshot(&mut s, moved, &path), before);
    assert!(s.undo().is_err());
}

#[test]
fn suppressed_modifier_persists_through_open_app_edit_and_engine_edit() {
    let (mut s, _, base) = block(10., 20., 20.);
    let moved = shift(&mut s, base, [5., 0., 0.]);
    let r = s.feature_suppress(moved, true).unwrap();
    assert_eq!(r.feature.id, moved);
    assert!(r.changed && r.suppressed);
    volume(&r.rebuild, moved, 10. * 20. * 20.); // 4000.
    let path = scratch("e56-suppressed-move-persistence.qcad");
    assert!(s.save(Some(&path)).is_ok());
    let (mut opened, r) = Session::open(&path).unwrap();
    volume(&r, moved, 10. * 20. * 20.);
    assert!(flag(&opened, moved));
    assert_close(gui_edit_param(&path, "h", "30"), 10. * 20. * 30., 1e-5, "native app h20→30: area times h = 6000");
    let (v, b, _, app) = gui_edit_param_model(&path, "h", "30");
    assert_close(v, 10. * 20. * 30., 1e-5, "native app volume 6000");
    assert!(app.timeline.iter().find(|n| n.id == moved).unwrap().suppressed);
    for (a, e) in b.into_iter().zip([-5., -10., 0., 5., 10., 30.]) {
        assert_close(a, e, 2e-6, "native app suppressed identity bounds");
    }
    volume(&opened.param_set("h", &30.0.into()).unwrap(), moved, 10. * 20. * 30.); // 6000.
    bounds(&opened, moved, [-5., -10., 0., 5., 10., 30.]);
    assert!(flag(&opened, moved));
    let r = opened.feature_suppress(moved, false).unwrap();
    assert_eq!(r.feature.id, moved);
    assert!(r.changed && !r.suppressed);
    volume(&r.rebuild, moved, 10. * 20. * 30.);
    bounds(&opened, moved, [0., -10., 0., 10., 10., 30.]); // Original +5 X returns.
}

#[test]
fn non_body_targets_and_ambiguous_names_refuse_with_exact_state() {
    let (mut s, sk, base) = block(10., 20., 20.);
    let (plane, r) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &2.0.into(), Some("datum")).unwrap();
    assert!(r.errors.is_empty());
    let path = scratch("e56-refusals.qcad");
    for id in [sk, plane, u64::MAX] {
        let before = snapshot(&mut s, base, &path);
        assert!(s.feature_suppress(id, true).is_err());
        assert_eq!(snapshot(&mut s, base, &path), before, "target {id}");
    }
    assert!(s.rename(RenameTarget::Feature(sk), "same").is_ok());
    assert!(s.rename(RenameTarget::Feature(base), "same").is_ok());
    let before = snapshot(&mut s, base, &path);
    assert!(s.resolve("same").is_err());
    assert_eq!(snapshot(&mut s, base, &path), before);
}

#[test]
fn no_op_has_no_history_and_undo_restores_flag_and_exact_geometry() {
    let (mut s, _, base) = block(10., 20., 20.);
    let moved = shift(&mut s, base, [5., 0., 0.]);
    let path = scratch("e56-undo.qcad");
    let before = snapshot(&mut s, moved, &path);
    let r = s.feature_suppress(moved, false).unwrap();
    assert_eq!(r.feature.id, moved);
    assert!(!r.changed && !r.suppressed);
    assert!(r.unbuilt.is_empty());
    volume(&r.rebuild, moved, 10. * 20. * 20.);
    assert_eq!(snapshot(&mut s, moved, &path), before);
    assert!(s.undo().is_err());
    let r = s.feature_suppress(moved, true).unwrap();
    assert!(r.changed && r.suppressed);
    volume(&r.rebuild, moved, 10. * 20. * 20.);
    let suppressed = snapshot(&mut s, moved, &path);
    let r = s.feature_suppress(moved, true).unwrap();
    assert!(!r.changed && r.suppressed);
    volume(&r.rebuild, moved, 10. * 20. * 20.);
    assert_eq!(snapshot(&mut s, moved, &path), suppressed);
    let undone = s.undo().unwrap();
    volume(&undone.rebuild, moved, 10. * 20. * 20.);
    assert_eq!(snapshot(&mut s, moved, &path), before);
    assert!(s.undo().is_err(), "exactly one undo entry");
}

fn native_save(project: &mut qymcad_core::model::Project, shapes: &std::collections::HashMap<Id, qymcad_kernel::Shape>, path: &Path) {
    for b in &mut project.bodies {
        if let Some(f) = project.regen_faces.get(&b.id) {
            b.faces = f.clone();
        }
    }
    let breps = {
        let _g = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(&id, sh)| (id, sh.to_brep_bytes().unwrap())).collect::<Vec<_>>()
    };
    assert!(qymcad_io::save_project_with_brep(project, path.to_str().unwrap(), &breps).is_ok());
}

#[test]
fn native_rollback_prefix_hides_tail_from_info_export_measure_and_defaults() {
    let (mut s, _, base) = block(10., 20., 20.);
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("tail_profile")).unwrap();
    assert!(s.sketch_rect(sk, &30.0.into(), &0.0.into(), &10.0.into(), &20.0.into(), false).is_ok());
    let (tail, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":"h","op":"new_body","name":"tail"})).unwrap()).unwrap();
    assert!(r.errors.is_empty());
    assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![base, tail]);
    for body in &r.bodies {
        assert_close(body.volume, 10. * 20. * 20., 1e-5, "independent rectangle area times height, 4000 each");
    }
    let path = scratch("e56-rollback-prefix.qcad");
    assert!(s.save(Some(&path)).is_ok());
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    let shapes = restore_shapes(loaded.breps);
    let ti = p.timeline.iter().position(|n| n.id == tail).unwrap();
    p.set_rollback(Some(ti));
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, shapes);
    assert!(r.errors.is_empty());
    assert!(p.mesh_index(tail).is_none());
    native_save(&mut p, &shapes, &path); // Testkit may retain old shapes; body presence is authoritative.
    let (mut opened, r) = Session::open(&path).unwrap();
    volume(&r, base, 10. * 20. * 20.);
    assert_eq!(opened.info().bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![base]);
    assert_eq!(opened.topology(None, true).unwrap().body, base);
    assert_eq!(opened.select(None, Element::Faces, &Sel::Largest).unwrap().0, base);
    let measured = opened
        .measure(
            MeasureOperand::Face { body: base, selection: Sel::Facing { dir: [0., 0., 1.], tol_deg: 1. } },
            Some(MeasureOperand::Point([0., 0., 25.])),
        )
        .unwrap();
    assert_eq!(measured.distance_mm, Some(25. - 20.)); // Point 5 mm above the top support plane.
    let before = opened.info();
    assert!(opened.measure(MeasureOperand::Face { body: tail, selection: Sel::Largest }, None).is_err());
    assert_eq!(opened.info(), before);
    let exported = opened.export(ExportFormat::Step, &scratch("e56-rollback-active.step"), Quality::Standard, None).unwrap();
    assert_eq!(exported.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![base]);
    let before = opened.info();
    assert!(opened.export(ExportFormat::Step, &scratch("e56-rollback-tail.step"), Quality::Standard, Some(&[tail])).is_err());
    assert_eq!(opened.info(), before);
    let image = opened.render(View::Iso, 64, 64, None).unwrap();
    assert_eq!(image.bodies, vec![base]);
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    p.set_rollback(None);
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, restore_shapes(loaded.breps));
    assert!(r.errors.is_empty());
    native_save(&mut p, &shapes, &path);
    let (restored, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![base, tail]);
    for body in &r.bodies {
        assert_close(body.volume, 10. * 20. * 20., 1e-5, "cleared rollback restores 4000 each");
    }
    assert_eq!(restored.info().bodies.len(), 2);
}

#[test]
fn retained_import_original_is_private_while_suppressed_or_rolled_back() {
    let (mut s, _, base) = block(10., 20., 20.);
    let step = scratch("e56-import-source.step");
    let export = s.export(ExportFormat::Step, &step, Quality::Standard, None).unwrap();
    assert_eq!(export.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![base]);
    let imported = s.import(&step, 1., None, Some("imported")).unwrap();
    assert_eq!(imported.bodies.len(), 1);
    let body = imported.bodies[0].body;
    assert_close(imported.bodies[0].volume, 10. * 20. * 20., 1e-5, "STEP prism volume = 4000");
    let r = s.feature_suppress(body, true).unwrap();
    assert_eq!(r.feature.id, body);
    assert!(r.changed && r.suppressed);
    assert_eq!(r.unbuilt, vec![body]);
    volume(&r.rebuild, base, 10. * 20. * 20.);
    assert!(s.shape(body).is_none());
    let path = scratch("e56-private-import.qcad");
    assert!(s.save(Some(&path)).is_ok());
    // The native loader returns B-reps only for document body records (project_file.rs:233–237).
    // Inspect the archive itself to prove that the private original was retained on save.
    assert!(archive(&path).contains_key(&format!("brep/{body}.brep")), "retain native import original privately");
    let (mut opened, r) = Session::open(&path).unwrap();
    volume(&r, base, 10. * 20. * 20.);
    let before = opened.info();
    assert!(opened.measure(MeasureOperand::Face { body, selection: Sel::Largest }, None).is_err());
    assert_eq!(opened.info(), before);
    assert_eq!(
        opened
            .export(ExportFormat::Step, &scratch("e56-private-import-export.step"), Quality::Standard, None)
            .unwrap()
            .bodies
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        vec![base]
    );
    let r = opened.feature_suppress(body, false).unwrap();
    assert_eq!(r.feature.id, body);
    assert!(r.changed && !r.suppressed);
    assert!(r.unbuilt.is_empty());
    assert!(r.rebuild.errors.is_empty());
    assert_eq!(r.rebuild.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![base, body]);
    for b in &r.rebuild.bodies {
        assert_close(b.volume, 10. * 20. * 20., 1e-5, "each independent original/import prism is 4000");
    }
    assert!(opened.save(Some(&path)).is_ok());
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    let ti = p.timeline.iter().position(|n| n.id == body).unwrap();
    p.set_rollback(Some(ti));
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, restore_shapes(loaded.breps));
    assert!(r.errors.is_empty());
    assert!(p.mesh_index(body).is_none());
    assert!(shapes.contains_key(&body));
    native_save(&mut p, &shapes, &path);
    let (opened, r) = Session::open(&path).unwrap();
    volume(&r, base, 10. * 20. * 20.);
    assert!(opened.shape(body).is_none());
    assert_eq!(opened.render(View::Iso, 64, 64, None).unwrap().bodies, vec![base]);
    let export = opened.export(ExportFormat::Step, &scratch("e56-rollback-import-export.step"), Quality::Standard, None).unwrap();
    assert_eq!(export.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![base]);
}

#[test]
fn unsuppress_preserves_overlapping_fillet_guard_f074() {
    use qymcad_core::feature::FeatureKind;
    let (mut s, _, base) = block(20., 20., 20.);
    let (fillet, r) = s.fillet(Some(base), &Sel::Along { dir: [0., 0., 1.], tol_deg: 1. }, &2.0.into(), Some("round")).unwrap();
    // Four disjoint quarter-cylinder corner losses, (r²-πr²/4)*h each.
    volume(&r, fillet, 20. * 20. * 20. - 4. * (2_f64.powi(2) - std::f64::consts::PI * 2_f64.powi(2) / 4.) * 20.);
    let path = scratch("e56-f074-suppressed.qcad");
    assert!(s.save(Some(&path)).is_ok());
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    let ti = p.timeline.iter().position(|n| n.id == fillet).unwrap();
    let FeatureKind::Fillet { radius, .. } = &mut p.timeline[ti].kind else { panic!("fillet") };
    *radius = 15.;
    p.feat_dims.entry(fillet).or_default().insert("radius".into(), "15".into());
    assert!(p.set_feature_suppressed(ti, true));
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, restore_shapes(loaded.breps));
    assert!(r.errors.is_empty());
    native_save(&mut p, &shapes, &path);
    let (mut opened, r) = Session::open(&path).unwrap();
    volume(&r, fillet, 20. * 20. * 20.); // Identity bypass, 8000.
    assert!(flag(&opened, fillet));
    let before = snapshot(&mut opened, fillet, &path);
    let error = opened.feature_suppress(fillet, false).unwrap_err().to_string();
    assert!(error.contains("fillet R15.00 does not fit"), "F-074: {error}");
    assert_eq!(snapshot(&mut opened, fillet, &path), before);
    assert!(opened.undo().is_err());
}

#[test]
fn unsuppress_preserves_unnamed_stored_edge_query_guard_f024() {
    use qymcad_core::{
        feature::FeatureKind,
        refs::{Query, Ref},
    };
    let (mut s, _, base) = block(20., 20., 20.);
    let (fillet, r) = s.fillet(Some(base), &Sel::Along { dir: [0., 0., 1.], tol_deg: 1. }, &2.0.into(), Some("round")).unwrap();
    // Same four analytic corner losses as above, r=2 and h=20.
    volume(&r, fillet, 8000. - 4. * (4. - std::f64::consts::PI) * 20.);
    let path = scratch("e56-f024-suppressed.qcad");
    assert!(s.save(Some(&path)).is_ok());
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    let ti = p.timeline.iter().position(|n| n.id == fillet).unwrap();
    let FeatureKind::Fillet { edges, .. } = &mut p.timeline[ti].kind else { panic!("fillet") };
    *edges = Ref::many(Query::Oriented { dir: [0., 0., 1.], tol_deg: 1. });
    assert!(p.set_feature_suppressed(ti, true));
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, restore_shapes(loaded.breps));
    assert!(r.errors.is_empty());
    native_save(&mut p, &shapes, &path);
    let mut loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    native::rename_names(&mut loaded.breps.iter_mut().find(|(id, _)| *id == base).unwrap().1, &[], true);
    assert!(qymcad_io::save_project_guarded_with_brep(&loaded.project, path.to_str().unwrap(), &loaded.breps).is_ok());
    let (mut opened, r) = Session::open(&path).unwrap();
    volume(&r, fillet, 20. * 20. * 20.); // Suppressed identity, 8000.
                                         // Topology deliberately cannot be restored in this legacy unnamed-edge fixture. Compare all
                                         // archive entries (including every B-rep), the full recipe and diagnostics without repairing it.
    let state = |s: &mut Session| {
        assert!(s.save(Some(&path)).is_ok());
        (ron::to_string(s.project()).unwrap(), s.info(), archive(&path))
    };
    let before = state(&mut opened);
    let error = opened.feature_suppress(fillet, false).unwrap_err().to_string();
    assert!(error.contains("stored edge query") && error.contains("F-024"), "{error}");
    assert_eq!(state(&mut opened), before);
    assert!(opened.undo().is_err());
}
