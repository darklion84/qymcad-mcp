//! Reorder acceptance against native app scheduling and projection/pattern guards.
mod common;
use common::*;
use qymcad_core::model::{entity_points, ProjSource};
use qymcad_engine::*;
use std::{collections::BTreeMap, f64::consts::PI, io::Read};

fn extrude(s: &mut Session, sketch: Id, height: f64) -> Id {
    s.extrude(&Extrude {
        sketch,
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
fn block(s: &mut Session, x: f64, width: Num, height: f64) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &x.into(), &0.0.into(), &width, &10.0.into(), false).unwrap();
    extrude(s, sk, height)
}
fn linear(dx: f64) -> SketchPatternKind {
    SketchPatternKind::Linear { count: 3.0.into(), dx: dx.into(), dy: 0.0.into(), count2: None, dx2: None, dy2: None }
}
fn top_edges(s: &mut Session, body: Id, height: f64, x: std::ops::Range<f64>) -> Vec<ProjSource> {
    s.topology(Some(body), false)
        .unwrap()
        .edges
        .into_iter()
        .filter(|e| (e.a[2] - height).abs() < 1e-6 && (e.b[2] - height).abs() < 1e-6 && x.contains(&e.a[0]) && x.contains(&e.b[0]))
        .map(|e| ProjSource::Edge(e.id))
        .collect()
}
fn archive(s: &mut Session, label: &str) -> BTreeMap<String, Vec<u8>> {
    let path = scratch(label);
    s.save_with_options(Some(&path), true).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).unwrap();
            (f.name().into(), bytes)
        })
        .collect()
}
fn shift(s: &mut Session, body: Id) -> Id {
    s.move_body(&Move { body: Some(body), translation: Some([5.0.into(), 0.0.into(), 0.0.into()]), rotation: None, name: None })
        .unwrap()
        .0
        .body
}
fn assert_copies(s: &Session, sketch: Id, dx: f64) {
    let p = s.project();
    let si = p.sketch_index(sketch).unwrap();
    let sk = &p.sketches[si];
    let pat = &sk.patterns[0];
    let source = p.copy_sketch_geometry(si, &pat.source, 0.0, 0.0).entities;
    for (copy, chunk) in pat.instances.chunks(source.len()).enumerate() {
        for (src, dst) in source.iter().zip(chunk) {
            for (a, b) in entity_points(src).into_iter().zip(entity_points(sk.entity(*dst).unwrap())) {
                let a = sk.point(a).unwrap();
                let b = sk.point(b).unwrap();
                // Three copies: p' = p + (copy+1)*(dx,0).
                assert_close(b.x, a.x + (copy + 1) as f64 * dx, 1e-7, "settled copy x");
                assert_close(b.y, a.y, 1e-7, "settled copy y");
            }
        }
    }
}

fn prism_array_hole() -> (Session, Id, Id, Id) {
    let mut s = Session::new_part();
    s.param_set("w", &10.0.into()).unwrap();
    let stock = block(&mut s, 0.0, "w".into(), 10.0);
    // An explicit face pick retains the original prism cap. An Extreme-Z query becomes ambiguous
    // when the array introduces a second equally high cap; retaining that query must refuse.
    let top = s.topology(Some(stock), false).unwrap().faces.into_iter().find(|f| f.normal.unwrap()[2] > 0.99).unwrap().id;
    let hole = s
        .hole(&Hole {
            body: Some(stock),
            face: Sel::Ids(vec![top]),
            at: Some([0.0, 0.0, 10.0]),
            offset: None,
            diameter: 2.0.into(),
            depth: Some(10.0.into()),
            kind: HoleKind::Plain,
            dia2: None,
            depth2: None,
            name: None,
        })
        .unwrap()
        .0;
    let array =
        s.linear_array(Some(hole), &ArrayDir { dx: 20.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: 2.0.into() }, None, None).unwrap().0;
    (s, stock, hole, array)
}

