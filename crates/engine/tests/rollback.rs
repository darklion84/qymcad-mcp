//! Rollbacks preserve both the recipe and the live geometry, including pending engine-level sketch edits.

mod common;
use common::*;
use qymcad_engine::*;
use std::f64::consts::PI;

fn extrude(sketch: Id, height: Num, op: Op) -> Extrude {
    Extrude { sketch, profiles: None, height, op, direction: Direction::Normal, through: false, target: None, name: None }
}

fn along_z() -> Sel {
    Sel::Along { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }
}

fn pending_circle() -> (Session, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let (body, _) = s.extrude(&extrude(sk, 20.0.into(), Op::Add)).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
    assert!(s.project().timeline.iter().any(|n| n.dirty), "engine-level sketch edit leaves a pending rebuild");
    (s, body)
}

#[test]
fn a_failed_feature_uses_a_clean_baseline_for_pending_sketch_edits() {
    let (mut s, body) = pending_circle();
    let (mut control, control_body) = pending_circle();
    assert!(control.rebuild().errors.is_empty());
    // 20³ block minus a through cylinder of radius 4/2 and height 20: 8000 - 80π mm³.
    let expected = 20.0_f64.powi(3) - PI * (4.0_f64 / 2.0).powi(2) * 20.0;
    assert_close(control.shape(control_body).unwrap().volume(), expected, 1e-6, "clean baseline volume");
    let control_mesh = control.project().bodies[control.project().mesh_index(control_body).unwrap()].mesh.volume();
    let e = s.fillet(None, &along_z(), &100.0.into(), None).unwrap_err();
    assert!(matches!(e, Error::Rebuild(_)), "{e}");
    let mesh = s.project().bodies[s.project().mesh_index(body).unwrap()].mesh.volume();
    assert_eq!(mesh.to_bits(), control_mesh.to_bits(), "restored project mesh must match the clean baseline: {mesh} vs {control_mesh}");
    let baseline_bits = control.shape(control_body).unwrap().volume().to_bits();
    assert_eq!(s.shape(body).unwrap().volume().to_bits(), baseline_bits, "live extrusion must match the clean baseline");
    assert!(!s.project().timeline.iter().any(|n| n.dirty), "rollback restores a clean baseline");
    assert!(s.rebuild().errors.is_empty());
    assert_eq!(s.shape(body).unwrap().volume().to_bits(), baseline_bits, "a later rebuild changes nothing");
}

fn volume_bits(s: &Session) -> Vec<(Id, u64)> {
    s.project().timeline.iter().flat_map(|n| n.kind.bodies()).filter_map(|id| s.shape(id).map(|sh| (id, sh.volume().to_bits()))).collect()
}

#[test]
fn a_feature_refuses_a_failing_dirty_baseline_before_editing() {
    let (mut s, _) = pending_circle();
    let sketch = s.sketches()[0].id;
    let entities: Vec<Id> = s.sketch_detail(sketch).unwrap().entities.iter().map(|e| e.id).collect();
    for entity in entities {
        s.sketch_remove_entity(sketch, entity).unwrap();
    }
    let nodes = s.project().timeline.len();
    let planes = s.project().planes.len();
    let e = s.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), Some("must not be created")).unwrap_err();
    assert!(matches!(e, Error::Rebuild(_)), "{e}");
    assert_eq!(s.project().timeline.len(), nodes, "a baseline failure must prevent creation of the datum node");
    assert_eq!(s.project().planes.len(), planes, "a baseline failure must prevent creation of the datum plane");
}

