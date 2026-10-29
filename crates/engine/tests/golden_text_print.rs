//! Fixed native sketch text: independent polygon areas, profile holes and app-edit persistence.
mod common;
use common::*;
use qymcad_core::geom::Point2;
use qymcad_engine::*;

fn spec(text: &str) -> TextSpec {
    TextSpec { text: text.into(), at: [3., 4.], height: 10., angle: 0., font: None, construction: false }
}

fn ext(sketch: Id, height: Num, op: Op) -> Extrude {
    Extrude { down: None, sketch, profiles: None, height, op, direction: Direction::Normal, through: false, target: None, name: None }
}

fn sketch(s: &mut Session, text: &TextSpec) -> (Id, Id) {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let id = s.sketch_text(sk, text).unwrap();
    (sk, id)
}

fn saved(s: &mut Session, name: &str) -> qymcad_core::model::Project {
    let path = scratch(name);
    s.save(Some(&path)).unwrap();
    qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap().project
}

fn contains(poly: &[Point2], p: Point2) -> bool {
    let mut inside = false;
    for (i, a) in poly.iter().enumerate() {
        let b = poly[(i + 1) % poly.len()];
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
}

fn net_area(loops: &[Vec<Point2>]) -> f64 {
    loops
        .iter()
        .enumerate()
        .map(|(i, poly)| {
            let area = (0..poly.len())
                .map(|j| {
                    let (a, b) = (poly[j], poly[(j + 1) % poly.len()]);
                    a.x * b.y - b.x * a.y
                })
                .sum::<f64>()
                .abs()
                / 2.;
            let depth = loops.iter().enumerate().filter(|(j, outer)| *j != i && contains(outer, poly[0])).count();
            if depth % 2 == 0 {
                area
            } else {
                -area
            }
        })
        .sum()
}

#[test]
fn text_all_letters_and_counters_extrude_with_independent_area() {
    for (n, text) in ["ABC", "Привет", "B8Oi", "ABCDEFGHIJ"].iter().enumerate() {
        let mut s = Session::new_part();
        let (sk, id) = sketch(&mut s, &spec(text));
        let info = s.sketch_info(sk).unwrap();
        assert_eq!(info.texts[0].id, id);
        assert_eq!(info.texts[0].text, *text);
        assert_eq!(info.texts[0].family, "Arial");
        assert_eq!(info.texts[0].font.index, 0);
        assert_eq!(info.dof, (0, 0));
        let p = saved(&mut s, &format!("text_area_{n}.qcad"));
        let loops = &p.sketches[0].texts[0].glyphs;
        assert_eq!(info.texts[0].contours, loops.len());
        assert!(loops.iter().all(|g| g.len() >= 3
            && g.iter().enumerate().all(|(j, a)| {
                let b = g[(j + 1) % g.len()];
                a.x != b.x || a.y != b.y
            })));
        let a = net_area(loops);
        assert!(a > 0.);
        assert!(info.contours.iter().any(|c| c.parent.is_some()), "counters");
        let (_, r) = s.extrude(&ext(sk, 2.0.into(), Op::Add)).unwrap();
        assert!(r.errors.is_empty());
        assert_close(r.bodies[0].volume, a * 2., 1e-5, "all glyph roots minus holes");
    }
}

#[test]
fn text_rotation_is_about_the_recorded_baseline_anchor() {
    let mut s = Session::new_part();
    sketch(&mut s, &spec("ABC"));
    let base = saved(&mut s, "text_rotation_base.qcad").sketches[0].texts[0].glyphs.clone();
    let mut r = Session::new_part();
    let mut t = spec("ABC");
    t.angle = 90.;
    let (sk, _) = sketch(&mut r, &t);
    let rotated = saved(&mut r, "text_rotation_90.qcad").sketches[0].texts[0].glyphs.clone();
    for (a, b) in base.iter().flatten().zip(rotated.iter().flatten()) {
        assert_close(b.x, 3. - (a.y - 4.), 1e-10, "rotated x");
        assert_close(b.y, 4. + (a.x - 3.), 1e-10, "rotated y");
    }
    let (_, report) = r.extrude(&ext(sk, 2.0.into(), Op::Add)).unwrap();
    let bbox = report.bodies[0].bbox;
    for axis in 0..2 {
        let values = rotated.iter().flatten().map(|p| if axis == 0 { p.x } else { p.y }).collect::<Vec<_>>();
        assert_close(bbox[axis], values.iter().copied().fold(f64::INFINITY, f64::min), 0.005, "rotated lower bound");
        assert_close(bbox[axis + 3], values.iter().copied().fold(f64::NEG_INFINITY, f64::max), 0.005, "rotated upper bound");
    }
}

#[test]
fn text_depth_edit_roundtrip_and_app_path_keep_baked_missing_font_glyphs() {
    let mut s = Session::new_part();
    s.param_set("depth", &2.0.into()).unwrap();
    let (sk, _) = sketch(&mut s, &spec("B8O"));
    s.extrude(&ext(sk, Num::Expr("depth".into()), Op::Add)).unwrap();
    let path = scratch("text_depth.qcad");
    s.save(Some(&path)).unwrap();
    let p = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap().project;
    let area = net_area(&p.sketches[0].texts[0].glyphs);
    let initial = s.result_bodies()[0].volume;
    let (reopened, _) = Session::open(&path).unwrap();
    assert_close(reopened.result_bodies()[0].volume, initial, 1e-8, "roundtrip");
    assert_eq!(reopened.sketch_info(sk).unwrap(), s.sketch_info(sk).unwrap());
    let r = s.param_set("depth", &3.0.into()).unwrap();
    assert_close(r.bodies[0].volume, area * 3., 1e-5, "server depth edit");
    let mut loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    loaded.project.sketches[0].texts[0].font.path = "/missing/intentionally-absent-font.ttf".into();
    let absent = scratch("text_baked_without_font.qcad");
    qymcad_io::save_project_with_brep(&loaded.project, absent.to_str().unwrap(), &loaded.breps).unwrap();
    assert_close(Session::open(&absent).unwrap().0.result_bodies()[0].volume, initial, 1e-8, "baked missing-font open");
    let (v, _, _) = gui_edit_param_faces(&absent, "depth", "4");
    assert_close(v, area * 4., 1e-5, "app depth edit uses saved glyphs");
}

#[test]
fn text_add_and_cut_on_stock_follow_independent_area() {
    for cut in [false, true] {
        let mut s = Session::new_part();
        let stock = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(stock, &15.0.into(), &10.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
        s.extrude(&ext(stock, 5.0.into(), Op::Add)).unwrap();
        let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &5.0.into(), None).unwrap();
        let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
        s.sketch_text(sk, &spec("B8O")).unwrap();
        let p = saved(&mut s, if cut { "text_stock_cut.qcad" } else { "text_stock_add.qcad" });
        let a = net_area(&p.sketches[1].texts[0].glyphs);
        let mut e = ext(sk, 1.0.into(), if cut { Op::Cut } else { Op::Add });
        if cut {
            e.direction = Direction::Reverse;
        }
        let (_, r) = s.extrude(&e).unwrap();
        assert_close(r.bodies[0].volume, 6000. + if cut { -a } else { a }, 1e-5, "stock plus/minus glyph area");
    }
}

#[test]
fn text_font_glyph_and_numeric_errors_leave_sketch_unchanged() {
    let mut s = Session::new_part();
    let (sk, _) = sketch(&mut s, &spec("A"));
    let before = s.sketch_detail(sk).unwrap();
    for (mut t, needle) in
        [(spec("A🦄🦄"), "🦄"), (spec("   "), "outline"), (spec("A"), "font"), (spec("A"), "positive"), (spec("A"), "finite")]
    {
        if needle == "font" {
            t.font = Some(TextFont { path: "/missing/no-font.ttf".into(), index: 0 });
        }
        if needle == "positive" {
            t.height = 0.;
        }
        if needle == "finite" {
            t.at[0] = f64::NAN;
        }
        let err = s.sketch_text(sk, &t).unwrap_err().to_string();
        assert!(err.contains(needle), "{err}");
        assert_eq!(s.sketch_detail(sk).unwrap(), before);
    }
}

#[test]
fn construction_text_is_inspectable_removable_and_real_text_deletion_rolls_back() {
    let mut s = Session::new_part();
    let mut t = spec("B");
    t.construction = true;
    let (sk, id) = sketch(&mut s, &t);
    assert_eq!(s.sketch_info(sk).unwrap().texts.len(), 1);
    assert!(s.sketch_info(sk).unwrap().contours.is_empty());
    s.sketch_remove_entity(sk, id).unwrap();
    assert!(s.sketch_info(sk).unwrap().texts.is_empty());
    let id = s.sketch_text(sk, &spec("B")).unwrap();
    s.extrude(&ext(sk, 2.0.into(), Op::Add)).unwrap();
    let before = s.sketch_detail(sk).unwrap();
    let v = s.result_bodies()[0].volume;
    assert!(s.sketch_edit(sk, |s| s.sketch_remove_entity(sk, id)).is_err());
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
    assert_close(s.result_bodies()[0].volume, v, 1e-8, "delete rollback");
}

#[test]
fn text_item_driver_name_is_refused_atomically() {
    let mut s = Session::new_part();
    let (sk, id) = sketch(&mut s, &spec("B"));
    let before = s.sketch_detail(sk).unwrap();
    let err =
        s.dimension_set(&DimensionTarget::Feature { feature: id, key: "height".into() }, None, Some("textheight")).unwrap_err().to_string();
    assert!(err.contains("does not dirty its owning sketch"), "{err}");
    assert_eq!(s.sketch_detail(sk).unwrap(), before);
    assert!(s.named_dimensions().is_empty());
}

#[test]
fn upstream_native_named_text_height_setdriver_changes_metadata_without_dirtying_solid() {
    let bytes = std::fs::read("/System/Library/Fonts/Supplemental/Arial.ttf").unwrap();
    let glyphs: Vec<_> = qymcad_core::text::text_outline_contours(&bytes, 0, "B", 10., 3., 4.).into_iter().map(|c| c.points).collect();
    let area = net_area(&glyphs);
    let mut p = qymcad_core::model::Project::default();
    p.new_document();
    let sk = p.add_sketch("native text", vec![], None);
    p.add_sketch_node(sk, "native text");
    let si = p.sketch_index(sk).unwrap();
    let id = p.add_sketch_text(
        si,
        qymcad_core::model::TextSpec {
            at: Point2::new(3., 4.),
            height: 10.,
            angle: 0.,
            text: "B".into(),
            glyphs,
            font: Default::default(),
        },
        qymcad_core::feature::Purpose::Real,
    );
    let roots = p.sketches[si].contour_ids.iter().copied().filter(|c| p.contours.parent_of(*c).is_none_or(|x| x == 0)).collect();
    p.add_extrude_multi(sk, roots, 2., qymcad_core::feature::Reach::Forward, 0., vec![]);
    assert!(p.add_named_feat_dim("textheight".into(), id, "height"));
    let (r, shapes) = qymcad_testkit::regenerate(&mut p);
    assert!(r.errors.is_empty());
    let body = p.timeline.iter().filter_map(|n| n.kind.body()).next_back().unwrap();
    let target = qymcad_core::model::DimTarget::Feature { node: id, key: "height".into() };
    let seen = p.param_map();
    p.check_dim_target_text(&target, "15").unwrap();
    assert!(p.set_dim_target_text(&target, "15").unwrap());
    p.eval_parameters();
    for i in 0..p.sketches.len() {
        if p.sketch_reads_names(i) {
            assert!(p.solve_sketch(i) <= 1e-6);
        }
    }
    let names = p.param_map();
    assert_eq!(names["textheight"], 15.);
    for (name, value) in names {
        if seen.get(&name).is_none_or(|old| (value - *old).abs() > 1e-12) {
            p.mark_param_dependents_dirty_for(&name);
        }
    }
    assert_eq!(p.sketches[si].texts[0].height, 15.);
    assert!(p.timeline.iter().all(|n| !n.dirty), "SetDriver misses item's timeline owner");
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, shapes);
    assert!(r.errors.is_empty());
    let volume = {
        let _gate = qymcad_kernel::kernel_gate();
        shapes[&body].volume()
    };
    assert_close(volume, area * 2., 1e-6, "native app keeps stale solid after metadata changes");
    assert!((area * 2. * 2.25 - volume).abs() > 1., "new height would require 2.25 times the area");
}

