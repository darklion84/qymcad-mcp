//! P19: the six primitives: hand volumes, formulas, the app's field limits, union with the Part's body, placement, lifecycle.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

fn spec(v: serde_json::Value) -> PrimitiveSpec {
    let n = |k: &str| -> Num {
        match &v[k] {
            serde_json::Value::String(s) => Num::Expr(s.clone()),
            other => Num::Value(other.as_f64().unwrap()),
        }
    };
    match v["kind"].as_str().unwrap() {
        "box" => PrimitiveSpec::Box { dx: n("dx"), dy: n("dy"), dz: n("dz") },
        "cylinder" => PrimitiveSpec::Cylinder { r: n("r"), h: n("h") },
        "sphere" => PrimitiveSpec::Sphere { r: n("r") },
        "cone" => PrimitiveSpec::Cone { r1: n("r1"), r2: n("r2"), h: n("h") },
        "torus" => PrimitiveSpec::Torus { major: n("major"), minor: n("minor") },
        "prism" => PrimitiveSpec::Prism { r: n("r"), h: n("h"), n: n("n") },
        other => panic!("{other}"),
    }
}

fn make(s: &mut Session, v: serde_json::Value) -> PrimitiveInfo {
    s.primitive(&spec(v), &Placement::Origin, None).unwrap().0
}

fn snap(s: &Session) -> String {
    ron::to_string(s.project()).unwrap()
}

fn only_body(s: &Session) -> BodyInfo {
    let bodies = s.result_bodies();
    assert_eq!(bodies.len(), 1, "one body in the Part: {:?}", bodies.iter().map(|b| b.id).collect::<Vec<_>>());
    bodies[0].clone()
}

fn faces(s: &Session, body: u64) -> usize {
    s.project().regen_faces[&body].len()
}

fn assert_box(b: &BodyInfo, want: [f64; 6], tol: f64, what: &str) {
    for (i, (got, want)) in b.bbox_local.iter().zip(want).enumerate() {
        assert_close(*got, want, tol, &format!("{what} bbox[{i}]"));
    }
}

fn refused(s: &mut Session, v: serde_json::Value) -> String {
    let before = snap(s);
    let e = s.primitive(&spec(v.clone()), &Placement::Origin, None).unwrap_err().to_string();
    assert!(snap(s) == before, "a refused {v} changes nothing");
    e
}

use std::f64::consts::PI;

#[test]
fn each_primitive_has_the_hand_volume_box_and_faces() {
    // (spec, volume, bbox, tolerance of the tessellated bbox, faces)
    let cases = [
        (json!({"kind":"box","dx":30,"dy":20,"dz":10}), 6000.0, [-15.0, -10.0, 0.0, 15.0, 10.0, 10.0], 1e-3, 6),
        (json!({"kind":"cylinder","r":10,"h":20}), PI * 100.0 * 20.0, [-10.0, -10.0, 0.0, 10.0, 10.0, 20.0], 0.05, 3),
        (json!({"kind":"sphere","r":10}), 4.0 / 3.0 * PI * 1000.0, [-10.0, -10.0, -10.0, 10.0, 10.0, 10.0], 0.05, 1),
        (
            json!({"kind":"cone","r1":10,"r2":4,"h":20}),
            PI * 20.0 / 3.0 * (100.0 + 40.0 + 16.0),
            [-10.0, -10.0, 0.0, 10.0, 10.0, 20.0],
            0.1,
            3,
        ),
        (json!({"kind":"cone","r1":10,"r2":0,"h":20}), PI * 100.0 * 20.0 / 3.0, [-10.0, -10.0, 0.0, 10.0, 10.0, 20.0], 0.1, 2),
        (json!({"kind":"torus","major":12,"minor":4}), 2.0 * PI * PI * 12.0 * 16.0, [-16.0, -16.0, -4.0, 16.0, 16.0, 4.0], 0.1, 1),
        (
            json!({"kind":"prism","r":10,"h":20,"n":6}),
            6.0 * 0.5 * 100.0 * (PI / 3.0).sin() * 20.0,
            [-10.0, -8.6603, 0.0, 10.0, 8.6603, 20.0],
            1e-3,
            8,
        ),
        (
            json!({"kind":"prism","r":10,"h":20,"n":3}),
            3.0 * 0.5 * 100.0 * (2.0 * PI / 3.0).sin() * 20.0,
            [-5.0, -8.6603, 0.0, 10.0, 8.6603, 20.0],
            1e-3,
            5,
        ),
    ];
    for (v, volume, bbox, tol, face_count) in cases {
        let mut s = Session::new_part();
        let (info, r) = s.primitive(&spec(v.clone()), &Placement::Origin, None).unwrap();
        assert!(r.errors.is_empty(), "{v}: {:?}", r.errors);
        assert_eq!(
            (info.feature, info.joined.is_none(), info.placed_at),
            (info.body, true, None),
            "{v}: the first primitive is the Part's body"
        );
        let b = only_body(&s);
        assert_eq!(b.id, info.body);
        assert_close(b.volume, volume, volume * 1e-6, &format!("{v} volume"));
        assert_box(&b, bbox, tol, &format!("{v}"));
        assert_eq!(faces(&s, b.id), face_count, "{v} faces");
        assert_eq!(s.project().timeline.len(), 1, "{v}: one node for the first primitive");
    }
}

