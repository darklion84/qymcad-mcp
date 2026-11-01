mod common;
use common::*;
use qymcad_core::feature::FeatureKind;
use qymcad_core::refs::Ref;
use qymcad_engine::*;
use std::f64::consts::PI;

type Snapshot = (String, Vec<(Id, Vec<u8>)>, DocInfo);

fn extrusion(sketch: Id, height: Num, op: Op) -> Extrude {
    Extrude {
        sketch,
        profiles: None,
        profiles_at: None,
        height,
        down: None,
        op,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    }
}

fn stock() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("h", &10.0.into()).unwrap();
    s.param_set("r", &2.0.into()).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let (body, report) = s.extrude(&extrusion(sketch, "h".into(), Op::Add)).unwrap();
    volume(&report, 20.0 * 20.0 * 10.0);
    (s, body)
}

fn long_stock() -> (Session, Id) {
    let mut s = Session::new_part();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &100.0.into(), &20.0.into(), false).unwrap();
    let (body, report) = s.extrude(&extrusion(sketch, 20.0.into(), Op::Add)).unwrap();
    volume(&report, 40000.0);
    (s, body)
}

fn volume(report: &Rebuild, expected: f64) {
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.bodies.len(), 1);
    assert_close(report.bodies[0].volume, expected, 1e-6, "analytic BREP volume");
}

fn vertical(s: &mut Session, body: Id) -> Vec<u32> {
    let ids: Vec<_> = s.topology(Some(body), true).unwrap().edges.iter().filter(|e| (e.a[2] - e.b[2]).abs() > 9.9).map(|e| e.id).collect();
    assert_eq!(ids.len(), 4);
    ids
}

fn parallel_top(s: &mut Session, body: Id) -> Vec<u32> {
    let ids: Vec<_> = s
        .topology(Some(body), true)
        .unwrap()
        .edges
        .iter()
        .filter(|e| e.a[2] > 9.9 && e.b[2] > 9.9 && (e.a[0] - e.b[0]).abs() > 19.9)
        .map(|e| e.id)
        .collect();
    assert_eq!(ids.len(), 2);
    ids
}

fn snapshot(s: &Session) -> Snapshot {
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        s.project().bodies.iter().filter_map(|b| s.shape(b.id).map(|shape| (b.id, shape.to_brep_bytes().unwrap()))).collect()
    };
    (ron::ser::to_string(s.project()).unwrap(), breps, s.info())
}

fn unchanged(s: &Session, before: &Snapshot) {
    assert_eq!(ron::ser::to_string(s.project()).unwrap(), before.0, "exact Project rollback");
    assert_eq!(snapshot(s).1, before.1, "original live BREP bytes");
    assert_eq!(s.info(), before.2, "diagnostics, dimensions and names");
}

fn picks(s: &Session, feature: Id) -> Vec<u32> {
    match &s.project().timeline.iter().find(|n| n.id == feature).unwrap().kind {
        FeatureKind::Fillet { edges, .. } | FeatureKind::Chamfer { edges, .. } => {
            assert!(edges.query.is_pick_list(), "F-024: re-pick must persist fixed picks: {edges:?}");
            edges.query.picked_descs()
        }
        _ => panic!("blend feature"),
    }
}

fn along_z() -> Sel {
    Sel::Along { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }
}

#[test]
fn fillet_repick_replaces_edges_and_preserves_formula() {
    let (mut s, body) = stock();
    let edges = vertical(&mut s, body);
    let (feature, first) = s.fillet(None, &Sel::Ids(vec![edges[0]]), &"r".into(), Some("round")).unwrap();
    volume(&first, 4000.0 - (4.0 - PI) * 10.0); // V=20*20*h-(r²-pi*r²/4)*h, r=2.
    let before_dims = s.project().feat_dims.clone();
    let before_timeline = s.info().timeline;
    let (same, selected, report) = s.repick_fillet(feature, &Sel::Ids(vec![edges[1], edges[2]])).unwrap();
    assert_eq!(same, feature);
    assert_eq!(selected, vec![edges[1], edges[2]]);
    assert_eq!(picks(&s, feature), selected);
    assert_eq!(s.info().timeline, before_timeline);
    assert_eq!(s.project().feat_dims, before_dims);
    assert_eq!(s.project().feat_dims[&feature]["radius"], "r");
    volume(&report, 4000.0 - 2.0 * (4.0 - PI) * 10.0); // Two disjoint quarter-cylinder corner losses.
    volume(&s.param_set("r", &1.0.into()).unwrap(), 4000.0 - 2.0 * (1.0 - PI / 4.0) * 10.0);
}