#[test]
fn a_failed_sketch_edit_restores_old_shapes_bit_identically() {
    let mut s = Session::new_part();
    s.param_set("t", &6.0.into()).unwrap();
    let plate = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let lines = s.sketch_rect(plate, &0.0.into(), &0.0.into(), &60.0.into(), &40.0.into(), false).unwrap();
    s.extrude(&extrude(plate, Num::Expr("t".into()), Op::Add)).unwrap();
    let (top, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &Num::Expr("t".into()), None).unwrap();
    let pocket = s.sketch_create(&PlaneRef::Plane(top), None).unwrap();
    s.sketch_rect(pocket, &0.0.into(), &0.0.into(), &30.0.into(), &16.0.into(), false).unwrap();
    s.extrude(&Extrude { direction: Direction::Reverse, ..extrude(pocket, 3.0.into(), Op::Cut) }).unwrap();
    s.param_set("t", &10.0.into()).unwrap();
    // Plate minus rectangular pocket: 60·40·10 - 30·16·3 mm³; F-017 allows a 0.001 mm cut-depth nudge.
    assert_close(s.result_bodies()[0].volume, 60.0 * 40.0 * 10.0 - 30.0 * 16.0 * 3.0, 1.0, "edited plate");
    let before = volume_bits(&s);
    let recipe = serde_json::to_value(s.project()).unwrap();
    // Opening the outer contour prevents the already built extrusion (and its pocket) from rebuilding.
    let e = s.sketch_edit(plate, |s| s.sketch_remove_entity(plate, lines[0])).unwrap_err();
    assert!(matches!(e, Error::Rebuild(_)), "{e}");
    let after = volume_bits(&s);
    let show = |v: &[(Id, u64)]| v.iter().map(|(id, bits)| (*id, f64::from_bits(*bits))).collect::<Vec<_>>();
    assert_eq!(after, before, "old body volumes after sketch rollback {:?} vs before {:?}", show(&after), show(&before));
    assert_eq!(serde_json::to_value(s.project()).unwrap(), recipe, "project restored exactly");
    assert!(s.rebuild().errors.is_empty());
    assert_eq!(volume_bits(&s), before, "a later rebuild changes nothing");
}

fn edited_pocket() -> Session {
    let mut s = Session::new_part();
    s.param_set("t", &6.0.into()).unwrap();
    s.param_set("x", &20.0.into()).unwrap();
    let plate = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(plate, &0.0.into(), &0.0.into(), &60.0.into(), &40.0.into(), false).unwrap();
    s.extrude(&extrude(plate, Num::Expr("t".into()), Op::Add)).unwrap();
    let (top, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &Num::Expr("t".into()), None).unwrap();
    let pocket = s.sketch_create(&PlaneRef::Plane(top), None).unwrap();
    s.sketch_rect(pocket, &0.0.into(), &0.0.into(), &30.0.into(), &16.0.into(), false).unwrap();
    s.extrude(&Extrude { direction: Direction::Reverse, ..extrude(pocket, 3.0.into(), Op::Cut) }).unwrap();
    let helper = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("signed coordinate")).unwrap();
    s.sketch_rect(helper, &Num::Expr("x".into()), &0.0.into(), &4.0.into(), &4.0.into(), true).unwrap();
    s.param_set("t", &10.0.into()).unwrap();
    // V = stock width*length*height - pocket width*length*depth; allow F-017's 0.001 mm depth nudge.
    assert_close(s.result_bodies()[0].volume, 60.0 * 40.0 * 10.0 - 30.0 * 16.0 * 3.0, 1.0, "parameter-edited pocket");
    s
}

fn brep_bytes(s: &Session) -> Vec<(Id, Vec<u8>)> {
    let _gate = qymcad_kernel::kernel_gate();
    s.project()
        .timeline
        .iter()
        .flat_map(|n| n.kind.bodies())
        .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
        .collect()
}

fn rejected_parameter_preserves_state(s: &mut Session, name: &str, value: f64, rebuild_error: bool) {
    let before = serde_json::to_vec(s.project()).unwrap();
    let shapes = brep_bytes(s);
    let error = s.param_set(name, &value.into()).unwrap_err();
    assert_eq!(matches!(error, Error::Rebuild(_)), rebuild_error, "{error}");
    assert!(brep_bytes(s) == shapes, "parameter rollback changed committed B-rep bytes ({name})");
    assert!(serde_json::to_vec(s.project()).unwrap() == before, "parameter rollback changed the project ({name})");
}

#[test]
fn an_unsolved_parameter_edit_restores_project_and_breps_exactly() {
    let mut s = edited_pocket();
    // Keep an unrelated pending dirty sketch to prove restoring the snapshot also restores dirty flags.
    let pending = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("pending sketch")).unwrap();
    s.sketch_circle(pending, &0.0.into(), &0.0.into(), &2.0.into(), true).unwrap();
    assert!(s.project().timeline.iter().any(|n| n.dirty));
    rejected_parameter_preserves_state(&mut s, "x", -20.0, false);
}

#[test]
fn a_failed_parameter_rebuild_restores_project_and_breps_exactly() {
    let mut s = edited_pocket();
    // A negative extrusion height is rejected by regeneration, after propagation has succeeded.
    rejected_parameter_preserves_state(&mut s, "t", -1.0, true);
    let before = brep_bytes(&s);
    assert!(s.rebuild().errors.is_empty());
    assert!(brep_bytes(&s) == before, "a later clean rebuild must change nothing");
}