#[test]
fn sizes_are_reported_after_the_rebuild_and_formulas_follow_parameters() {
    let mut s = Session::new_part();
    s.param_set("w", &30.0.into()).unwrap();
    let (info, _) = s.primitive(&spec(json!({"kind":"box","dx":"w","dy":"w/2","dz":10})), &Placement::Origin, Some("slab")).unwrap();
    assert_eq!(info.name, "slab");
    assert_eq!((info.sizes["dx"], info.sizes["dy"], info.sizes["dz"]), (30.0, 15.0, 10.0));
    assert_eq!(s.project().feat_dim(info.feature, "dx"), Some("w"), "a formula stands on the node");
    assert_eq!(s.project().feat_dim(info.feature, "dz"), None, "a plain number clears the key, as the app's fields do");
    assert_close(only_body(&s).volume, 4500.0, 1e-6, "slab");
    s.param_set("w", &40.0.into()).unwrap();
    assert_close(only_body(&s).volume, 40.0 * 20.0 * 10.0, 1e-6, "dx and dy follow w");
    // dimension_set takes the same keys: a literal, then a formula.
    s.dimension_set(&DimensionTarget::Feature { feature: info.feature, key: "dz".into() }, Some(&25.0.into()), None).unwrap();
    assert_close(only_body(&s).volume, 40.0 * 20.0 * 25.0, 1e-6, "dz literal");
    s.dimension_set(&DimensionTarget::Feature { feature: info.feature, key: "dz".into() }, Some(&"w/4".into()), None).unwrap();
    assert_close(only_body(&s).volume, 40.0 * 20.0 * 10.0, 1e-6, "dz formula");
    // The Part's name for the node is in the timeline.
    assert!(s.info().timeline.iter().any(|n| n.id == info.feature && n.name == "slab"));
}

#[test]
fn the_prism_side_count_follows_dimension_set_and_a_formula() {
    let mut s = Session::new_part();
    s.param_set("k", &5.0.into()).unwrap();
    let (info, _) = s.primitive(&spec(json!({"kind":"prism","r":10,"h":20,"n":"k"})), &Placement::Origin, None).unwrap();
    let area = |n: f64| n * 0.5 * 100.0 * (2.0 * PI / n).sin();
    assert_close(only_body(&s).volume, area(5.0) * 20.0, 1e-6, "pentagon");
    assert_eq!(faces(&s, only_body(&s).id), 7);
    s.param_set("k", &8.0.into()).unwrap();
    assert_close(only_body(&s).volume, area(8.0) * 20.0, 1e-6, "octagon through k");
    // F-181: the core rounds and clamps silently; the server holds the app's field: whole, 3..=64, on every edit.
    for bad in [2.5, 2.0, 65.0, 0.0, -4.0, 3.5] {
        let before = snap(&s);
        let e = s.param_set("k", &bad.into()).unwrap_err().to_string();
        assert!(e.contains("prism n must be a whole number of sides in 3..=64"), "param {bad}: {e}");
        assert!(snap(&s) == before);
    }
    s.dimension_set(&DimensionTarget::Feature { feature: info.feature, key: "n".into() }, Some(&12.0.into()), None).unwrap();
    assert_close(only_body(&s).volume, area(12.0) * 20.0, 1e-6, "dodecagon through dimension_set");
    assert_eq!(faces(&s, only_body(&s).id), 14);
    for bad in [2.5, 2.0, 65.0, 0.0, -4.0, 3.5] {
        let before = snap(&s);
        let e = s
            .dimension_set(&DimensionTarget::Feature { feature: info.feature, key: "n".into() }, Some(&bad.into()), None)
            .unwrap_err()
            .to_string();
        assert!(e.contains("prism n must be a whole number of sides in 3..=64"), "{bad}: {e}");
        assert!(snap(&s) == before);
    }
    s.dimension_set(&DimensionTarget::Feature { feature: info.feature, key: "n".into() }, Some(&64.0.into()), None).unwrap();
    s.dimension_set(&DimensionTarget::Feature { feature: info.feature, key: "n".into() }, Some(&3.0.into()), None).unwrap();
    assert_close(only_body(&s).volume, area(3.0) * 20.0, 1e-6, "triangle");
}

