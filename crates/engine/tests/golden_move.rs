//! P28 rigid-transform geometry, native app dependencies, persistent names and refusals.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;
use std::{collections::BTreeMap, io::Read, path::Path};

const INITIAL: [f64; 6] = [-4., -8., 0., 6., 12., 30.];

fn block() -> (Session, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &1.0.into(), &2.0.into(), &10.0.into(), &20.0.into(), false).unwrap();
    let (id, report) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":30})).unwrap()).unwrap();
    assert!(report.errors.is_empty());
    (s, id)
}
fn shift(body: Option<Id>, delta: [Num; 3]) -> Move {
    Move { body, translation: Some(delta), rotation: None, name: None }
}
fn turn(body: Option<Id>, axis: Axis, angle: f64, pivot: Option<[f64; 3]>) -> Move {
    Move { body, translation: None, rotation: Some(Rotation { axis, angle, pivot }), name: None }
}
fn translated(b: [f64; 6], d: [f64; 3]) -> [f64; 6] {
    std::array::from_fn(|i| b[i] + d[i % 3])
}
/// Independent elementary right-handed rotation of all eight analytic box corners.
fn rotated(b: [f64; 6], axis: Axis, degrees: f64, pivot: [f64; 3]) -> [f64; 6] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let mut out = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for x in [b[0], b[3]] {
        for y in [b[1], b[4]] {
            for z in [b[2], b[5]] {
                let [x, y, z] = [x - pivot[0], y - pivot[1], z - pivot[2]];
                let p = match axis {
                    Axis::X => [x, cos * y - sin * z, sin * y + cos * z],
                    Axis::Y => [cos * x + sin * z, y, -sin * x + cos * z],
                    Axis::Z => [cos * x - sin * y, sin * x + cos * y, z],
                };
                for i in 0..3 {
                    out[i] = out[i].min(p[i] + pivot[i]);
                    out[i + 3] = out[i + 3].max(p[i] + pivot[i]);
                }
            }
        }
    }
    out
}
fn bounds(actual: [f64; 6], expected: [f64; 6], tol: f64) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert_close(actual, expected, tol, "independent rigid-transform corner bound");
    }
}
fn geometry(s: &Session, id: Id, expected: [f64; 6]) {
    let _gate = qymcad_kernel::kernel_gate();
    let shape = s.shape(id).unwrap();
    assert_close(shape.volume(), 10. * 20. * 30., 2e-5, "analytic prism volume");
    bounds(shape.bbox().unwrap(), expected, 3e-7);
}
fn ids(s: &mut Session, id: Id) -> (Vec<u32>, Vec<u32>) {
    let t = s.topology(Some(id), true).unwrap();
    (t.faces.iter().map(|f| f.id).collect(), t.edges.iter().map(|e| e.id).collect())
}
// Session caches use a HashMap, so ZIP entry order may change after rollback.
// Compare every complete entry byte-for-byte, including document and exact B-reps.
fn archive(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}

#[test]
fn translation_each_axis_uses_formulas_and_preserves_native_names() {
    for axis in 0..3 {
        for value in [-7.25, 4.125] {
            let (mut s, src) = block();
            s.param_set("Shift", &value.into()).unwrap();
            let before = ids(&mut s, src);
            let mut d = [0.0.into(), 0.0.into(), 0.0.into()];
            d[axis] = "Shift".into();
            let (m, report) = s.move_body(&shift(Some(src), d)).unwrap();
            assert!(report.errors.is_empty());
            assert!(!m.reused);
            assert_ne!(m.body, src);
            assert_eq!(m.source_body, src);
            let mut delta = [0.; 3];
            delta[axis] = value;
            geometry(&s, m.body, translated(INITIAL, delta));
            assert_eq!(ids(&mut s, m.body), before, "Move must pass through native face AND edge names");
            assert_eq!(s.result_bodies().iter().map(|b| b.id).collect::<Vec<_>>(), vec![m.body]);
            for element in [Element::Faces, Element::Edges] {
                let feature_faces = Sel::OfFeature { feature: m.body, role: None };
                let selection = if element == Element::Edges { Sel::EdgesOf(Box::new(feature_faces)) } else { feature_faces };
                assert!(s.select(Some(m.body), element, &selection).unwrap().1.is_empty(), "a Move creates no topology role names");
            }
            // A matching initial shape cannot establish that the native formula was persisted.
            let edited = value + 2.3456789;
            s.param_set("Shift", &edited.into()).unwrap();
            delta[axis] = edited;
            geometry(&s, m.body, translated(INITIAL, delta));
            assert_eq!(ids(&mut s, m.body), before, "driver edit preserves pass-through topology names");
        }
    }
}

