//! T: checked native GUI-neighbour reorder. Analytic numbers are independent of recipe order.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;
use std::{collections::BTreeMap, f64::consts::PI, io::Read, path::Path};

fn prism() -> (Session, Id, Id) {
    let mut s = Session::new_part();
    s.param_set("w", &10.0.into()).unwrap();
    let sk = profile(&mut s, 0., 10., 10.);
    let (base, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":10,"name":"stock"})).unwrap()).unwrap();
    one(&r, base, 1000.); // 10*10*10.
    (s, sk, base)
}
fn profile(s: &mut Session, x: f64, w: f64, l: f64) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &x.into(), &0.0.into(), &w.into(), &l.into(), false).unwrap();
    sk
}
fn body(s: &mut Session, sk: Id, name: &str) -> Id {
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":10,"name":name,"op":"new_body"})).unwrap()).unwrap().0
}
fn drill(s: &mut Session, source: Id, x: f64, name: &str) -> Id {
    let top = s.project().regen_faces[&source]
        .iter()
        .filter(|f| f.normal[2] > 0.9)
        .min_by(|a, b| (a.centroid.x - x).abs().total_cmp(&(b.centroid.x - x).abs()))
        .unwrap()
        .id;
    s.hole(&Hole {
        body: Some(source),
        face: Sel::Ids(vec![top]),
        at: Some([x, 0., 10.]),
        offset: None,
        diameter: 2.0.into(),
        depth: None,
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: Some(name.into()),
    })
    .unwrap()
    .0
}
fn array(s: &mut Session, source: Id) -> Id {
    s.linear_array(Some(source), &ArrayDir { dx: 20.0.into(), dy: 0.0.into(), dz: 0.0.into(), count: 2.0.into() }, None, Some("array"))
        .unwrap()
        .0
}
fn chain() -> (Session, Id, Id, Id, Id) {
    let (mut s, sk, base) = prism();
    let hole = drill(&mut s, base, 0., "hole");
    let arr = array(&mut s, hole);
    one(&s.current_rebuild(), arr, 2000. - 20. * PI); // Two copies of (1000 - pi*1²*10).
    (s, sk, base, hole, arr)
}
fn one(r: &Rebuild, id: Id, v: f64) {
    assert!(r.errors.is_empty(), "{r:?}");
    assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![id]);
    assert_close(r.bodies[0].volume, v, 1e-6, "hand-computed volume mm³");
}
fn order(s: &Session) -> Vec<Id> {
    s.project().timeline.iter().map(|n| n.id).collect()
}
fn inputs(s: &Session, id: Id) -> Vec<Id> {
    s.project().timeline.iter().find(|n| n.id == id).unwrap().kind.inputs()
}
fn archive(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            let name = f.name().to_owned();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            (name, b)
        })
        .collect()
}
type State = (String, DocInfo, Vec<(Id, Vec<u8>)>, BTreeMap<String, Vec<u8>>);
fn exact(s: &mut Session, path: &Path) -> State {
    s.save_with_options(Some(path), true).unwrap();
    let bytes = {
        let _g = qymcad_kernel::kernel_gate();
        s.project()
            .timeline
            .iter()
            .flat_map(|n| n.kind.bodies())
            .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
            .collect()
    };
    (ron::to_string(s.project()).unwrap(), s.info(), bytes, archive(path))
}
fn native_refusal(id: Id, d: &str, neighbor: Id) -> String {
    format!("invalid request: cannot move feature {id} {d} across feature {neighbor}: native QymCAD refuses this order or chain rewrite")
}