#[test]
fn reordered_prism_native_load_full_rebuild_and_parameter_edit() {
    for direction in ["up", "down"] {
        let (mut s, stock, hole, array) = prism_array_hole();
        // 2 copies of 10*10*10, one d2 through bore: 2000 - pi*1^2*10.
        let path = scratch(&format!("reorder-native-before-{direction}.qcad"));
        s.save(Some(&path)).unwrap();
        let mut native = gui_reorder_load(&path);
        let target = if direction == "up" { array } else { hole };
        assert!(gui_reorder_step(&mut native, target, direction));
        assert_eq!(gui_reorder_results(&native).iter().map(|x| x.0).collect::<Vec<_>>(), vec![hole]);
        assert_close(gui_reorder_results(&native)[0].1, 2000.0 - 10.0 * PI, 1e-6, "GUI reorder");
        let result = s.feature_reorder(target, direction).unwrap();
        assert!(result.changed);
        let path = scratch(&format!("reorder-native-after-{direction}.qcad"));
        s.save(Some(&path)).unwrap();
        let mut native = gui_reorder_load(&path);
        assert_eq!(native.project.timeline.iter().map(|n| n.id).collect::<Vec<_>>(), result.order);
        let input = |id| native.project.timeline.iter().find(|n| n.id == id).unwrap().kind.inputs();
        assert_eq!(input(array), vec![stock]);
        assert_eq!(input(hole), vec![array]);
        assert_close(gui_reorder_results(&native)[0].1, 2000.0 - 10.0 * PI, 1e-6, "saved native B-rep");
        gui_reorder_rebuild(&mut native, true);
        assert_eq!(gui_reorder_results(&native).iter().map(|x| x.0).collect::<Vec<_>>(), vec![hole]);
        assert_close(gui_reorder_results(&native)[0].1, 2000.0 - 10.0 * PI, 1e-6, "forced native full rebuild");
        // Width 12: two 12*10*10 prisms minus the same one bore, 2400 - 10*pi.
        gui_reorder_edit_param(&mut native, "w", "12");
        assert_eq!(gui_reorder_results(&native).iter().map(|x| x.0).collect::<Vec<_>>(), vec![hole]);
        assert_close(gui_reorder_results(&native)[0].1, 2400.0 - 10.0 * PI, 1e-6, "same native model after width edit");
        assert_close(gui_edit_param_model(&path, "w", "12").0, 2400.0 - 10.0 * PI, 1e-6, "native parameter schedule");
    }
}

#[test]
fn projected_spline_source_change_refuses_but_unrelated_move_succeeds() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_spline(sk, &serde_json::from_value(serde_json::json!({"points":[[2,3],[12,8],[22,3]]})).unwrap()).unwrap();
    s.sketch_line(
        sk,
        &LineSpec { x1: 22.0.into(), y1: 3.0.into(), x2: 2.0.into(), y2: 3.0.into(), construction: false, dimensioned: false },
    )
    .unwrap();
    let stock = extrude(&mut s, sk, 2.0);
    let independent = block(&mut s, 100.0, 2.0.into(), 2.0);
    let edges = top_edges(&mut s, stock, 2.0, -100.0..100.0);
    let projected = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_project(projected, stock, &edges).unwrap();
    let follower = extrude(&mut s, projected, 2.0);
    let moved = shift(&mut s, stock);
    // Move stays below projected sketch: unrelated timeline permutation is legal even with a live spline.
    assert!(s.feature_reorder(moved, "up").unwrap().changed);
    assert!(s.shape(follower).is_some());
    let before = archive(&mut s, "reorder-spline-before.qcad");
    let error = s.feature_reorder(moved, "up").unwrap_err().to_string();
    assert_eq!(error,format!("invalid request: cannot reorder: projected spline in sketch {projected} would change its source geometry; delete that sketch with cascade, reorder, then recreate it (F-141)"));
    assert_eq!(archive(&mut s, "reorder-spline-after.qcad"), before);
    assert!(s.project().timeline.iter().any(|n| n.id == independent));
}

