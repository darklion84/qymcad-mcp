mod common;
use common::assert_close;
use qymcad_engine::*;

fn rectangular_sketch(session: &mut Session, cx: f64) -> Id {
    let sketch = session.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    session.sketch_rect(sketch, &cx.into(), &0.0.into(), &20.0.into(), &16.0.into(), false).unwrap();
    sketch
}

fn extrusion(sketch: Id, op: Op) -> Extrude {
    Extrude { sketch, profiles: None, height: 8.0.into(), op, direction: Direction::Normal, through: false, target: None, name: None }
}

#[test]
fn unnamed_features_and_datum_planes_have_english_names() {
    let mut session = Session::new_part();
    let sketch = rectangular_sketch(&mut session, 0.0);
    let (body, report) = session.extrude(&extrusion(sketch, Op::Add)).unwrap();
    // Rectangular prism: V=20*16*8 mm³. Localizing names must preserve the modelling result.
    assert_close(report.bodies[0].volume, 20.0 * 16.0 * 8.0, 1e-6, "stock volume");
    assert_eq!(report.bodies[0].name, "Extrusion");
    assert_eq!(session.resolve("Extrusion").unwrap(), body, "displayed default names are valid references");
    assert_eq!(session.resolve("feat-name-extrude").unwrap(), body, "stored key references keep working");
    let (plane, _) = session.plane_offset(&PlaneRef::Base(BaseName::XY), &8.0.into(), None).unwrap();
    let top = session.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    assert_eq!(session.sketch_info(top).unwrap().plane, format!("Plane (plane {plane})"));
    let (_, hole) = session
        .hole(&Hole {
            body: None,
            face: Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 5.0 },
            at: None,
            diameter: 4.0.into(),
            depth: Some(8.0.into()),
            kind: HoleKind::Plain,
            dia2: None,
            depth2: None,
            name: None,
        })
        .unwrap();
    assert_eq!(hole.bodies[0].name, "Hole");
    assert!(session.info().timeline.iter().all(|n| !n.name.starts_with("feat-name-") && !n.name.starts_with("name-")));
    assert_eq!(
        session.project().timeline.iter().find(|n| n.id == body).unwrap().name,
        "feat-name-extrude",
        "presentation preserves recipes"
    );
}

#[test]
fn a_cut_that_misses_stock_has_an_english_reason_and_direction_hint() {
    let mut session = Session::new_part();
    let sketch = rectangular_sketch(&mut session, 0.0);
    session.extrude(&extrusion(sketch, Op::Add)).unwrap();
    let tool = rectangular_sketch(&mut session, 100.0);
    let before = session.result_bodies();
    let error = session.extrude(&extrusion(tool, Op::Cut)).unwrap_err().to_string();
    // Stock spans x∈[-10,10], tool x∈[90,110]: disjoint cuts remove exactly zero volume.
    assert!(error.contains("Cut with a sketch") && error.contains("The cut removed nothing"), "{error}");
    assert!(error.contains("the cut does not reach the body; check direction (reverse) or height"), "{error}");
    assert!(!error.contains("feat-name-") && !error.contains("error-cut-removed-nothing"), "{error}");
    assert_eq!(session.result_bodies(), before, "failed cut remains atomic");
}
