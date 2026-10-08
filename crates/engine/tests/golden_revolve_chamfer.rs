//! The native cone-frame handedness issue, including the real acceptance shelf.
mod common;
use common::*;
use qymcad_engine::*;
use std::f64::consts::PI;
use std::path::Path;

fn conical_seat(sign: f64) -> Session {
    let mut s = Session::new_part();
    let stock = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(stock, &0.0.into(), &0.0.into(), &30.0.into(), &30.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch: stock,
        profiles: None,
        height: 10.0.into(),
        op: Op::Add,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XZ), None).unwrap();
    // r(z)=3.25-(3.25-2)*z/10; overrun by 1 mm preserves the intended taper.
    s.sketch_polyline(
        sketch,
        &PolylineSpec {
            points: vec![
                Xy(0.0.into(), (-1.0).into()),
                Xy((sign * 3.375).into(), (-1.0).into()),
                Xy((sign * 1.875).into(), 11.0.into()),
                Xy(0.0.into(), 11.0.into()),
            ],
            closed: true,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    let axis = s
        .sketch_line(
            sketch,
            &LineSpec { x1: 0.0.into(), y1: (-2.0).into(), x2: 0.0.into(), y2: 12.0.into(), construction: true, dimensioned: true },
        )
        .unwrap();
    s.revolve(&Revolve {
        sketch,
        profiles: None,
        axis: AxisRef::Line(axis.entities[0]),
        angle: 360.0.into(),
        direction: Direction::Normal,
        op: Op::Cut,
        target: None,
        name: None,
    })
    .unwrap();
    s
}

#[test]
fn centered_conical_seat_chamfers_from_both_profile_sides() {
    for sign in [1.0, -1.0] {
        let mut s = conical_seat(sign);
        // Frustum V=pi*h*(R²+Rr+r²)/3, subtracted from the rectangular stock.
        let unchamfered = 30.0 * 30.0 * 10.0 - PI * 10.0 * (3.25_f64.powi(2) + 3.25 * 2.0 + 2.0_f64.powi(2)) / 3.0;
        assert_close(s.result_bodies()[0].volume, unchamfered, 1e-5, "seat frustum cut");
        let ids = s.topology(None, false).unwrap().edges.iter().filter(|e| e.kind == EdgeKind::Circle).map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(ids.len(), 2);
        s.chamfer(None, &Sel::Ids(ids), &0.5.into(), None, None).unwrap();
        // Setback c along the cone gives axial reach a=c/sqrt(1+k²), k=(R-r)/h.
        // At inward distance t each bevel increases radius by c*(1-t/a). Integrating
        // pi*((r(t)+delta)²-r(t)²) gives pi*(Rm*c*a+m*c*a²/3+c²*a/3).
        // The two rims have m=±k, so their slope terms cancel.
        let c = 0.5;
        let a = c / (1.0_f64 + ((3.25_f64 - 2.0) / 10.0).powi(2)).sqrt();
        let removed = PI * c * a * (3.25 + 2.0 + 2.0 * c / 3.0);
        assert_close(s.result_bodies()[0].volume, unchamfered - removed, 2e-5, "two conical mouth bevels");
    }
}

fn shelf_without_mouth_chamfer(reflect: bool, reverse_axis: bool) -> Session {
    let fixture = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hanging_shelf.qcad"));
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(fixture.to_str().unwrap()).unwrap();
    project.ensure_document();
    // Remove only the last modifier; body 122 is the shelf with all four seat cuts.
    project.timeline.retain(|n| n.id != 123);
    project.bodies.retain(|b| b.id != 123);
    for sk in project.sketches.iter_mut().filter(|sk| sk.id == 61 || sk.id == 62) {
        // The fixture's driving dimensions encode the original point coordinates. This
        // experimental native recipe retains fixed geometry, so clear those dimensions
        // before reflecting the profile; do not ask the solver to restore its old side.
        sk.constraints.clear();
        if reflect {
            for pt in &mut sk.points {
                // Reflect around each x=±119 seat axis; origin/frame points stay put.
                if pt.x > 100.0 {
                    pt.x = 2.0 * 119.0 - pt.x;
                } else if pt.x < -100.0 {
                    pt.x = -2.0 * 119.0 - pt.x;
                }
            }
        }
        if reverse_axis {
            for e in sk.entities.iter_mut().filter(|e| e.construction) {
                if let qymcad_core::model::EntityKind::Line { a, b } = &mut e.kind {
                    std::mem::swap(a, b);
                }
            }
        }
    }
    project.mark_all_dirty();
    let shapes = breps.into_iter().filter_map(|(id, b)| qymcad_kernel::Shape::from_brep_bytes(&b).map(|s| (id, s))).collect();
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, shapes);
    assert!(report.errors.is_empty(), "upstream recipe regeneration: {:?}", report.errors);
    for (id, faces) in &report.built {
        project.set_body_faces(*id, faces.clone());
    }
    let saved_breps = shapes.iter().map(|(&id, shape)| (id, shape.to_brep_bytes().unwrap())).collect::<Vec<_>>();
    let path = scratch(&format!("h3-shelf-{reflect}-{reverse_axis}.qcad"));
    qymcad_io::save_project_with_brep(&project, path.to_str().unwrap(), &saved_breps).unwrap();
    let (s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    s
}

fn mouth_circles(s: &Session) -> Vec<u32> {
    s.shape(122)
        .unwrap()
        .edges_info()
        .into_iter()
        .filter(|e| {
            e.circle.is_some_and(|(center, _, radius)| {
                (center[0].abs() - 119.0).abs() < 1e-5 && (center[1].abs() - 70.0).abs() < 1e-5 && radius < 4.0
            })
        })
        .map(|e| e.id)
        .collect()
}

#[test]
fn upstream_negative_side_shelf_refuses_each_mouth_but_axis_reversal_recovers() {
    let original = shelf_without_mouth_chamfer(false, false);
    let negative = shelf_without_mouth_chamfer(true, false);
    let mut recovered = shelf_without_mouth_chamfer(true, true);
    // Full-turn reflection and axis reversal preserve r(z) and therefore the complete
    // solid before chamfer; this is a geometric invariance, not an observed numeric reference.
    assert_close(
        negative.result_bodies()[0].volume,
        original.result_bodies()[0].volume,
        1e-5,
        "full-turn profile reflection preserves volume",
    );
    assert_close(recovered.result_bodies()[0].volume, original.result_bodies()[0].volume, 1e-5, "full-turn axis reversal preserves volume");
    let ids = mouth_circles(&negative);
    assert_eq!(ids.len(), 8);
    // This bypasses the engine error wording and proves the pinned upstream failure.
    let shape = negative.shape(122).unwrap();
    assert!(shape.chamfer_edges(0.5, &ids, &[], &[], &[]).is_none());
    for &id in &ids {
        assert!(shape.chamfer_edges(0.5, &[id], &[], &[], &[]).is_none(), "native bevel of mouth {id}");
    }
    let ids = mouth_circles(&recovered);
    recovered.chamfer(None, &Sel::Ids(ids), &0.5.into(), None, None).unwrap();
    assert!(recovered.result_bodies()[0].volume < original.result_bodies()[0].volume);
}

#[test]
fn negative_side_revolve_chamfer_error_suggests_profile_or_axis_workaround() {
    let mut s = shelf_without_mouth_chamfer(true, false);
    let ids = mouth_circles(&s);
    let before = s.result_bodies()[0].volume;
    let error = s.chamfer(None, &Sel::Ids(ids), &0.5.into(), None, None).unwrap_err().to_string();
    for required in ["kernel's reason", "chamfer 0.50 too big", "larger x", "sketch's +y", "endpoints", "overlapping", "partial turn"] {
        assert!(error.contains(required), "failed revolve-mouth chamfer must explain {required}: {error}");
    }
    assert!(!error.contains("F-067") && !error.contains("negative side"), "{error}");
    assert_close(s.result_bodies()[0].volume, before, 1e-8, "failed chamfer rolls back geometry");
}