#[test]
fn rotations_xyz_use_literal_degrees_and_sampled_or_explicit_pivot() {
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        for pivot in [None, Some([0., 0., 0.]), Some([3., -7., 11.])] {
            let (mut s, src) = block();
            let before = ids(&mut s, src);
            let (m, report) = s.move_body(&turn(None, axis, 90., pivot)).unwrap();
            assert!(report.errors.is_empty());
            geometry(&s, m.body, rotated(INITIAL, axis, 90., pivot.unwrap_or([1., 2., 15.])));
            assert_eq!(ids(&mut s, m.body), before);
        }
    }
}

#[test]
fn repeated_numeric_moves_reuse_node_and_compose_in_action_order() {
    let (mut s, src) = block();
    let (first, _) = s.move_body(&shift(Some(src), [10.0.into(), (-3.0).into(), 2.0.into()])).unwrap();
    let nodes = s.project().timeline.len();
    let (second, _) = s.move_body(&turn(None, Axis::Z, 90., Some([0.; 3]))).unwrap();
    assert!(second.reused);
    assert_eq!(second.body, first.body);
    assert_eq!(s.project().timeline.len(), nodes);
    let expected = rotated(translated(INITIAL, [10., -3., 2.]), Axis::Z, 90., [0.; 3]);
    geometry(&s, second.body, expected);
    let (third, _) = s.move_body(&turn(None, Axis::X, -90., Some([0.; 3]))).unwrap();
    assert!(third.reused);
    assert_eq!(third.body, first.body);
    geometry(&s, third.body, rotated(expected, Axis::X, -90., [0.; 3]));
}

#[test]
fn accumulated_formula_microshifts_keep_full_precision_and_app_dependencies() {
    for axis in 0..3 {
        let (mut s, _) = block();
        s.param_set("Shift", &10.0.into()).unwrap();
        let mut delta = [0.0.into(), 0.0.into(), 0.0.into()];
        delta[axis] = "Shift + 3".into();
        let (first, _) = s.move_body(&shift(None, delta)).unwrap();
        let key = ["dx", "dy", "dz"][axis];
        s.dimension_set(&DimensionTarget::Feature { feature: first.body, key: key.into() }, None, Some("MoveShift")).unwrap();
        let old_ids = ids(&mut s, first.body);
        for tiny in [Num::Value(0.00000123456789), "0.00000234567891".into()] {
            let mut d = [0.0.into(), 0.0.into(), 0.0.into()];
            d[axis] = tiny;
            let (next, report) = s.move_body(&shift(None, d)).unwrap();
            assert!(report.errors.is_empty());
            assert!(next.reused);
            assert_eq!(next.body, first.body);
        }
        let tiny = 0.00000123456789 + 0.00000234567891;
        let mut expected = [0.; 3];
        expected[axis] = 13. + tiny;
        geometry(&s, first.body, translated(INITIAL, expected));
        assert_eq!(ids(&mut s, first.body), old_ids);
        let path = scratch(&format!("move-microshift-{axis}.qcad"));
        s.save(Some(&path)).unwrap();
        let (mut loaded, r) = Session::open(&path).unwrap();
        assert!(r.errors.is_empty());
        geometry(&loaded, first.body, translated(INITIAL, expected));
        assert_eq!(ids(&mut loaded, first.body), old_ids);
        let (v, b, faces, _) = gui_edit_param_model(&path, "Shift", "25");
        assert_close(v, 6000., 2e-5, "native app global Move driver volume");
        expected[axis] = 28. + tiny;
        bounds(b, translated(INITIAL, expected), 3e-7);
        let mut face_ids = faces.iter().map(|f| f.id).collect::<Vec<_>>();
        face_ids.sort_unstable();
        assert_eq!(face_ids, old_ids.0);
        let (v, b, _, _) = gui_edit_driver_model(&path, "MoveShift", "43");
        assert_close(v, 6000., 2e-5, "native app named Move driver volume");
        expected[axis] = 43.;
        bounds(b, translated(INITIAL, expected), 3e-7);
    }
}

