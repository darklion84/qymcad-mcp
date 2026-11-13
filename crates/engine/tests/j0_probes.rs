//! Package J phase 0: probes of what the app does with primitives (box, cylinder, sphere, cone, torus, prism).
//! The native probes go through the core API only (`Project` + the app's regeneration), no Session and no server guard.
//! Each probe prints what it observed (run with --nocapture); assertions pin the observation, so a changed app shows here.
mod common;
use qymcad_core::model::{Id, Project};
use std::collections::HashMap;

/// What the native regeneration made of a primitive document.
#[derive(Debug)]
struct Native {
    volume: Option<f64>,
    bbox: Option<[f64; 6]>,
    solids: u32,
    shells: u32,
    faces: usize,
    errors: Vec<String>,
}

fn fresh() -> Project {
    let mut p = Project::default();
    p.new_document();
    p
}

/// The numbers the app's panel writes on the node (`write_feat_formula(.., Stands)`): the text stands as typed.
fn typed(p: &mut Project, node: Id, key: &str, text: &str) {
    p.set_feat_dim(node, key, text.to_string());
}

/// Regenerate everything and measure `body` (the last result body when `None`).
fn measure(mut p: Project, body: Option<Id>) -> Native {
    p.eval_parameters();
    let (report, shapes): (_, HashMap<Id, qymcad_kernel::Shape>) = qymcad_testkit::regenerate(&mut p);
    let consumed = p.consumed_bodies();
    let last = body.or_else(|| p.timeline.iter().flat_map(|n| n.kind.bodies()).rfind(|b| !consumed.contains(b)));
    let errors = report.errors.iter().map(|e| format!("{e:?}")).collect();
    let Some(shape) = last.and_then(|b| shapes.get(&b)) else {
        return Native { volume: None, bbox: None, solids: 0, shells: 0, faces: 0, errors };
    };
    let _g = qymcad_kernel::kernel_gate();
    Native {
        volume: Some(shape.volume()),
        bbox: shape.bbox(),
        solids: shape.solid_count(),
        shells: shape.shell_count(),
        faces: last.and_then(|b| p.regen_faces.get(&b)).map_or(0, |f| f.len()),
        errors,
    }
}

/// Create one primitive of `kind` with the app's default sizes, then type `key = text` into its node as the properties
/// panel does, and regenerate.
fn one(kind: &str, key: &str, text: &str) -> Native {
    let mut p = fresh();
    let id = match kind {
        "box" => p.add_box(20.0, 20.0, 20.0),
        "cylinder" => p.add_cylinder(10.0, 20.0),
        "sphere" => p.add_sphere(10.0),
        "cone" => p.add_cone(10.0, 4.0, 20.0),
        "torus" => p.add_torus(12.0, 4.0),
        "prism" => p.add_prism(10.0, 6, 20.0),
        _ => unreachable!(),
    };
    typed(&mut p, id, key, text);
    measure(p, Some(id))
}

fn show(label: &str, n: &Native) {
    eprintln!(
        "{label}: volume {:?} bbox {:?} solids {} shells {} faces {} errors {:?}",
        n.volume, n.bbox, n.solids, n.shells, n.faces, n.errors
    );
}

use std::f64::consts::PI;

fn close(got: Option<f64>, want: f64, rel: f64, what: &str) {
    let got = got.unwrap_or_else(|| panic!("{what}: no body"));
    assert!((got - want).abs() <= want.abs() * rel, "{what}: got {got}, expected {want} (rel {rel})");
}

/// P1: sizes below the field minimum. The core builds them correctly down to 1e-5 (the property panel and `dimension_set` have
/// no minimum), and silently wrong below: a thin extrusion of 1e-6 has a third of its volume and two faces, errors empty (F-180).
#[test]
fn p1_sizes_below_the_field_minimum() {
    // Correct below the field's 0.1 / 0.05.
    for (kind, key, text, want) in [
        ("box", "dx", "0.01", 0.01 * 400.0),
        ("box", "dx", "0.00001", 0.00001 * 400.0),
        ("cylinder", "h", "0.00001", PI * 100.0 * 0.00001),
        ("cylinder", "r", "0.0001", PI * 1e-8 * 20.0),
        ("sphere", "r", "0.001", 4.0 / 3.0 * PI * 1e-9),
        ("torus", "minor", "0.00001", 2.0 * PI * PI * 12.0 * 1e-10),
        ("prism", "h", "0.00001", 259.8076211353316 * 0.00001),
        ("prism", "r", "0.0001", 3.0 * 1e-8 * (PI / 3.0).sin() * 20.0),
        ("cone", "h", "0.000001", 1.6336281798667556e-4),
    ] {
        let n = one(kind, key, text);
        show(&format!("P1 {kind} {key}={text}"), &n);
        assert!(n.errors.is_empty(), "{kind} {key}={text}: {:?}", n.errors);
        close(n.volume, want, 1e-6, &format!("{kind} {key}={text}"));
    }
    // Silently wrong: errors empty, a third of the volume, two faces.
    for (kind, key, want) in [("box", "dx", 4.0e-4), ("cylinder", "h", PI * 100.0 * 1e-6), ("prism", "h", 259.8076211353316 * 1e-6)] {
        let n = one(kind, key, "0.000001");
        show(&format!("P1 {kind} {key}=1e-6"), &n);
        assert!(n.errors.is_empty(), "{kind}: the app reports nothing: {:?}", n.errors);
        close(n.volume, want / 3.0, 1e-6, &format!("{kind} {key}=1e-6 is a third of the exact volume"));
        assert_eq!(n.faces, 2, "{kind}");
    }
    let n = one("prism", "r", "0.00001");
    assert!(n.errors.is_empty() && n.faces == 6, "{n:?}");
    close(n.volume, 3.752776749732568e-9, 1e-9, "prism r=1e-5 is 0.72 of the exact 5.196e-9");
    // Visible errors further down.
    for (kind, key, text, code) in [
        ("box", "dz", "0.00000001", "OpFailed(ExtrudeContour)"),
        ("cylinder", "r", "0.000001", "EmptyResult"),
        ("sphere", "r", "0.0001", "EmptyResult"),
        ("torus", "minor", "0.000001", "EmptyResult"),
        ("prism", "r", "0.000001", "EmptyResult"),
    ] {
        let n = one(kind, key, text);
        assert!(n.volume.is_none() && n.errors.iter().any(|e| e.contains(code)), "{kind} {key}={text}: {n:?}");
    }
}

