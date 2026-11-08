//! Reorder acceptance against native app scheduling and projection/pattern guards.
mod common;
use common::*;
use qymcad_core::model::{entity_points, Constraint, ProjSource, Project};
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

fn shift_by(s: &mut Session, body: Id, dx: f64) -> Id {
    s.move_body(&Move { body: Some(body), translation: Some([dx.into(), 0.0.into(), 0.0.into()]), rotation: None, name: None })
        .unwrap()
        .0
        .body
}
fn spline_sketch(s: &mut Session) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_spline(sk, &serde_json::from_value(serde_json::json!({"points":[[2,3],[12,8],[22,3]]})).unwrap()).unwrap();
    s.sketch_line(
        sk,
        &LineSpec { x1: 22.0.into(), y1: 3.0.into(), x2: 2.0.into(), y2: 3.0.into(), construction: false, dimensioned: false },
    )
    .unwrap();
    sk
}
fn projected_points(s: &Session, sketch: Id) -> Vec<f64> {
    let p = s.project();
    let sk = &p.sketches[p.sketch_index(sketch).unwrap()];
    assert!(!sk.projections.is_empty());
    sk.projections
        .iter()
        .flat_map(|projection| &projection.entities)
        .flat_map(|id| entity_points(sk.entity(*id).unwrap()))
        .flat_map(|id| {
            let point = sk.point(id).unwrap();
            [point.x, point.y]
        })
        .collect()
}
/// A 10 mm cube with a bore at the origin, arrayed 20 mm along X.
fn bored_array(s: &mut Session) -> (Id, Id) {
    let stock = block(s, 0.0, 10.0.into(), 10.0);
    let top = s.topology(Some(stock), false).unwrap().faces.into_iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.9)).unwrap().id;
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
    (hole, array)
}

/// Review R1: the array's second bore vanishes when the hole moves after the array, so its projected rim has no source.
#[test]
fn move_that_would_release_a_projected_circle_refuses_atomically() {
    let mut s = Session::new_part();
    let (_, array) = bored_array(&mut s);
    let rim = s
        .topology(Some(array), false)
        .unwrap()
        .edges
        .iter()
        .find(|e| e.kind == EdgeKind::Circle && e.a[0] > 15.0 && (e.a[2] - 10.0).abs() < 1e-6)
        .unwrap()
        .id;
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_project(sk, array, &[ProjSource::Edge(rim)]).unwrap();
    extrude(&mut s, sk, 10.0);
    let before = archive(&mut s, "reorder-lost-rim-before.qcad");
    let error = s.feature_reorder(array, "up").unwrap_err().to_string();
    assert!(error.contains(&format!("in sketch {sk} would be released")), "{error}");
    assert_eq!(archive(&mut s, "reorder-lost-rim-after.qcad"), before);
}

/// Review R2: box 2 already reads a value measured in the later sketch 6; exchanging 4 and 5 adds no such relation.
#[test]
fn existing_forward_measurement_does_not_block_an_independent_exchange() {
    let mut p = Project::default();
    p.ensure_document();
    let a = p.add_box(10.0, 10.0, 10.0);
    let b = p.add_box(2.0, 2.0, 2.0);
    let c = p.add_box(3.0, 3.0, 3.0);
    let sk = p.add_sketch("measurement", vec![], None);
    p.add_sketch_node(sk, "measurement");
    let si = p.sketch_index(sk).unwrap();
    let pa = p.sketch_point_at(si, 0.0, 0.0, 1e-9);
    let pb = p.sketch_point_at(si, 10.0, 0.0, 1e-9);
    p.sketches[si].constraints.push(Constraint::Distance {
        a: pa,
        b: pb,
        d: 10.0,
        off: 0.0,
        expr: String::new(),
        driven: true,
        axis: 1,
        at: None,
    });
    assert!(p.add_named_dim("measured_w".into(), sk, vec![pa, pb]));
    p.solve_sketch(si);
    p.set_feat_dim(a, "dx", "measured_w".into());
    let path = scratch("reorder-forward-before.qcad");
    qymcad_io::save_project(&p, path.to_str().unwrap()).unwrap();
    let (mut s, opened) = Session::open(&path).unwrap();
    assert!(opened.errors.is_empty(), "{opened:?}");

    let result = s.feature_reorder(c, "up").unwrap();
    assert!(result.changed);
    assert_eq!(result.order, vec![a, c, b, sk]);
    let saved = scratch("reorder-forward-after.qcad");
    s.save(Some(&saved)).unwrap();
    let mut native = gui_reorder_load(&saved);
    gui_reorder_rebuild(&mut native, true);
    // Boxes 10^3, 3^3, 2^3 in the new order.
    let volumes = gui_reorder_results(&native);
    assert_eq!(volumes.iter().map(|v| v.0).collect::<Vec<_>>(), vec![a, c, b]);
    for (actual, expected) in volumes.iter().zip([1000.0, 27.0, 8.0]) {
        assert_close(actual.1, expected, 1e-6, "forced native rebuild");
    }
}

