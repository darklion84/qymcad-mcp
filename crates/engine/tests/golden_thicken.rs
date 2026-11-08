//! G2: native Thicken (P15). Hand volumes: a planar face is extruded along its outward normal and united with the
//! part (block top +3 = 4000 + 20·20·3); a cylinder side +2 becomes a cylinder of radius 12 (π·12²·20).
mod common;
use common::*;
use qymcad_core::feature::FeatureKind;
use qymcad_engine::*;
use std::f64::consts::PI;

const TOP: [f64; 3] = [0.0, 0.0, 1.0];

fn extrude(sketch: Id, height: &str) -> Extrude {
    Extrude {
        sketch,
        profiles: None,
        profiles_at: None,
        height: Num::from(height),
        down: None,
        op: Op::Add,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    }
}

fn block() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("h", &10.0.into()).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let (body, report) = s.extrude(&extrude(sketch, "h")).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    (s, body)
}

fn cylinder() -> (Session, Id) {
    let mut s = Session::new_part();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sketch, &0.0.into(), &0.0.into(), &20.0.into(), false).unwrap();
    let (body, report) = s.extrude(&extrude(sketch, "20")).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    (s, body)
}

fn facing(dir: [f64; 3]) -> Sel {
    Sel::Facing { dir, tol_deg: 1.0 }
}

fn face_id(s: &mut Session, body: Id, dir: [f64; 3]) -> u32 {
    s.topology(Some(body), false)
        .unwrap()
        .faces
        .into_iter()
        .find(|f| f.normal.is_some_and(|n| (0..3).map(|k| n[k] * dir[k]).sum::<f64>() > 0.999))
        .unwrap()
        .id
}

fn volume(report: &Rebuild, expected: f64) {
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.bodies.len(), 1, "the plate is united with the part: {report:?}");
    assert_close(report.bodies[0].volume, expected, 1e-3, "thickened volume");
}

type Snapshot = (String, Vec<(Id, Vec<u8>)>, DocInfo);

fn snapshot(s: &Session) -> Snapshot {
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        s.project().bodies.iter().filter_map(|b| s.shape(b.id).map(|shape| (b.id, shape.to_brep_bytes().unwrap()))).collect()
    };
    (ron::ser::to_string(s.project()).unwrap(), breps, s.info())
}

fn unchanged(s: &Session, before: &Snapshot) {
    assert_eq!(ron::ser::to_string(s.project()).unwrap(), before.0, "recipe changed");
    assert_eq!(snapshot(s).1, before.1, "live shapes changed");
    assert_eq!(s.info(), before.2, "doc info changed");
}

#[test]
fn block_top_plus_three_adds_a_plate() {
    let (mut s, body) = block();
    let top = face_id(&mut s, body, TOP);
    let (feature, report) = s.thicken(None, &facing(TOP), &3.0.into(), Some("plate")).unwrap();
    volume(&report, 5200.0);
    assert_eq!(s.node_name(feature), "plate");
    // A description is frozen to the one face id it matched (native Thicken stores a bare id).
    match s.project().timeline.iter().find(|n| n.id == feature).unwrap().kind {
        FeatureKind::Thicken { face, src, .. } => {
            assert_eq!(face, top);
            assert_eq!(src, body);
        }
        ref other => panic!("{other:?}"),
    }
}

#[test]
fn cylinder_side_plus_two_grows_the_radius() {
    let (mut s, body) = cylinder();
    let side = s.topology(Some(body), false).unwrap().faces.into_iter().find(|f| f.kind == FaceKind::Cylinder).unwrap().id;
    let (_, report) = s.thicken(None, &Sel::Ids(vec![side]), &2.0.into(), None).unwrap();
    volume(&report, PI * 144.0 * 20.0);
    assert_close(PI * 144.0 * 20.0, 9047.7868, 1e-3, "research table");
}