#[test]
fn the_apps_field_limits_refuse_at_creation_with_the_document_unchanged() {
    let mut s = Session::new_part();
    s.param_set("w", &2.0.into()).unwrap();
    for (v, word) in [
        (json!({"kind":"box","dx":0.05,"dy":10,"dz":10}), "box dx must be within 0.1..=100000"),
        (json!({"kind":"box","dx":10,"dy":100001,"dz":10}), "box dy must be within 0.1..=100000"),
        (json!({"kind":"box","dx":10,"dy":10,"dz":-3}), "box dz must be within"),
        (json!({"kind":"box","dx":"w*100000","dy":10,"dz":10}), "box dx must be within"),
        (json!({"kind":"cylinder","r":0.04,"h":10}), "cylinder r must be within 0.05"),
        (json!({"kind":"cylinder","r":5,"h":0}), "cylinder h must be within 0.1"),
        (json!({"kind":"sphere","r":0}), "sphere r must be within 0.05"),
        (json!({"kind":"sphere","r":100001}), "sphere r must be within"),
        (json!({"kind":"cone","r1":0,"r2":5,"h":10}), "cone r1 must be within 0.01"),
        (json!({"kind":"cone","r1":5,"r2":-1,"h":10}), "cone r2 must be within 0"),
        (json!({"kind":"cone","r1":5,"r2":0,"h":0.05}), "cone h must be within 0.1"),
        (json!({"kind":"cone","r1":10,"r2":10,"h":10}), "cone r1 and r2 are equal"),
        (json!({"kind":"cone","r1":10,"r2":10.00000001,"h":10}), "cone r1 and r2 are equal"),
        (json!({"kind":"torus","major":0.05,"minor":0.01}), "torus major must be within 0.1"),
        (json!({"kind":"torus","major":12,"minor":0.05}), "torus minor must be within 0.1"),
        (json!({"kind":"torus","major":12,"minor":12}), "torus minor must be within 0.1..=11.999"),
        (json!({"kind":"torus","major":12,"minor":11.9995}), "torus minor must be within 0.1..=11.999"),
        (json!({"kind":"torus","major":12,"minor":15}), "torus minor must be within"),
        (json!({"kind":"prism","r":0.05,"h":10,"n":6}), "prism r must be within 0.1"),
        (json!({"kind":"prism","r":5,"h":0.05,"n":6}), "prism h must be within 0.1"),
        (json!({"kind":"prism","r":5,"h":10,"n":2}), "prism n must be a whole number"),
        (json!({"kind":"prism","r":5,"h":10,"n":2.5}), "prism n must be a whole number"),
        (json!({"kind":"prism","r":5,"h":10,"n":65}), "prism n must be a whole number"),
        (json!({"kind":"box","dx":"nope+1","dy":10,"dz":10}), "nope"),
        (json!({"kind":"box","dx":"1e400","dy":10,"dz":10}), "is not a finite number"),
    ] {
        let e = refused(&mut s, v.clone());
        assert!(e.contains(word), "{v}: {e}");
    }
    assert!(s.project().timeline.is_empty(), "nothing was created");
    // The edges of the fields are inside.
    for v in [
        json!({"kind":"box","dx":0.1,"dy":100000,"dz":0.1}),
        json!({"kind":"cylinder","r":0.05,"h":0.1}),
        json!({"kind":"sphere","r":0.05}),
        json!({"kind":"cone","r1":0.01,"r2":0,"h":0.1}),
        json!({"kind":"torus","major":12,"minor":11.99}),
    ] {
        let mut t = Session::new_part();
        let (info, r) = t.primitive(&spec(v.clone()), &Placement::Origin, None).unwrap();
        assert!(r.errors.is_empty(), "{v}: {:?}", r.errors);
        assert!(only_body(&t).volume > 0.0, "{v}");
        assert_eq!(info.feature, info.body);
    }
}

