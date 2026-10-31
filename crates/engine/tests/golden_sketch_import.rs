mod common;
use common::{assert_close, gui_edit_param_model};
use qymcad_core::model::entity_points;
use qymcad_engine::*;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;

fn file(name: &str, text: &str) -> std::path::PathBuf {
    let path = common::scratch(name);
    fs::write(&path, text).unwrap();
    path
}

fn dxf() -> std::path::PathBuf {
    let text = "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\n0\n90\n4\n70\n1\n10\n0\n20\n0\n10\n40\n20\n0\n10\n40\n20\n20\n10\n0\n20\n20\n0\nCIRCLE\n8\n0\n10\n20\n20\n10\n40\n5\n0\nTEXT\n8\n0\n10\n2\n20\n2\n40\n1\n1\nignored\n0\nENDSEC\n0\nEOF\n";
    file("import-golden.dxf", text)
}

fn square_dxf(name: &str, size: f64) -> std::path::PathBuf {
    file(name, &format!("0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\n0\n90\n4\n70\n1\n10\n0\n20\n0\n10\n{size}\n20\n0\n10\n{size}\n20\n{size}\n10\n0\n20\n{size}\n0\nENDSEC\n0\nEOF\n"))
}

fn archive(s: &mut Session, name: &str) -> BTreeMap<String, Vec<u8>> {
    let path = common::scratch(name);
    s.save_with_options(Some(&path), true).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        out.insert(entry.name().to_string(), bytes);
    }
    out
}