/// P2: above the field maximum 1e5 the core builds correct bodies, at least up to 1e12 mm: no refusal is needed for the result's sake.
#[test]
fn p2_sizes_above_1e5_are_built_correctly() {
    for (kind, key, text, want) in [
        ("box", "dx", "1000000", 1.0e6 * 400.0),
        ("box", "dx", "1000000000000", 1.0e12 * 400.0),
        ("cylinder", "r", "1000000", PI * 1e12 * 20.0),
        ("cylinder", "h", "1000000000", PI * 100.0 * 1e9),
        ("sphere", "r", "1000000", 4.0 / 3.0 * PI * 1e18),
        ("torus", "major", "1000000", 2.0 * PI * PI * 1e6 * 16.0),
        ("prism", "r", "1000000", 3.0 * 1e12 * (PI / 3.0).sin() * 20.0),
    ] {
        let n = one(kind, key, text);
        show(&format!("P2 {kind} {key}={text}"), &n);
        assert!(n.errors.is_empty(), "{kind}: {:?}", n.errors);
        close(n.volume, want, 1e-4, &format!("{kind} {key}={text}"));
    }
}

/// P3: the prism's n. The core rounds half away from zero, takes at least 3, and has no upper limit (F-181).
#[test]
fn p3_prism_side_count_is_rounded_clamped_and_unbounded() {
    let area = |n: f64| n * 0.5 * 100.0 * (2.0 * PI / n).sin() * 20.0;
    for (text, sides) in [
        ("2", 3.0),
        ("2.5", 3.0),
        ("3.4", 3.0),
        ("3.5", 4.0),
        ("5.5", 6.0),
        ("0", 3.0),
        ("-4", 3.0),
        ("64", 64.0),
        ("65", 65.0),
        ("1000", 1000.0),
    ] {
        let n = one("prism", "n", text);
        show(&format!("P3 n={text}"), &n);
        assert!(n.errors.is_empty(), "{text}: {:?}", n.errors);
        close(n.volume, area(sides), 1e-9, &format!("n={text} is a {sides}-gon"));
        assert_eq!(n.faces, sides as usize + 2, "n={text}");
    }
}

/// P4: zero and negative sizes are a visible core error on every kind.
#[test]
fn p4_zero_and_negative_sizes_are_a_visible_error() {
    for (kind, key, text) in [
        ("box", "dx", "0"),
        ("box", "dx", "-5"),
        ("cylinder", "r", "-3"),
        ("cylinder", "h", "0"),
        ("sphere", "r", "0"),
        ("cone", "r1", "-1"),
        ("cone", "r2", "-1"),
        ("cone", "h", "-2"),
        ("torus", "major", "-12"),
        ("torus", "minor", "-1"),
        ("prism", "r", "-10"),
        ("prism", "h", "-5"),
    ] {
        let n = one(kind, key, text);
        assert!(n.volume.is_none() && n.errors.iter().any(|e| e.contains("PrimitiveSizeNotAboveZero")), "{kind} {key}={text}: {n:?}");
    }
}

