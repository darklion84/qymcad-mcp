//! Sheet bodies (T0): reports, the false void warning, default bodies, and what each tool does on a loft sheet.
use crate::common;
use qymcad_engine::*;

fn v(x: f64) -> Num {
    Num::Value(x)
}

fn extrude(sketch: Id, height: f64, op: Op) -> Extrude {
    serde_json::from_value(serde_json::json!({"sketch": sketch, "height": height, "op": op})).unwrap()
}

/// A ruled loft sheet between a 20×16 rectangle on XY and the same one `h` above: a tube of four planar faces,
/// area 2·(20+16)·h, open at both ends.
fn loft_sheet(s: &mut Session, h: f64) -> Id {
    let low = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(low, &v(0.), &v(0.), &v(20.), &v(16.), false).unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &v(h), None).unwrap();
    let high = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    s.sketch_rect(high, &v(0.), &v(0.), &v(20.), &v(16.), false).unwrap();
    let sections = vec![LoftSection { sketch: low, profile: None }, LoftSection { sketch: high, profile: None }];
    let (_, r) = s.loft(&Loft { sections, ruled: true, op: Op::NewBody, target: None, surface: true, name: None }).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r.bodies.last().unwrap().id
}

fn info(s: &Session, body: Id) -> BodyInfo {
    s.result_bodies().into_iter().find(|b| b.id == body).unwrap_or_else(|| panic!("body {body} is not current"))
}

/// A solid box `w`×`d`×`h` from the XY plane up, centred on the origin, as a body of its own.
fn box_xy(s: &mut Session, w: f64, d: f64, h: f64) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(0.), &v(0.), &v(w), &v(d), false).unwrap();
    let (_, r) = s.extrude(&extrude(sk, h, Op::NewBody)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r.bodies.last().unwrap().id
}

fn close(actual: f64, expected: f64, tol: f64, what: &str) {
    assert!((actual - expected).abs() <= tol, "{what}: got {actual}, expected {expected} ± {tol}");
}

// ---- (a) reports ----

/// The tube: 2·(20+16)·10 = 720 mm² of surface, no volume, flagged as a sheet; a solid beside it is not.
#[test]
fn a_loft_sheet_reports_sheet_area_and_no_volume_and_a_solid_does_not() {
    let mut s = Session::new_part();
    let sheet = loft_sheet(&mut s, 10.);
    let solid = box_xy(&mut s, 12., 12., 6.);
    for b in s.result_bodies() {
        if b.id == sheet {
            assert!(b.sheet);
            close(b.area, 720., 1e-6, "tube area");
            assert_eq!(b.volume, 0.);
        } else {
            assert_eq!(b.id, solid);
            assert!(!b.sheet);
            close(b.area, 2. * 144. + 4. * 72., 1e-6, "block area");
            close(b.volume, 864., 1e-6, "block volume");
        }
    }
    let json = serde_json::to_value(info(&s, sheet)).unwrap();
    assert_eq!(json["sheet"], true);
    close(json["area_mm2"].as_f64().unwrap(), 720., 1e-6, "area_mm2");
    assert_eq!(json["volume_mm3"], 0.0);
    let json = serde_json::to_value(info(&s, solid)).unwrap();
    assert_eq!(json["sheet"], false);
    close(json["area_mm2"].as_f64().unwrap(), 576., 1e-6, "block area_mm2");
    // The rebuild entry a tool returns is the same record.
    let rebuilt = s.current_rebuild();
    assert!(rebuilt.bodies.iter().any(|b| b.id == sheet && b.sheet && (b.area - 720.).abs() < 1e-6));
    // The document stores the flag and the faces: a file opened without a rebuild reports the same.
    let path = common::scratch("t0-sheet-report.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, _) = Session::open(&path).unwrap();
    let b = info(&opened, sheet);
    assert!(b.sheet);
    close(b.area, 720., 1e-6, "area after open");
}

/// Export: a sheet goes out like any body, says it is an open surface and carries no tetrahedron-sum volume; a solid keeps it.
#[test]
fn export_marks_a_sheet_and_leaves_out_its_meaningless_mesh_volume() {
    let mut s = Session::new_part();
    let sheet = loft_sheet(&mut s, 10.);
    let solid = box_xy(&mut s, 12., 12., 6.);
    for (format, name) in [(ExportFormat::Stl, "t0-sheet.stl"), (ExportFormat::Step, "t0-sheet.step")] {
        let r = s.export(format, &common::scratch(name), Quality::Standard, None).unwrap();
        let one = r.bodies.iter().find(|b| b.id == sheet).unwrap();
        assert!(one.sheet);
        close(one.area_mm2, 720., 1e-6, "exported sheet area");
        assert_eq!(one.mesh_volume_mm3, None, "a sheet has no mesh volume");
        assert!(one.note.is_some_and(|n| n.contains("open surface")), "{one:?}");
        let other = r.bodies.iter().find(|b| b.id == solid).unwrap();
        assert!(!other.sheet && other.note.is_none());
        close(other.area_mm2, 576., 1e-6, "exported block area");
        assert_eq!(other.mesh_volume_mm3.is_some(), format.is_mesh());
    }
}

// ---- (b) the false warning ----

/// A sheet has a shell and no solid; the rebuild and the open both used to warn of a sealed void.
#[test]
fn a_sheet_is_not_a_sealed_void() {
    let mut s = Session::new_part();
    let low = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(low, &v(0.), &v(0.), &v(20.), &v(16.), false).unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &v(10.), None).unwrap();
    let high = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    s.sketch_rect(high, &v(0.), &v(0.), &v(20.), &v(16.), false).unwrap();
    let sections = vec![LoftSection { sketch: low, profile: None }, LoftSection { sketch: high, profile: None }];
    let (_, r) = s.loft(&Loft { sections, ruled: true, op: Op::NewBody, target: None, surface: true, name: None }).unwrap();
    let sealed = |warnings: Vec<String>| warnings.into_iter().filter(|w| w.contains("sealed")).collect::<Vec<_>>();
    assert!(sealed(r.warnings.iter().map(|w| w.to_string()).collect()).is_empty(), "{:?}", r.warnings);
    assert!(sealed(s.info().warnings).is_empty(), "{:?}", s.info().warnings);
    let path = common::scratch("t0-sheet-warning.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, r) = Session::open(&path).unwrap();
    assert!(sealed(r.warnings.iter().map(|w| w.to_string()).collect()).is_empty(), "{:?}", r.warnings);
    assert!(sealed(opened.info().warnings).is_empty(), "{:?}", opened.info().warnings);
}

