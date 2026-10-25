//! Point-mirror copies tied to their source through the centre: area, centroid, formulas and app rebuilds.
mod common;
use common::*;
use qymcad_engine::*;

fn n(v: f64) -> Num {
    Num::Value(v)
}
fn e(v: &str) -> Num {
    Num::Expr(v.into())
}
fn sketch(s: &mut Session) -> Id {
    s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap()
}
fn mirror(s: &mut Session, sk: Id, curves: Vec<Id>) -> EditedSketch {
    s.sketch_mirror_point(sk, &MirrorPointSpec { curves: SketchCurveSel::Ids(curves), about: PointSel::Origin(OriginRef::Origin) }).unwrap()
}
fn extrude(s: &mut Session, sk: Id) -> Id {
    let (body, report) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            height: n(3.0),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    body
}

#[test]
fn point_mirror_triangle_area_centroid_and_copy_follows_its_source_in_app() {
    let mut s = Session::new_part();
    s.param_set("H", &n(8.0)).unwrap();
    let sk = sketch(&mut s);
    let source = s
        .sketch_polyline(
            sk,
            &PolylineSpec {
                points: vec![Xy(n(10.0), n(10.0)), Xy(n(16.0), n(10.0)), Xy(n(10.0), e("H+10"))],
                closed: true,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    let mut native = s.project().clone();
    let si = native.sketch_index(sk).unwrap();
    native.mirror_about_point(si, &source.entities, 0.0, 0.0);
    assert_eq!(native.sketch_dof(si), (6, 0), "app's native point mirror leaves triangle coordinate freedoms");
    let copied = mirror(&mut s, sk, source.entities.clone());
    assert_eq!(copied.created_entities.len(), 3);
    assert_eq!(copied.created_points.len(), 3);
    let detail = s.sketch_detail(sk).unwrap();
    assert_eq!(detail.summary.dof, (0, 0));
    for c in &detail.summary.contours {
        assert_close(c.area, 6.0 * 8.0 / 2.0, 1e-6, "triangle area b*h/2 is invariant");
    }
    let point = |id| detail.points.iter().find(|p| p.id == id).unwrap();
    let centroid = |ids: &[Id]| ids.iter().fold([0.0, 0.0], |a, id| [a[0] + point(*id).x / 3.0, a[1] + point(*id).y / 3.0]);
    let c = centroid(&source.points);
    let m = centroid(&copied.created_points);
    assert_close(m[0], -c[0], 1e-6, "mirror triangle centroid x = -12");
    assert_close(m[1], -c[1], 1e-6, "mirror triangle centroid y = -(10+8/3)");
    let body = extrude(&mut s, sk);
    assert_close(s.shape(body).unwrap().volume(), 2.0 * 24.0 * 3.0, 1e-6, "two mirrored triangle prisms");
    let path = scratch("mirror_triangle_2.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    assert_eq!(opened.sketch_info(sk).unwrap().dof, (0, 0));
    assert_close(opened.shape(body).unwrap().volume(), 144.0, 1e-6, "mirror roundtrip");
    s.param_set("H", &n(12.0)).unwrap();
    // The copy is tied to its source through the centre, so the source's formula moves the copy too.
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    assert_close(s.shape(body).unwrap().volume(), 2.0 * 36.0 * 3.0, 1e-6, "the copy follows the source formula");
    assert_close(gui_edit_param(&path, "H", "12"), 216.0, 1e-6, "the copy follows the source in the app");
}

#[test]
fn point_mirror_circle_keeps_native_size_formula_and_named_app_driver() {
    let mut s = Session::new_part();
    s.param_set("D", &n(4.0)).unwrap();
    let sk = sketch(&mut s);
    let source = s.sketch_circle(sk, &n(10.0), &n(5.0), &e("D"), false).unwrap();
    let mut native = s.project().clone();
    let si = native.sketch_index(sk).unwrap();
    native.mirror_about_point(si, &[source], 0.0, 0.0);
    assert_eq!(native.sketch_dof(si), (2, 0), "app's native mirror carries diameter but leaves centre free");
    let copy = mirror(&mut s, sk, vec![source]);
    let detail = s.sketch_detail(sk).unwrap();
    assert_eq!(detail.summary.dof, (0, 0));
    let centre = detail.entities.iter().find(|c| c.id == copy.created_entities[0]).unwrap().points[0];
    let constraint = detail.constraints.iter().find(|c| c.kind == "diameter" && c.points.contains(&centre)).unwrap();
    assert_eq!(constraint.expr.as_deref(), Some("D"));
    s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: constraint.index }, None, Some("CopyDiameter")).unwrap();
    let body = extrude(&mut s, sk);
    let pi = std::f64::consts::PI;
    assert_close(s.shape(body).unwrap().volume(), 24.0 * pi, 1e-6, "two circles 2*pi*r^2*t");
    let path = scratch("mirror_circle_2.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    assert_close(opened.shape(body).unwrap().volume(), 24.0 * pi, 1e-6, "circle mirror roundtrip");
    s.param_set("D", &n(6.0)).unwrap();
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    assert_close(s.shape(body).unwrap().volume(), 54.0 * pi, 1e-6, "native mirrored size formula follows parameter");
    assert_close(gui_edit_param(&path, "D", "6"), 54.0 * pi, 1e-6, "app mirrored size formula");
    assert_close(gui_edit_driver(&path, "CopyDiameter", "6"), 39.0 * pi, 1e-6, "SetDriver edits copied circle alone");
}

#[test]
fn point_mirror_resolves_points_and_refuses_invalid_selection_atomically() {
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let source = s.sketch_circle(sk, &n(10.0), &n(5.0), &n(4.0), false).unwrap();
    let before = s.sketch_detail(sk).unwrap();
    for spec in [
        MirrorPointSpec { curves: SketchCurveSel::Ids(vec![]), about: PointSel::Origin(OriginRef::Origin) },
        MirrorPointSpec { curves: SketchCurveSel::Id(source), about: PointSel::Id(u64::MAX) },
    ] {
        assert!(s.sketch_mirror_point(sk, &spec).is_err());
        assert_eq!(s.sketch_detail(sk).unwrap(), before);
    }
    let copy = s
        .sketch_mirror_point(
            sk,
            &MirrorPointSpec {
                curves: SketchCurveSel::Id(source),
                about: PointSel::Role(PointRoleSel { curve: SketchCurveSel::Id(source), role: PointRole::Centre, node_index: None }),
            },
        )
        .unwrap();
    let detail = s.sketch_detail(sk).unwrap();
    let p = detail.points.iter().find(|p| p.id == copy.created_points[0]).unwrap();
    assert_close(p.x, 10.0, 1e-6, "mirroring about own centre keeps centre x");
    assert_close(p.y, 5.0, 1e-6, "mirroring about own centre keeps centre y");
    assert_eq!(detail.summary.dof, (0, 0));
}

#[test]
fn point_mirror_retains_native_rectangle_and_transforms_curved_geometry() {
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let rect = s.sketch_rect(sk, &n(30.0), &n(20.0), &n(8.0), &n(6.0), true).unwrap();
    mirror(&mut s, sk, rect);
    let si = s.project().sketch_index(sk).unwrap();
    assert_eq!(s.project().sketches[si].rects().len(), 2, "native figure metadata copied");
    assert_eq!(s.sketch_info(sk).unwrap().dof, (0, 0));
    let spline = s
        .sketch_spline(
            sk,
            &SplineSpec {
                points: vec![Xy(n(1.0), n(3.0)), Xy(n(4.0), n(6.0)), Xy(n(9.0), n(2.0))],
                closed: false,
                construction: true,
                dimensioned: true,
            },
        )
        .unwrap();
    let ellipse = s
        .sketch_ellipse_arc(
            sk,
            &EllipseArcSpec {
                cx: n(20.0),
                cy: n(30.0),
                rx: n(8.0),
                ry: n(4.0),
                turn: n(30.0),
                start_angle: n(30.0),
                end_angle: n(150.0),
                ccw: true,
                construction: true,
                dimensioned: true,
            },
        )
        .unwrap();
    let original = s.sketch_detail(sk).unwrap();
    let sources = [spline.entities[0], ellipse.entities[0]];
    let copy = mirror(&mut s, sk, sources.to_vec());
    let after = s.sketch_detail(sk).unwrap();
    assert_eq!(after.summary.dof, (0, 0));
    for (old, new) in sources.into_iter().zip(copy.created_entities) {
        let a = original.entities.iter().find(|e| e.id == old).unwrap();
        let b = after.entities.iter().find(|e| e.id == new).unwrap();
        assert_eq!(a.kind, b.kind);
        assert_eq!(a.ccw, b.ccw, "point mirror preserves winding");
        assert!(b.construction);
        for (op, np) in a.points.iter().zip(&b.points) {
            let p = original.points.iter().find(|p| p.id == *op).unwrap();
            let q = after.points.iter().find(|p| p.id == *np).unwrap();
            assert_close(q.x, -p.x, 1e-6, "every owned curve point mirror x");
            assert_close(q.y, -p.y, 1e-6, "every owned curve point mirror y");
        }
    }
}

#[test]
fn point_mirrored_profile_keeps_f074_fillet_acceptance() {
    let mut s = Session::new_part();
    let sk = sketch(&mut s);
    let source = s.sketch_rect(sk, &n(30.0), &n(30.0), &n(20.0), &n(20.0), false).unwrap();
    mirror(&mut s, sk, source);
    let mirrored = s
        .sketch_info(sk)
        .unwrap()
        .contours
        .iter()
        .find(|c| {
            let i = s.project().contours.index_of(c.id).unwrap();
            s.project().contours[i].centroid().x < 0.0
        })
        .unwrap()
        .id;
    let (body, report) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: Some(vec![mirrored]),
            height: n(20.0),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(report.errors.is_empty());
    let before = s.shape(body).unwrap().volume();
    let vertical = Sel::Along { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 };
    let error = s.fillet(None, &vertical, &n(15.0), None).unwrap_err();
    assert!(error.to_string().contains("fillet R15.00 does not fit these edges together"), "{error}");
    assert_eq!(s.shape(body).unwrap().volume().to_bits(), before.to_bits(), "F-074 refusal rolls back edited-sketch body");
    let (_, report) = s.fillet(None, &vertical, &n(2.0), None).unwrap();
    assert!(report.errors.is_empty());
    assert_close(
        report.bodies[0].volume,
        (400.0 - (4.0 - std::f64::consts::PI) * 4.0) * 20.0,
        1e-5,
        "mirrored prism corner fillets analytic volume",
    );
}