#[test]
fn fillet_on_extruded_glyph_edge_rebuilds_and_survives_app_edit() {
    let mut s = Session::new_part();
    s.param_set("round", &0.1.into()).unwrap();
    let (sk, _) = sketch(&mut s, &spec("E"));
    let (body, _) = s.extrude(&ext(sk, 2.0.into(), Op::Add)).unwrap();
    let before = s.result_bodies()[0].volume;
    let t = s.topology(Some(body), false).unwrap();
    let edge = t
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Line && (e.b[2] - e.a[2]).abs() > 1.9)
        .min_by(|a, b| a.mid[0].total_cmp(&b.mid[0]).then(a.mid[1].total_cmp(&b.mid[1])))
        .unwrap();
    s.fillet(Some(body), &Sel::Ids(vec![edge.id]), &Num::Expr("round".into()), None).unwrap();
    let expected = |r: f64| before - (1. - std::f64::consts::PI / 4.) * r * r * 2.;
    assert_close(s.result_bodies()[0].volume, expected(0.1), 1e-6, "one convex vertical glyph-edge fillet");
    let path = scratch("text_fillet.qcad");
    s.save(Some(&path)).unwrap();
    assert_close(Session::open(&path).unwrap().0.result_bodies()[0].volume, expected(0.1), 1e-6, "saved glyph fillet");
    s.param_set("round", &0.12.into()).unwrap();
    assert_close(s.result_bodies()[0].volume, expected(0.12), 1e-6, "server glyph fillet edit");
    let (v, _, _) = gui_edit_param_faces(&path, "round", "0.15");
    assert_close(v, expected(0.15), 1e-6, "app glyph fillet edit");
}

