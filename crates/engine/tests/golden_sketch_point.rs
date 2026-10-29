//! Native isolated sketch points: independent coordinates and persistence.
mod common;
use common::{assert_close, scratch};
use qymcad_engine::*;

fn sketch(s: &mut Session) -> Id {
    s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap()
}
fn xy(p: &qymcad_core::model::Project, sk: Id, point: Id, x: f64, y: f64) {
    let p = p.sketches[p.sketch_index(sk).unwrap()].points.iter().find(|p| p.id == point).unwrap();
    assert_close(p.x, x, 1e-6, "point x");
    assert_close(p.y, y, 1e-6, "point y");
}
fn stock(s: &mut Session, sk: Id) {
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch: sk,
        profiles: None,
        height: 7.0.into(),
        down: None,
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap();
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
fn native_point_creation_reuse_and_isolated_inspection() {
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let p = s.sketch_point(sk, &8.0.into(), &6.0.into(), true).unwrap();
    assert!(!p.reused);
    let info = s.sketch_info(sk).unwrap();
    assert_eq!(info.dof, (0, 0));
    assert!(info.contours.is_empty());
    assert_eq!(info.isolated_points.len(), 1);
    assert!(info.isolated_points[0].dimensioned);
    assert_eq!(info.isolated_points[0].id, p.point);
    let before = serde_json::to_value(s.project()).unwrap();
    let reused = s.sketch_point(sk, &8.0000005.into(), &6.0.into(), true).unwrap();
    assert!(reused.reused);
    assert_eq!(reused.point, p.point);
    assert_eq!(serde_json::to_value(s.project()).unwrap(), before, "reuse adds no constraints or edits");
    let zero = s.sketch_point(sk, &0.0.into(), &0.0.into(), true).unwrap();
    assert!(!s.project().sketches[0].system_ids().contains(&zero.point));
    assert_eq!(
        s.sketch_detail(sk).unwrap().constraints.iter().filter(|c| c.kind == "point_on_line" && c.points.contains(&zero.point)).count(),
        2
    );
    let free = s.sketch_point(sk, &20.0.into(), &30.0.into(), false).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (2, 0));
    assert!(!s.sketch_info(sk).unwrap().isolated_points.iter().find(|p| p.id == free.point).unwrap().dimensioned);
    let line = s
        .sketch_line(
            sk,
            &LineSpec { x1: 40.0.into(), y1: 40.0.into(), x2: 50.0.into(), y2: 40.0.into(), construction: true, dimensioned: true },
        )
        .unwrap();
    let before = serde_json::to_value(s.project()).unwrap();
    let endpoint = s.sketch_point(sk, &40.0.into(), &40.0.into(), true).unwrap();
    assert!(endpoint.reused);
    assert_eq!(endpoint.point, line.points[0]);
    assert_eq!(serde_json::to_value(s.project()).unwrap(), before);
    assert!(!s.sketch_info(sk).unwrap().isolated_points.iter().any(|p| p.id == endpoint.point));
}

#[test]
fn isolated_points_do_not_create_profiles_or_material() {
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    s.sketch_point(sk, &20.0.into(), &30.0.into(), true).unwrap();
    assert!(s.contours_at(sk, &[[20.0.into(), 30.0.into()]]).is_err());
    assert!(s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: 7.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None
        })
        .is_err());
    assert!(s
        .revolve(&Revolve {
            sketch: sk,
            profiles: None,
            axis: AxisRef::SketchX,
            angle: 360.0.into(),
            direction: Direction::Normal,
            op: Op::NewBody,
            target: None,
            name: None
        })
        .is_err());
    stock(&mut s, sk);
    assert_close(s.result_bodies()[0].volume, 420.0, 1e-6, "isolated point adds no material");
    let mut r = Session::new_part();
    let sk = sketch(&mut r);
    r.sketch_rect(sk, &0.0.into(), &5.0.into(), &10.0.into(), &2.0.into(), false).unwrap();
    r.sketch_point(sk, &20.0.into(), &30.0.into(), true).unwrap();
    let (_, report) = r
        .revolve(&Revolve {
            sketch: sk,
            profiles: None,
            axis: AxisRef::SketchX,
            angle: 360.0.into(),
            direction: Direction::Normal,
            op: Op::NewBody,
            target: None,
            name: None,
        })
        .unwrap();
    assert_close(report.bodies[0].volume, std::f64::consts::PI * (36.0 - 16.0) * 10.0, 1e-5, "annular cylinder independent oracle");
}