#[test]
fn fillet_repick_rebuilds_downstream_hole() {
    let (mut s, body) = stock();
    let edges = vertical(&mut s, body);
    let (feature, first) = s.fillet(None, &Sel::Ids(vec![edges[0]]), &2.0.into(), None).unwrap();
    volume(&first, 4000.0 - (4.0 - PI) * 10.0);
    let (hole, first) = s
        .hole(&Hole {
            body: Some(feature),
            face: Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 },
            at: Some([0.0, 0.0, 10.0]),
            offset: None,
            diameter: 4.0.into(),
            depth: None,
            kind: HoleKind::Plain,
            dia2: None,
            depth2: None,
            name: Some("bore".into()),
        })
        .unwrap();
    volume(&first, 4000.0 - (4.0 - PI) * 10.0 - PI * 2.0_f64.powi(2) * 10.0);
    let timeline = s.info().timeline;
    let (_, _, report) = s.repick_fillet(feature, &Sel::Ids(vec![edges[1], edges[2]])).unwrap();
    volume(&report, 4000.0 - 2.0 * (4.0 - PI) * 10.0 - PI * 2.0_f64.powi(2) * 10.0); // Stock minus two rounds minus through bore.
    assert_eq!(report.bodies[0].id, hole);
    assert_eq!(s.info().timeline, timeline);
}

#[test]
fn fillet_repick_lost_downstream_arc_refuses_with_exact_rollback() {
    let (mut s, body) = stock();
    let topo = s.topology(Some(body), true).unwrap();
    let edges: Vec<_> = topo.edges.iter().filter(|e| (e.a[2] - e.b[2]).abs() > 9.9 && e.mid[1] < -9.9).map(|e| e.id).collect();
    assert_eq!(edges.len(), 2);
    let (feature, first) = s.fillet(None, &Sel::Ids(vec![edges[0]]), &10.0.into(), None).unwrap();
    volume(&first, 4000.0 - (100.0 - 25.0 * PI) * 10.0);
    let face = s.topology(Some(feature), true).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[1] < -0.99)).unwrap().id;
    let (_, report) = s.push_face(Some(feature), &Sel::Ids(vec![face]), &1.0.into(), None).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let before = snapshot(&s);
    let error = s.repick_fillet(feature, &Sel::Ids(edges)).unwrap_err().to_string();
    assert!(error.contains("face") || error.contains("Face"), "{error}");
    unchanged(&s, &before);
}

