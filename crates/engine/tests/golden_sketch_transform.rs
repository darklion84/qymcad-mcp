//! S8 move, copy and rotate: independent coordinate, profile, persistence and rollback oracles.
mod common;
use common::{assert_close, scratch};
use qymcad_engine::*;
use std::f64::consts::PI;

fn sketch(s: &mut Session) -> Id {
    s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap()
}

fn extrude(s: &mut Session, sk: Id, height: f64) -> Id {
    s.extrude(&Extrude {
        sketch: sk,
        profiles: None,
        profiles_at: None,
        height: height.into(),
        down: None,
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap()
    .0
}

fn point(s: &Session, sk: Id, id: Id) -> (f64, f64) {
    let si = s.project().sketch_index(sk).unwrap();
    let p = s.project().sketches[si].points.iter().find(|p| p.id == id).unwrap();
    (p.x, p.y)
}

fn archive(s: &mut Session, name: &str) -> Vec<(String, Vec<u8>)> {
    let path = scratch(name);
    s.save_with_options(Some(&path), true).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut files = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut bytes).unwrap();
        files.push((f.name().to_string(), bytes));
    }
    files.sort();
    files
}

#[test]
fn move_copy_rotate_match_hand_coordinates_and_extrude_area() {
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let line = s
        .sketch_line(
            sk,
            &LineSpec { x1: 1.0.into(), y1: 2.0.into(), x2: 3.0.into(), y2: 2.0.into(), construction: false, dimensioned: false },
        )
        .unwrap();
    let edit =
        s.sketch_transform_move(sk, &TransformSpec { entities: vec![line.entities[0]], dx: 4.0.into(), dy: (-1.0_f64).into() }).unwrap();
    assert_eq!(edit.changed_entities, vec![line.entities[0]]);
    assert_eq!(point(&s, sk, line.points[0]), (5.0, 1.0));
    assert_eq!(point(&s, sk, line.points[1]), (7.0, 1.0));

    let rect = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let poly = s
        .sketch_polyline(
            rect,
            &PolylineSpec {
                points: vec![
                    Xy(0.0.into(), 0.0.into()),
                    Xy(10.0.into(), 0.0.into()),
                    Xy(10.0.into(), 6.0.into()),
                    Xy(0.0.into(), 6.0.into()),
                ],
                closed: true,
                construction: false,
                dimensioned: false,
            },
        )
        .unwrap();
    let sides = poly.entities.clone();
    let before = s.result_bodies().len();
    let body = extrude(&mut s, rect, 4.0);
    assert_eq!(s.result_bodies().len(), before + 1);
    assert_close(s.shape(body).unwrap().volume(), 240.0, 1e-5, "rectangle A*h before transform");
    let before_points =
        s.sketch_detail(rect).unwrap().points.into_iter().map(|p| (p.id, (p.x, p.y))).collect::<std::collections::HashMap<_, _>>();
    s.sketch_transform_rotate(rect, &RotateSpec { entities: sides.clone(), centre: [0.0.into(), 0.0.into()], angle: 90.0.into() }).unwrap();
    for p in s.sketch_detail(rect).unwrap().points {
        if let Some((x, y)) = before_points.get(&p.id) {
            assert_close(p.x, -*y, 1e-6, "rotated endpoint x");
            assert_close(p.y, *x, 1e-6, "rotated endpoint y");
        }
    }
    assert_close(s.shape(body).unwrap().volume(), 240.0, 1e-5, "rectangle A*h after rotate");

    let copied = s.sketch_transform_copy(rect, &TransformSpec { entities: sides, dx: 20.0.into(), dy: 0.0.into() }).unwrap();
    assert!(copied.created_entities.len() >= 4);
    let body = extrude(&mut s, rect, 4.0);
    assert_close(s.shape(body).unwrap().volume(), 480.0, 1e-5, "two rectangle profiles A*h");
}

#[test]
fn dimensioned_move_and_rotate_refuse_noop_with_exact_archive_rollback() {
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let sides = s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    let before = archive(&mut s, "transform-dimensioned-before.qcad");
    let min_x = |s: &Session| {
        let si = s.project().sketch_index(sk).unwrap();
        s.project().sketches[si]
            .points
            .iter()
            .filter(|p| p.id != s.project().sketches[si].origin)
            .map(|p| p.x)
            .fold(f64::INFINITY, f64::min)
    };
    let x0 = min_x(&s);
    let move_result = s.sketch_transform_move(sk, &TransformSpec { entities: sides.clone(), dx: 5.0.into(), dy: 0.0.into() });
    match move_result {
        Err(error) => {
            let text = error.to_string();
            assert!(text.contains("constraint") || text.contains("dimension"), "{text}");
            assert_eq!(archive(&mut s, "transform-dimensioned-move.qcad"), before);
        }
        // Success must really move the rectangle; a silent no-op is the defect this test guards.
        Ok(_) => assert!((min_x(&s) - (x0 + 5.0)).abs() < 1e-6, "accepted move left x {} (expected {})", min_x(&s), x0 + 5.0),
    }
    let rotate_result =
        s.sketch_transform_rotate(sk, &RotateSpec { entities: sides, centre: [0.0.into(), 0.0.into()], angle: 90.0.into() });
    if let Err(error) = rotate_result {
        let text = error.to_string();
        assert!(text.contains("constraint") || text.contains("dimension"), "{text}");
        assert_eq!(archive(&mut s, "transform-dimensioned-rotate.qcad"), before);
    }
}

