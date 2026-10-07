//! A datum-only parameter moves a literal sketch extrusion; its invariant volume cannot prove placement.

mod common;
use common::*;
use qymcad_engine::*;

const W: f64 = 30.0;
const L: f64 = 10.0;
const H: f64 = 3.0;

fn build(mode: &str) -> Session {
    let mut s = Session::new_part();
    s.param_set("t", &6.0.into()).unwrap();
    let expr = |e: &str| Num::Expr(e.into());
    let (base, dist) = match mode {
        "direct" => (PlaneRef::Base(BaseName::XY), expr("t")),
        "parent" => {
            let (p, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &expr("t-2"), None).unwrap();
            (PlaneRef::Plane(p), 2.0.into())
        }
        "child" => {
            let (p, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &2.0.into(), None).unwrap();
            (PlaneRef::Plane(p), expr("t-2"))
        }
        "both" => {
            let (p, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &expr("t/2"), None).unwrap();
            (PlaneRef::Plane(p), expr("t/2"))
        }
        _ => unreachable!(),
    };
    let (p, _) = s.plane_offset(&base, &dist, None).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Plane(p), None).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &W.into(), &L.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch,
        profiles: None,
        height: H.into(),
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap();
    s
}

fn check_caps(faces: &[(f64, [f64; 3])], t: f64) {
    assert_eq!(faces.len(), 6);
    let bottom = faces.iter().find(|(_, n)| n[2] < -1.0 + 1e-9).expect("bottom cap");
    let top = faces.iter().find(|(_, n)| n[2] > 1.0 - 1e-9).expect("top cap");
    assert_close(bottom.0, t, 1e-6, "datum extrusion bottom z = offset");
    assert_close(top.0, t + H, 1e-6, "datum extrusion top z = offset + height");
}

fn check(s: &mut Session, t: f64) {
    let topo = s.topology(None, false).unwrap();
    check_caps(&topo.faces.iter().map(|f| (f.centroid[2], f.normal.expect("planar block face"))).collect::<Vec<_>>(), t);
    assert_close(s.result_bodies()[0].volume, W * L * H, 1e-6, "literal extrusion volume");
    assert!(s.info().errors.is_empty());
}

#[test]
fn datum_only_param_moves_a_literal_extrusion() {
    for mode in ["direct", "parent", "child", "both"] {
        let mut s = build(mode);
        check(&mut s, 6.0);
        s.param_set("t", &10.0.into()).unwrap();
        check(&mut s, 10.0);
    }
}

#[test]
fn datum_only_gui_param_moves_a_literal_extrusion() {
    for mode in ["direct", "parent", "child", "both"] {
        let mut s = build(mode);
        let path = scratch(&format!("datum_only_gui_{mode}.qcad"));
        s.save(Some(&path)).unwrap();
        let (v, _, faces) = gui_edit_param_faces(&path, "t", "10");
        check_caps(&faces.iter().map(|f| (f.centroid.z, f.normal)).collect::<Vec<_>>(), 10.0);
        assert_close(v, W * L * H, 1e-6, "GUI literal extrusion volume");
    }
}

#[test]
fn datum_only_reopened_param_moves_a_literal_extrusion() {
    for mode in ["direct", "parent", "child", "both"] {
        let mut s = build(mode);
        let path = scratch(&format!("datum_only_open_{mode}.qcad"));
        s.save(Some(&path)).unwrap();
        let (mut s, r) = Session::open(&path).unwrap();
        assert!(r.errors.is_empty());
        s.param_set("t", &10.0.into()).unwrap();
        check(&mut s, 10.0);
    }
}

#[test]
fn open_refreshes_guards_after_gui_datum_definition_changes() {
    for mode in ["replaced", "reattached"] {
        let mut s = build("direct");
        let path = scratch(&format!("datum_guard_refresh_{mode}.qcad"));
        s.save(Some(&path)).unwrap();
        let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
        let plane = project.planes[0].id;
        let t = if mode == "replaced" {
            project.parameters[0].name = "u".into();
            project.set_feat_dim(plane, "dist", "u".into());
            6.0
        } else {
            project.parameters.clear();
            project.sketches[0].plane = qymcad_core::feature::SketchPlane::World(qymcad_core::feature::BasePlane::XY);
            project.planes.clear();
            project.timeline.retain(|n| n.id != plane);
            project.feat_dims.remove(&plane);
            0.0
        };
        qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
        let (mut s, r) = Session::open(&path).unwrap();
        assert!(r.errors.is_empty(), "obsolete guard must not reference deleted t: {r:?}");
        let sketch = s.project().sketches[0].id;
        let guards: Vec<_> = s
            .project()
            .feat_dims
            .get(&sketch)
            .into_iter()
            .flat_map(|dims| dims.iter())
            .filter(|(key, _)| key.starts_with("datum_dist_"))
            .map(|(key, expr)| (key.clone(), expr.clone()))
            .collect();
        let expected = if mode == "replaced" { vec![(format!("datum_dist_{plane}"), "u".into())] } else { vec![] };
        assert_eq!(guards, expected, "reserved guards must match the sketch's current datum ancestry");
        check(&mut s, t);
        if mode == "replaced" {
            s.param_set("u", &10.0.into()).unwrap();
            check(&mut s, 10.0);
            s.save(Some(&path)).unwrap();
            let (volume, _, faces) = gui_edit_param_faces(&path, "u", "14");
            check_caps(&faces.iter().map(|f| (f.centroid.z, f.normal)).collect::<Vec<_>>(), 14.0);
            assert_close(volume, W * L * H, 1e-6, "GUI refreshed datum extrusion volume");
        }
    }
}

#[test]
fn top_pocket_area_cannot_be_smaller_than_the_analytic_area() {
    // Synthetic planar faces isolate the assertion: expected top = 30*10 - 10*5 = 250 mm².
    let faces = [(3.0, [0.0, 0.0, 1.0], 50.0), (6.0, [0.0, 0.0, 1.0], 249.0)];
    let rejected = std::panic::catch_unwind(|| assert_pocket_faces(&faces, 30.0, 10.0, 6.0, 10.0, 5.0, 3.0, 0.0, 2));
    assert!(rejected.is_err(), "a meshed top face smaller than its analytic area must be rejected");
}