#[test]
fn descriptive_repick_persists_picks_through_open_and_app_edit() {
    let (mut s, body) = stock();
    let edges = vertical(&mut s, body);
    let (feature, first) = s.fillet(None, &Sel::Ids(vec![edges[0]]), &"r".into(), None).unwrap();
    volume(&first, 4000.0 - (4.0 - PI) * 10.0);
    let (_, selected, report) = s.repick_fillet(feature, &along_z()).unwrap();
    assert_eq!(selected, edges);
    assert_eq!(picks(&s, feature), selected);
    volume(&report, 4000.0 - 4.0 * (4.0 - PI) * 10.0);
    let path = scratch("d1-descriptive-repick.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    volume(&report, 4000.0 - 4.0 * (4.0 - PI) * 10.0);
    assert_eq!(picks(&opened, feature), selected);
    let engine = opened.param_set("h", &15.0.into()).unwrap();
    let expected = 20.0 * 20.0 * 15.0 - 4.0 * (4.0 - PI) * 15.0;
    volume(&engine, expected); // Four radius-2 corners, new height 15.
    let app = gui_edit_param(&path, "h", "15");
    assert_close(app, expected, 1e-6, "native app height edit");
    assert_close(app, engine.bodies[0].volume, 1e-6, "app and engine agree");
}

#[test]
fn two_edge_repick_survives_saved_app_height_edit() {
    let (mut s, body) = stock();
    let edges = vertical(&mut s, body);
    let (feature, _) = s.fillet(None, &Sel::Ids(vec![edges[0]]), &"r".into(), None).unwrap();
    let (_, _, report) = s.repick_fillet(feature, &Sel::Ids(vec![edges[1], edges[2]])).unwrap();
    volume(&report, 4000.0 - 2.0 * (4.0 - PI) * 10.0);
    let path = scratch("d1-two-edge-app.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    volume(&report, 4000.0 - 2.0 * (4.0 - PI) * 10.0);
    let expected = 20.0 * 20.0 * 15.0 - 2.0 * (4.0 - PI) * 15.0;
    volume(&opened.param_set("h", &15.0.into()).unwrap(), expected);
    assert_close(gui_edit_param(&path, "h", "15"), expected, 1e-6, "two-edge app edit");
}

#[test]
fn symmetric_chamfer_repick_replaces_with_parallel_top_edges() {
    let (mut s, body) = stock();
    let edges = parallel_top(&mut s, body);
    let (feature, first) = s.chamfer(None, &Sel::Ids(vec![edges[0]]), &1.0.into(), None, Some("bevel")).unwrap();
    volume(&first, 4000.0 - 0.5 * 1.0_f64.powi(2) * 20.0); // One triangular prism, area d²/2, L=20.
    let timeline = s.info().timeline;
    let (_, selected, report) = s.repick_chamfer(feature, &Sel::Ids(edges.clone()), None).unwrap();
    volume(&report, 4000.0 - 2.0 * 0.5 * 1.0_f64.powi(2) * 20.0);
    assert_eq!(selected, edges);
    assert_eq!(picks(&s, feature), selected);
    assert_eq!(s.info().timeline, timeline);
}

#[test]
fn asymmetric_chamfer_repick_keeps_or_replaces_reference_face() {
    let (mut s, body) = stock();
    s.param_set("d", &1.0.into()).unwrap();
    s.param_set("other", &2.0.into()).unwrap();
    let topo = s.topology(Some(body), true).unwrap();
    let face = |axis: usize, positive: bool| {
        topo.faces.iter().find(|f| f.normal.is_some_and(|n| if positive { n[axis] > 0.99 } else { n[axis] < -0.99 })).unwrap().id
    };
    let x = face(0, true);
    let nx = face(0, false);
    let y = face(1, true);
    let edge = |a, b| topo.edges.iter().find(|e| e.faces.is_some_and(|f| f.contains(&a) && f.contains(&b))).unwrap().id;
    let old = edge(x, y);
    let new = edge(nx, y);
    let (feature, first) = s
        .chamfer_ex(&Chamfer {
            body: Some(body),
            edges: Sel::Ids(vec![old]),
            dist: "d".into(),
            d2: Some("other".into()),
            angle: None,
            reference_face: Some(Sel::Ids(vec![x])),
            flip: true,
            name: Some("two setbacks".into()),
        })
        .unwrap();
    volume(&first, 4000.0 - 0.5 * 1.0 * 2.0 * 10.0); // Triangular section d*d2/2 along vertical L=10.
    let before = snapshot(&s);
    let error = s.repick_chamfer(feature, &Sel::Ids(vec![new]), None).unwrap_err().to_string();
    assert!(error.contains(&format!("chamfer reference face {x} is not adjacent to selected edge {new}")), "{error}");
    unchanged(&s, &before);
    let dimensions = s.project().feat_dims.clone();
    let (_, selected, report) = s.repick_chamfer(feature, &Sel::Ids(vec![new]), Some(&Sel::Ids(vec![nx]))).unwrap();
    volume(&report, 4000.0 - 0.5 * 1.0 * 2.0 * 10.0);
    assert_eq!(selected, vec![new]);
    assert_eq!(picks(&s, feature), selected);
    let info = s.chamfer_info(feature).unwrap();
    assert_eq!(info.mode, "two_dist");
    assert_eq!(info.reference_face, Some(nx));
    assert!(info.flip);
    assert_eq!(s.project().feat_dims, dimensions);
    let FeatureKind::Chamfer { d2, .. } = s.project().timeline.iter().find(|n| n.id == feature).unwrap().kind else { panic!("chamfer") };
    assert_eq!(d2, 2.0);
}

#[test]
fn repick_refuses_wrong_kinds_empty_and_unknown_without_changes() {
    let (mut s, body) = stock();
    let edges = vertical(&mut s, body);
    let (fillet, first) = s.fillet(None, &Sel::Ids(vec![edges[0]]), &2.0.into(), None).unwrap();
    volume(&first, 4000.0 - (4.0 - PI) * 10.0);
    let before = snapshot(&s);
    assert!(s
        .repick_chamfer(fillet, &Sel::Ids(vec![edges[1]]), None)
        .unwrap_err()
        .to_string()
        .contains(&format!("feature {fillet} is not a chamfer feature")));
    unchanged(&s, &before);
    let before = snapshot(&s);
    assert!(s.repick_fillet(fillet, &Sel::Ids(vec![])).unwrap_err().to_string().contains("matched no edge"));
    unchanged(&s, &before);
    let before = snapshot(&s);
    assert!(s.repick_fillet(999999, &along_z()).unwrap_err().to_string().contains("feature 999999"));
    unchanged(&s, &before);
    let (mut s, body) = stock();
    let edges = parallel_top(&mut s, body);
    let (chamfer, first) = s.chamfer(None, &Sel::Ids(vec![edges[0]]), &1.0.into(), None, None).unwrap();
    volume(&first, 4000.0 - 0.5 * 20.0);
    let before = snapshot(&s);
    assert!(s.repick_fillet(chamfer, &along_z()).unwrap_err().to_string().contains(&format!("feature {chamfer} is not a fillet feature")));
    unchanged(&s, &before);
    let before = snapshot(&s);
    assert!(s.repick_chamfer(chamfer, &Sel::Ids(vec![]), None).unwrap_err().to_string().contains("matched no edge"));
    unchanged(&s, &before);
    let before = snapshot(&s);
    assert!(s.repick_chamfer(999999, &along_z(), None).unwrap_err().to_string().contains("feature 999999"));
    unchanged(&s, &before);
    let before = snapshot(&s);
    assert!(s
        .repick_chamfer(chamfer, &Sel::Ids(vec![edges[1]]), Some(&Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 1.0 }))
        .unwrap_err()
        .to_string()
        .contains("symmetric chamfer does not accept reference_face"));
    unchanged(&s, &before);
}

#[test]
fn repick_refuses_foreign_body_edges_with_exact_rollback() {
    let (mut s, body) = stock();
    let edges = vertical(&mut s, body);
    let (feature, first) = s.fillet(None, &Sel::Ids(vec![edges[0]]), &2.0.into(), None).unwrap();
    volume(&first, 4000.0 - (4.0 - PI) * 10.0);
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sketch, &40.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let (foreign, report) = s.extrude(&extrusion(sketch, 10.0.into(), Op::NewBody)).unwrap();
    assert!(report.errors.is_empty());
    assert_eq!(report.bodies.len(), 2);
    assert_close(s.shape(foreign).unwrap().volume(), 20.0 * 20.0 * 10.0, 1e-6, "foreign box");
    let foreign_edges = vertical(&mut s, foreign);
    assert!(!edges.contains(&foreign_edges[0]));
    let before = snapshot(&s);
    let error = s.repick_fillet(feature, &Sel::Ids(vec![foreign_edges[0]])).unwrap_err().to_string();
    assert!(error.contains("not on body"), "{error}");
    unchanged(&s, &before);
    let (chamfer, report) = s.chamfer(Some(foreign), &Sel::Ids(vec![foreign_edges[0]]), &1.0.into(), None, None).unwrap();
    assert!(report.errors.is_empty());
    assert_close(s.shape(chamfer).unwrap().volume(), 4000.0 - 0.5 * 10.0, 1e-6, "foreign chamfer prism");
    let before = snapshot(&s);
    let error = s.repick_chamfer(chamfer, &Sel::Ids(vec![edges[0]]), None).unwrap_err().to_string();
    assert!(error.contains("not on body"), "{error}");
    unchanged(&s, &before);
}

#[test]
fn repick_refuses_native_per_vertex_table_exactly() {
    let (mut s, body) = stock();
    let edges = vertical(&mut s, body);
    let (feature, first) = s.fillet(None, &Sel::Ids(vec![edges[0]]), &2.0.into(), None).unwrap();
    volume(&first, 4000.0 - (4.0 - PI) * 10.0);
    let mut project = s.project().clone();
    let vertex = project.vertex_pool(body).first().unwrap().desc;
    let node = project.timeline.iter_mut().find(|n| n.id == feature).unwrap();
    let FeatureKind::Fillet { at_vertices, .. } = &mut node.kind else { panic!("fillet") };
    *at_vertices = vec![(Ref::one(vertex, Default::default()), 2.0)];
    let path = scratch("d1-per-vertex.qcad");
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &snapshot(&s).1).unwrap();
    let (mut s, report) = Session::open(&path).unwrap();
    volume(&report, 4000.0 - (4.0 - PI) * 10.0);
    let before = snapshot(&s);
    let error = s.repick_fillet(feature, &Sel::Ids(vec![edges[1]])).unwrap_err().to_string();
    assert!(
        error.contains(&format!("fillet {feature} has a per-vertex radius table made in the app; re-pick is not supported")),
        "{error}"
    );
    unchanged(&s, &before);
}