#[test]
fn the_fields_hold_on_every_edit_and_the_cones_first_radius_may_reach_zero_on_edit() {
    let mut s = Session::new_part();
    s.param_set("w", &30.0.into()).unwrap();
    let b = make(&mut s, json!({"kind":"box","dx":"w","dy":20,"dz":20}));
    let before = snap(&s);
    // Every one is stopped by the field, before the rebuild (the core's own zero check is never reached).
    for (bad, word) in [
        (0.05, "box dx must be within 0.1..=100000"),
        (0.0, "box dx must be within 0.1..=100000"),
        (-1.0, "box dx must be within 0.1..=100000"),
        (100001.0, "box dx must be within 0.1..=100000"),
    ] {
        let e = s.param_set("w", &bad.into()).unwrap_err().to_string();
        assert!(e.contains(word), "param {bad}: {e}");
        assert!(snap(&s) == before, "a refused param_set changes nothing");
        let e = s
            .dimension_set(&DimensionTarget::Feature { feature: b.feature, key: "dy".into() }, Some(&bad.into()), None)
            .unwrap_err()
            .to_string();
        assert!(e.contains("box dy must be within 0.1..=100000"), "dimension {bad}: {e}");
        assert!(snap(&s) == before, "a refused dimension_set changes nothing");
    }
    s.param_set("w", &0.1.into()).unwrap();
    assert_close(only_body(&s).volume, 0.1 * 400.0, 1e-9, "a box at the field's floor");
    // Torus: minor against major, on an edit of either.
    let mut t = Session::new_part();
    let tor = make(&mut t, json!({"kind":"torus","major":12,"minor":4}));
    let before = snap(&t);
    let e = t
        .dimension_set(&DimensionTarget::Feature { feature: tor.feature, key: "minor".into() }, Some(&11.9995.into()), None)
        .unwrap_err()
        .to_string();
    assert!(e.contains("torus minor must be within 0.1..=11.999"), "{e}");
    let e = t
        .dimension_set(&DimensionTarget::Feature { feature: tor.feature, key: "major".into() }, Some(&4.0.into()), None)
        .unwrap_err()
        .to_string();
    assert!(e.contains("torus minor must be within"), "{e}");
    assert!(snap(&t) == before);
    t.dimension_set(&DimensionTarget::Feature { feature: tor.feature, key: "minor".into() }, Some(&11.99.into()), None).unwrap();
    assert_close(
        only_body(&t).volume,
        2.0 * PI * PI * 12.0 * 11.99 * 11.99,
        2.0 * PI * PI * 12.0 * 11.99 * 11.99 * 1e-6,
        "a fat torus is a hand volume",
    );
    // Cone: creation holds r1 >= 0.01, an edit may bring it to 0 (the app's edit field).
    let mut c = Session::new_part();
    let cone = make(&mut c, json!({"kind":"cone","r1":10,"r2":4,"h":20}));
    c.dimension_set(&DimensionTarget::Feature { feature: cone.feature, key: "r1".into() }, Some(&0.0.into()), None).unwrap();
    assert_close(only_body(&c).volume, PI * 20.0 / 3.0 * 16.0, 1e-3, "r1 = 0: an apex at the bottom");
    let before = snap(&c);
    let e = c
        .dimension_set(&DimensionTarget::Feature { feature: cone.feature, key: "r2".into() }, Some(&0.0.into()), None)
        .unwrap_err()
        .to_string();
    assert!(e.contains("cone r1 and r2 are equal"), "{e}");
    c.dimension_set(&DimensionTarget::Feature { feature: cone.feature, key: "r1".into() }, Some(&4.0.into()), None).unwrap_err();
    assert!(snap(&c) == before, "equal radii (F-182) and the double zero change nothing");
}

#[test]
fn a_second_primitive_unites_with_the_parts_body_and_an_apart_one_stays_one_body_of_two_pieces() {
    let mut s = Session::new_part();
    let a = make(&mut s, json!({"kind":"box","dx":20,"dy":20,"dz":20}));
    let (b, r) = s.primitive(&spec(json!({"kind":"sphere","r":5})), &Placement::Origin, None).unwrap();
    assert!(r.errors.is_empty());
    assert_eq!(b.joined, Some(Joined { prior: a.body, union: b.body }));
    assert_ne!(b.feature, b.body);
    let body = only_body(&s);
    assert_eq!(body.id, b.body);
    assert_close(body.volume, 8000.0 + 2.0 / 3.0 * PI * 125.0, 1e-3, "the box and the half of the sphere outside it");
    assert_eq!(s.project().timeline.len(), 3, "box, sphere, union");
    // A cylinder far away: one body, the volumes add, the box spans both, nothing warns (the app's own reading).
    let mut p = Session::new_part();
    make(&mut p, json!({"kind":"box","dx":20,"dy":20,"dz":20}));
    let (point, _) = p.datum_point(&PointSpec::At([Num::Value(100.0), Num::Value(0.0), Num::Value(0.0)]), None).unwrap();
    let (far, r) = p.primitive(&spec(json!({"kind":"cylinder","r":5,"h":10})), &Placement::Point(point.id), None).unwrap();
    assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:?} {:?}", r.errors, r.warnings);
    assert_eq!(far.placed_at, Some([100.0, 0.0, 0.0]));
    let body = only_body(&p);
    assert_close(body.volume, 8000.0 + PI * 25.0 * 10.0, 1e-3, "two pieces, volumes add");
    assert_box(&body, [-10.0, -10.0, 0.0, 105.0, 10.0, 20.0], 0.05, "spans both pieces");
    assert_eq!(p.project().timeline.iter().filter(|n| matches!(n.kind, qymcad_core::feature::FeatureKind::Move { .. })).count(), 1);
    // Overlap order does not matter for the volume: box + cylinder + sphere in any order is the same body.
    let vol = |order: [usize; 3]| {
        let specs =
            [json!({"kind":"box","dx":20,"dy":20,"dz":20}), json!({"kind":"cylinder","r":3,"h":40}), json!({"kind":"sphere","r":5})];
        let mut s = Session::new_part();
        for i in order {
            make(&mut s, specs[i].clone());
        }
        only_body(&s).volume
    };
    assert_close(vol([0, 1, 2]), vol([2, 1, 0]), 1e-3, "union is commutative");
}

