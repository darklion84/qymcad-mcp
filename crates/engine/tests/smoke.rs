//! The pinned QymCAD kernel links and builds a solid. If this fails, nothing else can pass:
//! check OCCT (`brew install opencascade`, .cargo/config.toml) before anything else.

#[test]
fn kernel_builds_a_box() {
    let mut p = qymcad_core::model::Project::default();
    p.new_document();
    let b = p.add_box(10.0, 20.0, 30.0);
    let (report, shapes) = qymcad_testkit::regenerate(&mut p);
    assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
    let v = shapes.get(&b).expect("box shape").volume();
    assert!((v - 6000.0).abs() < 1e-6, "volume {v}");
}
