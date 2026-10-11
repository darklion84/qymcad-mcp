//! App-saved imported bodies survive engine open, edits, save, and history.
mod common;
use common::assert_close;
use qymcad_core::feature::PLACE_IDENTITY;
use qymcad_core::model::{ImportNode, Project};
use qymcad_engine::*;
use qymcad_kernel::{ExactFormat, LengthUnit, Shape};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Fixture {
    // The upstream app-path helper uses source-id temp names and no kernel gate.
    _lock: std::sync::MutexGuard<'static, ()>,
    dir: PathBuf,
    path: PathBuf,
    body: Id,
    source: Id,
    parametric: Option<Id>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(format: ExactFormat, stored: bool, parametric: bool) -> Fixture {
    scaled_fixture(format, stored, parametric, 1.0)
}

fn scaled_fixture(format: ExactFormat, stored: bool, parametric: bool, scale: f64) -> Fixture {
    let lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("qymcad-imports-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("import.qcad");
    let mut s = Session::new_part();
    let own = if parametric {
        s.param_set("h", &5.0.into()).unwrap();
        let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("beside import")).unwrap();
        s.sketch_rect(sketch, &30.0.into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
        Some(
            s.extrude(&Extrude {
                sketch,
                profiles: None,
                height: Num::Expr("h".into()),
                op: Op::NewBody,
                direction: Direction::Normal,
                through: false,
                target: None,
                name: Some("parametric".into()),
            })
            .unwrap()
            .0,
        )
    } else {
        None
    };
    let mut p = s.project().clone();
    let ext = match format {
        ExactFormat::Step => "step",
        ExactFormat::Iges => "igs",
    };
    let input = dir.join(format!("cube.{ext}"));
    let _gate = qymcad_kernel::kernel_gate();
    let cube = Shape::extrude(&[0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0], 10.0).unwrap();
    match format {
        ExactFormat::Step => qymcad_kernel::write_step(&[(&cube, PLACE_IDENTITY)], input.to_str().unwrap()).unwrap(),
        ExactFormat::Iges => {
            qymcad_kernel::write_iges(&[(&cube, PLACE_IDENTITY)], input.to_str().unwrap(), LengthUnit::Millimetre).unwrap()
        }
    }
    let source = p.add_source(format!("cube.{ext}"), std::fs::read(&input).unwrap());
    let (mesh, faces) = qymcad_kernel::import_exact(format, input.to_str().unwrap(), 0.5).unwrap().remove(0);
    let body = p.add_mesh(mesh);
    p.import_tree_as_parts(vec![ImportNode { name: "imported cube".into(), body: Some(body), ..Default::default() }], source, "cube")
        .unwrap();
    let live = qymcad_kernel::exact_solids(format, input.to_str().unwrap()).unwrap().remove(0);
    if let qymcad_core::feature::FeatureKind::Import { scale: value, .. } = &mut p.timeline.iter_mut().find(|n| n.id == body).unwrap().kind
    {
        *value = scale;
    }
    let live = live.transformed(&[scale, 0.0, 0.0, 0.0, 0.0, scale, 0.0, 0.0, 0.0, 0.0, scale, 0.0]).unwrap();
    let mut shapes: HashMap<Id, Shape> =
        own.into_iter().map(|id| (id, Shape::from_brep_bytes(&s.shape(id).unwrap().to_brep_bytes().unwrap()).unwrap())).collect();
    shapes.insert(body, live);
    drop(_gate);
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, shapes);
    assert!(report.errors.is_empty(), "fixture regeneration: {:?}", report.errors);
    p.set_body_faces(body, faces);
    for (id, faces) in report.built {
        p.set_body_faces(id, faces);
    }
    let _gate = qymcad_kernel::kernel_gate();
    let breps: Vec<_> = shapes.iter().filter(|(id, _)| stored || **id != body).map(|(&id, sh)| (id, sh.to_brep_bytes().unwrap())).collect();
    qymcad_io::save_project_guarded_with_brep(&p, path.to_str().unwrap(), &breps).unwrap();
    // All later restoration must use the embedded source, never the original pathname.
    std::fs::remove_file(input).unwrap();
    Fixture { _lock: lock, dir, path, body, source, parametric: own }
}

fn check_import(s: &Session, f: &Fixture) {
    let shape = s.shape(f.body).unwrap_or_else(|| panic!("import body {} has no live B-rep", f.body));
    assert_close(shape.volume(), 1000.0, 1.0, "import cube volume");
    let info = s.info();
    assert!(info.errors.is_empty(), "doc_info errors: {:?}", info.errors);
    let body = info.bodies.iter().find(|b| b.id == f.body).expect("doc_info contains import");
    assert_close(body.volume, 1000.0, 1.0, "doc_info import volume");
    for (actual, expected) in body.bbox.into_iter().zip([0.0, 0.0, 0.0, 10.0, 10.0, 10.0]) {
        assert_close(actual, expected, 1e-4, "doc_info import bbox");
    }
    assert!(s.project().sources.iter().any(|sf| sf.id == f.source && !sf.data.is_empty()));
}

#[test]
fn s1_stored_step_and_iges_imports_open_and_export_exact_geometry() {
    for format in [ExactFormat::Step, ExactFormat::Iges] {
        let f = fixture(format, true, false);
        let (s, report) = Session::open(&f.path).unwrap();
        assert!(report.errors.is_empty(), "open: {:?}", report.errors);
        check_import(&s, &f);
        assert_eq!(s.info().bodies.len(), 1);
        let output = f.dir.join("export.step");
        s.export(ExportFormat::Step, &output, Quality::Standard, None).unwrap();
        let _gate = qymcad_kernel::kernel_gate();
        let shapes = qymcad_kernel::exact_solids(ExactFormat::Step, output.to_str().unwrap()).unwrap();
        assert_eq!(shapes.len(), 1);
        assert_close(shapes[0].volume(), 1000.0, 1.0, "exported STEP volume");
    }
}

fn missing_brep(format: ExactFormat) {
    let f = fixture(format, false, false);
    let (s, report) = Session::open(&f.path).unwrap();
    assert!(report.errors.is_empty(), "open: {:?}", report.errors);
    check_import(&s, &f);
}
#[test]
fn s2_missing_step_brep_restores_from_embedded_source() {
    missing_brep(ExactFormat::Step);
}
#[test]
fn s2_missing_iges_brep_restores_from_embedded_source() {
    missing_brep(ExactFormat::Iges);
}

#[test]
fn s3_parameter_rebuild_preserves_import_and_updates_parametric_body() {
    for (format, stored) in [(ExactFormat::Step, true), (ExactFormat::Step, false), (ExactFormat::Iges, false)] {
        let f = fixture(format, stored, true);
        let (mut s, report) = Session::open(&f.path).unwrap();
        assert!(report.errors.is_empty());
        check_import(&s, &f);
        let before = s.shape(f.body).unwrap().to_brep_bytes().unwrap();
        let report = s.param_set("h", &8.0.into()).unwrap();
        assert!(report.errors.is_empty(), "parameter rebuild: {:?}", report.errors);
        check_import(&s, &f);
        assert_eq!(s.shape(f.body).unwrap().to_brep_bytes().unwrap(), before);
        assert_close(s.shape(f.parametric.unwrap()).unwrap().volume(), 800.0, 1e-6, "updated parametric volume");
        assert_eq!(s.info().bodies.len(), 2);
        assert_eq!(report.bodies.iter().find(|b| b.id == f.parametric.unwrap()).unwrap().bbox, [25.0, -5.0, 0.0, 35.0, 5.0, 8.0]);
    }
}

#[test]
fn s4_engine_save_reopens_with_app_and_engine_paths() {
    for (format, stored) in [(ExactFormat::Step, true), (ExactFormat::Iges, true), (ExactFormat::Step, false), (ExactFormat::Iges, false)] {
        let f = fixture(format, stored, false);
        let (mut s, report) = Session::open(&f.path).unwrap();
        assert!(report.errors.is_empty());
        let source = s.project().sources.iter().find(|sf| sf.id == f.source).unwrap().clone();
        s.save(None).unwrap();
        let mut p: Project = qymcad_io::load_project(f.path.to_str().unwrap()).unwrap();
        let sf = p.sources.iter().find(|sf| sf.id == source.id).unwrap();
        assert_eq!(sf.data, source.data);
        assert_eq!(sf.ext, source.ext);
        let report = qymcad_testkit::open_like_the_app(&mut p);
        assert!(report.errors.is_empty(), "app reopen: {:?}", report.errors);
        assert!(p.mesh_index(f.body).is_some());
        // open_like_the_app returns no live shapes: inspect its restored source geometry separately.
        let live = qymcad_testkit::restore_import_shapes(&p);
        assert_close(live[&f.body].volume(), 1000.0, 1.0, "app-restored import volume");
        let (s, report) = Session::open(&f.path).unwrap();
        assert!(report.errors.is_empty());
        check_import(&s, &f);
    }
}

#[test]
fn s5_delete_and_undo_after_import_preserve_geometry_and_source() {
    let f = fixture(ExactFormat::Step, true, false);
    let (mut s, _) = Session::open(&f.path).unwrap();
    let before = s.shape(f.body).unwrap().to_brep_bytes().unwrap();
    let snapshot = s.begin_tool_edit().unwrap();
    let (plane, report) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &20.0.into(), Some("after import")).unwrap();
    assert!(report.errors.is_empty());
    s.finish_tool_edit(snapshot, true);
    check_import(&s, &f);
    let snapshot = s.begin_tool_edit().unwrap();
    assert!(s.feature_delete(plane, false).unwrap().errors.is_empty());
    s.finish_tool_edit(snapshot, true);
    check_import(&s, &f);
    s.undo().unwrap();
    check_import(&s, &f);
    assert!(s.project().timeline.iter().any(|n| n.id == plane));
    s.undo().unwrap();
    check_import(&s, &f);
    assert!(s.project().planes.is_empty());
    assert_eq!(s.shape(f.body).unwrap().to_brep_bytes().unwrap(), before);
}