#[test]
fn placement_at_a_vertex_or_a_datum_point_is_a_pure_translation_without_association() {
    let mut s = Session::new_part();
    s.param_set("w", &10.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &Num::Expr("w".into()), &6.0.into(), false).unwrap();
    let body = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":4,"op":"new_body"})).unwrap()).unwrap().0;
    let topo = s.topology(Some(body), false).unwrap();
    let e = topo.edges.iter().find(|e| e.kind == EdgeKind::Line && (e.a[2] - e.b[2]).abs() > 1.0 && e.b[2] > 3.0).unwrap().clone();
    let corner = e.b;
    let (info, r) = s
        .primitive(
            &spec(json!({"kind":"box","dx":2,"dy":2,"dz":2})),
            &Placement::Vertex { edge: e.id, end: VertexEnd::End, body: None },
            Some("cube"),
        )
        .unwrap();
    assert!(r.errors.is_empty());
    assert_eq!(info.placed_at, Some(corner));
    let moves: Vec<_> =
        s.project().timeline.iter().filter(|n| matches!(n.kind, qymcad_core::feature::FeatureKind::Move { .. })).map(|n| n.id).collect();
    assert_eq!(moves.len(), 1, "one Move between the primitive and the union");
    assert_eq!(s.project().timeline.len(), 5, "sketch, block, cube, move, union");
    assert_eq!(info.joined.as_ref().map(|j| j.prior), Some(body));
    let b = only_body(&s);
    // The cube stands on the corner's z = 0 base and is centred on it: it is NOT a corner-anchored cube.
    assert_close(b.bbox_local[5], corner[2] + 2.0, 1e-3, "cube base lifted to the corner, top at corner z + 2");
    assert_close(b.bbox_local[3].max(b.bbox_local[0].abs()), corner[0].abs() + 1.0, 1e-3, "centred on the corner in x");
    // No association: the corner moves with w, the cube does not.
    let cube_box = |s: &Session| {
        let b = only_body(s);
        (b.bbox_local[3], b.volume)
    };
    s.param_set("w", &30.0.into()).unwrap();
    let (xmax, volume) = cube_box(&s);
    // The cube (z 4..6) only touches the block's top face: the volumes add exactly, the cube did not follow the corner.
    assert_close(volume, 30.0 * 6.0 * 4.0 + 2.0 * 2.0 * 2.0, 1e-3, "the cube stayed where it was placed; only the block grew");
    assert_close(xmax, 15.0, 1e-3, "the block's new face is the largest x; the cube did not follow the corner");
}

#[test]
fn refusals_of_placement_and_context_leave_the_document_untouched() {
    let mut s = Session::new_part();
    let a = make(&mut s, json!({"kind":"box","dx":20,"dy":20,"dz":20}));
    let edge = s.topology(Some(a.body), false).unwrap().edges[0].id;
    let before = snap(&s);
    let spec_box = spec(json!({"kind":"box","dx":2,"dy":2,"dz":2}));
    let e = s.primitive(&spec_box, &Placement::Point(9999), None).unwrap_err().to_string();
    assert!(e.contains("datum point 9999"), "{e}");
    let e = s.primitive(&spec_box, &Placement::Vertex { edge: 99999, end: VertexEnd::Start, body: None }, None).unwrap_err().to_string();
    assert!(e.contains("edge 99999"), "{e}");
    let e = s.primitive(&spec_box, &Placement::Vertex { edge, end: VertexEnd::Start, body: Some(12345) }, None).unwrap_err().to_string();
    assert!(e.contains("12345"), "{e}");
    assert!(snap(&s) == before, "a refusal changes nothing");
    // A datum point of another Part is in another frame.
    let (point, _) = s.datum_point(&PointSpec::At([Num::Value(1.0), Num::Value(2.0), Num::Value(3.0)]), None).unwrap();
    s.part_new("second").unwrap();
    let before = snap(&s);
    let e = s.primitive(&spec_box, &Placement::Point(point.id), None).unwrap_err().to_string();
    assert!(e.contains("another Part"), "{e}");
    assert!(snap(&s) == before);
    // With the assembly root active the app would make a Part: not covered.
    s.part_select(&PartRef::Root { root: true }).unwrap();
    let before = snap(&s);
    let e = s.primitive(&spec_box, &Placement::Origin, None).unwrap_err().to_string();
    assert!(e.contains("the active context is an assembly"), "{e}");
    assert!(snap(&s) == before);
}