#[test]
#[ignore = "upstream contour identity defect: native H10→20 swaps glyphs and counters; not reported upstream yet"]
fn upstream_native_text_height_h10_to_20_changes_saved_profile_identity() {
    let bytes = std::fs::read("/System/Library/Fonts/Supplemental/Arial.ttf").unwrap();
    let glyphs: Vec<_> =
        qymcad_core::text::text_outline_contours(&bytes, 0, "ABCDEFGHIJ", 10., 3., 4.).into_iter().map(|c| c.points).collect();
    let a = net_area(&glyphs);
    let mut p = qymcad_core::model::Project::default();
    p.new_document();
    let sk = p.add_sketch("native text", vec![], None);
    p.add_sketch_node(sk, "native text");
    let si = p.sketch_index(sk).unwrap();
    let id = p.add_sketch_text(
        si,
        qymcad_core::model::TextSpec {
            at: Point2::new(3., 4.),
            height: 10.,
            angle: 0.,
            text: "ABCDEFGHIJ".into(),
            glyphs,
            font: Default::default(),
        },
        qymcad_core::feature::Purpose::Real,
    );
    p.parameters.push(qymcad_core::model::Param { name: "h".into(), expr: "10".into(), value: 10. });
    p.set_feat_dim(id, "height", "h".into());
    let roots = p.sketches[si].contour_ids.iter().copied().filter(|c| p.contours.parent_of(*c).is_none_or(|x| x == 0)).collect();
    p.add_extrude_multi(sk, roots, 2., qymcad_core::feature::Reach::Forward, 0., vec![]);
    let (r, shapes) = qymcad_testkit::regenerate(&mut p);
    assert!(r.errors.is_empty());
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        shapes.into_iter().map(|(id, s)| (id, s.to_brep_bytes().unwrap())).collect::<Vec<_>>()
    };
    let path = scratch("text_upstream_height.qcad");
    qymcad_io::save_project_with_brep(&p, path.to_str().unwrap(), &breps).unwrap();
    let (v, _, _) = gui_edit_param_faces(&path, "h", "20");
    assert_close(v, a * 2. * 4., 1e-5, "native H10→20 must preserve glyph profiles");
}