#[test]
fn affine_translation_dimensions_edit_rotated_matrix_and_reject_angle() {
    let (mut s, _) = block();
    let (m, _) = s.move_body(&turn(None, Axis::Z, 90., Some([1., 2., 15.]))).unwrap();
    // x'=3-y and y'=1+x. Changing affine dx from 3 to 8 translates five mm.
    s.dimension_set(&DimensionTarget::Feature { feature: m.body, key: "dx".into() }, Some(&8.0.into()), None).unwrap();
    geometry(&s, m.body, translated(rotated(INITIAL, Axis::Z, 90., [1., 2., 15.]), [5., 0., 0.]));
    let path = scratch("move-dimension-atomic.qcad");
    s.save(Some(&path)).unwrap();
    let before = archive(&path);
    for key in ["angle", "pivot", "height"] {
        assert!(s.dimension_set(&DimensionTarget::Feature { feature: m.body, key: key.into() }, Some(&20.0.into()), None).is_err());
        s.save(Some(&path)).unwrap();
        assert_eq!(archive(&path), before);
    }
}

#[test]
fn edited_numeric_affine_translation_is_used_by_later_rotation_accumulation() {
    let (mut s, _) = block();
    let (m, _) = s.move_body(&shift(None, [10.0.into(), 0.0.into(), 0.0.into()])).unwrap();
    s.dimension_set(&DimensionTarget::Feature { feature: m.body, key: "dx".into() }, Some(&"25.123456789".into()), None).unwrap();
    geometry(&s, m.body, translated(INITIAL, [25.123456789, 0., 0.]));
    let (next, report) = s.move_body(&turn(None, Axis::Z, 90., Some([0.; 3]))).unwrap();
    assert!(report.errors.is_empty());
    assert!(next.reused);
    assert_eq!(next.body, m.body);
    geometry(&s, m.body, rotated(translated(INITIAL, [25.123456789, 0., 0.]), Axis::Z, 90., [0.; 3]));
}

#[test]
fn formula_rotation_and_invalid_inputs_restore_exact_archive() {
    let (mut s, _) = block();
    s.param_set("Shift", &10.0.into()).unwrap();
    let (m, _) = s.move_body(&shift(None, ["Shift".into(), 0.0.into(), 0.0.into()])).unwrap();
    s.param_set("Shift", &25.0.into()).unwrap();
    let path = scratch("move-refusal-atomic.qcad");
    s.save(Some(&path)).unwrap();
    let before = archive(&path);
    let err = s.move_body(&turn(None, Axis::Z, 90., Some([0.; 3]))).unwrap_err().to_string();
    assert!(err.contains("rotation accumulation") && err.contains("formula"), "precise formula-mixing refusal: {err}");
    s.save(Some(&path)).unwrap();
    assert_eq!(archive(&path), before);
    let mut both = shift(None, [1.0.into(), 0.0.into(), 0.0.into()]);
    both.rotation = Some(Rotation { axis: Axis::X, angle: 30., pivot: None });
    for bad in [
        both,
        Move { body: None, translation: None, rotation: None, name: None },
        shift(None, ["missing_driver".into(), 0.0.into(), 0.0.into()]),
        shift(None, [f64::INFINITY.into(), 0.0.into(), 0.0.into()]),
        turn(None, Axis::Z, f64::NAN, None),
        turn(None, Axis::Z, 30., Some([f64::INFINITY, 0., 0.])),
    ] {
        assert!(s.move_body(&bad).is_err());
        s.save(Some(&path)).unwrap();
        assert_eq!(archive(&path), before, "failed Move must restore Project AND exact B-reps");
    }
    geometry(&s, m.body, translated(INITIAL, [25., 0., 0.]));
}

#[test]
fn formula_rotation_guard_checks_every_translation_axis() {
    for axis in 0..3 {
        let (mut s, _) = block();
        s.param_set("Shift", &10.0.into()).unwrap();
        let mut t: [Num; 3] = [0.0.into(), 0.0.into(), 0.0.into()];
        t[axis] = "Shift".into();
        let (m, _) = s.move_body(&shift(None, t)).unwrap();
        let path = scratch(&format!("move-formula-axis-{axis}.qcad"));
        s.save(Some(&path)).unwrap();
        let before = archive(&path);
        let err = s.move_body(&turn(None, Axis::Z, 90., Some([0.; 3]))).unwrap_err().to_string();
        assert!(err.contains("rotation accumulation") && err.contains("formula"), "axis {axis}: {err}");
        s.save(Some(&path)).unwrap();
        assert_eq!(archive(&path), before, "axis {axis}: refused rotation must leave the archive exact");
        let mut moved = [0.; 3];
        moved[axis] = 10.;
        geometry(&s, m.body, translated(INITIAL, moved));
    }
}