#[test]
fn delete_suppress_reorder_and_rollback_with_a_united_primitive() {
    let make_two = || {
        let mut s = Session::new_part();
        let a = make(&mut s, json!({"kind":"box","dx":20,"dy":20,"dz":20}));
        let (point, _) = s.datum_point(&PointSpec::At([Num::Value(5.0), Num::Value(0.0), Num::Value(0.0)]), None).unwrap();
        let (b, _) = s.primitive(&spec(json!({"kind":"sphere","r":5})), &Placement::Point(point.id), None).unwrap();
        (s, a, b)
    };
    let timeline = |s: &Session| s.project().timeline.iter().map(|n| n.id).collect::<Vec<_>>();
    // Delete: the union (and the move) depend on the primitive; without cascade it is refused, with it the chain goes.
    let (mut s, a, b) = make_two();
    let ids = timeline(&s);
    let before = snap(&s);
    let e = s.feature_delete(b.feature, false).unwrap_err().to_string();
    assert!(e.contains("has dependents"), "{e}");
    assert!(snap(&s) == before);
    s.feature_delete(b.feature, true).unwrap();
    let remaining = timeline(&s);
    assert!(remaining.contains(&a.feature) && !remaining.contains(&b.feature) && !remaining.contains(&b.body), "{ids:?} -> {remaining:?}");
    assert_close(only_body(&s).volume, 8000.0, 1e-6, "the box alone again");
    // Deleting the first primitive takes the union built on it; the second primitive (and its move) stays as the Part's body.
    let (mut s, a, b) = make_two();
    s.feature_delete(a.feature, true).unwrap();
    assert!(!timeline(&s).contains(&a.feature) && !timeline(&s).contains(&b.body) && timeline(&s).contains(&b.feature));
    assert_close(only_body(&s).volume, 4.0 / 3.0 * PI * 125.0, 1e-3, "the sphere alone");
    // F-183: suppressing the united primitive unbuilds the union too, so the Part's body disappears (reported in `unbuilt`).
    let (mut s, _, b) = make_two();
    let r = s.feature_suppress(b.feature, true).unwrap();
    assert!(r.unbuilt.contains(&b.feature) && r.unbuilt.contains(&b.body), "{:?}", r.unbuilt);
    assert!(s.result_bodies().is_empty(), "the union is unbuilt and the box was consumed by it");
    s.feature_suppress(b.feature, false).unwrap();
    assert_eq!(s.result_bodies().len(), 1);
    // Reorder: the sphere ahead of the box (the Move follows it by its input, the point does not lead it): the same body.
    let (mut s, a, b) = make_two();
    let volume = only_body(&s).volume;
    let r = s.feature_reorder(b.feature, "up").unwrap();
    assert!(r.changed && r.order.iter().position(|id| *id == b.feature) < r.order.iter().position(|id| *id == a.feature), "{:?}", r.order);
    assert_close(only_body(&s).volume, volume, 1e-6, "union order does not change the body");
    // Rollback: a primitive made under the bar lands there, and rolling to the end keeps one body with all three volumes.
    let (mut s, a, _) = make_two();
    s.history_rollback(Some(a.feature), None).unwrap();
    assert_close(only_body(&s).volume, 8000.0, 1e-6, "the box only, under the bar");
    s.primitive(&spec(json!({"kind":"cylinder","r":3,"h":40})), &Placement::Origin, None).unwrap();
    s.history_rollback(None, Some("end")).unwrap();
    let rolled = only_body(&s).volume;
    let mut t = Session::new_part();
    for v in [json!({"kind":"box","dx":20,"dy":20,"dz":20}), json!({"kind":"cylinder","r":3,"h":40})] {
        make(&mut t, v);
    }
    let (point, _) = t.datum_point(&PointSpec::At([Num::Value(5.0), Num::Value(0.0), Num::Value(0.0)]), None).unwrap();
    t.primitive(&spec(json!({"kind":"sphere","r":5})), &Placement::Point(point.id), None).unwrap();
    assert_close(rolled, only_body(&t).volume, 1e-3, "box, cylinder and sphere: the same body in either order of making");
}

#[test]
fn a_parameter_under_a_primitive_formula_cannot_be_deleted_and_a_copy_or_instance_follows_the_source() {
    let mut s = Session::new_part();
    s.param_set("w", &30.0.into()).unwrap();
    make(&mut s, json!({"kind":"box","dx":"w","dy":20,"dz":20}));
    let e = s.param_delete("w").unwrap_err().to_string();
    assert!(e.contains("is used by feature"), "{e}");
    let part = s.project().components.iter().find(|c| c.kind == qymcad_core::feature::ComponentKind::Part).unwrap().id;
    s.part_copy(&PartRef::Id(part), "copy", None, None).unwrap();
    s.part_instance(&PartRef::Id(part), "inst", None, None).unwrap();
    assert_eq!(s.result_bodies().len(), 3);
    s.param_set("w", &10.0.into()).unwrap();
    for b in s.result_bodies() {
        assert_close(b.volume, 10.0 * 400.0, 1e-6, "every copy follows the global parameter");
    }
}