#[test]
fn reorder_hole_array_both_gui_forms_rewire_and_change_the_solid() {
    for direction in ["up", "down"] {
        let (mut s, sk, base, hole, arr) = chain();
        let selected = if direction == "up" { arr } else { hole };
        let r = s.feature_reorder(selected, direction).unwrap();
        assert!(r.changed);
        assert_eq!(r.feature.id, selected);
        assert_eq!(r.direction, direction);
        assert_eq!(r.from_index, if direction == "up" { 3 } else { 2 });
        assert_eq!(r.to_index, if direction == "up" { 2 } else { 3 });
        assert_eq!(r.order, vec![sk, base, arr, hole]);
        assert_eq!(order(&s), r.order);
        assert_eq!(inputs(&s, arr), vec![base]);
        assert_eq!(inputs(&s, hole), vec![arr]);
        assert_eq!(
            serde_json::to_value(&r.rewired).unwrap(),
            json!([
            {"feature":arr,"before_inputs":[hole],"after_inputs":[base]},
            {"feature":hole,"before_inputs":[base],"after_inputs":[arr]}])
        );
        assert!(r.rehosted_sketches.is_empty());
        assert!(r.lost_body_metadata.is_empty());
        one(&r.rebuild, hole, 2000. - 10. * PI); // Array first, only original copy drilled: 2*1000 - pi*1²*10.
        assert_eq!(s.project().bodies.iter().map(|b| b.id).collect::<std::collections::HashSet<_>>().len(), s.project().bodies.len());
        let u = s.undo().unwrap();
        assert_eq!(u.call.unwrap().tool, "feature_reorder");
        one(&u.rebuild, arr, 2000. - 20. * PI);
        assert!(s.undo().is_err(), "one real move = one undo entry");
    }
}
#[test]
fn reorder_hole_above_stock_refuses_exactly_and_independent_holes_succeed() {
    let (mut s, _, base) = prism();
    let h = drill(&mut s, base, -2., "first");
    let path = scratch("t-stock-refusal.qcad");
    let before = exact(&mut s, &path);
    assert_eq!(s.feature_reorder(h, "up").unwrap_err().to_string(), native_refusal(h, "up", base));
    assert_eq!(exact(&mut s, &path), before);
    assert!(s.undo().is_err());
    let h2 = drill(&mut s, h, 2., "second");
    let r = s.feature_reorder(h2, "up").unwrap();
    one(&r.rebuild, h, 1000. - 20. * PI); // Independent radius-one bores at x +/-2: 1000 - 2*pi*10.
    assert_eq!(inputs(&s, h2), vec![base]);
    assert_eq!(inputs(&s, h), vec![h2]);
}
#[test]
fn reorder_boundary_noops_do_not_dirty_or_create_history_despite_native_true() {
    let (mut s, _, base, _, arr) = chain();
    let path = scratch("t-noop.qcad");
    let before = exact(&mut s, &path);
    let mut p = s.project().clone();
    let original = order(&s);
    assert!(p.reorder_feature(0, 1));
    assert_eq!(p.timeline.iter().map(|n| n.id).collect::<Vec<_>>(), original);
    assert!(p.timeline.iter().all(|n| n.dirty));
    for (id, direction) in [(base, "up"), (arr, "down")] {
        let r = s.feature_reorder(id, direction).unwrap();
        assert!(!r.changed);
        assert_eq!(r.from_index, r.to_index);
        assert!(r.rewired.is_empty());
        assert!(r.rehosted_sketches.is_empty());
        assert!(r.lost_body_metadata.is_empty());
        assert_eq!(exact(&mut s, &path), before);
        assert!(s.undo().is_err());
    }
}
#[test]
fn reorder_consumed_boolean_tool_is_addressable_but_consumer_above_tool_refuses() {
    let mut s = Session::new_part();
    let a_sk = profile(&mut s, 0., 10., 10.);
    let b_sk = profile(&mut s, 0., 5., 5.);
    let a = body(&mut s, a_sk, "base");
    let b = body(&mut s, b_sk, "tool");
    let bo = s.body_boolean(&BodyBooleanOptions { a, b, op: BooleanOp::Cut, name: Some("cut".into()), piece_names: None }).unwrap();
    one(&bo.rebuild, bo.body, 750.); // 10*10*10 - 5*5*10 = 750.
    let r = s.feature_reorder(b, "up").unwrap();
    one(&r.rebuild, bo.body, 750.);
    assert_eq!(inputs(&s, bo.feature), vec![a, b]);
    let path = scratch("t-boolean-refusal.qcad");
    let before = exact(&mut s, &path);
    assert_eq!(s.feature_reorder(bo.feature, "up").unwrap_err().to_string(), native_refusal(bo.feature, "up", a));
    assert_eq!(exact(&mut s, &path), before);
}
#[test]
fn reorder_reports_no_lost_metadata_when_the_named_body_survives() {
    let (mut s, _, _, hole, arr) = chain();
    s.rename(RenameTarget::Body(arr), "kept-array").unwrap();
    s.appearance(arr, Some(false), None).unwrap();
    let r = s.feature_reorder(hole, "down").unwrap();
    assert!(r.changed);
    assert_eq!(serde_json::to_value(&r.lost_body_metadata).unwrap(), json!([]));
    let kept = s.project().mesh_index(arr).expect("the array body record survives the move");
    assert_eq!(s.project().bodies[kept].name, "kept-array");
    assert!(!s.project().bodies[kept].visible);
}
#[test]
fn reorder_rollback_keeps_numeric_marker_reports_loss_and_undo_restores() {
    let (mut s, _, base) = prism();
    let arr = array(&mut s, base);
    let hole = drill(&mut s, arr, 0., "hole");
    s.history_rollback(Some(arr), None).unwrap();
    s.rename(RenameTarget::Body(arr), "hidden-array").unwrap();
    s.appearance(arr, Some(false), None).unwrap();
    let marker = s.project().rollback;
    let path = scratch("t-rollback.qcad");
    let before = exact(&mut s, &path);
    let r = s.feature_reorder(hole, "up").unwrap();
    assert_eq!(s.project().rollback, marker);
    one(&r.rebuild, hole, 1000. - 10. * PI); // Numeric prefix now contains stock and one bore, not two array copies.
    assert!(s.project().mesh_index(arr).is_none());
    assert!(s.shape(arr).is_none());
    assert_eq!(serde_json::to_value(&r.lost_body_metadata).unwrap(), json!([{"body":arr,"body_name":"hidden-array","visible":false}]));
    assert_eq!(r.rollback.active_nodes, 3);
    assert_eq!(r.rollback.after.as_ref().unwrap().id, hole);
    assert_eq!(r.rollback.next_feature.as_ref().unwrap().id, arr);
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    one(&r, hole, 1000. - 10. * PI);
    assert_eq!(opened.project().rollback, marker);
    assert!(opened.project().mesh_index(arr).is_none());
    assert!(opened.shape(arr).is_none());
    one(&opened.history_rollback(None, Some("end")).unwrap().rebuild, arr, 2000. - 20. * PI);
    assert_eq!(opened.result_bodies().len(), 1); // Resumed tail replaces active hole; no ghost result.
    s.undo().unwrap();
    assert_eq!(exact(&mut s, &path), before);
}
#[test]
fn reorder_suppressed_hole_keeps_flag_then_back_and_enable_drills_both_copies() {
    let (mut s, _, _, hole, arr) = chain();
    s.feature_suppress(hole, true).unwrap();
    let r = s.feature_reorder(hole, "down").unwrap();
    one(&r.rebuild, hole, 2000.); // Suppressed bore passes both 1000-mm³ copies through.
    assert!(s.project().timeline.iter().find(|n| n.id == hole).unwrap().suppressed);
    one(&s.feature_reorder(hole, "up").unwrap().rebuild, arr, 2000.);
    one(&s.feature_suppress(hole, false).unwrap().rebuild, arr, 2000. - 20. * PI);
}
#[test]
fn reorder_lost_hole_rim_refuses_without_repick_and_restores_every_byte() {
    let (mut s, _, base) = prism();
    let hole = drill(&mut s, base, 0., "bore");
    let rim = s.project().regen_edges[&hole].iter().find(|e| (e.radius - 1.).abs() < 1e-7 && (e.center[2] - 10.).abs() < 1e-7).unwrap().id;
    let (fillet, r) = s.fillet(Some(hole), &Sel::Ids(vec![rim]), &0.25.into(), Some("rim")).unwrap();
    // Concave bore-rim loss = pi*integral((R+f-sqrt(f²-u²))²-R²), u=0..f.
    let (big_r, f) = (1., 0.25_f64);
    let loss = PI * (2. * big_r * f * f + 5. / 3. * f * f * f - PI / 2. * (big_r + f) * f * f);
    one(&r, fillet, 1000. - 10. * PI - loss);
    let topology = s.topology(Some(fillet), true).unwrap();
    let path = scratch("t-rim-refusal.qcad");
    let before = exact(&mut s, &path);
    let error = s.feature_reorder(fillet, "up").unwrap_err().to_string();
    assert_eq!(error, format!("the model did not rebuild (operation rolled back): rim ({fillet}): Not one of the 1 named edges is left in the body. Their names came from an operation higher in the timeline and it has changed — pick the edges again.; bore ({hole}): The source body was not built — fix the feature above it first"));
    assert_eq!(exact(&mut s, &path), before);
    assert_eq!(s.topology(Some(fillet), true).unwrap(), topology);
    assert!(s.undo().is_err());
}
#[test]
fn reorder_hole_marker_edge_native_omits_is_checked_after_candidate_rewrite() {
    let (mut s, _, base) = prism();
    let other_sk = profile(&mut s, 40., 10., 10.);
    let other = body(&mut s, other_sk, "independent");
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), None).unwrap();
    let marker = s.sketch_create(&PlaneRef::Plane(plane), Some("markers")).unwrap();
    s.sketch_point(marker, &0.0.into(), &0.0.into(), true).unwrap();
    let (hole, _) = s
        .hole_by_sketch(&HoleSketch {
            body: Some(base),
            sketch: marker,
            points: None,
            flip: false,
            diameter: 2.0.into(),
            depth: None,
            kind: HoleKind::Plain,
            dia2: None,
            depth2: None,
            name: Some("marker bore".into()),
        })
        .unwrap();
    let mut candidate = s.project().clone();
    assert!(candidate.reorder_feature(candidate.timeline_index(hole).unwrap(), candidate.timeline_index(other).unwrap()));
    assert!(candidate.timeline_index(hole) < candidate.timeline_index(marker));
    let path = scratch("t-marker-refusal.qcad");
    let before = exact(&mut s, &path);
    assert_eq!(
        s.feature_reorder(hole, "up").unwrap_err().to_string(),
        format!("invalid request: cannot reorder: feature {hole} would precede required object {marker}")
    );
    assert_eq!(exact(&mut s, &path), before);
    assert!(s.undo().is_err());
}