fn unavailable_source_with_brep(damage: &str, stored: bool) {
    let f = fixture(ExactFormat::Step, stored, false);
    let breps = qymcad_io::load_project_with_brep(f.path.to_str().unwrap()).unwrap().breps;
    let mut p = qymcad_io::load_project(f.path.to_str().unwrap()).unwrap();
    match damage {
        "missing" => p.sources.retain(|sf| sf.id != f.source),
        "empty" => p.sources.iter_mut().find(|sf| sf.id == f.source).unwrap().data.clear(),
        _ => p.sources.iter_mut().find(|sf| sf.id == f.source).unwrap().data = b"not a STEP file".to_vec(),
    }
    if stored {
        p.timeline.iter_mut().find(|n| n.id == f.body).unwrap().dirty = true;
    }
    qymcad_io::save_project_guarded_with_brep(&p, f.path.to_str().unwrap(), &breps).unwrap();
    let (mut s, report) = Session::open(&f.path).unwrap();
    assert_eq!(s.shape(f.body).is_some(), stored);
    if stored {
        assert_close(s.shape(f.body).unwrap().volume(), 1000.0, 1.0, "stored import despite damaged source");
    }
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.node == f.body && !w.name.is_empty() && w.message.contains("import") && w.message.contains(&f.body.to_string())),
        "{damage} source has no warning for import body {}: {:?}",
        f.body,
        report.warnings
    );
    assert!(s.info().warnings.iter().any(|w| w.contains("import")));
    assert!(report.warnings.iter().any(|w| w.message.contains(if stored {
        "stored exact geometry is retained"
    } else {
        "exact geometry is unavailable"
    })));
    // Explicit new target exercises saving the unavailable body's retained document data.
    let saved = f.dir.join("retained.qcad");
    s.save(Some(&saved)).unwrap();
    let p = qymcad_io::load_project(saved.to_str().unwrap()).unwrap();
    assert!(p.timeline.iter().any(|n| n.id == f.body));
    assert!(p.mesh_index(f.body).is_some(), "save dropped imported display body");
    let original = s.project().sources.iter().find(|sf| sf.id == f.source);
    let saved_source = p.sources.iter().find(|sf| sf.id == f.source);
    assert_eq!(original.map(|sf| (&sf.name, &sf.ext, &sf.data)), saved_source.map(|sf| (&sf.name, &sf.ext, &sf.data)));
    let (s, report) = Session::open(&saved).unwrap();
    assert!(s.project().mesh_index(f.body).is_some());
    assert!(report.warnings.iter().any(|w| w.node == f.body));
}

