//! Datum tilt geometry is checked by independent rotations and eight prism corners, never volume alone.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}
fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|v| v * k)
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn axes(n: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let x = if n[0].abs() > 0.999 {
        [0., 1., 0.]
    } else if n[1].abs() > 0.999 || n[2].abs() > 0.999 {
        [1., 0., 0.]
    } else {
        let v = cross([0., 0., 1.], n);
        scale(v, 1. / dot(v, v).sqrt())
    };
    // Fixtures use positive X/Z normals and negative Y normals, hence these aligned axes agree with the drawing axes.
    (x, cross(n, x))
}
fn rotate(v: [f64; 3], k: [f64; 3], angle: f64) -> [f64; 3] {
    let (s, c) = angle.to_radians().sin_cos();
    add(add(scale(v, c), scale(cross(k, v), s)), scale(k, dot(k, v) * (1. - c)))
}
fn tilted(n: [f64; 3], x: f64, y: f64) -> [f64; 3] {
    let (a, b) = axes(n);
    rotate(rotate(n, a, x), b, y)
}
fn expected(origin: [f64; 3], n: [f64; 3], turn: f64) -> [f64; 6] {
    let o = scale(n, dot(origin, n));
    let (u, v) = axes(n);
    let (s, c) = turn.to_radians().sin_cos();
    let x = add(scale(u, c), scale(v, s));
    let y = add(scale(v, c), scale(u, -s));
    let mut bb = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for a in [-5., 5.] {
        for b in [-3., 3.] {
            for h in [0., 4.] {
                let p = add(add(add(o, scale(x, a)), scale(y, b)), scale(n, h));
                for i in 0..3 {
                    bb[i] = bb[i].min(p[i]);
                    bb[i + 3] = bb[i + 3].max(p[i]);
                }
            }
        }
    }
    bb
}
fn check(v: f64, actual: [f64; 6], want: [f64; 6]) {
    assert_close(v, 240., 1e-6, "10×6×4 volume");
    for (a, w) in actual.into_iter().zip(want) {
        assert_close(a, w, 0.001, "transformed prism corner bbox");
    }
}
fn check_frame(frame: SketchWorldFrame, origin: [f64; 3], normal: [f64; 3], turn: f64) {
    let (u, v) = axes(normal);
    let (s, c) = turn.to_radians().sin_cos();
    for (actual, want) in [
        (frame.origin, scale(normal, dot(origin, normal))),
        (frame.normal, normal),
        (frame.x_axis, add(scale(u, c), scale(v, s))),
        (frame.y_axis, add(scale(v, c), scale(u, -s))),
    ] {
        for (a, w) in actual.into_iter().zip(want) {
            assert_close(a, w, 1e-9, "datum sketch frame");
        }
    }
}
fn prism(s: &mut Session, plane: u64) -> u64 {
    let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":4,"op":"new_body"})).unwrap()).unwrap().0
}
fn direct(base: BaseName) -> (Session, u64) {
    let mut s = Session::new_part();
    for (n, v) in [("offset", 7.), ("ax", 30.), ("ay", 20.), ("spin", 15.)] {
        s.param_set(n, &v.into()).unwrap();
    }
    let (p, _) =
        s.plane_offset_tilt(&PlaneRef::Base(base), &"offset".into(), &"ax".into(), &"ay".into(), &"spin".into(), Some("tilted")).unwrap();
    prism(&mut s, p);
    (s, p)
}
#[test]
fn direct_all_base_frames_and_repeated_rebuilds() {
    for (base, origin, n) in [
        (BaseName::XY, [0., 0., 7.], [0., 0., 1.]),
        (BaseName::XZ, [0., 7., 0.], [0., -1., 0.]),
        (BaseName::YZ, [7., 0., 0.], [1., 0., 0.]),
    ] {
        let (mut s, _) = direct(base);
        let want = expected(origin, tilted(n, 30., 20.), 15.);
        for _ in 0..3 {
            let r = s.rebuild();
            assert!(r.errors.is_empty());
            check(r.bodies[0].volume, r.bodies[0].bbox, want);
            check_frame(s.sketch_info(s.project().sketches[0].id).unwrap().world_frame.unwrap(), origin, tilted(n, 30., 20.), 15.);
        }
    }
}
#[test]
fn saved_global_and_named_fields_follow_unmodified_app_path() {
    let (mut s, p) = direct(BaseName::XY);
    for key in ["dist", "tilt_x", "tilt_y", "turn"] {
        s.dimension_set(&DimensionTarget::Feature { feature: p, key: key.into() }, None, Some(&format!("plane_{key}"))).unwrap();
    }
    let path = scratch("datum-direct.qcad");
    s.save(Some(&path)).unwrap();
    let (loaded, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    check(loaded.result_bodies()[0].volume, loaded.result_bodies()[0].bbox, expected([0., 0., 7.], tilted([0., 0., 1.], 30., 20.), 15.));
    for (global, key, value, d, x, y, t) in [
        ("offset", "dist", "11", 11., 30., 20., 15.),
        ("ax", "tilt_x", "40", 7., 40., 20., 15.),
        ("ay", "tilt_y", "35", 7., 30., 35., 15.),
        ("spin", "turn", "45", 7., 30., 20., 45.),
    ] {
        let want = expected([0., 0., d], tilted([0., 0., 1.], x, y), t);
        let (v, b, _, _) = gui_edit_param_model(&path, global, value);
        check(v, b, want);
        let (v, b, _, _) = gui_edit_driver_model(&path, &format!("plane_{key}"), value);
        check(v, b, want);
    }
}
fn chain() -> (Session, u64, u64, u64) {
    let mut s = Session::new_part();
    s.param_set("ax", &30.0.into()).unwrap();
    s.param_set("other", &30.0.into()).unwrap();
    let (p, _) =
        s.plane_offset_tilt(&PlaneRef::Base(BaseName::XY), &7.0.into(), &"ax".into(), &0.0.into(), &15.0.into(), Some("parent")).unwrap();
    let (c, _) = s.plane_offset_tilt(&PlaneRef::Plane(p), &2.0.into(), &10.0.into(), &20.0.into(), &25.0.into(), Some("child")).unwrap();
    let b = prism(&mut s, c);
    (s, p, c, b)
}
fn chain_expected(angle: f64) -> [f64; 6] {
    let n = tilted([0., 0., 1.], angle, 0.);
    expected(add([0., 0., 7.], scale(n, 2.)), tilted(n, 10., 20.), 25.)
}
#[test]
fn two_deep_chain_ancestor_global_and_named_drivers_move_child_solid() {
    let (mut s, p, _, _) = chain();
    let b = &s.result_bodies()[0];
    check(b.volume, b.bbox, chain_expected(30.));
    let path = scratch("datum-chain.qcad");
    s.save(Some(&path)).unwrap();
    // First run without a named target: its value propagation would mask a missing ax guard.
    let (v, b, _, _) = gui_edit_param_model(&path, "ax", "40");
    check(v, b, chain_expected(40.));
    s.dimension_set(&DimensionTarget::Feature { feature: p, key: "tilt_x".into() }, None, Some("ancestor_tilt")).unwrap();
    s.save(Some(&path)).unwrap();
    let (v, b, _, _) = gui_edit_driver_model(&path, "ancestor_tilt", "40");
    check(v, b, chain_expected(40.));
    s.param_set("ax", &40.0.into()).unwrap();
    let b = &s.result_bodies()[0];
    check(b.volume, b.bbox, chain_expected(40.));
    s.dimension_set(&DimensionTarget::Feature { feature: p, key: "tilt_x".into() }, Some(&45.0.into()), None).unwrap();
    let b = &s.result_bodies()[0];
    check(b.volume, b.bbox, chain_expected(45.));
}
#[test]
fn changed_ancestor_formula_reconciles_and_removes_old_guards() {
    let (mut s, p, _, _) = chain();
    s.dimension_set(&DimensionTarget::Feature { feature: p, key: "tilt_x".into() }, Some(&"other".into()), None).unwrap();
    let path = scratch("datum-chain-reconcile.qcad");
    s.save(Some(&path)).unwrap();
    let (v, b, _, _) = gui_edit_param_model(&path, "other", "40");
    check(v, b, chain_expected(40.));
    s.dimension_set(&DimensionTarget::Feature { feature: p, key: "tilt_x".into() }, None, Some("renamed_tilt")).unwrap();
    s.save(Some(&path)).unwrap();
    let (v, b, _, _) = gui_edit_driver_model(&path, "renamed_tilt", "40");
    check(v, b, chain_expected(40.));
    let sk = s.project().sketches[0].id;
    let dims = &s.project().feat_dims[&sk];
    assert_eq!(dims.get(&format!("datum_tilt_x_{p}")).map(String::as_str), Some("other"));
    assert!(!dims.values().any(|v| v == "ax" || v == "ancestor_tilt"));
    s.dimension_set(&DimensionTarget::Feature { feature: p, key: "tilt_x".into() }, None, Some("")).unwrap();
    assert!(!s.project().feat_dims[&sk].values().any(|v| v == "renamed_tilt"));
}
#[test]
fn creation_bounds_are_atomic_and_boundary_values_are_not_normalized() {
    let mut s = Session::new_part();
    let before = ron::ser::to_string(s.project()).unwrap();
    for (d, x, y, t) in [(100001., 0., 0., 0.), (-100001., 0., 0., 0.), (0., 361., 0., 0.), (0., 0., -361., 0.), (0., 0., 0., 361.)] {
        assert!(s.plane_offset_tilt(&PlaneRef::Base(BaseName::XY), &d.into(), &x.into(), &y.into(), &t.into(), None).is_err());
        assert_eq!(ron::ser::to_string(s.project()).unwrap(), before);
    }
    let (p, _) = s
        .plane_offset_tilt(&PlaneRef::Base(BaseName::XY), &(-100000.).into(), &(-360.).into(), &360.0.into(), &360.0.into(), None)
        .unwrap();
    let pl = s.project().planes.iter().find(|pl| pl.id == p).unwrap();
    assert_eq!(pl.tilt.about_x, -360.);
    assert_eq!(pl.tilt.about_y, 360.);
    assert_eq!(pl.rot_deg, 360.);
}
#[test]
fn evaluated_bounds_refuse_parameter_and_dimension_edits_with_exact_archive() {
    let (mut s, p) = direct(BaseName::XY);
    let path = scratch("datum-atomic.qcad");
    s.save(Some(&path)).unwrap();
    let before = std::fs::read(&path).unwrap();
    for (key, param, limit) in [("dist", "offset", 100000.), ("tilt_x", "ax", 360.), ("tilt_y", "ay", 360.), ("turn", "spin", 360.)] {
        for value in [limit + 1., -limit - 1.] {
            assert!(s.param_set(param, &value.into()).is_err(), "invalid {param}");
            s.save(Some(&path)).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert!(
                s.dimension_set(&DimensionTarget::Feature { feature: p, key: key.into() }, Some(&value.into()), None).is_err(),
                "invalid {key}"
            );
            s.save(Some(&path)).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }
}
#[test]
fn legacy_without_angle_formulas_opens_unchanged() {
    let mut s = Session::new_part();
    let (p, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &7.0.into(), None).unwrap();
    prism(&mut s, p);
    assert!(["tilt_x", "tilt_y", "turn"].iter().all(|k| s.project().feat_dim(p, k).is_none()));
    let path = scratch("datum-legacy.qcad");
    s.save(Some(&path)).unwrap();
    let (loaded, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    check(loaded.result_bodies()[0].volume, loaded.result_bodies()[0].bbox, [-5., -3., 7., 5., 3., 11.]);
}

#[test]
fn opening_native_out_of_range_or_invalid_formula_is_refused() {
    let (mut s, p) = direct(BaseName::XY);
    let good = scratch("datum-open-good.qcad");
    s.save(Some(&good)).unwrap();
    for (key, text) in
        [("dist", "100001"), ("tilt_x", "361"), ("tilt_y", "-361"), ("turn", "361"), ("tilt_x", "missing"), ("turn", "1e400")]
    {
        let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(good.to_str().unwrap()).unwrap();
        project.set_feat_dim(p, key, text.into());
        let bad = scratch(&format!("datum-open-{key}-{text}.qcad"));
        qymcad_io::save_project_with_brep(&project, bad.to_str().unwrap(), &breps).unwrap();
        assert!(Session::open(&bad).is_err(), "open must refuse {key}={text}");
    }
}
