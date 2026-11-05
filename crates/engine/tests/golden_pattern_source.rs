mod common;
use common::{assert_close, scratch};
use qymcad_core::model::{entity_points, Constraint, PatternKind, Project};
use qymcad_engine::*;
use std::collections::BTreeMap;
use std::io::Read;

fn linear(count: Num, dx: Num, dy: Num) -> SketchPatternKind {
    SketchPatternKind::Linear { count, dx, dy, count2: None, dx2: None, dy2: None }
}
fn extrude(s: &mut Session, sk: Id, picks: Option<Vec<[Num; 2]>>) -> Id {
    let (body, report) = s
        .extrude(&Extrude {
            sketch: sk,
            profiles: None,
            profiles_at: picks,
            height: 2.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    body
}
fn bbox(s: &Session, body: Id, expected: [f64; 6]) {
    bbox_with_tolerance(s, body, expected, 1e-6);
}
fn bbox_with_tolerance(s: &Session, body: Id, expected: [f64; 6], tolerance: f64) {
    for (a, e) in s.shape(body).unwrap().bbox().unwrap().into_iter().zip(expected) {
        assert_close(a, e, tolerance, "analytic B-rep bbox");
    }
}
fn coordinates(p: &Project, sk: Id) -> Vec<(f64, f64)> {
    let sk = &p.sketches[p.sketch_index(sk).unwrap()];
    sk.entities
        .iter()
        .flat_map(entity_points)
        .map(|id| {
            let p = sk.point(id).unwrap();
            (p.x, p.y)
        })
        .collect()
}
fn matches(p: &Project, sk: Id) -> usize {
    let si = p.sketch_index(sk).unwrap();
    let sk = &p.sketches[si];
    let mut checked = 0;
    for pattern in &sk.patterns {
        // Native whole-figure copies carry rectangle construction diagonals too
        // (sketch/edit.rs:526-579); the public clipboard gives that complete source order.
        let source = p.copy_sketch_geometry(si, &pattern.source, 0.0, 0.0).entities;
        assert_eq!(pattern.instances.len() % source.len(), 0);
        for (copy, chunk) in pattern.instances.chunks(source.len()).enumerate() {
            let k = (copy + 1) as u32;
            for (src, &dst) in source.iter().zip(chunk) {
                let a = entity_points(src);
                let b = entity_points(sk.entity(dst).unwrap());
                assert_eq!(a.len(), b.len());
                for (a, b) in a.into_iter().zip(b) {
                    let a = sk.point(a).unwrap();
                    let b = sk.point(b).unwrap();
                    let expected = match pattern.kind {
                        PatternKind::Linear { dx, dy, dx2, dy2, count2, .. } => {
                            let col = f64::from(k / count2.max(1));
                            let row = f64::from(k % count2.max(1));
                            // Native enumeration is i-outer/j-inner; p' = p + i*d + j*d2.
                            (a.x + col * dx + row * dx2, a.y + col * dy + row * dy2)
                        }
                        PatternKind::Circular { cx, cy, total_deg: angle, count } => {
                            // Native full circle step=360/count; sectors use angle/count.
                            let t = (f64::from(k) * angle / f64::from(count.max(2))).to_radians();
                            (cx + (a.x - cx) * t.cos() - (a.y - cy) * t.sin(), cy + (a.x - cx) * t.sin() + (a.y - cy) * t.cos())
                        }
                    };
                    assert_close(b.x, expected.0, 1e-7, "instance x = transformed final source");
                    assert_close(b.y, expected.1, 1e-7, "instance y = transformed final source");
                    checked += 2;
                }
            }
        }
    }
    assert!(checked > 0);
    checked
}
fn archive(s: &mut Session, name: &str) -> BTreeMap<String, Vec<u8>> {
    let path = scratch(name);
    s.save_with_options(Some(&path), true).unwrap();
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut b = Vec::new();
        f.read_to_end(&mut b).unwrap();
        out.insert(f.name().to_string(), b);
    }
    out
}
fn minimal() -> (Session, Id, Id) {
    let mut s = Session::new_part();
    for (n, v) in
        [("leg_span", 40.0.into()), ("leg_w", 6.0.into()), ("leg_n", 3.0.into()), ("leg_margin", 5.0.into()), ("step", "leg_w+4".into())]
    {
        assert!(s.param_set(n, &v).unwrap().errors.is_empty());
    }
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("minimal_hex")).unwrap();
    let source = s
        .sketch_polygon(
            sk,
            &PolygonSpec {
                cx: "leg_margin+leg_w/2".into(),
                cy: "leg_span/2".into(),
                sides: 6,
                r: Some("leg_w/1.7320508".into()),
                angle: None,
                vertex: None,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap()
        .entities;
    s.sketch_pattern_create(sk, &source, &linear("leg_n".into(), 0.0.into(), "step".into())).unwrap();
    let body = extrude(&mut s, sk, None);
    (s, sk, body)
}
fn four(s: &mut Session) -> Result<()> {
    for (n, v) in [("leg_span", 50.0.into()), ("leg_w", 8.0.into()), ("leg_n", 4.0.into()), ("leg_margin", "leg_span/5".into())] {
        let r = s.param_set(n, &v)?;
        assert!(r.errors.is_empty(), "{r:?}");
    }
    Ok(())
}
fn final_minimal(s: &Session, sk: Id, body: Id) {
    assert_eq!(matches(s.project(), sk), 3 * 13 * 2); // Three copies, circle centre + six two-ended edges.
    let sketch = &s.project().sketches[s.project().sketch_index(sk).unwrap()];
    let pattern = &sketch.patterns[0];
    assert_eq!(pattern.instances.len(), 3 * pattern.source.len());
    for (k, id) in
        std::iter::once(&pattern.source[0]).chain(pattern.instances.chunks(pattern.source.len()).map(|copy| &copy[0])).enumerate()
    {
        let point = sketch.point(entity_points(sketch.entity(*id).unwrap())[0]).unwrap();
        assert_close(point.x, 50.0 / 5.0 + 8.0 / 2.0, 1e-7, "hex centre x = margin + width/2");
        assert_close(point.y, 50.0 / 2.0 + k as f64 * (8.0 + 4.0), 1e-7, "hex centre y = span/2 + k*pitch");
    }
    // cx=50/5+8/2=14, cy=50/2=25; centres 25+k*(8+4), k=0..3.
    // R=8/1.7320508; hexagon bounds cx+-R, cy+-R*sin(pi/3), z=0..2.
    let r = 8.0 / 1.7320508;
    let h = r * (std::f64::consts::PI / 3.0).sin();
    bbox(s, body, [14.0 - r, 25.0 - h, 0.0, 14.0 + r, 61.0 + h, 2.0]);
}
#[test]
fn f5_minimal_four_batch_and_singles_match_final_geometry() {
    let (mut batch, sk, body) = minimal();
    batch.transaction(four).unwrap();
    final_minimal(&batch, sk, body);
    let (mut singles, sk2, body2) = minimal();
    four(&mut singles).unwrap();
    final_minimal(&singles, sk2, body2);
    assert_eq!(coordinates(batch.project(), sk), coordinates(singles.project(), sk2));
}
#[test]
fn f5_source_only_param_set_moves_instances_and_brep() {
    let (mut s, sk, body) = minimal();
    assert!(s.param_set("leg_margin", &10.0.into()).unwrap().errors.is_empty());
    matches(s.project(), sk);
    // cx=10+6/2=13, cy=20; copies at y=30,40; R=6/1.7320508.
    let r = 6.0 / 1.7320508;
    let h = r * (std::f64::consts::PI / 3.0).sin();
    bbox(&s, body, [13.0 - r, 20.0 - h, 0.0, 13.0 + r, 40.0 + h, 2.0]);
}
fn real(attempt: usize) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/f5-real.qcad");
    let (mut s, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    let start = std::time::Instant::now();
    let r = s.param_set("bed_frame_depth", &70.0.into()).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    println!("F5 real fixture param_set bed_frame_depth=70 elapsed_ms={:.3}", start.elapsed().as_secs_f64() * 1000.0);
    let edits: Vec<(&str, Num)> = if attempt == 1 {
        vec![
            ("leg_span", "bed_frame_depth-18.4".into()),
            ("leg_w", "min((2*leg_span-leg_web)/3, 24)".into()),
            ("leg_n", "floor((Lz-10-2*leg_R)/leg_half)+1".into()),
            ("leg_margin", "8+(leg_span-(1.5*leg_w+leg_web/2))/2".into()),
        ]
    } else {
        vec![
            ("leg_span", "bed_frame_depth-20".into()),
            ("leg_n", 6.0.into()),
            ("leg_margin", "6+(leg_span-(1.5*leg_w+leg_web/2))/2".into()),
            ("leg_w", 22.0.into()),
        ]
    };
    s.transaction(|s| {
        for (n, v) in &edits {
            let r = s.param_set(n, v)?;
            assert!(r.errors.is_empty(), "{r:?}");
        }
        Ok(())
    })
    .unwrap();
    let sk = s.project().sketches.iter().find(|s| s.name == "leg_hex_sk").unwrap().id;
    // Two interleaved patterns, each with two copies of 13 source point references, x and y each.
    assert_eq!(matches(s.project(), sk), 2 * 2 * 13 * 2);
    let sketch = &s.project().sketches[s.project().sketch_index(sk).unwrap()];
    assert_eq!(sketch.patterns.len(), 2);
    // Fixture definitions: truss_web=5, Lz=170, half=(w+web)*0.8660254,
    // R=w/1.7320508, z0=(Lz-((n-1)*half+2R))/2+R. Both attempts end with n=6.
    let span = if attempt == 1 { 70.0 - 18.4 } else { 70.0 - 20.0 };
    let width = if attempt == 1 { ((2.0_f64 * span - 5.0) / 3.0).min(24.0) } else { 22.0 };
    let margin = (if attempt == 1 { 8.0 } else { 6.0 }) + (span - (1.5 * width + 5.0 / 2.0)) / 2.0;
    let half = (width + 5.0) * 0.8660254;
    let radius = width / 1.7320508;
    let z0 = (170.0 - ((6.0 - 1.0) * half + 2.0 * radius)) / 2.0 + radius;
    for (j, pattern) in sketch.patterns.iter().enumerate() {
        match pattern.kind {
            PatternKind::Linear { count, dx, dy, count2, dx2, dy2 } => {
                assert_eq!(count, 3);
                assert_eq!(count2, 1);
                assert_close(dx, 0.0, 1e-12, "fixture dx=0");
                assert_close(dy, 2.0 * half, 1e-7, "fixture pitch=2*(width+web)*0.8660254");
                assert_eq!((dx2, dy2), (0.0, 0.0));
            }
            _ => panic!("fixture must retain linear patterns"),
        }
        let centre = sketch.point(entity_points(sketch.entity(pattern.source[0]).unwrap())[0]).unwrap();
        assert_close(centre.x, -margin - width / 2.0 - j as f64 * (width + 5.0) / 2.0, 1e-7, "interleaved source x hand formula");
        assert_close(centre.y, z0 + j as f64 * half, 1e-7, "interleaved source y hand formula");
    }
}
#[test]
fn f5_real_attempt1_all_instance_coordinates() {
    real(1);
}
#[test]
fn f5_real_attempt2_control() {
    real(2);
}
fn free_rect() -> (Session, Id, Vec<Id>, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let source = s
        .sketch_polyline(
            sk,
            &PolylineSpec {
                points: vec![
                    Xy(2.0.into(), 2.0.into()),
                    Xy(4.0.into(), 2.0.into()),
                    Xy(4.0.into(), 4.0.into()),
                    Xy(2.0.into(), 4.0.into()),
                ],
                closed: true,
                construction: false,
                dimensioned: false,
            },
        )
        .unwrap()
        .entities;
    s.sketch_pattern_create(sk, &source, &linear(3.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, sk, None);
    (s, sk, source, body)
}
#[test]
fn f5_sketch_edit_source_transform_refreshes() {
    let (mut s, sk, source, body) = free_rect();
    s.sketch_edit(sk, |s| s.sketch_transform_move(sk, &TransformSpec { entities: source, dx: 3.0.into(), dy: 0.0.into() })).unwrap();
    matches(s.project(), sk); // rect [2,4]^2 +3X, final copy +20X.
    bbox(&s, body, [5.0, 2.0, 0.0, 27.0, 4.0, 2.0]);
}
fn dimension_fixture() -> (Session, Id, Id, DimensionTarget) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let source = s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
    let constraint = s.project().sketches[s.project().sketch_index(sk).unwrap()]
        .constraints
        .iter()
        .position(|c| Project::dim_value_of(c) == Some(10.0))
        .unwrap();
    s.sketch_pattern_create(sk, &source, &linear(3.0.into(), 20.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, sk, None);
    (s, sk, body, DimensionTarget::Sketch { sketch: sk, constraint })
}
#[test]
fn f5_dimension_source_final_solve_refreshes() {
    let (mut s, sk, body, target) = dimension_fixture();
    let edit = s.dimension_set(&target, Some(&14.0.into()), None).unwrap();
    assert!(edit.rebuild.errors.is_empty());
    matches(s.project(), sk);
    let sketch = &s.project().sketches[s.project().sketch_index(sk).unwrap()];
    for expected in [(-14.0 / 2.0, -6.0 / 2.0), (14.0 / 2.0, -6.0 / 2.0), (14.0 / 2.0, 6.0 / 2.0), (-14.0 / 2.0, 6.0 / 2.0)] {
        assert!(
            sketch.patterns[0].source.iter().flat_map(|id| entity_points(sketch.entity(*id).unwrap())).any(|id| {
                let p = sketch.point(id).unwrap();
                (p.x - expected.0).abs() < 1e-7 && (p.y - expected.1).abs() < 1e-7
            }),
            "missing source corner {expected:?} after FINAL width solve"
        );
    }
    // Width=14 about origin, copies +20,+40 => x=-7..47, y=-3..3.
    bbox(&s, body, [-7.0, -3.0, 0.0, 47.0, 3.0, 2.0]);
}
#[test]
fn f5_constrain_source_refreshes() {
    let (mut s, sk, point, body) = free_circle();
    s.sketch_edit(sk, |s| {
        s.sketch_constrain(
            sk,
            &ConstrainSpec {
                kind: ConstraintKind::Distance,
                refs: vec![SketchRef::Frame(FrameRef::Origin), SketchRef::Id(point)],
                value: Some(8.0.into()),
                axis: DistAxis::X,
                reference: false,
            },
        )
    })
    .unwrap();
    matches(s.project(), sk);
    let si = s.project().sketch_index(sk).unwrap();
    assert_close(s.project().sketches[si].point(point).unwrap().x, 8.0, 1e-7, "constrained source x");
    // Radius1 circles centred at (8 + k*10,2), k=0..2, extruded from z0 to2.
    // Curved BRepBndLib bounds include tessellation deflection (F-066), up to .005mm.
    bbox_with_tolerance(&s, body, [8.0 - 1.0, 2.0 - 1.0, 0.0, 8.0 + 2.0 * 10.0 + 1.0, 2.0 + 1.0, 2.0], 0.005);
}
#[test]
fn f5_circular_source_only_move() {
    let mut s = Session::new_part();
    assert!(s.param_set("x", &10.0.into()).unwrap().errors.is_empty());
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let src = s.sketch_rect(sk, &"x".into(), &0.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    s.sketch_pattern_create(
        sk,
        &src,
        &SketchPatternKind::Circular { count: 4.0.into(), centre: [0.0.into(), 0.0.into()], angle: 360.0.into() },
    )
    .unwrap();
    let body = extrude(&mut s, sk, None);
    assert!(s.param_set("x", &20.0.into()).unwrap().errors.is_empty());
    matches(s.project(), sk);
    // Four 2x2 squares centred on (+-20,0),(0,+-20).
    bbox(&s, body, [-21.0, -21.0, 0.0, 21.0, 21.0, 2.0]);
}
#[test]
fn f5_grid_source_only_move() {
    let mut s = Session::new_part();
    assert!(s.param_set("x", &3.0.into()).unwrap().errors.is_empty());
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let src = s.sketch_rect(sk, &"x".into(), &0.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    s.sketch_pattern_create(
        sk,
        &src,
        &SketchPatternKind::Linear {
            count: 3.0.into(),
            dx: 10.0.into(),
            dy: 0.0.into(),
            count2: Some(2.0.into()),
            dx2: Some(0.0.into()),
            dy2: Some(8.0.into()),
        },
    )
    .unwrap();
    let body = extrude(&mut s, sk, None);
    assert!(s.param_set("x", &6.0.into()).unwrap().errors.is_empty());
    matches(s.project(), sk);
    // centres (6+i*10,j*8), i=0..2,j=0..1; each side 2.
    bbox(&s, body, [5.0, -1.0, 0.0, 27.0, 9.0, 2.0]);
}
#[test]
fn f5_subset_pick_retains_instance_position() {
    let mut s = Session::new_part();
    assert!(s.param_set("x", &3.0.into()).unwrap().errors.is_empty());
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let src = s.sketch_rect(sk, &"x".into(), &0.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    s.sketch_pattern_create(sk, &src, &linear(4.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, sk, Some(vec![[13.0.into(), 0.0.into()], [33.0.into(), 0.0.into()]]));
    assert!(s.param_set("x", &6.0.into()).unwrap().errors.is_empty());
    matches(s.project(), sk);
    // Selected copy indices 1 and 3 => centres 6+10=16 and 6+30=36.
    bbox(&s, body, [15.0, -1.0, 0.0, 37.0, 1.0, 2.0]);
    assert_close(s.shape(body).unwrap().volume(), 2.0 * 2.0 * 2.0 * 2.0, 1e-6, "two selected prisms");
}
#[test]
fn f5_unrelated_sketch_instance_ids_and_coordinates_unchanged() {
    let (mut s, sk, _) = minimal();
    let other = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let src = s.sketch_rect(other, &0.0.into(), &100.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    s.sketch_pattern_create(other, &src, &linear(3.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let ids = s.project().sketches[s.project().sketch_index(other).unwrap()].patterns[0].instances.clone();
    let pts = coordinates(s.project(), other);
    assert!(s.param_set("leg_margin", &10.0.into()).unwrap().errors.is_empty());
    matches(s.project(), sk);
    assert_eq!(s.project().sketches[s.project().sketch_index(other).unwrap()].patterns[0].instances, ids);
    assert_eq!(coordinates(s.project(), other), pts);
}
#[test]
fn f5_undo_restores_source_instances_and_brep_in_one_step() {
    let (mut s, sk, body) = minimal();
    let before = coordinates(s.project(), sk);
    let old_bbox = s.shape(body).unwrap().bbox().unwrap();
    let snapshot = s.begin_tool_edit().unwrap();
    let r = s.param_set("leg_margin", &10.0.into());
    s.finish_tool_edit(snapshot, r.is_ok());
    assert!(r.unwrap().errors.is_empty());
    matches(s.project(), sk);
    s.undo().unwrap();
    assert_eq!(coordinates(s.project(), sk), before);
    assert_eq!(s.shape(body).unwrap().bbox().unwrap(), old_bbox);
    let before = archive(&mut s, "f5-fail-before.qcad");
    let history = s.begin_tool_edit().unwrap();
    let result = s.transaction(|s| {
        s.param_set("leg_margin", &10.0.into())?;
        s.param_set("leg_n", &100001.0.into())?;
        Ok(())
    });
    assert!(result.is_err());
    s.finish_tool_edit(history, false);
    assert_eq!(archive(&mut s, "f5-fail-after.qcad"), before);
}
#[test]
fn f5_chained_pattern_refused_before_mutation() {
    let (mut s, sk, _) = minimal();
    let instances = s.project().sketches[s.project().sketch_index(sk).unwrap()].patterns[0].instances.clone();
    let before = archive(&mut s, "f5-chain-before.qcad");
    let error = s.sketch_pattern_create(sk, &instances, &linear(2.0.into(), 100.0.into(), 0.0.into())).unwrap_err();
    assert!(error.to_string().contains("pattern"), "{error}");
    assert_eq!(archive(&mut s, "f5-chain-after.qcad"), before);
}
#[test]
fn f5_native_saved_corrected_points_and_source_only_gap_control() {
    let (mut s, sk, _) = minimal();
    four(&mut s).unwrap();
    matches(s.project(), sk);
    let path = scratch("f5-native.qcad");
    s.save_with_options(Some(&path), true).unwrap();
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap().project;
    assert_eq!(coordinates(&loaded, sk), coordinates(s.project(), sk));
    // App load + unchanged-value edit exercises common's native load/scheduling without source movement.
    let (_, _, _, native_loaded) = common::gui_edit_param_model(&path, "leg_margin", "leg_span/5");
    assert_eq!(coordinates(&native_loaded, sk), coordinates(s.project(), sk));
    let (_, _, _, native) = common::gui_edit_param_model(&path, "leg_margin", "15");
    let a = &s.project().sketches[s.project().sketch_index(sk).unwrap()];
    let b = &native.sketches[native.sketch_index(sk).unwrap()];
    for pattern in &a.patterns {
        for id in &pattern.instances {
            for p in entity_points(a.entity(*id).unwrap()) {
                let old = a.point(p).unwrap();
                let new = b.point(p).unwrap();
                assert_close(new.x, old.x, 1e-7, "native copy remains at saved x");
                assert_close(new.y, old.y, 1e-7, "native copy remains at saved y");
            }
        }
    }
    // Source centre changes from 10+4=14 to 15+4=19; copies stay at x=14.
    let centre = entity_points(b.entity(b.patterns[0].source[0]).unwrap())[0];
    assert_close(b.point(centre).unwrap().x, 19.0, 1e-7, "native source centre");
}

#[test]
fn f5_named_source_propagation_refreshes_reached_sketch() {
    let (mut s, _, _, target) = dimension_fixture();
    assert!(s.dimension_set(&target, None, Some("moving_centre")).unwrap().rebuild.errors.is_empty());
    let other = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("named_follower")).unwrap();
    let source = s.sketch_rect(other, &"moving_centre".into(), &100.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    s.sketch_pattern_create(other, &source, &linear(3.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, other, None);
    assert!(s.dimension_set(&target, Some(&14.0.into()), None).unwrap().rebuild.errors.is_empty());
    matches(s.project(), other);
    // Named centre=14, copies at x=24,34; each rectangle has half-size 1.
    bbox(&s, body, [13.0, 99.0, 0.0, 35.0, 101.0, 2.0]);
}

#[test]
fn f5_native_source_only_gap_characterization_control() {
    let (mut s, sk, _) = minimal();
    let path = scratch("f5-native-control.qcad");
    s.save_with_options(Some(&path), true).unwrap();
    let (_, _, _, native) = common::gui_edit_param_model(&path, "leg_margin", "10");
    let a = &s.project().sketches[s.project().sketch_index(sk).unwrap()];
    let b = &native.sketches[native.sketch_index(sk).unwrap()];
    let centre = entity_points(b.entity(b.patterns[0].source[0]).unwrap())[0];
    // Old source cx=5+6/2=8; edited cx=10+6/2=13. Native copies stay at x=8.
    assert_close(b.point(centre).unwrap().x, 13.0, 1e-7, "native moved source");
    for pattern in &a.patterns {
        for id in &pattern.instances {
            for p in entity_points(a.entity(*id).unwrap()) {
                let old = a.point(p).unwrap();
                let new = b.point(p).unwrap();
                assert_close(new.x, old.x, 1e-7, "native copy stale x");
                assert_close(new.y, old.y, 1e-7, "native copy stale y");
            }
        }
    }
}

fn stale_saved_source(name: &str) -> (Session, Id, Id) {
    let (mut s, sk, body) = minimal();
    let path = scratch(&format!("{name}-old.qcad"));
    s.save_with_options(Some(&path), true).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    project.parameters.iter_mut().find(|p| p.name == "leg_margin").unwrap().expr = "10".into();
    assert!(project.eval_parameters().is_empty());
    let si = project.sketch_index(sk).unwrap();
    assert!(project.solve_sketch(si) < 1e-7);
    let path = scratch(&format!("{name}-stale.qcad"));
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (s, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    // Open deliberately retains source cx=13 and stale copy cx=8 (native app gap).
    let p = &s.project().sketches[si];
    let src = entity_points(p.entity(p.patterns[0].source[0]).unwrap())[0];
    let dst = entity_points(p.entity(p.patterns[0].instances[0]).unwrap())[0];
    assert_close(p.point(src).unwrap().x, 13.0, 1e-7, "saved solved source");
    assert_close(p.point(dst).unwrap().x, 8.0, 1e-7, "open does not repair stale copy");
    (s, sk, body)
}
#[test]
fn f5_sketch_add_joining_source_repairs_solved_sketch() {
    let (mut s, sk, body) = stale_saved_source("f5-add");
    s.sketch_edit(sk, |s| {
        s.sketch_line(
            sk,
            &LineSpec { x1: 13.0.into(), y1: 20.0.into(), x2: 13.0.into(), y2: 25.0.into(), construction: true, dimensioned: false },
        )
    })
    .unwrap();
    matches(s.project(), sk);
    let r = 6.0 / 1.7320508;
    let h = r * (std::f64::consts::PI / 3.0).sin();
    bbox(&s, body, [13.0 - r, 20.0 - h, 0.0, 13.0 + r, 40.0 + h, 2.0]);
}
#[test]
fn f5_sketch_remove_constraint_repairs_solved_sketch() {
    let (mut s, sk, body) = stale_saved_source("f5-remove");
    let si = s.project().sketch_index(sk).unwrap();
    let ci = s.project().sketches[si]
        .constraints
        .iter()
        .position(|c| matches!(c,qymcad_core::model::Constraint::Distance{expr,..} if expr=="leg_margin+leg_w/2"))
        .unwrap();
    s.sketch_edit(sk, |s| s.sketch_remove_constraint(sk, ci)).unwrap();
    matches(s.project(), sk);
    let r = 6.0 / 1.7320508;
    let h = r * (std::f64::consts::PI / 3.0).sin();
    bbox(&s, body, [13.0 - r, 20.0 - h, 0.0, 13.0 + r, 40.0 + h, 2.0]);
}
#[test]
fn f5_subset_prefix_pick_does_not_expand_to_full_pattern() {
    let mut s = Session::new_part();
    assert!(s.param_set("x", &3.0.into()).unwrap().errors.is_empty());
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let src = s.sketch_rect(sk, &"x".into(), &0.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    s.sketch_pattern_create(sk, &src, &linear(4.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, sk, Some(vec![[3.0.into(), 0.0.into()], [13.0.into(), 0.0.into()]]));
    assert!(s.param_set("x", &6.0.into()).unwrap().errors.is_empty());
    matches(s.project(), sk);
    // Source and first copy only: centres 6,16, half-size1; V=2 prisms*2*2*2.
    bbox(&s, body, [5.0, -1.0, 0.0, 17.0, 1.0, 2.0]);
    assert_close(s.shape(body).unwrap().volume(), 16.0, 1e-6, "two selected prisms");
}

fn free_circle() -> (Session, Id, Id, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let source = s.sketch_circle(sk, &2.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    let si = s.project().sketch_index(sk).unwrap();
    let point = entity_points(s.project().sketches[si].entity(source).unwrap())[0];
    let x = s.project().sketches[si]
        .constraints
        .iter()
        .position(|c| matches!(c,Constraint::Distance {axis:1,a,b,..} if *a==point || *b==point))
        .unwrap();
    s.sketch_remove_constraint(sk, x).unwrap();
    s.sketch_pattern_create(sk, &[source], &linear(3.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, sk, None);
    // Radius=d/2=1; centres (2+k*10,2), k=0..2.
    bbox_with_tolerance(&s, body, [1.0, 1.0, 0.0, 23.0, 3.0, 2.0], 0.005);
    (s, sk, point, body)
}

#[test]
fn f5_sketch_add_join_dimensions_move_source() {
    let (mut s, sk, point, body) = free_circle();
    // Native point joining uses distance <=1e-6. The new driving x differs by5e-7,
    // so it joins the existing free centre and then moves it to the requested x.
    let x = 2.0 + 0.0000005;
    let (added, _) = s
        .sketch_edit(sk, |s| {
            s.sketch_line(
                sk,
                &LineSpec { x1: x.into(), y1: 2.0.into(), x2: x.into(), y2: 5.0.into(), construction: true, dimensioned: true },
            )
        })
        .unwrap();
    assert_eq!(added.points[0], point, "new line joins the source centre");
    let sketch = &s.project().sketches[s.project().sketch_index(sk).unwrap()];
    assert_close(sketch.point(point).unwrap().x, x, 1e-9, "new driving dimension moves joined source");
    matches(s.project(), sk);
    // Three radius1 circles translated by x-2; max centre=x+2*10.
    bbox_with_tolerance(&s, body, [x - 1.0, 1.0, 0.0, x + 2.0 * 10.0 + 1.0, 3.0, 2.0], 0.005);
}

#[test]
fn f5_loaded_chained_pattern_refusal_restores_recipe_and_brep() {
    let (mut s, sk, body) = minimal();
    let source_path = scratch("f5-loaded-chain-source.qcad");
    s.save_with_options(Some(&source_path), true).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(source_path.to_str().unwrap()).unwrap();
    let si = project.sketch_index(sk).unwrap();
    let instances = project.sketches[si].patterns[0].instances.clone();
    // Native files can contain chains; the engine creation guard must not be the only guard.
    project.add_pattern(si, &instances, PatternKind::Linear { count: 2, dx: 100.0, dy: 0.0, count2: 1, dx2: 0.0, dy2: 0.0 });
    let path = scratch("f5-loaded-chain.qcad");
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let before = archive(&mut s, "f5-loaded-chain-before.qcad");
    let old_points = coordinates(s.project(), sk);
    let old_bbox = s.shape(body).unwrap().bbox().unwrap();
    let error = s.param_set("leg_margin", &10.0.into()).unwrap_err();
    assert!(error.to_string().contains("pattern"), "{error}");
    assert_eq!(archive(&mut s, "f5-loaded-chain-after.qcad"), before);
    assert_eq!(coordinates(s.project(), sk), old_points);
    assert_eq!(s.shape(body).unwrap().bbox().unwrap(), old_bbox);
}

#[test]
fn f5_projection_consumer_rebuilds_after_source_only_refresh() {
    use qymcad_core::model::ProjSource;
    let mut s = Session::new_part();
    assert!(s.param_set("x", &3.0.into()).unwrap().errors.is_empty());
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let source = s.sketch_rect(sk, &"x".into(), &0.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    s.sketch_pattern_create(sk, &source, &linear(3.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, sk, None);
    let top = s
        .topology(Some(body), false)
        .unwrap()
        .faces
        .into_iter()
        .filter(|f| f.normal.is_some_and(|n| n[2] > 0.99))
        .min_by(|a, b| a.centroid[0].total_cmp(&b.centroid[0]))
        .unwrap()
        .id;
    let projected = s.sketch_create(&PlaneRef::Face { body, face: top }, Some("projected_source")).unwrap();
    let (projection, report) = s.sketch_project(projected, body, &[ProjSource::Face(top)]).unwrap();
    assert_eq!(projection.entities.len(), 4);
    assert!(report.is_none_or(|r| r.errors.is_empty()));
    let follower = extrude(&mut s, projected, None);
    // Original source centre=3, side2; follower starts on its stable top face at z2.
    bbox(&s, follower, [2.0, -1.0, 2.0, 4.0, 1.0, 4.0]);
    assert!(s.param_set("x", &6.0.into()).unwrap().errors.is_empty());
    matches(s.project(), sk);
    // New source centre=6; both source body and projection consumer translate +3X.
    bbox(&s, body, [5.0, -1.0, 0.0, 27.0, 1.0, 2.0]);
    bbox(&s, follower, [5.0, -1.0, 2.0, 7.0, 1.0, 4.0]);
}

#[test]
fn f5_polygon_subset_pick_keeps_hexagons_with_construction_circle() {
    let (mut s, sk, _) = minimal();
    // Source centre=(5+6/2,40/2)=(8,20); selected copy centres are (8,30),(8,40).
    // Each polygon source contains its construction circle, which is absent from its six-edge contour.
    let selected = extrude(&mut s, sk, Some(vec![[8.0.into(), 30.0.into()], [8.0.into(), 40.0.into()]]));
    assert!(s.param_set("leg_margin", &10.0.into()).unwrap().errors.is_empty());
    matches(s.project(), sk);
    // cx=10+6/2=13; preserve copy indices1,2, so cy=20+k*(6+4)=30,40.
    let radius = 6.0 / 1.7320508;
    let half_height = radius * (std::f64::consts::PI / 3.0).sin();
    bbox(&s, selected, [13.0 - radius, 30.0 - half_height, 0.0, 13.0 + radius, 40.0 + half_height, 2.0]);
    // Two extruded regular hexagons: V=2*(3sqrt3/2)*R^2*height2.
    assert_close(
        s.shape(selected).unwrap().volume(),
        2.0 * (3.0 * 3.0_f64.sqrt() / 2.0) * radius * radius * 2.0,
        1e-6,
        "two selected hexagonal prisms",
    );
}

#[test]
fn f5_legacy_prefix_subset_archive_keeps_only_source_and_first_copy() {
    let mut s = Session::new_part();
    assert!(s.param_set("x", &3.0.into()).unwrap().errors.is_empty());
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let source = s.sketch_rect(sk, &"x".into(), &0.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    s.sketch_pattern_create(sk, &source, &linear(4.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, sk, Some(vec![[3.0.into(), 0.0.into()], [13.0.into(), 0.0.into()]]));
    let path = scratch("f5-legacy-prefix-original.qcad");
    s.save_with_options(Some(&path), true).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    // Keep the legacy numeric instance-index fields, stripping the newer full/owner/boundary metadata.
    let metadata = project.feat_dims.get_mut(&body).unwrap();
    metadata.retain(|key, _| key.strip_prefix("__pattern_profile_").is_none_or(|suffix| suffix.parse::<usize>().is_ok()));
    assert_eq!(metadata.get("__pattern_profile_0").map(String::as_str), Some("0"));
    assert_eq!(metadata.get("__pattern_profile_1").map(String::as_str), Some("1"));
    assert!(!metadata.contains_key("__pattern_profile_full"));
    let legacy = scratch("f5-legacy-prefix.qcad");
    qymcad_io::save_project_guarded_with_brep(&project, legacy.to_str().unwrap(), &breps).unwrap();
    let (mut s, report) = Session::open(&legacy).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    // Before edit: two side2 prisms centred at3 and3+10, spanning x2..14.
    bbox(&s, body, [2.0, -1.0, 0.0, 14.0, 1.0, 2.0]);
    assert!(s.param_set("x", &6.0.into()).unwrap().errors.is_empty());
    matches(s.project(), sk);
    // After edit: preserve only indices0,1, centres6 and16; half-size1, height2.
    bbox(&s, body, [6.0 - 1.0, -1.0, 0.0, 6.0 + 10.0 + 1.0, 1.0, 2.0]);
    assert_close(s.shape(body).unwrap().volume(), 2.0 * 2.0 * 2.0 * 2.0, 1e-6, "legacy subset remains two prisms");
    let sketch = &s.project().sketches[s.project().sketch_index(sk).unwrap()];
    let pattern = &sketch.patterns[0];
    let expected: std::collections::HashSet<Id> = pattern.source.iter().chain(pattern.instances[..source.len()].iter()).copied().collect();
    let qymcad_core::feature::FeatureKind::Extrude { profiles, .. } = &s.project().timeline.iter().find(|n| n.id == body).unwrap().kind
    else {
        panic!("extrude")
    };
    assert_eq!(profiles.len(), 2);
    let actual: std::collections::HashSet<Id> =
        profiles.iter().flat_map(|cid| s.project().contours.ents_of(*cid).unwrap()).copied().collect();
    assert_eq!(actual, expected, "legacy subset picks exactly source and copy1 entities");
}

#[test]
fn f5_disconnected_source_subset_keeps_only_second_contour_of_copy_one() {
    let mut s = Session::new_part();
    assert!(s.param_set("x", &3.0.into()).unwrap().errors.is_empty());
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let mut source = s.sketch_rect(sk, &"x".into(), &0.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    source.extend(s.sketch_rect(sk, &"x".into(), &5.0.into(), &2.0.into(), &2.0.into(), false).unwrap());
    assert_eq!(source.len(), 8); // Two disconnected four-edge source contours.
    s.sketch_pattern_create(sk, &source, &linear(3.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, sk, Some(vec![[13.0.into(), 5.0.into()]]));
    // Select second source contour, translated by copy1*10: centre(3+10,5), side2.
    bbox(&s, body, [12.0, 4.0, 0.0, 14.0, 6.0, 2.0]);
    assert!(s.param_set("x", &6.0.into()).unwrap().errors.is_empty());
    assert_eq!(matches(s.project(), sk), 2 * 12 * 2 * 2); // Two copies of eight sides + four carried diagonals.
                                                          // Retain precisely second contour of copy1: centre(6+10,5), half-size1, height2.
    bbox(&s, body, [6.0 + 10.0 - 1.0, 5.0 - 1.0, 0.0, 6.0 + 10.0 + 1.0, 5.0 + 1.0, 2.0]);
    assert_close(s.shape(body).unwrap().volume(), 2.0 * 2.0 * 2.0, 1e-6, "one selected square prism");
    let sketch = &s.project().sketches[s.project().sketch_index(sk).unwrap()];
    let effective = s.project().copy_sketch_geometry(s.project().sketch_index(sk).unwrap(), &source, 0.0, 0.0).entities;
    let expected: std::collections::HashSet<Id> = effective
        .iter()
        .enumerate()
        .filter(|(_, e)| source[4..8].contains(&e.id))
        .map(|(offset, _)| sketch.patterns[0].instances[offset])
        .collect();
    let qymcad_core::feature::FeatureKind::Extrude { profiles, .. } = &s.project().timeline.iter().find(|n| n.id == body).unwrap().kind
    else {
        panic!("extrude")
    };
    assert_eq!(profiles.len(), 1);
    let actual: std::collections::HashSet<Id> = s.project().contours.ents_of(profiles[0]).unwrap().iter().copied().collect();
    assert_eq!(actual, expected, "subset keeps second contour of copy1, no sibling contours");
}

#[test]
fn f5_competing_native_source_driver_refuses_atomically() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let added = s
        .sketch_polygon(
            sk,
            &PolygonSpec {
                cx: 10.0.into(),
                cy: 10.0.into(),
                sides: 6,
                r: Some("2".into()),
                angle: Some(30.0.into()),
                vertex: None,
                construction: false,
                dimensioned: true,
            },
        )
        .unwrap();
    s.sketch_pattern_create(sk, &added.entities, &linear(3.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, sk, None);
    let path = scratch("f5-final-turn-source.qcad");
    s.save_with_options(Some(&path), true).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    // A clean native recipe may retain an item turn alongside its matching sketch driver.
    // Initially both impose 30 degrees; native evaluation reimposes the item before the final solve.
    project.set_feat_dim(added.points[0], "turn", "30".into());
    let path = scratch("f5-final-turn-native.qcad");
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let si = s.project().sketch_index(sk).unwrap();
    let constraint = s.project().sketches[si].constraints.iter().position(|c| matches!(c, Constraint::ArcLength { .. })).unwrap();
    let before = archive(&mut s, "f5-competing-driver-before.qcad");
    let old_points = coordinates(s.project(), sk);
    let old_bbox = s.shape(body).unwrap().bbox().unwrap();
    // Requested ArcLength=R*pi/3 means60 degrees; native item turn remains30.
    // Competing drivers cannot satisfy both: refuse instead of returning mismatched copies.
    let error = s
        .dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint }, Some(&(2.0 * std::f64::consts::PI / 3.0).into()), None)
        .unwrap_err();
    assert!(error.to_string().contains("conflict"), "{error}");
    assert_eq!(archive(&mut s, "f5-competing-driver-after.qcad"), before);
    assert_eq!(coordinates(s.project(), sk), old_points);
    assert_eq!(s.shape(body).unwrap().bbox().unwrap(), old_bbox);
    matches(s.project(), sk);
    // Restored radius2 hexagons at x10,20,30 with angles30+j*60: x+-sqrt3, y+-2.
    let h = 3.0_f64.sqrt();
    bbox(&s, body, [10.0 - h, 8.0, 0.0, 30.0 + h, 12.0, 2.0]);
}

#[test]
fn f5_dimension_source_position_refreshes_after_solve() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let source = s.sketch_circle(sk, &3.0.into(), &0.0.into(), &2.0.into(), false).unwrap();
    let si = s.project().sketch_index(sk).unwrap();
    // y=0 uses PointOnLine, leaving a unique native x-distance reference set.
    let constraint = s.project().sketches[si]
        .constraints
        .iter()
        .position(|c| matches!(c, Constraint::Distance { axis:1, d, .. } if (*d-3.0).abs()<1e-12))
        .unwrap();
    s.sketch_pattern_create(sk, &[source], &linear(3.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, sk, None);
    let result = s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint }, Some(&6.0.into()), None).unwrap();
    assert!(result.rebuild.errors.is_empty(), "{:?}", result.rebuild);
    matches(s.project(), sk);
    let sketch = &s.project().sketches[si];
    let centre = sketch.point(entity_points(sketch.entity(source).unwrap())[0]).unwrap();
    assert_close(centre.x, 6.0, 1e-7, "edited source x=6");
    assert_close(centre.y, 0.0, 1e-7, "source stays on x axis");
    // Radius=diameter2/2=1, centres6+k*10 for k0..2, height2; curved bbox F-066 deflection.
    bbox_with_tolerance(&s, body, [6.0 - 1.0, -1.0, 0.0, 6.0 + 2.0 * 10.0 + 1.0, 1.0, 2.0], 0.005);
}

/// Stock 2x2 at (x,0), height 2; a second sketch projects its top edges and patterns them 3 x dx 10.
fn projected_pattern(reach_pattern_sketch: bool) -> (Session, Id, Id) {
    use qymcad_core::model::ProjSource;
    let mut s = Session::new_part();
    s.param_set("x", &3.0.into()).unwrap();
    let stock = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(stock, &"x".into(), &0.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    let body = extrude(&mut s, stock, None);
    let edges: Vec<_> = s
        .topology(Some(body), false)
        .unwrap()
        .edges
        .into_iter()
        .filter(|e| (e.a[2] - 2.0).abs() < 1e-6 && (e.b[2] - 2.0).abs() < 1e-6)
        .map(|e| ProjSource::Edge(e.id))
        .collect();
    assert_eq!(edges.len(), 4);
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("projected")).unwrap();
    let (projection, _) = s.sketch_project(sk, body, &edges).unwrap();
    if reach_pattern_sketch {
        s.sketch_point(sk, &"x".into(), &100.0.into(), true).unwrap();
    }
    s.sketch_pattern_create(sk, &projection.entities, &linear(3.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let follower = extrude(&mut s, sk, None);
    (s, sk, follower)
}
fn f5r_projected_source_follows_rebuild(reach_pattern_sketch: bool) {
    let (mut s, sk, follower) = projected_pattern(reach_pattern_sketch);
    bbox(&s, follower, [2.0, -1.0, 0.0, 24.0, 1.0, 2.0]);
    let r = s.param_set("x", &6.0.into()).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    matches(s.project(), sk);
    // Source [5,7], copies +10 and +20: the far edge is 7 + 20 = 27. Volume is 3 x 4 x 2 = 24 either way.
    bbox(&s, follower, [5.0, -1.0, 0.0, 27.0, 1.0, 2.0]);
    let path = scratch(&format!("f5r-projected-{reach_pattern_sketch}.qcad"));
    s.save(Some(&path)).unwrap();
    let (reopened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    matches(reopened.project(), sk);
    bbox(&reopened, follower, [5.0, -1.0, 0.0, 27.0, 1.0, 2.0]);
}
#[test]
fn f5r_projected_source_moved_only_by_the_rebuild_refreshes_instances() {
    f5r_projected_source_follows_rebuild(false);
}
#[test]
fn f5r_projected_source_of_a_solved_sketch_refreshes_after_the_rebuild() {
    f5r_projected_source_follows_rebuild(true);
}
#[test]
fn f5r_projected_refresh_undoes_in_one_step() {
    let (mut s, sk, follower) = projected_pattern(false);
    let before = coordinates(s.project(), sk);
    let snapshot = s.begin_tool_edit().unwrap();
    let r = s.param_set("x", &6.0.into());
    s.finish_tool_edit(snapshot, r.is_ok());
    assert!(r.unwrap().errors.is_empty());
    assert_ne!(coordinates(s.project(), sk), before);
    s.undo().unwrap();
    assert_eq!(coordinates(s.project(), sk), before);
    bbox(&s, follower, [2.0, -1.0, 0.0, 24.0, 1.0, 2.0]);
}
#[test]
fn f5r_explicit_pick_of_a_nested_contour_survives_a_source_move() {
    let mut s = Session::new_part();
    s.param_set("x", &3.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let outer = s.sketch_circle(sk, &"x".into(), &0.0.into(), &4.0.into(), false).unwrap();
    let inner = s.sketch_circle(sk, &"x".into(), &0.0.into(), &2.0.into(), false).unwrap();
    s.sketch_pattern_create(sk, &[outer, inner], &linear(3.0.into(), 10.0.into(), 0.0.into())).unwrap();
    // The point (13,0) lies in the inner disc of copy 1: radius 1, height 2, V = 2*pi.
    let body = extrude(&mut s, sk, Some(vec![[13.0.into(), 0.0.into()]]));
    assert_close(s.shape(body).unwrap().volume(), 2.0 * std::f64::consts::PI, 1e-5, "inner disc");
    let r = s.param_set("x", &6.0.into()).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    matches(s.project(), sk);
    assert_close(s.shape(body).unwrap().volume(), 2.0 * std::f64::consts::PI, 1e-5, "still the inner disc");
    bbox_with_tolerance(&s, body, [15.0, -1.0, 0.0, 17.0, 1.0, 2.0], 0.005);
}

fn top_edges(s: &mut Session, body: Id, x: std::ops::Range<f64>) -> Vec<qymcad_core::model::ProjSource> {
    s.topology(Some(body), false)
        .unwrap()
        .edges
        .into_iter()
        .filter(|e| (e.a[2] - 2.0).abs() < 1e-6 && (e.b[2] - 2.0).abs() < 1e-6 && x.contains(&e.a[0]) && x.contains(&e.b[0]))
        .map(|e| qymcad_core::model::ProjSource::Edge(e.id))
        .collect()
}
#[test]
fn f5r_projected_circle_resize_refreshes_instances() {
    let mut s = Session::new_part();
    s.param_set("d", &2.0.into()).unwrap();
    let stock = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(stock, &3.0.into(), &0.0.into(), &"d".into(), false).unwrap();
    let body = extrude(&mut s, stock, None);
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("projected")).unwrap();
    let edges = top_edges(&mut s, body, -100.0..100.0);
    let (projection, _) = s.sketch_project(sk, body, &edges).unwrap();
    s.sketch_pattern_create(sk, &projection.entities, &linear(3.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let follower = extrude(&mut s, sk, None);
    assert_close(s.shape(follower).unwrap().volume(), 6.0 * std::f64::consts::PI, 1e-5, "3 discs r1 h2");
    assert!(s.param_set("d", &4.0.into()).unwrap().errors.is_empty());
    // Three discs of radius 2, height 2: V = 3 x 4pi x 2 = 24pi.
    assert_close(s.shape(follower).unwrap().volume(), 24.0 * std::f64::consts::PI, 1e-5, "3 discs r2 h2");
    bbox_with_tolerance(&s, follower, [1.0, -2.0, 0.0, 25.0, 2.0, 2.0], 0.02);
}
/// Sketch A: 2x2 at (x,0) patterned 3 x dx 10 and extruded. Sketch B projects copy 1 of A's body.
fn projection_of_a_copy() -> (Session, Id, Id, Id) {
    let mut s = Session::new_part();
    s.param_set("x", &3.0.into()).unwrap();
    let a = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("A")).unwrap();
    let rect = s.sketch_rect(a, &"x".into(), &0.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    s.sketch_pattern_create(a, &rect, &linear(3.0.into(), 10.0.into(), 0.0.into())).unwrap();
    let a_body = extrude(&mut s, a, None);
    let b = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("B")).unwrap();
    let edges = top_edges(&mut s, a_body, 11.0..15.0);
    assert_eq!(edges.len(), 4);
    let (projection, _) = s.sketch_project(b, a_body, &edges).unwrap();
    s.sketch_pattern_create(b, &projection.entities, &linear(3.0.into(), 100.0.into(), 0.0.into())).unwrap();
    let b_body = extrude(&mut s, b, None);
    (s, a, b, b_body)
}
fn projection_count(s: &Session, sketch: Id) -> usize {
    s.project().sketches[s.project().sketch_index(sketch).unwrap()].projections.len()
}
#[test]
fn f5r_recreating_a_projected_copy_is_refused_before_it_detaches_the_projection() {
    let (mut s, _, b, b_body) = projection_of_a_copy();
    let before = serde_json::to_value(s.project()).unwrap();
    let error = s.param_set("x", &6.0.into()).unwrap_err().to_string();
    assert!(error.contains("would be released"), "{error}");
    assert_eq!(serde_json::to_value(s.project()).unwrap(), before);
    assert_eq!(projection_count(&s, b), 4);
    bbox(&s, b_body, [12.0, -1.0, 0.0, 214.0, 1.0, 2.0]);

    // The advice in the refusal works: delete the projecting sketch, edit, then build it again.
    assert!(error.contains(&format!("delete sketch {b} with cascade, make the edit, then build it again")), "{error}");
    s.feature_delete(b, true).unwrap();
    s.param_set("x", &6.0.into()).unwrap();
    let a_body = s.project().timeline.iter().find_map(|n| n.kind.bodies().first().copied()).unwrap();
    let edges = top_edges(&mut s, a_body, 14.0..18.0);
    assert_eq!(edges.len(), 4);
    let again = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("B again")).unwrap();
    let (projection, _) = s.sketch_project(again, a_body, &edges).unwrap();
    s.sketch_pattern_create(again, &projection.entities, &linear(3.0.into(), 100.0.into(), 0.0.into())).unwrap();
    let rebuilt = extrude(&mut s, again, None);
    // Copy 1 of A at x = 6 + 10, 2 wide, repeated at 100 and 200.
    bbox(&s, rebuilt, [15.0, -1.0, 0.0, 217.0, 1.0, 2.0]);
}
#[test]
fn f5r_edit_that_leaves_copies_in_place_keeps_their_ids_and_the_projection() {
    let (mut s, a, b, b_body) = projection_of_a_copy();
    let instances = |s: &Session| s.project().sketches[s.project().sketch_index(a).unwrap()].patterns[0].instances.clone();
    let before = instances(&s);
    // A separate curve solves sketch A without moving the source of its pattern.
    s.sketch_point(a, &50.0.into(), &50.0.into(), true).unwrap();
    assert_eq!(instances(&s), before, "unchanged copies are not recreated");
    assert_eq!(projection_count(&s, b), 4);
    bbox(&s, b_body, [12.0, -1.0, 0.0, 214.0, 1.0, 2.0]);
}
#[test]
fn f5r_legacy_nested_pick_is_refused_instead_of_becoming_the_annulus() {
    let mut s = Session::new_part();
    s.param_set("n", &3.0.into()).unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let outer = s.sketch_circle(sk, &3.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
    let inner = s.sketch_circle(sk, &3.0.into(), &0.0.into(), &2.0.into(), false).unwrap();
    s.sketch_pattern_create(sk, &[outer, inner], &linear("n".into(), 10.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, sk, Some(vec![[13.0.into(), 0.0.into()]]));
    let path = scratch("f5r-legacy-nested-original.qcad");
    s.save_with_options(Some(&path), true).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let metadata = project.feat_dims.get_mut(&body).unwrap();
    metadata.retain(|key, _| key.strip_prefix("__pattern_profile_").is_none_or(|suffix| suffix.parse::<usize>().is_ok()));
    let legacy = scratch("f5r-legacy-nested.qcad");
    qymcad_io::save_project_guarded_with_brep(&project, legacy.to_str().unwrap(), &breps).unwrap();
    let (mut s, report) = Session::open(&legacy).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let two_pi = 2.0 * std::f64::consts::PI; // Inner disc: radius 1, height 2.
    assert_close(s.shape(body).unwrap().volume(), two_pi, 1e-5, "inner disc");
    let before = serde_json::to_value(s.project()).unwrap();
    let error = s.param_set("n", &4.0.into()).unwrap_err().to_string();
    assert!(error.contains("ambiguous"), "{error}");
    assert_eq!(serde_json::to_value(s.project()).unwrap(), before);
    assert_close(s.shape(body).unwrap().volume(), two_pi, 1e-5, "still the inner disc");
}
/// Native stale case: only the source arc is reversed in the file. Same endpoints and radius, other winding.
#[test]
fn f5r_copies_with_the_opposite_arc_winding_are_not_considered_unchanged() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let spec: ArcSpec =
        serde_json::from_value(serde_json::json!({"cx":3,"cy":4,"r":2,"start_angle":0,"end_angle":90,"dimensioned":false})).unwrap();
    let arc = s.sketch_arc(sk, &spec).unwrap();
    let chord = s
        .sketch_line(
            sk,
            &LineSpec { x1: 5.0.into(), y1: 4.0.into(), x2: 3.0.into(), y2: 6.0.into(), construction: false, dimensioned: false },
        )
        .unwrap();
    let mut source = arc.entities.clone();
    source.extend(chord.entities.iter().copied());
    s.sketch_pattern_create(sk, &source, &linear(3.0.into(), 20.0.into(), 0.0.into())).unwrap();
    let body = extrude(&mut s, sk, None);
    // Minor segment of a quarter circle r=2: (pi - 2), height 2, three of them.
    assert_close(s.shape(body).unwrap().volume(), 6.0 * std::f64::consts::PI - 12.0, 1e-5, "minor segments");
    let path = scratch("f5r-winding-original.qcad");
    s.save_with_options(Some(&path), true).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let si = project.sketch_index(sk).unwrap();
    let curve = project.sketches[si].entities.iter_mut().find(|e| e.id == arc.entities[0]).unwrap();
    assert!(curve.kind.reverse_arc());
    project.regen_sketch(si);
    let stale = scratch("f5r-winding-stale.qcad");
    qymcad_io::save_project_guarded_with_brep(&project, stale.to_str().unwrap(), &breps).unwrap();
    let (mut s, report) = Session::open(&stale).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    s.sketch_edit(sk, |s| s.sketch_point(sk, &70.0.into(), &80.0.into(), true)).unwrap();
    // Major segments: area 3pi + 2, height 2, three of them = 18pi + 12.
    assert_close(s.shape(body).unwrap().volume(), 18.0 * std::f64::consts::PI + 12.0, 1e-5, "major segments");
}
/// Native reprojection replaces a changed spline with a new entity; the pattern source keeps the old one.
#[test]
fn f5r_replaced_projected_spline_source_is_refused_instead_of_freezing_the_copies() {
    let mut s = Session::new_part();
    s.param_set("v", &5.0.into()).unwrap();
    let stock = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let spec: SplineSpec = serde_json::from_value(serde_json::json!({"points":[[2,3],[12,"v+3"],[22,3]]})).unwrap();
    s.sketch_spline(stock, &spec).unwrap();
    s.sketch_line(
        stock,
        &LineSpec { x1: 22.0.into(), y1: 3.0.into(), x2: 2.0.into(), y2: 3.0.into(), construction: false, dimensioned: false },
    )
    .unwrap();
    let body = extrude(&mut s, stock, None);
    let edges = top_edges(&mut s, body, -100.0..100.0);
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("projected")).unwrap();
    let (projection, _) = s.sketch_project(sk, body, &edges).unwrap();
    s.sketch_pattern_create(sk, &projection.entities, &linear(3.0.into(), 40.0.into(), 0.0.into())).unwrap();
    let follower = extrude(&mut s, sk, None);
    let volume = s.shape(follower).unwrap().volume();
    let before = serde_json::to_value(s.project()).unwrap();
    let error = s.param_set("v", &8.0.into()).unwrap_err().to_string();
    assert!(error.contains("would be released"), "{error}");
    assert_eq!(serde_json::to_value(s.project()).unwrap(), before);
    assert_eq!(s.shape(follower).unwrap().volume(), volume);

    // The advice in the refusal works: delete the projecting sketch, edit, then build it again.
    assert!(error.contains(&format!("delete sketch {sk} with cascade, make the edit, then build it again")), "{error}");
    s.feature_delete(sk, true).unwrap();
    s.param_set("v", &8.0.into()).unwrap();
    let stock_area = s.shape(body).unwrap().volume() / 2.0;
    let edges = top_edges(&mut s, body, -100.0..100.0);
    let again = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("projected again")).unwrap();
    let (projection, _) = s.sketch_project(again, body, &edges).unwrap();
    s.sketch_pattern_create(again, &projection.entities, &linear(3.0.into(), 40.0.into(), 0.0.into())).unwrap();
    let rebuilt = extrude(&mut s, again, None);
    // Three copies of the v=8 profile, height 2; the projected spline is approximate, as in a fresh model.
    assert_close(s.shape(rebuilt).unwrap().volume(), 3.0 * 2.0 * stock_area, 0.1, "rebuilt from the new shape");
}