#[test]
fn point_coordinates_have_unique_native_names_and_saved_app_drivers() {
    let mut s = Session::new_part();
    s.param_set("px", &8.0.into()).unwrap();
    s.param_set("py", &6.0.into()).unwrap();
    let sk = sketch(&mut s);
    let point = s.sketch_point(sk, &"px".into(), &"py".into(), true).unwrap().point;
    let dims: Vec<_> = s
        .sketch_detail(sk)
        .unwrap()
        .constraints
        .into_iter()
        .filter(|c| c.kind == "distance_point_line" && c.points.contains(&point))
        .collect();
    assert_eq!(dims.len(), 2);
    for (dim, name) in dims.iter().zip(["point_x", "point_y"]) {
        let target = DimensionTarget::Sketch { sketch: sk, constraint: dim.index };
        s.dimension_set(&target, None, Some(name)).unwrap();
    }
    stock(&mut s, sk);
    let path = scratch("point-app-drivers.qcad");
    s.save(Some(&path)).unwrap();
    let (loaded, _) = Session::open(&path).unwrap();
    xy(loaded.project(), sk, point, 8.0, 6.0);
    assert_eq!(loaded.info().named_dimensions.len(), 2);
    for (name, text, x, y) in [("px", "12", 12.0, 6.0), ("py", "10", 8.0, 10.0)] {
        let (v, _, _, p) = common::gui_edit_param_model(&path, name, text);
        xy(&p, sk, point, x, y);
        assert_close(v, 420.0, 1e-6, "point edit cannot change material");
    }
    for (name, text, x, y) in [("point_x", "14", 14.0, 6.0), ("point_y", "11", 8.0, 11.0)] {
        let (v, _, _, p) = common::gui_edit_driver_model(&path, name, text);
        xy(&p, sk, point, x, y);
        assert_close(v, 420.0, 1e-6, "named point edit cannot change material");
    }
}

#[test]
fn signed_zero_formula_coordinates_and_refusals_are_archive_atomic() {
    let mut s = Session::new_part();
    s.param_set("px", &(-8.0).into()).unwrap();
    s.param_set("py", &0.0.into()).unwrap();
    let sk = sketch(&mut s);
    let point = s.sketch_point(sk, &"px".into(), &"py".into(), true).unwrap().point;
    xy(s.project(), sk, point, -8.0, 0.0);
    assert_eq!(
        s.sketch_detail(sk).unwrap().constraints.iter().filter(|c| c.kind == "distance_point_line" && c.points.contains(&point)).count(),
        2,
        "formula zero stays editable"
    );
    s.param_set("px", &(-12.0).into()).unwrap();
    s.param_set("py", &4.0.into()).unwrap();
    xy(s.project(), sk, point, -12.0, 4.0);
    stock(&mut s, sk);
    let before = archive(&mut s, "point-before.qcad");
    assert!(s.param_set("px", &3.0.into()).is_err(), "native abs must not silently change coordinate meaning");
    assert_eq!(archive(&mut s, "point-after-sign.qcad"), before);
    assert!(s.sketch_point(sk, &Num::Value(f64::NAN), &1.0.into(), true).is_err());
    assert_eq!(archive(&mut s, "point-after-nan.qcad"), before);
}

#[test]
fn a_negative_coordinate_retains_its_side_after_touching_zero() {
    let mut s = Session::new_part();
    s.param_set("px", &(-8.0).into()).unwrap();
    s.param_set("py", &(-6.0).into()).unwrap();
    let sk = sketch(&mut s);
    let point = s.sketch_point(sk, &"px".into(), &"py".into(), true).unwrap().point;
    s.param_set("px", &0.0.into()).unwrap();
    s.param_set("py", &0.0.into()).unwrap();
    xy(s.project(), sk, point, 0.0, 0.0);
    let dims: Vec<_> = s
        .sketch_detail(sk)
        .unwrap()
        .constraints
        .into_iter()
        .filter(|c| c.kind == "distance_point_line" && c.points.contains(&point))
        .collect();
    for (dim, name) in dims.iter().zip(["negative_x", "negative_y"]) {
        s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: dim.index }, None, Some(name)).unwrap();
    }
    stock(&mut s, sk);
    let path = scratch("negative-point-at-zero.qcad");
    s.save(Some(&path)).unwrap();
    let (loaded, _) = Session::open(&path).unwrap();
    xy(loaded.project(), sk, point, 0.0, 0.0);
    for (name, text, x, y) in [("px", "-12", -12.0, 0.0), ("py", "-10", 0.0, -10.0)] {
        let (_, _, _, p) = common::gui_edit_param_model(&path, name, text);
        xy(&p, sk, point, x, y);
    }
    for (name, text, x, y) in [("negative_x", "14", -14.0, 0.0), ("negative_y", "11", 0.0, -11.0)] {
        let (_, _, _, p) = common::gui_edit_driver_model(&path, name, text);
        xy(&p, sk, point, x, y);
    }
    s.param_set("px", &(-12.0).into()).unwrap();
    s.param_set("py", &(-10.0).into()).unwrap();
    xy(s.project(), sk, point, -12.0, -10.0);
}

