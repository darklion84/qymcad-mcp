//! Branch-aware native chamfer dimensions: the native d2 key changes units by mode.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

fn model(two_dist: bool) -> (Session, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":10})).unwrap()).unwrap();
    let t = s.topology(None, true).unwrap();
    let edge = t.edges.iter().find(|e| e.mid[0] > 19.9 && e.mid[1] > 14.9).unwrap().id;
    let other = 4.0.into();
    let (id, _) = s.chamfer(None, &Sel::Ids(vec![edge]), &2.0.into(), if two_dist { Some(&other) } else { None }, None).unwrap();
    (s, id)
}

#[test]
fn two_dist_dimension_reports_mm_and_mode() {
    let (mut s, id) = model(true);
    let target = DimensionTarget::Feature { feature: id, key: "d2".into() };
    let edit = s.dimension_set(&target, Some(&5.0.into()), Some("other_leg")).unwrap();
    let value = serde_json::to_value(&edit.dimension).unwrap();
    assert_eq!(value["unit"], "mm");
    assert_eq!(value["mode"], "two_dist");
    assert_close(s.result_bodies()[0].volume, 12000.0 - 2.0 * 5.0 / 2.0 * 10.0, 1e-6, "two-distance edited volume");
}

#[test]
fn symmetric_chamfer_cannot_edit_or_name_ignored_d2() {
    let (mut s, id) = model(false);
    let target = DimensionTarget::Feature { feature: id, key: "d2".into() };
    let path = scratch("a2-symmetric-d2-before.qcad");
    s.save(Some(&path)).unwrap();
    let before = std::fs::read(&path).unwrap();
    for (value, name) in [(Some(Num::Value(5.0)), None), (None, Some("ignored_other"))] {
        let error = s.dimension_set(&target, value.as_ref(), name).expect_err("symmetric has no effective d2");
        assert!(error.to_string().contains("symmetric"), "{error}");
        s.save(Some(&path)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before, "ignored-field refusal restores exact archive");
    }
}

#[test]
fn manual_reference_checks_every_selected_edge_before_the_kernel() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":10})).unwrap()).unwrap();
    let t = s.topology(None, true).unwrap();
    // Selections are sorted by persistent id. Ensure the first edge is adjacent and the
    // later edge is not: an implementation checking only the first must fail this oracle.
    let (face, first, second) = t
        .faces
        .iter()
        .find_map(|face| {
            t.edges.iter().find_map(|first| {
                if !first.faces.unwrap().contains(&face.id) {
                    return None;
                }
                t.edges
                    .iter()
                    .find(|second| second.id > first.id && !second.faces.unwrap().contains(&face.id))
                    .map(|second| (face.id, first.id, second.id))
            })
        })
        .unwrap();
    let path = scratch("a2-every-edge.qcad");
    s.save(Some(&path)).unwrap();
    let before = std::fs::read(&path).unwrap();
    let error = s
        .chamfer_ex(&Chamfer {
            body: None,
            edges: Sel::Ids(vec![first, second]),
            dist: 2.0.into(),
            d2: None,
            angle: Some(30.0.into()),
            reference_face: Some(Sel::Ids(vec![face])),
            flip: false,
            name: None,
        })
        .unwrap_err();
    let text = error.to_string();
    assert!(text.contains("not adjacent") && text.contains(&second.to_string()), "each edge must be checked explicitly: {text}");
    s.save(Some(&path)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