fn cube_stl() -> Vec<u8> {
    let vertices =
        [[0., 0., 0.], [10., 0., 0.], [10., 10., 0.], [0., 10., 0.], [0., 0., 10.], [10., 0., 10.], [10., 10., 10.], [0., 10., 10.]];
    let tris = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    let mut bytes = vec![0; 84 + tris.len() * 50];
    bytes[80..84].copy_from_slice(&(tris.len() as u32).to_le_bytes());
    for (k, t) in tris.into_iter().enumerate() {
        for (j, v) in t.into_iter().map(|i| vertices[i]).enumerate() {
            for (axis, value) in v.into_iter().enumerate() {
                let offset = 84 + k * 50 + 12 + j * 12 + axis * 4;
                bytes[offset..offset + 4].copy_from_slice(&(value as f32).to_le_bytes());
            }
        }
    }
    bytes
}

#[test]
fn reorder_mesh_target_and_neighbour_refuse_without_mutation() {
    let path = scratch("t-cube.stl");
    std::fs::write(&path, cube_stl()).unwrap();
    let mut s = Session::new_part();
    let imported = s.import_mesh(&path, None, Some("mm"), 1., Some("mesh")).unwrap();
    let mesh = imported.pieces[0].body;
    // Import creates its own part. The normal part's stock skips this cross-parent neighbour.
    let sk = profile(&mut s, 30., 10., 10.);
    let stock = body(&mut s, sk, "stock");
    assert_ne!(s.project().body_owner(mesh), s.project().body_owner(stock));
    assert!(!s.feature_reorder(stock, "up").unwrap().changed);
    // Author a native B-rep in the imported part to exercise a real same-parent mesh neighbour.
    let path = scratch("t-mesh-refusal.qcad");
    s.save(Some(&path)).unwrap();
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    p.set_active_component(p.body_owner(mesh));
    let same_parent_stock = p.add_box(10., 10., 10.);
    assert_eq!(p.body_owner(mesh), p.body_owner(same_parent_stock));
    let shapes = restore_shapes(loaded.breps);
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, shapes);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    for (id, faces) in report.built {
        p.set_body_faces(id, faces);
    }
    let breps = {
        let _g = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(&id, shape)| (id, shape.to_brep_bytes().unwrap())).collect::<Vec<_>>()
    };
    qymcad_io::save_project_with_brep(&p, path.to_str().unwrap(), &breps).unwrap();
    let (mut s, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    let before = exact(&mut s, &path);
    for id in [mesh, same_parent_stock] {
        assert_eq!(
            s.feature_reorder(id, "up").unwrap_err().to_string(),
            format!("invalid request: feature {mesh} is mesh-born; feature_reorder currently requires B-rep features")
        );
        assert_eq!(exact(&mut s, &path), before);
    }
}
#[test]
fn reorder_multioutput_target_and_neighbour_refuse_but_stable_downstream_split_allows_move() {
    let mut s = Session::new_part();
    let a_sk = profile(&mut s, 0., 10., 10.);
    let b_sk = profile(&mut s, 30., 10., 10.);
    let a = body(&mut s, a_sk, "A");
    let b = body(&mut s, b_sk, "B");
    let split = s
        .split(&SplitOptions {
            body: Some(a),
            plane: PlaneRef::Base(BaseName::XY),
            offset: 5.0.into(),
            name: Some("halves".into()),
            piece_names: None,
        })
        .unwrap();
    assert_eq!(split.pieces.len(), 2);
    for p in &split.pieces {
        assert_close(p.volume, 500., 1e-6, "10*10*5 half prism");
    }
    let piece_ids = split.pieces.iter().map(|p| p.id).collect::<Vec<_>>();
    let path = scratch("t-split-refusal.qcad");
    let before = exact(&mut s, &path);
    for (id, direction) in [(split.feature, "up"), (b, "down")] {
        assert_eq!(s.feature_reorder(id,direction).unwrap_err().to_string(),format!("invalid request: feature {} has multiple output bodies; moving multi-output features is not supported under the pinned app",split.feature));
        assert_eq!(exact(&mut s, &path), before);
    }
    let r = s.feature_reorder(b, "up").unwrap();
    assert!(r.changed);
    assert!(r.rebuild.errors.is_empty());
    let current = s.result_bodies();
    assert_eq!(current.len(), 3);
    assert_close(current.iter().map(|p| p.volume).sum(), 2000., 1e-6, "unchanged 1000 + two 500 halves");
    for id in piece_ids {
        assert!(current.iter().any(|p| p.id == id));
    }
}