#[test]
fn remove_only_isolated_non_system_points_and_keep_exact_archives_on_refusal() {
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let point = s.sketch_point(sk, &8.0.into(), &6.0.into(), true).unwrap().point;
    stock(&mut s, sk);
    let before = archive(&mut s, "point-remove-before.qcad");
    let detail = s.sketch_detail(sk).unwrap();
    let entity = detail.entities[0].clone();
    let error = s.sketch_remove_point(sk, entity.points[0]).unwrap_err().to_string();
    assert!(error.contains(&entity.id.to_string()), "{error}");
    assert_eq!(archive(&mut s, "point-remove-refusal.qcad"), before);
    let system = s.project().sketches[0].system_ids()[0];
    assert!(s.sketch_remove_point(sk, system).is_err());
    assert_eq!(archive(&mut s, "point-remove-system.qcad"), before);
    s.sketch_edit(sk, |s| s.sketch_remove_point(sk, point)).unwrap();
    assert!(!s.sketch_detail(sk).unwrap().points.iter().any(|p| p.id == point));
    assert!(!s.sketch_detail(sk).unwrap().constraints.iter().any(|c| c.points.contains(&point)));
    assert_close(s.result_bodies()[0].volume, 420.0, 1e-6, "removing a point keeps volume");
}

#[test]
fn coordinate_sign_guard_survives_point_reuse_by_an_entity() {
    let mut s = Session::new_part();
    s.param_set("px", &8.0.into()).unwrap();
    let sk = sketch(&mut s);
    let point = s.sketch_point(sk, &"px".into(), &6.0.into(), true).unwrap().point;
    let line = s
        .sketch_line(
            sk,
            &LineSpec { x1: 8.0.into(), y1: 6.0.into(), x2: 20.0.into(), y2: 10.0.into(), construction: true, dimensioned: false },
        )
        .unwrap();
    assert_eq!(line.points[0], point);
    stock(&mut s, sk);
    let before = archive(&mut s, "point-reused-before.qcad");
    assert!(s.param_set("px", &(-3.0).into()).is_err(), "entity reuse must not disable coordinate sign guard");
    assert_eq!(archive(&mut s, "point-reused-after.qcad"), before);
    xy(s.project(), sk, point, 8.0, 6.0);
}

#[test]
fn numeric_string_coordinates_keep_valid_native_formulas() {
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let point = s.sketch_point(sk, &" -8e0 ".into(), &"0e0".into(), true).unwrap().point;
    xy(s.project(), sk, point, -8.0, 0.0);
    s.param_set("unrelated", &1.0.into()).unwrap();
    let dims: Vec<_> = s
        .sketch_detail(sk)
        .unwrap()
        .constraints
        .into_iter()
        .filter(|c| c.kind == "distance_point_line" && c.points.contains(&point))
        .collect();
    assert_eq!(dims.len(), 2, "numeric-string zero keeps an editable coordinate formula");
    for (dim, name) in dims.iter().zip(["numeric_x", "numeric_y"]) {
        s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: dim.index }, None, Some(name)).unwrap();
    }
    stock(&mut s, sk);
    let path = scratch("numeric-string-point.qcad");
    s.save(Some(&path)).unwrap();
    let (loaded, _) = Session::open(&path).unwrap();
    xy(loaded.project(), sk, point, -8.0, 0.0);
    for (name, text, x, y) in [("numeric_x", "14", -14.0, 0.0), ("numeric_y", "11", -8.0, 11.0)] {
        let (_, _, _, p) = common::gui_edit_driver_model(&path, name, text);
        xy(&p, sk, point, x, y);
    }
}
