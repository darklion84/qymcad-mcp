//! Reusing Move changes native named dimensions as well as the body's placement.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;
use std::{collections::BTreeMap, io::Read, path::Path};

fn shift(body: Id, dx: Num, dy: Num) -> Move {
    Move { body: Some(body), translation: Some([dx, dy, 0.0.into()]), rotation: None, name: None }
}

fn named_move() -> (Session, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &20.0.into(), false).unwrap();
    let (src, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":30})).unwrap()).unwrap();
    assert!(r.errors.is_empty());
    let (m, r) = s.move_body(&shift(src, 5.0.into(), 0.0.into())).unwrap();
    assert!(r.errors.is_empty());
    s.dimension_set(&DimensionTarget::Feature { feature: m.body, key: "dx".into() }, None, Some("MoveX")).unwrap();
    (s, m.body)
}

fn prism(s: &Session, id: Id, volume: f64, bbox: [f64; 6]) {
    let _gate = qymcad_kernel::kernel_gate();
    let shape = s.shape(id).unwrap();
    assert_close(shape.volume(), volume, 2e-5, "independent named-Move prism volume");
    for (actual, expected) in shape.bbox().unwrap().into_iter().zip(bbox) {
        assert_close(actual, expected, 3e-7, "independent named-Move prism position");
    }
}

fn archive(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut entry = zip.by_index(i).unwrap();
            let name = entry.name().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}

#[test]
fn reused_move_response_evaluates_cross_axis_formulas_in_the_updated_scope() {
    let (mut s, moved) = named_move();
    // The new dx is 6; dy references that same native named driver, so it is also 6.
    let (result, r) = s.move_body(&shift(moved, 1.0.into(), "MoveX".into())).unwrap();
    assert!(r.errors.is_empty());
    assert!(result.reused);
    assert_eq!(result.body, moved);
    prism(&s, moved, 10.0 * 20.0 * 30.0, [1.0, -4.0, 0.0, 11.0, 16.0, 30.0]);
    assert_close(result.transform[3], 6.0, 1e-12, "returned affine dx");
    assert_close(result.transform[7], 6.0, 1e-12, "returned affine dy must use the updated MoveX value");
}

#[test]
fn reused_move_propagates_its_named_dimension_to_independent_feature_and_sketch_consumers() {
    let (mut s, moved) = named_move();
    // These bodies have no source-body edge from Move. Only their formulas read MoveX.
    let feature_sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(feature_sk, &50.0.into(), &0.0.into(), &2.0.into(), &3.0.into(), false).unwrap();
    let (feature_body, _) =
        s.extrude(&serde_json::from_value(json!({"sketch":feature_sk,"height":"MoveX","op":"new_body"})).unwrap()).unwrap();
    let sketch_sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sketch_sk, &100.0.into(), &0.0.into(), &"MoveX".into(), &4.0.into(), false).unwrap();
    let (sketch_body, _) = s.extrude(&serde_json::from_value(json!({"sketch":sketch_sk,"height":2,"op":"new_body"})).unwrap()).unwrap();
    prism(&s, feature_body, 2.0 * 3.0 * 5.0, [49.0, -1.5, 0.0, 51.0, 1.5, 5.0]);
    prism(&s, sketch_body, 5.0 * 4.0 * 2.0, [97.5, -2.0, 0.0, 102.5, 2.0, 2.0]);

    let (result, r) = s.move_body(&shift(moved, 1.0.into(), 0.0.into())).unwrap();
    assert!(r.errors.is_empty());
    assert!(result.reused);
    prism(&s, moved, 6000.0, [1.0, -10.0, 0.0, 11.0, 10.0, 30.0]);
    let consumer_volumes = {
        let _gate = qymcad_kernel::kernel_gate();
        [s.shape(feature_body).unwrap().volume(), s.shape(sketch_body).unwrap().volume()]
    };
    assert_eq!(
        consumer_volumes.map(|v| (v * 1e6).round() as i64),
        [36_000_000, 48_000_000],
        "feature AND solved sketch consumers must follow MoveX 5→6"
    );
    prism(&s, feature_body, 2.0 * 3.0 * 6.0, [49.0, -1.5, 0.0, 51.0, 1.5, 6.0]);
    prism(&s, sketch_body, 6.0 * 4.0 * 2.0, [97.0, -2.0, 0.0, 103.0, 2.0, 2.0]);
}

#[test]
fn reused_move_refuses_invalid_named_consumer_and_restores_the_exact_archive() {
    let (mut s, moved) = named_move();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &50.0.into(), &0.0.into(), &2.0.into(), &3.0.into(), false).unwrap();
    let (dependent, _) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":"MoveX","op":"new_body"})).unwrap()).unwrap();
    prism(&s, dependent, 2.0 * 3.0 * 5.0, [49.0, -1.5, 0.0, 51.0, 1.5, 5.0]);
    let fillet_sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(fillet_sk, &100.0.into(), &0.0.into(), &4.0.into(), &6.0.into(), false).unwrap();
    let (fillet_stock, _) = s.extrude(&serde_json::from_value(json!({"sketch":fillet_sk,"height":10,"op":"new_body"})).unwrap()).unwrap();
    let topology = s.topology(Some(fillet_stock), true).unwrap();
    let edge = topology.edges.iter().find(|e| (e.a[2] - e.b[2]).abs() > 9.9).unwrap().id;
    let (fillet, _) = s.fillet(Some(fillet_stock), &Sel::Ids(vec![edge]), &"MoveX/5".into(), None).unwrap();
    {
        let _gate = qymcad_kernel::kernel_gate();
        assert_close(
            s.shape(fillet).unwrap().volume(),
            4.0 * 6.0 * 10.0 - 10.0 * (1.0 - std::f64::consts::PI / 4.0),
            2e-5,
            "independent one-corner R1 fillet volume",
        );
    }
    let path = scratch("move-named-consumer-refusal.qcad");
    s.save(Some(&path)).unwrap();
    let before = archive(&path);
    // MoveX=25 makes the independent fillet R5 on width4 stock fail during native rebuild.
    // The earlier independent extrusion can already have rebuilt from height5 to height25.
    assert!(s.move_body(&shift(moved, 20.0.into(), 0.0.into())).is_err());
    s.save(Some(&path)).unwrap();
    assert_eq!(archive(&path), before, "named-driver refusal restores recipe, all B-reps and diagnostics exactly");
    prism(&s, moved, 6000.0, [0.0, -10.0, 0.0, 10.0, 10.0, 30.0]);
    prism(&s, dependent, 30.0, [49.0, -1.5, 0.0, 51.0, 1.5, 5.0]);
}