#[test]
fn downstream_fixed_pick_chamfer_follows_saved_global_and_named_move_drivers() {
    let (mut s, _) = block();
    s.param_set("Shift", &10.0.into()).unwrap();
    let (m, _) = s.move_body(&turn(None, Axis::Z, 90., None)).unwrap();
    s.move_body(&shift(None, ["Shift".into(), 0.0.into(), 0.0.into()])).unwrap();
    s.dimension_set(&DimensionTarget::Feature { feature: m.body, key: "dx".into() }, None, Some("MoveX")).unwrap();
    let t = s.topology(None, true).unwrap();
    let edge = t.edges.iter().find(|e| (e.a[2] - e.b[2]).abs() > 29.9).unwrap().id;
    let (chamfer, report) = s.chamfer(None, &Sel::Ids(vec![edge]), &1.0.into(), None, None).unwrap();
    assert!(report.errors.is_empty());
    assert_close(s.result_bodies()[0].volume, 6000. - 0.5 * 30., 2e-5, "one-edge chamfer triangular-prism volume");
    let path = scratch("move-downstream-chamfer.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    let node = opened.project().timeline.iter().find(|n| n.id == chamfer).unwrap();
    let qymcad_core::feature::FeatureKind::Chamfer { edges, .. } = &node.kind else { panic!("native chamfer") };
    assert_eq!(
        serde_json::to_value(edges).unwrap(),
        serde_json::to_value(qymcad_core::refs::Ref::picks(&[edge])).unwrap(),
        "F-024: saved modifier keeps a fixed persistent pick list"
    );
    for named in [false, true] {
        let (v, bb, _, _) = if named { gui_edit_driver_model(&path, "MoveX", "43") } else { gui_edit_param_model(&path, "Shift", "25") };
        assert_close(v, 5985., 2e-5, "downstream chamfer survives native saved-driver edit");
        bounds(bb, if named { [31., -3., 0., 51., 7., 30.] } else { [16., -3., 0., 36., 7., 30.] }, 3e-7);
    }
}

#[test]
fn moved_source_keeps_fillet_parameter_and_open_overlap_guards() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":20})).unwrap()).unwrap();
    s.move_body(&shift(None, [7.0.into(), (-4.0).into(), 2.0.into()])).unwrap();
    s.param_set("r", &2.0.into()).unwrap();
    let t = s.topology(None, true).unwrap();
    let edges = t.edges.iter().filter(|e| (e.a[2] - e.b[2]).abs() > 19.9).map(|e| e.id).collect::<Vec<_>>();
    let (fillet, _) = s.fillet(None, &Sel::Ids(edges), &"r".into(), None).unwrap();
    let volume = 8000. - 4. * (4. - std::f64::consts::PI) * 20.;
    assert_close(s.result_bodies()[0].volume, volume, 2e-5, "four R2 corner prisms");
    let path = scratch("move-fillet-param-open-guard.qcad");
    s.save(Some(&path)).unwrap();
    let before = archive(&path);
    let error = s.param_set("r", &15.0.into()).unwrap_err().to_string();
    assert!(error.contains("fillet R15.00 does not fit"), "{error}");
    s.save(Some(&path)).unwrap();
    assert!(archive(&path) == before, "fillet parameter refusal exact archive");
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.parameters.iter_mut().find(|p| p.name == "r").unwrap().expr = "15".into();
    project.eval_parameters();
    project.mark_param_dependents_dirty_for("r");
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (_, report) = Session::open(&path).unwrap();
    assert!(report.errors.iter().any(|e| e.node == fillet && e.message.contains("fillet R15.00 does not fit")), "{report:?}");
}