#[test]
fn dxf_import_preserves_circle_reports_text_and_extrudes() {
    let mut s = Session::new_part();
    let r = s.sketch_import(&dxf(), None, &PlaneRef::Base(BaseName::XY), Some("dxf"), &1.0.into()).unwrap();
    assert_eq!(r.format, SketchImportFormat::Dxf);
    assert_eq!(r.entities["circle"], 1);
    assert!(r.skipped.iter().any(|(kind, count)| kind == "TEXT" && *count == 1));
    assert!(r.contours.iter().any(|c| c.parent.is_none()));
    let body = s
        .extrude(&Extrude {
            sketch: r.sketch,
            profiles: None,
            profiles_at: Some(vec![[1.0.into(), 1.0.into()]]),
            height: 3.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    let expected = (800.0 - std::f64::consts::PI * 25.0) * 3.0;
    assert_close(s.shape(body).unwrap().volume(), expected, expected.abs() * 1e-6, "DXF volume");
}

#[test]
fn scale_is_evaluated_once_and_svg_flips_y_and_uses_px_units() {
    let mut s = Session::new_part();
    s.param_set("k", &2.0.into()).unwrap();
    let dxf = file("scale.dxf", "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\n0\n90\n4\n70\n1\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n10\n10\n0\n20\n10\n0\nENDSEC\n0\nEOF\n");
    let r = s.sketch_import(&dxf, None, &PlaneRef::Base(BaseName::XY), None, &Num::Expr("k".into())).unwrap();
    assert_eq!(r.scale, 2.0);
    let before = s.sketch_detail(r.sketch).unwrap();
    s.param_set("k", &3.0.into()).unwrap();
    let after = s.sketch_detail(r.sketch).unwrap();
    assert_eq!(before.points.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>(), after.points.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>());
    let svg = file(
        "units.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 96 96"><path d="M 0 0 L 96 0 L 96 96 L 0 96 Z"/></svg>"#,
    );
    let r = s.sketch_import(&svg, None, &PlaneRef::Base(BaseName::XY), None, &1.0.into()).unwrap();
    let d = s.sketch_detail(r.sketch).unwrap();
    let xs: Vec<f64> = d.points.iter().map(|p| p.x).collect();
    let ys: Vec<f64> = d.points.iter().map(|p| p.y).collect();
    assert!(xs.iter().any(|x| (*x - 25.4).abs() < 1e-12));
    assert!(ys.iter().any(|y| (*y - 25.4).abs() < 1e-12));
}

#[test]
fn import_refusals_are_atomic() {
    let mut s = Session::new_part();
    let path = square_dxf("refusal-seed.dxf", 10.0);
    s.sketch_import(&path, None, &PlaneRef::Base(BaseName::XY), None, &1.0.into()).unwrap();
    let before = archive(&mut s, "import-refusals-before.qcad");
    for (name, scale) in [("zero", 0.0.into()), ("negative", (-1.0).into()), ("nan", Num::Value(f64::NAN))] {
        let error = s.sketch_import(&path, None, &PlaneRef::Base(BaseName::XY), Some(name), &scale).unwrap_err();
        assert!(error.to_string().contains("scale") || error.to_string().contains("finite"), "{error}");
        assert_eq!(archive(&mut s, &format!("import-refusals-{name}-after.qcad")), before);
    }
    let empty = file("empty.dxf", "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n");
    let error = s.sketch_import(&empty, None, &PlaneRef::Base(BaseName::XY), Some("empty"), &1.0.into()).unwrap_err();
    assert!(error.to_string().contains("no curves"));
    assert_eq!(archive(&mut s, "import-empty-after.qcad"), before);
    let unknown = file("unknown.dat", "not an import");
    let error = s.sketch_import(&unknown, None, &PlaneRef::Base(BaseName::XY), Some("unknown"), &1.0.into()).unwrap_err();
    assert!(error.to_string().contains("format"));
    assert_eq!(archive(&mut s, "import-unknown-after.qcad"), before);
    let error = s.sketch_import(std::path::Path::new("missing.dxf"), None, &PlaneRef::Base(BaseName::XY), None, &1.0.into()).unwrap_err();
    assert!(error.to_string().contains("cannot read"));
    assert_eq!(archive(&mut s, "import-missing-after.qcad"), before);
}

#[test]
fn svg_rect_circle_text_exact() {
    let svg = file(
        "exact.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="30mm" height="10mm" viewBox="0 0 30 10"><path d="M0 0 L30 0 L30 10 L0 10 Z"/><circle cx="15" cy="5" r="2"/><text x="1" y="2">ignored</text></svg>"#,
    );
    let mut s = Session::new_part();
    let r = s.sketch_import(&svg, None, &PlaneRef::Base(BaseName::XY), None, &1.0.into()).unwrap();
    let d = s.sketch_detail(r.sketch).unwrap();
    let xs: Vec<f64> = d.points.iter().map(|p| p.x).collect();
    let ys: Vec<f64> = d.points.iter().map(|p| p.y).collect();
    assert_close(
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max) - xs.iter().copied().fold(f64::INFINITY, f64::min),
        30.0,
        1e-6,
        "SVG width",
    );
    assert_close(
        ys.iter().copied().fold(f64::NEG_INFINITY, f64::max) - ys.iter().copied().fold(f64::INFINITY, f64::min),
        10.0,
        2e-6,
        "SVG height",
    );
    assert!(d.points.iter().any(|p| (p.x - 0.0).abs() < 1e-5 && (p.y - 10.0).abs() < 1e-5), "SVG top-left is CAD-up: {:?}", d.points);
    assert_eq!(d.entities.iter().filter(|e| e.kind == "circle").count(), 0);
    assert!(d.entities.iter().any(|e| e.kind == "spline" || e.kind == "line"));
    assert!(r.skipped.is_empty(), "SVG text is ignored by the pinned importer: {:?}", r.skipped);
}

#[test]
fn dxf_scale_exact() {
    let mut s = Session::new_part();
    let r = s.sketch_import(&square_dxf("scaled.dxf", 10.0), None, &PlaneRef::Base(BaseName::XY), None, &2.5.into()).unwrap();
    let d = s.sketch_detail(r.sketch).unwrap();
    for expected in [(0.0, 0.0), (25.0, 0.0), (25.0, 25.0), (0.0, 25.0)] {
        assert!(
            d.points.iter().any(|p| (p.x - expected.0).abs() < 1e-9 && (p.y - expected.1).abs() < 1e-9),
            "missing scaled corner {expected:?}"
        );
    }
}

#[test]
fn imported_square_pattern_move_extrude() {
    let mut s = Session::new_part();
    let r = s.sketch_import(&square_dxf("pattern.dxf", 10.0), None, &PlaneRef::Base(BaseName::XY), None, &1.0.into()).unwrap();
    let detail = s.sketch_detail(r.sketch).unwrap();
    let source: Vec<Id> = detail.entities.iter().map(|e| e.id).collect();
    s.sketch_pattern_create(
        r.sketch,
        &source,
        &SketchPatternKind::Linear { count: 3.0.into(), dx: 15.0.into(), dy: 0.0.into(), count2: None, dx2: None, dy2: None },
    )
    .unwrap();
    let body = s
        .extrude(&Extrude {
            sketch: r.sketch,
            profiles: None,
            profiles_at: None,
            height: 4.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    assert_close(s.shape(body).unwrap().volume(), 1200.0, 1e-6, "patterned square volume");
    s.sketch_edit(r.sketch, |s| {
        s.sketch_transform_move(r.sketch, &TransformSpec { entities: vec![source[0]], dx: 1.0.into(), dy: 0.0.into() })
    })
    .unwrap();
    let si = s.project().sketch_index(r.sketch).unwrap();
    let instance = s.project().sketches[si].patterns[0].instances[0];
    let points = entity_points(s.project().sketches[si].entity(instance).unwrap());
    assert!(points.iter().map(|id| s.project().sketches[si].point(*id).unwrap().x).any(|x| (x - 16.0).abs() < 1e-9));
    assert_close(s.shape(body).unwrap().volume(), 1200.0, 1e-6, "moved patterned square volume");
}

#[test]
fn imported_profile_app_path_height_edit() {
    let mut s = Session::new_part();
    s.param_set("h", &3.0.into()).unwrap();
    let source = file("profile.dxf", "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\n0\n90\n4\n70\n1\n10\n0\n20\n0\n10\n40\n20\n0\n10\n40\n20\n20\n10\n0\n20\n20\n0\nCIRCLE\n8\n0\n10\n20\n20\n10\n40\n5\n0\nENDSEC\n0\nEOF\n");
    let r = s.sketch_import(&source, None, &PlaneRef::Base(BaseName::XY), None, &1.0.into()).unwrap();
    let body = s
        .extrude(&Extrude {
            sketch: r.sketch,
            profiles: None,
            profiles_at: None,
            height: Num::Expr("h".into()),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    assert_close(s.shape(body).unwrap().volume(), (800.0 - std::f64::consts::PI * 25.0) * 3.0, 1e-6, "imported profile initial volume");
    let path = common::scratch("imported-profile.qcad");
    s.save_with_options(Some(&path), true).unwrap();
    let (app_volume, _, _, _) = gui_edit_param_model(&path, "h", "5");
    assert_close(app_volume, (800.0 - std::f64::consts::PI * 25.0) * 5.0, 1e-6, "imported profile app height edit");
    let (mut loaded, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let result = loaded.param_set("h", &5.0.into()).unwrap();
    assert!(result.errors.is_empty(), "{result:?}");
    assert_close(
        result.bodies.iter().find(|b| b.id == body).unwrap().volume,
        (800.0 - std::f64::consts::PI * 25.0) * 5.0,
        1e-6,
        "imported profile engine height edit",
    );
}