#[test]
fn missing_import_source_warns_and_survives_save() {
    unavailable_source_with_brep("missing", false);
}
#[test]
fn empty_import_source_warns_and_survives_save() {
    unavailable_source_with_brep("empty", false);
}
#[test]
fn unreadable_import_source_warns_and_survives_save() {
    unavailable_source_with_brep("unreadable", false);
}

#[test]
fn damaged_sources_warn_even_when_the_import_has_a_stored_brep() {
    for damage in ["missing", "empty", "unreadable"] {
        unavailable_source_with_brep(damage, true);
    }
}

#[test]
fn s2_scaled_import_restores_the_app_import_scale() {
    let f = scaled_fixture(ExactFormat::Step, false, false, 2.0);
    let (s, report) = Session::open(&f.path).unwrap();
    assert!(report.errors.is_empty());
    assert_close(s.shape(f.body).expect("scaled import live B-rep").volume(), 8000.0, 1.0, "scaled source cube volume");
    let body = s.info().bodies.into_iter().find(|b| b.id == f.body).unwrap();
    for (actual, expected) in body.bbox.into_iter().zip([0.0, 0.0, 0.0, 20.0, 20.0, 20.0]) {
        assert_close(actual, expected, 1e-4, "scaled source cube bbox");
    }
}