#[test]
fn source_size_edit_keeps_creation_sampled_pivot_after_save() {
    let mut s = Session::new_part();
    s.param_set("Width", &10.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &1.0.into(), &2.0.into(), &"Width".into(), &20.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":30})).unwrap()).unwrap();
    s.move_body(&turn(None, Axis::Z, 90., None)).unwrap();
    let path = scratch("move-sampled-pivot.qcad");
    s.save(Some(&path)).unwrap();
    let (v, b, _, _) = gui_edit_param_model(&path, "Width", "14");
    assert_close(v, 14. * 20. * 30., 2e-5, "source width changes volume through baked rotation");
    bounds(b, rotated([-6., -8., 0., 8., 12., 30.], Axis::Z, 90., [1., 2., 15.]), 3e-7);
}

#[test]
fn legal_fillet_passes_through_move_and_invalid_downstream_blend_is_atomic() {
    let (mut s, _) = block();
    let t = s.topology(None, true).unwrap();
    let edges = t.edges.iter().filter(|e| (e.a[2] - e.b[2]).abs() > 29.9).map(|e| e.id).collect::<Vec<_>>();
    let (fillet, _) = s.fillet(None, &Sel::Ids(vec![edges[0]]), &1.0.into(), None).unwrap();
    let before = ids(&mut s, fillet);
    let (m, _) = s.move_body(&shift(None, [7.0.into(), (-4.0).into(), 2.0.into()])).unwrap();
    assert_eq!(ids(&mut s, m.body), before);
    assert_close(s.result_bodies()[0].volume, 6000. - 30. * (1. - std::f64::consts::PI / 4.), 2e-5, "one-edge R1 volume survives Move");
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":20})).unwrap()).unwrap();
    s.move_body(&shift(None, [7.0.into(), (-4.0).into(), 2.0.into()])).unwrap();
    let t = s.topology(None, true).unwrap();
    let edges = t.edges.iter().filter(|e| (e.a[2] - e.b[2]).abs() > 19.9).map(|e| e.id).collect::<Vec<_>>();
    let path = scratch("move-fillet-guard.qcad");
    s.save(Some(&path)).unwrap();
    let before = archive(&path);
    let error = s.fillet(None, &Sel::Ids(edges), &15.0.into(), None).unwrap_err().to_string();
    assert!(error.contains("does not fit"), "F-074 still guards a moved source: {error}");
    s.save(Some(&path)).unwrap();
    assert!(archive(&path) == before, "fillet create refusal exact archive");
}

#[test]
fn imported_body_move_preserves_owner_active_context_and_local_frame_after_reload() {
    let (maker, _) = block();
    let step = scratch("move-owner-fixture.step");
    maker.export(ExportFormat::Step, &step, Quality::Standard, None).unwrap();
    let mut s = Session::new_part();
    let src = s.import(&step, 1., None, Some("Imported move stock")).unwrap().bodies[0].body;
    let active = s.project().active_component;
    let owner = s.project().body_owner(src);
    let path = scratch("move-import-owner.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    // Parent placement rotates local X into world Y, then translates. Moving local
    // X must still affect the exact local shape's X coordinate, not its Y coordinate.
    let placement = [0., -1., 0., 100., 1., 0., 0., 50., 0., 0., 1., 20.];
    project.components.iter_mut().find(|c| Some(c.id) == owner).unwrap().transform = placement;
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    assert_eq!(s.project().active_component, active);
    geometry(&s, src, INITIAL);
    let before = ids(&mut s, src);
    let (m, report) = s.move_body(&shift(Some(src), [7.0.into(), 0.0.into(), 0.0.into()])).unwrap();
    assert!(report.errors.is_empty());
    assert_eq!(s.project().body_owner(m.body), owner);
    assert_eq!(s.project().active_component, active);
    geometry(&s, m.body, translated(INITIAL, [7., 0., 0.]));
    assert_eq!(ids(&mut s, m.body), before);
    assert_eq!(s.project().components.iter().find(|c| Some(c.id) == owner).unwrap().transform, placement);
    s.save(Some(&path)).unwrap();
    let project = qymcad_io::load_project(path.to_str().unwrap()).unwrap();
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &[]).unwrap();
    std::fs::remove_file(step).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    geometry(&opened, m.body, translated(INITIAL, [7., 0., 0.]));
    assert_eq!(opened.project().body_owner(m.body), owner);
    assert_eq!(opened.project().active_component, active);
    assert_eq!(opened.project().components.iter().find(|c| Some(c.id) == owner).unwrap().transform, placement);
    assert_eq!(ids(&mut opened, m.body), before);
}
