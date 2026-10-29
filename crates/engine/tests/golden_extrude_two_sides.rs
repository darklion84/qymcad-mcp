//! Independent rectangular-prism oracles for native Two sides, including native app edits.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

fn rect(s: &mut Session, plane: PlaneRef, w: f64, l: f64) -> u64 {
    let id = s.sketch_create(&plane, None).unwrap();
    s.sketch_rect(id, &0.0.into(), &0.0.into(), &w.into(), &l.into(), false).unwrap();
    id
}
fn args(sketch: u64, op: &str, reverse: bool) -> Extrude {
    serde_json::from_value(json!({"sketch":sketch,"height":"front","down":"back","op":if op=="first_add" {"add"} else {op},
        "direction":if reverse {"reverse"} else {"normal"}}))
    .unwrap()
}
fn model(op: &str, reverse: bool) -> (Session, u64) {
    let mut s = Session::new_part();
    s.param_set("front", &7.0.into()).unwrap();
    s.param_set("back", &3.0.into()).unwrap();
    let plane = if ["cut", "intersect", "add"].contains(&op) {
        let stock = rect(&mut s, PlaneRef::Base(BaseName::XY), 20.0, 16.0);
        let a: Extrude = serde_json::from_value(json!({"sketch":stock,"height":20})).unwrap();
        s.extrude(&a).unwrap();
        let (p, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &if op == "add" { 20.0 } else { 10.0 }.into(), None).unwrap();
        PlaneRef::Plane(p)
    } else {
        PlaneRef::Base(BaseName::XY)
    };
    let sketch = rect(&mut s, plane, 10.0, 6.0);
    let (id, r) = s.extrude(&args(sketch, op, reverse)).unwrap();
    assert!(r.errors.is_empty());
    let node = s.project().timeline.iter().find(|n| n.id == id).unwrap();
    let seed = match node.kind {
        qymcad_core::feature::FeatureKind::Extrude { down, .. } | qymcad_core::feature::FeatureKind::Combine { down, .. } => down,
        _ => panic!("expected native extrude/combine"),
    };
    assert_eq!(seed, 3.0, "both native adders receive the second-side length");
    (s, id)
}
fn body(s: &Session, v: f64, low: f64, high: f64) {
    let b = s.result_bodies();
    assert_eq!(b.len(), 1);
    assert_close(b[0].volume, v, 1e-5, "two-side volume");
    assert_close(b[0].bbox[2], low, 1e-5, "lower cap");
    assert_close(b[0].bbox[5], high, 1e-5, "upper cap");
}
fn caps(faces: &[qymcad_core::geom::MeshFace], expected: [f64; 2]) {
    let mut zs: Vec<_> = faces.iter().filter(|f| (f.area - 60.0).abs() < 1e-4 && f.normal[2].abs() > 0.999).map(|f| f.centroid.z).collect();
    zs.sort_by(f64::total_cmp);
    assert_eq!(zs.len(), 2, "two 10×6 end caps");
    for (z, e) in zs.into_iter().zip(expected) {
        assert_close(z, e, 1e-5, "cut cap position");
    }
}
#[test]
fn all_native_paths_and_direction_positions() {
    for reverse in [false, true] {
        for op in ["new_body", "first_add", "cut", "intersect", "add"] {
            let (mut s, _) = model(op, reverse);
            let (lo, hi) = if reverse { (-7., 3.) } else { (-3., 7.) };
            match op {
                "new_body" | "first_add" => body(&s, 600., lo, hi),
                "cut" => {
                    body(&s, 5800., 0., 20.);
                    let t = s.topology(None, false).unwrap();
                    let mut z: Vec<_> = t
                        .faces
                        .iter()
                        .filter(|f| (f.area - 60.).abs() < 1e-4 && f.normal.is_some_and(|n| n[2].abs() > 0.999))
                        .map(|f| f.centroid[2])
                        .collect();
                    z.sort_by(f64::total_cmp);
                    assert_eq!(z.len(), 2);
                    assert_close(z[0], 10. + lo, 1e-5, "cut lower cap");
                    assert_close(z[1], 10. + hi, 1e-5, "cut upper cap");
                }
                "intersect" => body(&s, 600., 10. + lo, 10. + hi),
                "add" => body(&s, if reverse { 6580. } else { 6820. }, 0., 20. + hi),
                _ => unreachable!(),
            }
        }
    }
}
#[test]
fn saved_global_drivers_follow_app_scheduler() {
    for reverse in [false, true] {
        for op in ["new_body", "first_add", "cut", "intersect", "add"] {
            let (mut s, _) = model(op, reverse);
            let path = scratch(&format!("two-side-global-{op}-{reverse}.qcad"));
            s.save(Some(&path)).unwrap();
            let (loaded, r) = Session::open(&path).unwrap();
            assert!(r.errors.is_empty());
            assert_close(
                loaded.result_bodies()[0].volume,
                if op == "cut" {
                    5800.
                } else if op == "add" {
                    if reverse {
                        6580.
                    } else {
                        6820.
                    }
                } else {
                    600.
                },
                1e-5,
                "loaded volume",
            );
            for driver in ["front", "back"] {
                let (v, b, f, _) = gui_edit_param_model(&path, driver, if driver == "front" { "9" } else { "5" });
                let expected = if op == "cut" {
                    5680.
                } else if op == "add" {
                    6400.
                        + 60.
                            * if reverse {
                                if driver == "back" {
                                    5.
                                } else {
                                    3.
                                }
                            } else if driver == "front" {
                                9.
                            } else {
                                7.
                            }
                } else {
                    720.
                };
                assert_close(v, expected, 1e-5, "app edited volume");
                let (lo, hi) = match (reverse, driver) {
                    (false, "front") => (-3., 9.),
                    (false, _) => (-5., 7.),
                    (true, "front") => (-9., 3.),
                    _ => (-7., 5.),
                };
                if op == "cut" {
                    caps(&f, [10. + lo, 10. + hi]);
                } else if op == "add" {
                    assert_close(b[2], 0., 1e-5, "added lower");
                    assert_close(b[5], 20. + hi, 1e-5, "added upper");
                } else {
                    let shift = if op == "intersect" { 10. } else { 0. };
                    assert_close(b[2], shift + lo, 1e-5, "app lower");
                    assert_close(b[5], shift + hi, 1e-5, "app upper");
                }
            }
        }
    }
}
#[test]
fn creation_refuses_invalid_and_ignored_second_side_atomically() {
    let mut s = Session::new_part();
    let sketch = rect(&mut s, PlaneRef::Base(BaseName::XY), 10., 6.);
    assert!(s.rebuild().errors.is_empty());
    let before = ron::ser::to_string(s.project()).unwrap();
    for d in [json!(-3.), json!(0.), json!(1e-12), json!(1e-9), json!("NaN"), json!("1e400")] {
        let a: Extrude = serde_json::from_value(json!({"sketch":sketch,"height":7,"down":d})).unwrap();
        assert!(s.extrude(&a).is_err(), "must refuse down={d}");
        assert_eq!(ron::ser::to_string(s.project()).unwrap(), before);
    }
    for extra in [json!({"direction":"symmetric"}), json!({"through":true})] {
        let mut a = json!({"sketch":sketch,"height":7,"down":3});
        a.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert!(s.extrude(&serde_json::from_value(a).unwrap()).is_err());
        assert_eq!(ron::ser::to_string(s.project()).unwrap(), before);
    }
}
#[test]
fn evaluated_parameter_and_dimension_refusals_retain_exact_archive() {
    let (mut s, id) = model("new_body", false);
    let path = scratch("two-side-atomic.qcad");
    s.save(Some(&path)).unwrap();
    let before = std::fs::read(&path).unwrap();
    for d in [-3., 0., 1e-12, 1e-9] {
        assert!(s.param_set("back", &d.into()).is_err(), "parameter down={d}");
        s.save(Some(&path)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            s.dimension_set(&DimensionTarget::Feature { feature: id, key: "down".into() }, Some(&d.into()), None).is_err(),
            "dimension down={d}"
        );
        s.save(Some(&path)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
#[test]
fn saved_named_height_and_down_follow_app_scheduler() {
    for reverse in [false, true] {
        for op in ["new_body", "first_add", "cut", "intersect", "add"] {
            let (mut s, id) = model(op, reverse);
            for (key, name) in [("height", "named_front"), ("down", "named_back")] {
                s.dimension_set(&DimensionTarget::Feature { feature: id, key: key.into() }, None, Some(name)).unwrap();
            }
            let path = scratch(&format!("two-side-named-{op}-{reverse}.qcad"));
            s.save(Some(&path)).unwrap();
            for (name, text) in [("named_front", "9"), ("named_back", "5")] {
                let (v, b, f, _) = gui_edit_driver_model(&path, name, text);
                let expected = if op == "cut" {
                    5680.
                } else if op == "add" {
                    6400.
                        + 60.
                            * if reverse {
                                if name == "named_back" {
                                    5.
                                } else {
                                    3.
                                }
                            } else if name == "named_front" {
                                9.
                            } else {
                                7.
                            }
                } else {
                    720.
                };
                assert_close(v, expected, 1e-5, "named app volume");
                let (lo, hi) = match (reverse, name) {
                    (false, "named_front") => (-3., 9.),
                    (false, _) => (-5., 7.),
                    (true, "named_front") => (-9., 3.),
                    _ => (-7., 5.),
                };
                if op == "cut" {
                    caps(&f, [10. + lo, 10. + hi]);
                } else if op == "add" {
                    assert_close(b[2], 0., 1e-5, "added lower");
                    assert_close(b[5], 20. + hi, 1e-5, "added upper");
                } else {
                    let shift = if op == "intersect" { 10. } else { 0. };
                    assert_close(b[2], shift + lo, 1e-5, "named lower");
                    assert_close(b[5], shift + hi, 1e-5, "named upper");
                }
            }
        }
    }
}
#[test]
fn one_side_dimension_zero_refused_and_raw_invalid_documents_refused_on_open() {
    let mut s = Session::new_part();
    let sketch = rect(&mut s, PlaneRef::Base(BaseName::XY), 10., 6.);
    let (id, _) = s.extrude(&serde_json::from_value(json!({"sketch":sketch,"height":7})).unwrap()).unwrap();
    assert!(s.dimension_set(&DimensionTarget::Feature { feature: id, key: "down".into() }, Some(&0.0.into()), None).is_err());
    let (mut s, id) = model("new_body", false);
    let path = scratch("two-side-invalid-open.qcad");
    s.save(Some(&path)).unwrap();
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    for d in ["-3", "0", "1e-12", "1e-9", "1e400"] {
        let mut p = loaded.project.clone();
        p.set_feat_dim(id, "down", d.into());
        qymcad_io::save_project_with_brep(&p, path.to_str().unwrap(), &loaded.breps).unwrap();
        assert!(Session::open(&path).is_err(), "open down={d}");
    }
}

#[test]
fn precise_numeric_down_and_numeric_strings_survive_edits_and_save() {
    for d in [json!(1.234567891234), json!("1.234567891234"), json!("  +3e0  "), json!("1.1e-9")] {
        let mut s = Session::new_part();
        let sketch = rect(&mut s, PlaneRef::Base(BaseName::XY), 10., 6.);
        let expected = Num::Expr(d.as_str().map(str::to_owned).unwrap_or_else(|| d.to_string())).eval(&Default::default()).unwrap();
        let a: Extrude = serde_json::from_value(json!({"sketch":sketch,"height":7,"down":d})).unwrap();
        let (id, _) = s.extrude(&a).unwrap();
        body(&s, 60. * (7. + expected), -expected, 7.);
        let target = DimensionTarget::Feature { feature: id, key: "down".into() };
        s.dimension_set(&target, None, Some("precise_down")).unwrap();
        assert_close(s.project().param_map()["precise_down"], expected, 1e-14, "native named down");
        let initial = scratch("numeric-down-initial.qcad");
        s.save(Some(&initial)).unwrap();
        let (v, b, _, _) = gui_edit_driver_model(&initial, "precise_down", "5");
        assert_close(v, 720.0, 1e-5, "numeric-string native app down");
        assert_close(b[2], -5.0, 1e-5, "numeric-string native app lower cap");
        s.dimension_set(&target, Some(&1.234567891234.into()), None).unwrap();
        body(&s, 60. * (7. + 1.234567891234), -1.234567891234, 7.);
        let path = scratch("precise-down.qcad");
        s.save(Some(&path)).unwrap();
        let (s, r) = Session::open(&path).unwrap();
        assert!(r.errors.is_empty());
        body(&s, 60. * (7. + 1.234567891234), -1.234567891234, 7.);
    }
}