#[test]
fn a_primitive_survives_save_and_open_with_its_formulas() {
    let mut s = Session::new_part();
    s.param_set("w", &30.0.into()).unwrap();
    make(&mut s, json!({"kind":"cone","r1":"w/3","r2":4,"h":"w"}));
    make(&mut s, json!({"kind":"prism","r":5,"h":10,"n":5}));
    let volume = only_body(&s).volume;
    let path = scratch("primitives.qcad");
    s.save(Some(&path)).unwrap();
    let (mut loaded, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:?} {:?}", r.errors, r.warnings);
    assert_close(only_body(&loaded).volume, volume, 1e-6, "the same body");
    loaded.param_set("w", &60.0.into()).unwrap();
    let cone = PI * 60.0 / 3.0 * (20.0 * 20.0 + 20.0 * 4.0 + 16.0);
    let prism = 5.0 * 0.5 * 25.0 * (2.0 * PI / 5.0).sin() * 10.0;
    let b = only_body(&loaded);
    assert!(b.volume > cone && b.volume < cone + prism + 1.0, "the cone follows w, the prism is inside it: {}", b.volume);
}

#[test]
fn in_a_part_with_several_bodies_the_primitive_unites_with_the_last_current_one_only() {
    // The app's Part holds one body; the server's extrude `new_body` can make several. `finish_base_body` takes the LAST
    // current body of the Part as the one to unite with (F-184).
    let mut s = Session::new_part();
    let mut ids = Vec::new();
    for x in [0.0, 100.0] {
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sk, &x.into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
        ids.push(s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":4,"op":"new_body"})).unwrap()).unwrap().0);
    }
    let (info, r) = s.primitive(&spec(json!({"kind":"box","dx":2,"dy":2,"dz":2})), &Placement::Origin, None).unwrap();
    assert!(r.errors.is_empty());
    assert_eq!(info.joined.as_ref().map(|j| j.prior), Some(ids[1]), "united with the second block, the last one");
    let bodies = s.result_bodies();
    assert_eq!(bodies.len(), 2);
    assert!(bodies.iter().any(|b| b.id == ids[0]), "the first block is untouched");
    let united = bodies.iter().find(|b| b.id == info.body).unwrap();
    // The cube stands at the Part's origin, 100 mm from the second block: two pieces, both volumes.
    assert_close(united.volume, 400.0 + 8.0, 1e-3, "the second block and the cube");
}

#[test]
fn a_primitive_over_a_prior_body_that_is_not_built_is_refused_f185() {
    let spec_cyl = spec(json!({"kind":"cylinder","r":3,"h":40}));
    // The Part's only body is suppressed: the app would unite with it and give an empty result without a word.
    let mut s = Session::new_part();
    let a = make(&mut s, json!({"kind":"box","dx":20,"dy":20,"dz":20}));
    s.feature_suppress(a.feature, true).unwrap();
    let before = snap(&s);
    let e = s.primitive(&spec_cyl, &Placement::Origin, None).unwrap_err().to_string();
    assert!(e.contains("is not built") && e.contains("F-185"), "{e}");
    assert!(snap(&s) == before, "a refusal changes nothing");
    s.feature_suppress(a.feature, false).unwrap();
    let (info, r) = s.primitive(&spec_cyl, &Placement::Origin, None).unwrap();
    assert!(r.errors.is_empty() && info.joined.is_some());
    assert_close(only_body(&s).volume, 8000.0 + PI * 9.0 * 20.0, 1e-3, "the union is built once the prior is");
    // The F-183 chain: the united primitive is suppressed, so the Part's last body (the union) is not built either.
    let mut s = Session::new_part();
    make(&mut s, json!({"kind":"box","dx":20,"dy":20,"dz":20}));
    let (b, _) = s.primitive(&spec(json!({"kind":"sphere","r":5})), &Placement::Origin, None).unwrap();
    s.feature_suppress(b.feature, true).unwrap();
    assert!(s.result_bodies().is_empty());
    let before = snap(&s);
    let e = s.primitive(&spec_cyl, &Placement::Origin, None).unwrap_err().to_string();
    assert!(e.contains("is not built") && e.contains("F-185"), "{e}");
    assert!(snap(&s) == before);
}

#[test]
fn the_first_placed_primitive_is_its_move_and_a_point_beyond_the_rollback_bar_is_refused() {
    let mut s = Session::new_part();
    let (point, _) = s.datum_point(&PointSpec::At([Num::Value(10.0), Num::Value(0.0), Num::Value(0.0)]), None).unwrap();
    let (info, r) = s.primitive(&spec(json!({"kind":"sphere","r":5})), &Placement::Point(point.id), None).unwrap();
    assert!(r.errors.is_empty());
    // Three variants of `body`: the primitive (unplaced, first), the Move (placed, Part empty), the union (a prior body).
    assert_ne!(info.body, info.feature, "placed: the body is the Move");
    assert!(info.joined.is_none(), "nothing to unite with");
    assert!(matches!(
        s.project().timeline.iter().find(|n| n.id == info.body).unwrap().kind,
        qymcad_core::feature::FeatureKind::Move { .. }
    ));
    let b = only_body(&s);
    assert_eq!(b.id, info.body);
    assert_close(b.volume, 4.0 / 3.0 * PI * 125.0, 1e-3, "the sphere");
    assert_box(&b, [5.0, -5.0, -5.0, 15.0, 5.0, 5.0], 0.05, "moved to the point");
    // A datum point after the rollback bar is not in the active history.
    let mut s = Session::new_part();
    let a = make(&mut s, json!({"kind":"box","dx":20,"dy":20,"dz":20}));
    let (late, _) = s.datum_point(&PointSpec::At([Num::Value(50.0), Num::Value(0.0), Num::Value(0.0)]), None).unwrap();
    s.history_rollback(Some(a.feature), None).unwrap();
    let before = snap(&s);
    let e = s.primitive(&spec(json!({"kind":"box","dx":2,"dy":2,"dz":2})), &Placement::Point(late.id), None).unwrap_err().to_string();
    assert!(e.contains("beyond the rollback bar"), "{e}");
    assert!(snap(&s) == before);
    s.history_rollback(None, Some("end")).unwrap();
    s.primitive(&spec(json!({"kind":"box","dx":2,"dy":2,"dz":2})), &Placement::Point(late.id), None).unwrap();
}

#[test]
fn a_vertex_the_apps_click_cannot_reach_is_refused() {
    // The app's vertex pick skips closed edges (a circle's single seam vertex) and smooth junctions.
    let mut s = Session::new_part();
    let c = make(&mut s, json!({"kind":"cylinder","r":10,"h":20}));
    let topo = s.topology(Some(c.body), false).unwrap();
    let circle = topo.edges.iter().find(|e| e.kind == EdgeKind::Circle).expect("the cylinder's rim");
    let before = snap(&s);
    let e = s
        .primitive(
            &spec(json!({"kind":"box","dx":2,"dy":2,"dz":2})),
            &Placement::Vertex { edge: circle.id, end: VertexEnd::Start, body: None },
            None,
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("is closed") && e.contains("vertex pick skips"), "{e}");
    assert!(!e.contains("datum point's"), "{e}");
    // The cylinder's seam line is a smooth junction of its face with itself.
    let seam = topo.edges.iter().find(|e| e.kind == EdgeKind::Line).expect("the seam line");
    let e = s
        .primitive(
            &spec(json!({"kind":"box","dx":2,"dy":2,"dz":2})),
            &Placement::Vertex { edge: seam.id, end: VertexEnd::End, body: None },
            None,
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("smooth, tangent junction") && e.contains("vertex pick skips"), "{e}");
    assert!(snap(&s) == before, "refusals change nothing");
    // A body of another Part: the refusal does not speak of a datum point.
    s.part_new("second").unwrap();
    let e = s
        .primitive(
            &spec(json!({"kind":"box","dx":2,"dy":2,"dz":2})),
            &Placement::Vertex { edge: circle.id, end: VertexEnd::Start, body: Some(c.body) },
            None,
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("belongs to another Part") && !e.contains("datum point's"), "{e}");
}

#[test]
fn the_torus_and_prism_limits_take_the_fields_tolerance_at_creation() {
    let mut s = Session::new_part();
    // The field's upper bound for the tube is max(major - 0.001, 0.1): a tube of 0.1 on a ring of 0.1005 is inside.
    let (info, r) = s.primitive(&spec(json!({"kind":"torus","major":0.1005,"minor":0.1})), &Placement::Origin, None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_close(only_body(&s).volume, 2.0 * PI * PI * 0.1005 * 0.01, 1e-9, "the fat thin torus");
    assert_eq!(info.sizes["minor"], 0.1);
    // Float noise at the bounds is inside; a whole number within 1e-9 builds that many sides.
    let mut t = Session::new_part();
    t.param_set("k", &0.3.into()).unwrap();
    make(&mut t, json!({"kind":"box","dx":"k-0.2","dy":10,"dz":10}));
    assert_close(only_body(&t).volume, 0.1 * 100.0, 1e-9, "0.3 - 0.2 is the field's floor, within its tolerance");
    let mut p = Session::new_part();
    let (prism, _) = p.primitive(&spec(json!({"kind":"prism","r":10,"h":20,"n":"0.1*3*10"})), &Placement::Origin, None).unwrap();
    assert_close(only_body(&p).volume, 3.0 * 0.5 * 100.0 * (2.0 * PI / 3.0).sin() * 20.0, 1e-6, "three sides");
    assert_close(prism.sizes["n"], 3.0, 1e-9, "n as evaluated");
}