#[test]
fn formula_thickness_follows_edits_and_refuses_non_positive_values() {
    let (mut s, _) = block();
    s.param_set("t", &3.0.into()).unwrap();
    let (feature, report) = s.thicken(None, &facing(TOP), &"t".into(), None).unwrap();
    volume(&report, 5200.0);
    assert_eq!(s.project().feat_dims[&feature]["thickness"], "t");
    volume(&s.param_set("t", &5.0.into()).unwrap(), 6000.0);
    volume(&s.param_set("h", &12.0.into()).unwrap(), 20.0 * 20.0 * 17.0);
    let before = snapshot(&s);
    // -15 would build green natively: the prism pokes out of the far side of the part.
    for bad in [-15.0, -5.0, 0.0] {
        let error = s.param_set("t", &bad.into()).unwrap_err().to_string();
        assert!(error.contains("thickness"), "{bad}: {error}");
        unchanged(&s, &before);
    }
    let target = DimensionTarget::Feature { feature, key: "thickness".into() };
    let error = s.dimension_set(&target, Some(&(-1.0).into()), None).unwrap_err().to_string();
    assert!(error.contains("thickness"), "{error}");
    unchanged(&s, &before);
    s.dimension_set(&target, Some(&"t-1".into()), None).unwrap();
    volume(&s.rebuild(), 20.0 * 20.0 * 16.0);
}

#[test]
fn refusals_leave_the_document_unchanged() {
    let (mut s, _) = block();
    let before = snapshot(&s);
    let walls = Sel::Union(vec![facing([1.0, 0.0, 0.0]), facing([-1.0, 0.0, 0.0])]);
    let cases: Vec<(Sel, Num, &str)> = vec![
        (facing(TOP), 0.0.into(), "thickness"),
        (facing(TOP), (-2.0).into(), "thickness"),
        (facing(TOP), (-15.0).into(), "thickness"),
        (walls, 2.0.into(), "exactly one face"),
        (facing([1.0, 1.0, 0.0]), 2.0.into(), "exactly one face"),
    ];
    for (sel, t, expected) in cases {
        let error = s.thicken(None, &sel, &t, None).unwrap_err().to_string();
        assert!(error.contains(expected), "{sel:?} {t:?}: expected `{expected}` in {error}");
        unchanged(&s, &before);
    }
}

#[test]
fn repick_moves_the_plate_and_refreshes_the_witness() {
    let (mut s, body) = block();
    let x = face_id(&mut s, body, [1.0, 0.0, 0.0]);
    let (feature, report) = s.thicken(None, &facing(TOP), &3.0.into(), None).unwrap();
    volume(&report, 5200.0);
    let (_, selected, report) = s.repick_thicken(feature, &facing([1.0, 0.0, 0.0])).unwrap();
    assert_eq!(selected, x);
    volume(&report, 4000.0 + 20.0 * 10.0 * 3.0);
    let witness = &s.project().face_refs[&feature];
    assert_eq!(witness.len(), 1);
    assert_eq!(witness[0].id, x);
    assert_eq!(witness[0].dir, [1.0, 0.0, 0.0]);
    let before = snapshot(&s);
    let error = s.repick_thicken(feature, &Sel::Union(vec![facing(TOP), facing([1.0, 0.0, 0.0])])).unwrap_err().to_string();
    assert!(error.contains("exactly one face"), "{error}");
    unchanged(&s, &before);
}

#[test]
fn save_and_reopen_keep_formula_volume_and_app_edit() {
    let (mut s, _) = block();
    s.param_set("t", &3.0.into()).unwrap();
    let (feature, report) = s.thicken(None, &facing(TOP), &"t".into(), None).unwrap();
    volume(&report, 5200.0);
    let path = scratch("g2-thicken.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    volume(&report, 5200.0);
    assert_eq!(opened.project().feat_dims[&feature]["thickness"], "t");
    volume(&opened.param_set("t", &4.0.into()).unwrap(), 5600.0);
    assert_close(gui_edit_param(&path, "t", "4"), 5600.0, 1e-6, "app thickness edit");
}
