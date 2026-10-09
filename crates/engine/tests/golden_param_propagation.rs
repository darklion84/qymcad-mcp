//! Formula regressions for parameter scheduling in GUI-authored native documents.
mod common;
use common::*;
use qymcad_core::feature::FaceKey;
use qymcad_core::model::{Id, Project};
use qymcad_engine::*;

fn extrude(s: &mut Session, sketch: Id, height: Num) -> Id {
    s.extrude(&Extrude {
        sketch,
        profiles: None,
        height,
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap()
    .0
}

fn face_datum_model(chained: bool, unrelated: bool, label: &str) -> (Session, Id) {
    let mut s = Session::new_part();
    for (name, value) in [("w", 10.0), ("h2", 3.0), ("other", 4.0)] {
        s.param_set(name, &value.into()).unwrap();
    }
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &8.0.into(), &6.0.into(), false).unwrap();
    let f = extrude(&mut s, sk, Num::Expr("w".into()));
    let top = s.topology(Some(f), false).unwrap().faces.into_iter().find(|face| face.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    let path = scratch(&format!("face-datum-native-{chained}-{unrelated}-{label}.qcad"));
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    // plane_offset deliberately refuses face hosts. Reproduce the datum a GUI-authored file contains.
    let plane = project.add_plane_from_face(f, FaceKey { id: top, ..Default::default() }, 2.0);
    project.mark_node_dirty(plane);
    let plane = if chained {
        let child = project.add_offset_from_plane(plane, 5.0);
        project.mark_node_dirty(child);
        child
    } else {
        plane
    };
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (mut s, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    let width = if unrelated { Num::Expr("other".into()) } else { 4.0.into() };
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &width, &2.0.into(), false).unwrap();
    let e2 = extrude(&mut s, sk, Num::Expr("h2".into()));
    (s, e2)
}

fn check_placement(s: &mut Session, e2: Id, w: f64, chained: bool) {
    let z = w + 2.0 + if chained { 5.0 } else { 0.0 };
    let faces = s.topology(Some(e2), false).unwrap().faces;
    for (normal, expected) in [(-1.0, z), (1.0, z + 3.0)] {
        let cap = faces.iter().find(|f| f.normal.is_some_and(|n| (n[2] - normal).abs() < 1e-9)).unwrap();
        assert_close(cap.centroid[2], expected, 1e-6, "E2 cap z follows w + datum offsets + extrusion reach");
    }
    let body = s.result_bodies().into_iter().find(|b| b.id == e2).unwrap();
    assert_close(body.volume, 4.0 * 2.0 * 3.0, 1e-6, "E2 invariant prism volume");
    assert!(s.info().errors.is_empty());
}

#[test]
fn parameter_edit_rebuilds_extrusion_on_face_derived_datums() {
    for chained in [false, true] {
        for unrelated in [false, true] {
            let (mut s, e2) = face_datum_model(chained, unrelated, "server");
            check_placement(&mut s, e2, 10.0, chained);
            // Exercise the actual opened-document edit path too, including persisted sketch guards.
            let path = scratch(&format!("face-datum-edit-{chained}-{unrelated}.qcad"));
            s.save(Some(&path)).unwrap();
            let (mut s, r) = Session::open(&path).unwrap();
            assert!(r.errors.is_empty());
            for w in [20.0, 12.0, 10.0] {
                s.param_set("w", &w.into()).unwrap();
                check_placement(&mut s, e2, w, chained);
            }
        }
    }
}

fn driven_model(label: &str) -> Session {
    let mut s = Session::new_part();
    s.param_set("w", &10.0.into()).unwrap();
    let a = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("A")).unwrap();
    let lines = s.sketch_rect(a, &0.0.into(), &0.0.into(), &Num::Expr("w".into()), &3.0.into(), false).unwrap();
    // Reference diagonal has distinct refs from both driving width/height dimensions.
    let details = s.sketch_detail(a).unwrap();
    let bottom = &details.entities.iter().find(|e| e.id == lines[0]).unwrap().points;
    let top = &details.entities.iter().find(|e| e.id == lines[2]).unwrap().points;
    let index = s
        .sketch_constrain(
            a,
            &ConstrainSpec {
                kind: ConstraintKind::Distance,
                refs: vec![SketchRef::Id(bottom[0]), SketchRef::Id(top[0])],
                value: None,
                axis: DistAxis::Aligned,
                reference: true,
            },
        )
        .unwrap()
        .index
        .unwrap();
    let path = scratch(&format!("named-driven-native-{label}.qcad"));
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let si = project.sketch_index(a).unwrap();
    let refs = Project::dim_refs(&project.sketches[si].constraints[index]).unwrap();
    assert!(project.add_named_dim("diagonal".into(), a, refs));
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (mut s, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert_close(s.project().param_map()["diagonal"], (10f64.powi(2) + 3f64.powi(2)).sqrt(), 1e-6, "A reference diagonal");
    let b = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("B")).unwrap();
    s.sketch_rect(b, &0.0.into(), &0.0.into(), &Num::Expr("2*diagonal".into()), &4.0.into(), false).unwrap();
    extrude(&mut s, b, 2.0.into());
    s
}

#[test]
fn parameter_edit_solves_sketch_reached_by_updated_named_reference_dimension() {
    let mut s = driven_model("server");
    for w in [12.0, 7.0, 10.0] {
        let r = s.param_set("w", &w.into()).unwrap();
        let diagonal = (w.powi(2) + 3f64.powi(2)).sqrt();
        assert_close(s.project().param_map()["diagonal"], diagonal, 1e-6, "updated reference diagonal sqrt(w²+3²)");
        assert_close(r.bodies[0].volume, 2.0 * diagonal * 4.0 * 2.0, 1e-6, "B volume from updated reference diagonal");
    }
}

#[test]
fn native_gui_face_datum_parameter_edit_characterization() {
    for chained in [false, true] {
        for unrelated in [false, true] {
            let (mut s, _) = face_datum_model(chained, unrelated, "gui");
            let path = scratch(&format!("face-datum-gui-{chained}-{unrelated}.qcad"));
            s.save(Some(&path)).unwrap();
            let (v, _, faces) = gui_edit_param_faces(&path, "w", "20");
            let bottom = faces.iter().find(|f| f.normal[2] < -0.99).unwrap();
            // GUI re-solves every expression-bearing sketch; constant sketches miss the datum barrier.
            let native_w = if unrelated { 20.0 } else { 10.0 };
            let z = native_w + 2.0 + if chained { 5.0 } else { 0.0 };
            assert_close(bottom.centroid.z, z, 1e-6, "native GUI datum placement characterization");
            assert_close(v, 4.0 * 2.0 * 3.0, 1e-6, "native GUI invariant volume");
        }
    }
}

#[test]
fn native_gui_named_reference_parameter_edit_characterization() {
    let mut s = driven_model("gui");
    let path = scratch("named-driven-gui.qcad");
    s.save(Some(&path)).unwrap();
    let v = gui_edit_param(&path, "w", "12");
    assert_close(v, 16.0 * (12f64.powi(2) + 3f64.powi(2)).sqrt(), 1e-6, "native GUI B follows measured diagonal");
}