/// P5/P6: the cone. Equal radii are a visible `OpFailed(Cone)` (also 1e-7 apart; 1e-5 apart builds exactly); r1 = 0 is an apex at the
/// bottom and builds (the field refuses it at creation, the edit field does not); both zero is a visible error.
#[test]
fn p5_cone_radii() {
    let cone = |r1: &str, r2: &str| {
        let mut p = fresh();
        let id = p.add_cone(10.0, 4.0, 20.0);
        typed(&mut p, id, "r1", r1);
        typed(&mut p, id, "r2", r2);
        let n = measure(p, Some(id));
        show(&format!("P5 cone r1={r1} r2={r2}"), &n);
        n
    };
    for (r1, r2) in [("10", "10"), ("4", "4"), ("10", "10.0000001")] {
        let n = cone(r1, r2);
        assert!(n.volume.is_none() && n.errors.iter().any(|e| e.contains("OpFailed(Cone)")), "{r1}/{r2}: {n:?}");
    }
    let n = cone("10", "10.00001");
    close(n.volume, PI * 20.0 / 3.0 * (100.0 + 10.00001 * 10.0 + 10.00001f64.powi(2)), 1e-9, "1e-5 apart builds exactly");
    for (r1, r2) in [("0", "10"), ("10", "0"), ("0.000001", "10")] {
        let n = cone(r1, r2);
        assert!(n.errors.is_empty() && n.faces == 2, "{r1}/{r2}: {n:?}");
        close(n.volume, PI * 100.0 * 20.0 / 3.0, 1e-6, &format!("apex cone {r1}/{r2}"));
    }
    let n = cone("0", "0");
    assert!(n.volume.is_none() && n.errors.iter().any(|e| e.contains("PrimitiveSizeNotAboveZero")), "{n:?}");
}

/// P7: a torus whose tube nearly touches the ring's radius builds with the exact volume (the field stops at major - 0.001; the core
/// only at minor >= major, a visible `TorusThroughItself`).
#[test]
fn p7_torus_minor_near_major() {
    for minor in ["11.9", "11.999", "11.9999999"] {
        let n = one("torus", "minor", minor);
        show(&format!("P7 torus 12/{minor}"), &n);
        let m: f64 = minor.parse().unwrap();
        assert!(n.errors.is_empty(), "{minor}: {:?}", n.errors);
        close(n.volume, 2.0 * PI * PI * 12.0 * m * m, 1e-9, &format!("torus 12/{minor}"));
    }
    for minor in ["12", "12.5"] {
        let n = one("torus", "minor", minor);
        assert!(n.volume.is_none() && n.errors.iter().any(|e| e.contains("TorusThroughItself")), "{minor}: {n:?}");
    }
}

/// P8: a formula that stopped evaluating is a visible error in the rebuild, not a silent fallback to the stored number.
#[test]
fn p8_a_formula_that_does_not_evaluate_is_a_visible_error() {
    let mut p = fresh();
    p.parameters.push(qymcad_core::model::Param { name: "w".into(), expr: "30".into(), value: 30.0 });
    let id = p.add_box(20.0, 20.0, 20.0);
    typed(&mut p, id, "dx", "w");
    let n = measure(p.clone(), Some(id));
    close(n.volume, 12000.0, 1e-9, "dx = w = 30");
    p.parameters.clear();
    let n = measure(p.clone(), Some(id));
    assert!(n.volume.is_none() && n.errors.iter().any(|e| e.contains("UnknownName")), "{n:?}");
    typed(&mut p, id, "dx", "1+");
    let n = measure(p, Some(id));
    assert!(n.volume.is_none() && n.errors.iter().any(|e| e.contains("UnexpectedEnd")), "{n:?}");
}

/// P9: a second primitive united with the first. Apart or touching at a point: ONE body of two solids, volumes added, no
/// error; overlapping: one solid.
#[test]
fn p9_union_with_a_second_primitive() {
    let sphere = 4.0 / 3.0 * PI * 125.0;
    for (label, dx, dy, solids, volume) in [
        ("apart", 100.0, 0.0, 2, 8000.0 + sphere),
        ("touching the box's face at a point", 15.0, 0.0, 2, 8000.0 + sphere),
        ("overlapping", 10.0, 0.0, 1, 8000.0 + sphere - inside_the_box(10.0)),
    ] {
        let mut p = fresh();
        let a = p.add_box(20.0, 20.0, 20.0);
        p.finish_base_body(a, 1);
        let b = p.add_sphere(5.0);
        let mut m = qymcad_core::feature::PLACE_IDENTITY;
        (m[3], m[7]) = (dx, dy);
        let moved = p.add_move(b, m);
        p.finish_base_body(moved, 1);
        let n = measure(p, None);
        show(&format!("P9 {label}"), &n);
        assert!(n.errors.is_empty(), "{label}: {:?}", n.errors);
        assert_eq!((n.solids, n.shells), (solids, solids), "{label}");
        close(n.volume, volume, 2e-3, label);
    }
}

/// The part of the sphere r = 5 (centre at x = `dx`, y = z = 0) inside the box x, y in -10..10, z in 0..20: its upper half, less the cap
/// beyond x = 10 (cap of height h: pi h^2 (3r - h) / 3), halved for the upper half.
fn inside_the_box(dx: f64) -> f64 {
    let h = (dx + 5.0 - 10.0).max(0.0);
    let cap = PI * h * h * (3.0 * 5.0 - h) / 3.0;
    4.0 / 3.0 * PI * 125.0 / 2.0 - cap / 2.0
}