#[test]
fn projected_pattern_copy_release_refuses_but_unrelated_moves_keep_ids_and_coordinates() {
    let mut s = Session::new_part();
    let stock = block(&mut s, 3.0, 2.0.into(), 2.0);
    let _independent = block(&mut s, 300.0, 2.0.into(), 2.0);
    let edges = top_edges(&mut s, stock, 2.0, -100.0..100.0);
    let a = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let projected = s.sketch_project(a, stock, &edges).unwrap().0;
    s.sketch_pattern_create(a, &projected.entities, &linear(20.0)).unwrap();
    let abody = extrude(&mut s, a, 2.0);
    let edges = top_edges(&mut s, abody, 2.0, 21.0..25.0);
    assert_eq!(edges.len(), 4);
    let b = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let projected = s.sketch_project(b, abody, &edges).unwrap().0;
    s.sketch_pattern_create(b, &projected.entities, &linear(100.0)).unwrap();
    let bbody = extrude(&mut s, b, 2.0);
    let moved = shift(&mut s, stock);
    let instances = s.project().sketches[s.project().sketch_index(a).unwrap()].patterns[0].instances.clone();
    // Two harmless moves with both projection and pattern present must preserve settled copy ids.
    assert!(s.feature_reorder(moved, "up").unwrap().changed);
    assert!(s.feature_reorder(moved, "up").unwrap().changed);
    assert_eq!(s.project().sketches[s.project().sketch_index(a).unwrap()].patterns[0].instances, instances);
    assert_copies(&s, a, 20.0);
    assert_copies(&s, b, 100.0);
    // A copy at x=3+20, 2 wide, 10 deep; B repeats it at +100/+200. V=3*2*10*2=120.
    assert_close(s.shape(bbody).unwrap().volume(), 120.0, 1e-6, "three projected copies");
    let before = archive(&mut s, "reorder-pattern-before.qcad");
    let sk = &s.project().sketches[s.project().sketch_index(b).unwrap()];
    let (entity, projection) = sk.projections.iter().flat_map(|p| p.entities.iter().map(|entity| (*entity, p.id))).min().unwrap();
    let expected = format!(
        "the model did not rebuild (operation rolled back): {} ({b}): invalid request: projection {projection} in sketch {b} would be released: this edit replaces its curve {entity}, a sketch pattern source, so the copies would stay at the old shape; delete sketch {b} with cascade, make the edit, then build it again",
        s.node_name(b)
    );
    let error = s.feature_reorder(moved, "up").unwrap_err().to_string();
    assert_eq!(error, expected);
    assert_eq!(archive(&mut s, "reorder-pattern-after.qcad"), before);
}

#[test]
fn projected_pattern_source_settles_after_a_legal_source_lineage_move() {
    let mut s = Session::new_part();
    let stock = block(&mut s, 3.0, 2.0.into(), 2.0);
    let _independent = block(&mut s, 300.0, 2.0.into(), 2.0);
    let edges = top_edges(&mut s, stock, 2.0, -100.0..100.0);
    let a = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let projected = s.sketch_project(a, stock, &edges).unwrap().0;
    s.sketch_pattern_create(a, &projected.entities, &linear(20.0)).unwrap();
    let abody = extrude(&mut s, a, 2.0);
    let moved = shift(&mut s, stock);
    assert!(s.feature_reorder(moved, "up").unwrap().changed);
    assert!(s.feature_reorder(moved, "up").unwrap().changed);
    assert_copies(&s, a, 20.0);
    // Source centre 3+5=8, width2: [7,9]. Copies +20/+40 produce bounds [7,49].
    // Three 2*10*2 prisms have total volume120.
    let _gate = qymcad_kernel::kernel_gate();
    let shape = s.shape(abody).unwrap();
    assert_close(shape.volume(), 120.0, 1e-6, "settled projected source volume");
    let bbox = shape.bbox().unwrap();
    for (got, want) in bbox.into_iter().zip([7.0, -5.0, 0.0, 49.0, 5.0, 2.0]) {
        assert_close(got, want, 1e-6, "settled projected source bbox");
    }
}