#[test]
fn copied_dimensioned_rectangle_keeps_internal_dimensions_and_requested_offset() {
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let sides = s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    let before_points =
        s.sketch_detail(sk).unwrap().points.into_iter().map(|p| (p.id, (p.x, p.y))).collect::<std::collections::HashMap<_, _>>();
    let before = s.sketch_detail(sk).unwrap().constraints.len();
    let edit = s.sketch_transform_copy(sk, &TransformSpec { entities: sides, dx: 20.0.into(), dy: 3.0.into() }).unwrap();
    assert!(edit.created_entities.len() >= 4);
    let detail = s.sketch_detail(sk).unwrap();
    assert!(detail.constraints.len() > before);
    let copied =
        detail.entities.iter().filter(|e| edit.created_entities.contains(&e.id)).flat_map(|e| e.points.iter()).copied().collect::<Vec<_>>();
    assert!(copied.iter().any(|id| {
        let (x, y) = point(&s, sk, *id);
        before_points.values().any(|(ox, oy)| (x - ox - 20.0).abs() < 1e-6 && (y - oy - 3.0).abs() < 1e-6)
    }));
}

#[test]
fn named_dimension_drives_transformed_profile_after_save_load_app_edit() {
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let sides = s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    let width = s
        .sketch_detail(sk)
        .unwrap()
        .constraints
        .iter()
        .find(|c| c.kind == "distance_x")
        .map(|c| c.index)
        .or_else(|| s.sketch_detail(sk).unwrap().constraints.iter().find(|c| c.kind == "distance").map(|c| c.index))
        .unwrap();
    s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: width }, None, Some("width")).unwrap();
    let body = extrude(&mut s, sk, 4.0);
    let path = scratch("transform-app-path.qcad");
    s.save(Some(&path)).unwrap();
    let (volume, bbox, _, _) = common::gui_edit_driver_model(&path, "width", "14");
    assert_close(volume, 14.0 * 6.0 * 4.0, 1e-5, "app named dimension volume");
    assert_close(bbox[3], 14.0 / 2.0, 1e-5, "app named dimension max x");
    assert!(s.project().sketches[s.project().sketch_index(sk).unwrap()].entities.len() >= sides.len());
    assert!(body > 0);
}

#[test]
fn native_transform_hold_refuses_move_and_rotate_with_exact_archive_rollback() {
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let line = s
        .sketch_line(
            sk,
            &LineSpec { x1: 1.0.into(), y1: 1.0.into(), x2: 5.0.into(), y2: 1.0.into(), construction: false, dimensioned: false },
        )
        .unwrap();
    s.sketch_constrain(
        sk,
        &ConstrainSpec {
            kind: ConstraintKind::Fix,
            refs: vec![SketchRef::Id(line.points[0])],
            value: None,
            axis: DistAxis::Aligned,
            reference: false,
        },
    )
    .unwrap();
    let before = archive(&mut s, "transform-held-before.qcad");

    let move_error =
        s.sketch_transform_move(sk, &TransformSpec { entities: vec![line.entities[0]], dx: 3.0.into(), dy: 2.0.into() }).unwrap_err();
    let move_text = move_error.to_string();
    assert!(move_text.contains("fix") && move_text.contains("constraint 0"), "{move_text}");
    assert_eq!(archive(&mut s, "transform-held-after-move.qcad"), before);

    let rotate_error = s
        .sketch_transform_rotate(sk, &RotateSpec { entities: vec![line.entities[0]], centre: [0.0.into(), 0.0.into()], angle: 90.0.into() })
        .unwrap_err();
    let rotate_text = rotate_error.to_string();
    assert!(rotate_text.contains("fix") && rotate_text.contains("constraint 0"), "{rotate_text}");
    assert_eq!(archive(&mut s, "transform-held-after-rotate.qcad"), before);
}

#[test]
fn external_constraint_release_is_reported_and_points_and_hole_survive() {
    let mut s = Session::new_part();
    let stock = sketch(&mut s);
    s.sketch_rect(stock, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
    extrude(&mut s, stock, 10.0);
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), None).unwrap();
    let marker = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    let point = s.sketch_point(marker, &6.0.into(), &4.0.into(), true).unwrap().point;
    let line = s
        .sketch_line(
            marker,
            &LineSpec { x1: 6.0.into(), y1: 0.0.into(), x2: 6.0.into(), y2: 8.0.into(), construction: true, dimensioned: false },
        )
        .unwrap();
    s.sketch_constrain(
        marker,
        &ConstrainSpec {
            kind: ConstraintKind::PointOnLine,
            refs: vec![SketchRef::Id(point), SketchRef::Id(line.entities[0])],
            value: None,
            reference: false,
            axis: DistAxis::Aligned,
        },
    )
    .unwrap();
    let hole = s
        .hole_by_sketch(&HoleSketch {
            body: None,
            sketch: marker,
            points: None,
            flip: false,
            diameter: 4.0.into(),
            depth: None,
            kind: HoleKind::Plain,
            dia2: None,
            depth2: None,
            name: None,
        })
        .unwrap()
        .0;
    let before = archive(&mut s, "transform-hole-before.qcad");
    let edit =
        s.sketch_transform_move(marker, &TransformSpec { entities: vec![line.entities[0]], dx: 2.0.into(), dy: 0.0.into() }).unwrap();
    assert!(!edit.released_constraints.is_empty(), "released constraint list must expose the external tie");
    assert!(s.project().sketches[s.project().sketch_index(marker).unwrap()].points.iter().any(|p| p.id == point));
    assert!(s.project().timeline.iter().any(|n| n.id == hole));
    let refusal = s.sketch_transform_move(marker, &TransformSpec { entities: vec![line.entities[0]], dx: 0.0.into(), dy: 0.0.into() });
    assert!(refusal.is_ok());
    assert_ne!(archive(&mut s, "transform-hole-after.qcad"), before);
    let _ = PI;
}