/// Review R3: moves of +5 and -5 accumulate to identity, so entering the projection's prefix changes the source id only.
#[test]
fn identity_move_before_a_projected_spline_succeeds() {
    let mut s = Session::new_part();
    let sk = spline_sketch(&mut s);
    let stock = extrude(&mut s, sk, 10.0);
    block(&mut s, 100.0, 10.0.into(), 10.0);
    let edges = top_edges(&mut s, stock, 10.0, -100.0..100.0);
    let projected = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_project(projected, stock, &edges).unwrap();
    let follower = extrude(&mut s, projected, 10.0);
    let moved = shift_by(&mut s, stock, 5.0);
    assert_eq!(shift_by(&mut s, moved, -5.0), moved);
    assert!(s.feature_reorder(moved, "up").unwrap().changed);
    let points = projected_points(&s, projected);
    let volume = s.current_rebuild().bodies.iter().find(|b| b.id == follower).unwrap().volume;

    let result = s.feature_reorder(moved, "up").unwrap();
    assert!(result.changed && result.rebuild.errors.is_empty());
    let after = projected_points(&s, projected);
    assert_eq!(after.len(), points.len());
    for (a, b) in after.iter().zip(&points) {
        assert_close(*a, *b, 1e-9, "projected spline point");
    }
    assert_close(s.current_rebuild().bodies.iter().find(|b| b.id == follower).unwrap().volume, volume, 1e-9, "follower");
}

/// Review R4: the instance id stays the projection's source while its part's last body changes from spline prism to box.
/// Only comparing the rebuilt geometry can see this.
#[test]
fn projected_spline_through_an_instance_refuses_when_only_geometry_changes() {
    let mut s = Session::new_part();
    let rect = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(rect, &0.0.into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    let spline = spline_sketch(&mut s);
    extrude(&mut s, rect, 10.0);
    let last = extrude(&mut s, spline, 10.0);
    let mut p = s.project().clone();
    let part = p.active_component.unwrap();
    let root = p.root;
    let copy = p.clone_part(part, root).unwrap();
    p.set_active_component(Some(copy));
    let instance = p.timeline.last().unwrap().id;
    let path = scratch("reorder-spline-instance.qcad");
    qymcad_io::save_project(&p, path.to_str().unwrap()).unwrap();
    let (mut s, opened) = Session::open(&path).unwrap();
    assert!(opened.errors.is_empty(), "{opened:?}");
    let edges = top_edges(&mut s, instance, 10.0, -100.0..100.0);
    // Package A refuses new modelling inside a multi-body instance. Prepare the legacy native
    // archive directly, preserving this reorder/projection regression without weakening that policy.
    s.save(Some(&path)).unwrap(); // The original source-only fixture had no embedded B-reps.
    let mut native = native_assembly_load(&path);
    assert!(native.shapes.contains_key(&instance));
    native.project = s.project().clone(); // Includes the named-edge cache inspected above.
    let projected = native.project.add_sketch("legacy projection", vec![], None);
    native.project.add_sketch_node(projected, "legacy projection");
    let si = native.project.sketch_index(projected).unwrap();
    let kernel = qymcad_kernel::OcctKernel { shapes: std::cell::RefCell::new(native.shapes), ..Default::default() };
    {
        let _gate = qymcad_kernel::kernel_gate();
        for edge in edges {
            assert_ne!(native.project.add_sketch_projection(si, instance, edge, &kernel), 0);
        }
    }
    native.shapes = kernel.shapes.into_inner();
    assert!(native.project.solve_sketch(si) <= 1e-6);
    native.project.add_extrude_multi(projected, vec![], 10.0, qymcad_core::feature::Reach::Forward, 0.0, vec![]);
    native_assembly_rebuild(&mut native, false);
    native_assembly_save(&native, &path);
    let (mut s, opened) = Session::open(&path).unwrap();
    assert!(opened.errors.is_empty(), "{opened:?}");
    let before = archive(&mut s, "reorder-spline-instance-before.qcad");
    let error = s.feature_reorder(last, "up").unwrap_err().to_string();
    assert!(error.contains(&format!("projected spline in sketch {projected}")), "{error}");
    assert_eq!(archive(&mut s, "reorder-spline-instance-after.qcad"), before);
}