#[test]
fn repick_undo_restores_previous_edges_volume_and_history() {
    for chamfer in [false, true] {
        let (mut s, body) = stock();
        let edges = if chamfer { parallel_top(&mut s, body) } else { vertical(&mut s, body) };
        let (feature, first) = if chamfer {
            s.chamfer(None, &Sel::Ids(vec![edges[0]]), &1.0.into(), None, None).unwrap()
        } else {
            s.fillet(None, &Sel::Ids(vec![edges[0]]), &"r".into(), None).unwrap()
        };
        volume(&first, if chamfer { 4000.0 - 0.5 * 20.0 } else { 4000.0 - (4.0 - PI) * 10.0 });
        let before = snapshot(&s);
        let mut history = s.begin_tool_edit().unwrap();
        history.record_call(if chamfer { "chamfer" } else { "fillet" }, serde_json::json!({"feature": feature}));
        let (_, _, report) = if chamfer {
            s.repick_chamfer(feature, &Sel::Ids(edges.clone()), None).unwrap()
        } else {
            s.repick_fillet(feature, &Sel::Ids(vec![edges[1], edges[2]])).unwrap()
        };
        volume(&report, if chamfer { 4000.0 - 2.0 * 0.5 * 20.0 } else { 4000.0 - 2.0 * (4.0 - PI) * 10.0 });
        s.finish_tool_edit(history, true);
        let undid = s.undo().unwrap();
        assert!(undid.rebuild.errors.is_empty());
        assert_eq!(picks(&s, feature), vec![edges[0]]);
        unchanged(&s, &before);
        assert!(match s.undo() {
            Err(error) => error.to_string().contains("nothing to undo"),
            Ok(_) => false,
        });
    }
}

fn thin_wall() -> (Session, Id, u32, Vec<u32>) {
    let mut s = Session::new_part();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let points = [(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (8.0, 10.0), (8.0, -2.0), (-10.0, -2.0)]
        .into_iter()
        .map(|(x, y)| Xy(x.into(), y.into()))
        .collect();
    s.sketch_polyline(sketch, &PolylineSpec { points, closed: true, construction: false, dimensioned: true }).unwrap();
    let (body, report) = s.extrude(&extrusion(sketch, 10.0.into(), Op::Add)).unwrap();
    volume(&report, (20.0 * 20.0 - 18.0 * 12.0) * 10.0); // Open pocket leaves a 2-mm right wall and 8-mm base.
    let topo = s.topology(Some(body), true).unwrap();
    let safe = topo.edges.iter().find(|e| (e.a[2] - e.b[2]).abs() > 9.9 && e.mid[0] < -9.9 && e.mid[1] < -9.9).unwrap().id;
    let thin: Vec<_> = topo.edges.iter().filter(|e| (e.a[2] - e.b[2]).abs() > 9.9 && e.mid[1] > 9.9).map(|e| e.id).collect();
    assert_eq!(thin.len(), 2);
    (s, body, safe, thin)
}

#[test]
fn repick_f074_thin_wall_matches_creation_refusal_and_restores_exact_state() {
    let (mut thin, _, _, thin_edges) = thin_wall();
    let thin_before = snapshot(&thin);
    let thin_error = thin.fillet(None, &Sel::Ids(thin_edges.clone()), &3.0.into(), None).unwrap_err().to_string();
    assert!(thin_error.contains("fillet R3.00 failed on 2 edges"), "{thin_error}");
    unchanged(&thin, &thin_before);
    let (mut creation, body) = long_stock();
    let topo = creation.topology(Some(body), true).unwrap();
    let end_edges: Vec<_> = topo.edges.iter().filter(|e| (e.a[2] - e.b[2]).abs() > 19.9 && e.mid[0] > 49.9).map(|e| e.id).collect();
    assert_eq!(end_edges.len(), 2);
    let before = snapshot(&creation);
    let create_error = creation.fillet(None, &Sel::Ids(end_edges.clone()), &15.0.into(), None).unwrap_err().to_string();
    assert!(create_error.contains("fillet R15.00 does not fit these edges together"), "{create_error}");
    unchanged(&creation, &before);
    let (mut s, body) = long_stock();
    let topo = s.topology(Some(body), true).unwrap();
    let safe = topo.edges.iter().find(|e| (e.a[2] - e.b[2]).abs() > 19.9 && e.mid[0] < -49.9).unwrap().id;
    let (feature, first) = s.fillet(None, &Sel::Ids(vec![safe]), &15.0.into(), None).unwrap();
    volume(&first, 40000.0 - (1.0 - PI / 4.0) * 225.0 * 20.0);
    let before = snapshot(&s);
    let error = s.repick_fillet(feature, &Sel::Ids(end_edges)).unwrap_err().to_string();
    assert!(error.contains("does not fit these edges together"), "{error}");
    unchanged(&s, &before);
}
