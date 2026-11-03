//! E7: native rollback prefix, persistence, insertion and exact refusal restoration.
mod common;
use common::*;
use qymcad_core::{feature::PLACE_IDENTITY, model::Project};
use qymcad_engine::*;
use qymcad_kernel::Shape;
use serde_json::json;
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    path::Path,
};

fn prism() -> (Session, Id, Id) {
    let mut s = Session::new_part();
    assert!(s.param_set("h", &20.0.into()).is_ok());
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
    assert!(s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &20.0.into(), false).is_ok());
    let (base, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":"h","name":"stock"})).unwrap()).unwrap();
    geometry(&r, base, 20., [0., 0.]);
    (s, sk, base)
}
fn geometry(r: &Rebuild, id: Id, h: f64, shift: [f64; 2]) {
    assert!(r.errors.is_empty(), "{r:?}");
    assert!(r.warnings.is_empty(), "{r:?}");
    assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![id]);
    // Centred 10×20 rectangle, extrusion z=0..h; rigid translations preserve volume.
    assert_close(r.bodies[0].volume, 10. * 20. * h, 1e-5, "10*20*h");
    let expected = [-5. + shift[0], -10. + shift[1], 0., 5. + shift[0], 10. + shift[1], h];
    for (a, e) in r.bodies[0].bbox.into_iter().zip(expected) {
        assert_close(a, e, 2e-6, "translated prism bounds");
    }
}
fn save_native(p: &mut Project, shapes: &HashMap<Id, Shape>, path: &Path) {
    for b in &mut p.bodies {
        if let Some(f) = p.regen_faces.get(&b.id) {
            b.faces = f.clone();
        }
    }
    let breps = {
        let _g = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(&id, sh)| (id, sh.to_brep_bytes().unwrap())).collect::<Vec<_>>()
    };
    assert!(qymcad_io::save_project_with_brep(p, path.to_str().unwrap(), &breps).is_ok());
}
fn chain(tag: &str) -> (Session, Id, Id, Id, Id) {
    let (mut s, sk, base) = prism();
    let path = scratch(&format!("e7-{tag}-seed.qcad"));
    assert!(s.save(Some(&path)).is_ok());
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    let shapes = restore_shapes(loaded.breps);
    // Native fixtures deliberately contain two distinct Moves; ordinary engine move reuses a Move (ADR0033).
    let mut mat = PLACE_IDENTITY;
    mat[3] = 5.;
    let m1 = p.add_move(base, mat);
    p.rename_node(m1, "M1");
    let mut mat = PLACE_IDENTITY;
    mat[7] = 7.;
    let m2 = p.add_move(m1, mat);
    p.rename_node(m2, "M2");
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, shapes);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    save_native(&mut p, &shapes, &path);
    let (s, r) = Session::open(&path).unwrap();
    geometry(&r, m2, 20., [5., 7.]);
    (s, sk, base, m1, m2)
}
fn marker(s: &Session, position: &str, after: Option<Id>, next: Option<Id>, active: usize, total: usize) {
    let actual = serde_json::to_value(s.rollback_info()).unwrap();
    let node = |id| json!({"id":id,"name":s.info().timeline.iter().find(|n|n.id==id).unwrap().name});
    assert_eq!(
        actual,
        json!({"position":position,"after":after.map(node),"next_feature":next.map(node),"active_nodes":active,"total_nodes":total})
    );
    assert_eq!(serde_json::to_value(s.info()).unwrap()["rollback"], actual);
}

#[test]
fn rollback_selectors_active_geometry_export_and_empty_start() {
    let (mut s, sk, _, m1, m2) = chain("selectors");
    assert_eq!(s.project().timeline.len(), 4); // profile, stock, M1, M2.
    let r = s.history_rollback(Some(m1), None).unwrap();
    assert!(r.changed);
    marker(&s, "after", Some(m1), Some(m2), 3, 4);
    geometry(&r.rebuild, m1, 20., [5., 0.]);
    assert_eq!(s.info().bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![m1]);
    for (format, ext) in [(ExportFormat::Step, "step"), (ExportFormat::ThreeMf, "3mf")] {
        let path = scratch(&format!("e7-prefix.{ext}"));
        let r = s.export(format, &path, Quality::Standard, None).unwrap();
        assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![m1]);
    }
    let topo = s.topology(None, true).unwrap();
    assert_eq!(topo.faces.len(), 6);
    assert_eq!(topo.edges.len(), 12); // rectangular prism
    let edge = topo.edges.iter().find(|e| (e.length - 20.).abs() < 1e-7).unwrap().id;
    let measured = s.measure(MeasureOperand::Edge { body: m1, selection: Sel::Ids(vec![edge]), at: None }, None).unwrap();
    assert_close(measured.length_mm.unwrap(), 20., 1e-6, "largest edge equals height/long side 20");
    assert_eq!(measured.resolved[0].body, Some(m1));
    let r = s.history_rollback(None, Some("end")).unwrap();
    assert!(r.changed);
    marker(&s, "end", None, None, 4, 4);
    geometry(&r.rebuild, m2, 20., [5., 7.]);
    let path = scratch("e7-empty-guard.qcad");
    assert!(s.save(Some(&path)).is_ok());
    let r = s.history_rollback(None, Some("start")).unwrap();
    assert!(r.changed);
    marker(&s, "start", None, Some(sk), 0, 4);
    assert!(r.rebuild.errors.is_empty());
    assert!(r.rebuild.bodies.is_empty());
    let state = |s: &Session| (ron::to_string(s.project()).unwrap(), s.info(), archive(&path));
    let before = state(&s);
    assert!(s.topology(None, false).is_err());
    assert_eq!(state(&s), before);
    let output = scratch("e7-empty.step");
    assert!(!output.exists());
    assert!(s.export(ExportFormat::Step, &output, Quality::Standard, None).is_err());
    assert!(!output.exists());
    assert_eq!(state(&s), before);
    assert!(s.save(Some(&path)).is_err());
    assert_eq!(state(&s), before);
}

// common::gui_edit_param_model's final-body scan ignores rollback and can select an unbuilt tail.
// Keep common/mod.rs untouched: use its restore_shapes and exactly its load/solve/changed-name/native
// regeneration functions below, then choose Project::active_body within the native prefix. No engine repair.
fn app_edit_prefix(path: &Path) -> (Project, f64, [f64; 6]) {
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    p.ensure_document();
    for b in &p.bodies {
        if !b.faces.is_empty() {
            p.regen_faces.insert(b.id, b.faces.clone());
        }
    }
    let shapes = restore_shapes(loaded.breps);
    let seen = p.param_map();
    p.parameters.iter_mut().find(|p| p.name == "h").unwrap().expr = "30".into();
    p.eval_parameters();
    for si in 0..p.sketches.len() {
        if p.sketch_reads_names(si) {
            assert!(p.solve_sketch(si) <= 1e-6);
        }
    }
    let vars = p.param_map();
    let mut changed =
        vars.iter().filter(|(k, v)| seen.get(*k).is_none_or(|o| (o - **v).abs() > 1e-12)).map(|(k, _)| k.clone()).collect::<Vec<_>>();
    changed.extend(seen.keys().filter(|k| !vars.contains_key(*k)).cloned());
    for name in changed {
        p.mark_param_dependents_dirty_for(&name);
    }
    let (r, mut shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, shapes);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // App keep_live_shapes prunes ordinary shapes; this fixture contains no imports.
    shapes.retain(|id, _| p.mesh_index(*id).is_some());
    let id = p.active_body(p.active_component.unwrap()).unwrap();
    assert!(p.timeline.iter().take(p.rollback_place()).any(|n| n.kind.bodies().contains(&id)));
    let (v, bbox) = {
        let _g = qymcad_kernel::kernel_gate();
        (shapes[&id].volume(), shapes[&id].bbox().unwrap())
    };
    (p, v, bbox)
}
fn app_geometry(path: &Path, shift: [f64; 2]) -> Project {
    let (p, v, bbox) = app_edit_prefix(path);
    assert_close(v, 10. * 20. * 30., 1e-5, "app 10*20*30=6000");
    for (a, e) in bbox.into_iter().zip([-5. + shift[0], -10. + shift[1], 0., 5. + shift[0], 10. + shift[1], 30.]) {
        assert_close(a, e, 2e-6, "app translated bounds");
    }
    p
}
#[test]
fn rollback_persists_and_native_app_and_engine_parameter_edits_agree() {
    let (mut s, _, _, m1, m2) = chain("persistence");
    assert!(s.history_rollback(Some(m1), None).unwrap().changed);
    let path = scratch("e7-persist.qcad");
    assert!(s.save(Some(&path)).is_ok());
    let (mut opened, r) = Session::open(&path).unwrap();
    geometry(&r, m1, 20., [5., 0.]);
    marker(&opened, "after", Some(m1), Some(m2), 3, 4);
    let p = app_geometry(&path, [5., 0.]);
    assert_eq!(p.rollback, Some(3));
    let r = opened.param_set("h", &30.0.into()).unwrap();
    geometry(&r, m1, 30., [5., 0.]);
    marker(&opened, "after", Some(m1), Some(m2), 3, 4);
}
#[test]
fn move_at_marker_reuses_m1_and_accumulates_per_adr0033() {
    let (mut s, sk, base, m1, m2) = chain("reuse");
    assert!(s.history_rollback(Some(m1), None).unwrap().changed);
    let (m, r) =
        s.move_body(&Move { body: Some(m1), translation: Some([3.0.into(), 0.0.into(), 0.0.into()]), rotation: None, name: None }).unwrap();
    assert!(m.reused, "ADR0033 §2 preserves Move reuse under the marker");
    assert_eq!(m.body, m1);
    assert_eq!(m.source_body, base);
    geometry(&r, m1, 20., [5. + 3., 0.]);
    assert_eq!(s.project().timeline.iter().map(|n| n.id).collect::<Vec<_>>(), vec![sk, base, m1, m2]);
    marker(&s, "after", Some(m1), Some(m2), 3, 4);
    assert_eq!(s.project().timeline[3].kind.consumed_body(), Some(m1));
    let r = s.history_rollback(None, Some("end")).unwrap();
    geometry(&r.rebuild, m2, 20., [5. + 3., 7.]);
}

fn cut_geometry(r: &Rebuild, id: Id, h: f64, shift: [f64; 2]) {
    assert!(r.errors.is_empty(), "{r:?}");
    assert!(r.warnings.is_empty(), "{r:?}");
    assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![id]);
    // Interior through-cut removes 2*2*h from stock 10*20*h, leaving outer bounds unchanged.
    piece_geometry(&r.bodies[0], 10. * 20. * h - 2. * 2. * h, [-5. + shift[0], -10. + shift[1], 0., 5. + shift[0], 10. + shift[1], h]);
}

#[test]
fn insert_cut_at_marker_remaps_m2_and_survives_app_edit() {
    let (mut s, sk, base, m1, m2) = chain("insert-cut");
    assert!(s.history_rollback(Some(m1), None).unwrap().changed);
    // Parametric top plane z=h, rectangle centred at (5,0) inside translated stock X=[0,10].
    let (top, r) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &Num::Expr("h".into()), Some("top")).unwrap();
    geometry(&r, m1, 20., [5., 0.]);
    let cut_sketch = s.sketch_create(&PlaneRef::Plane(top), Some("cut profile")).unwrap();
    assert!(s.sketch_rect(cut_sketch, &5.0.into(), &0.0.into(), &2.0.into(), &2.0.into(), false).is_ok());
    let (c, r) = s
        .extrude(
            &serde_json::from_value(json!({
                "sketch":cut_sketch,"height":1,"op":"cut","through":true,
                "direction":"reverse","target":m1,"name":"C"
            }))
            .unwrap(),
        )
        .unwrap();
    assert_eq!(s.project().timeline.iter().map(|n| n.id).collect::<Vec<_>>(), vec![sk, base, m1, top, cut_sketch, c, m2]);
    cut_geometry(&r, c, 20., [5., 0.]); // (200-4)*20=3920.
    marker(&s, "after", Some(c), Some(m2), 6, 7);
    assert_eq!(s.project().timeline[5].kind.consumed_body(), Some(m1));
    assert_eq!(s.project().timeline[6].kind.consumed_body(), Some(c));
    let prefix_path = scratch("e7-inserted-prefix.qcad");
    assert!(s.save(Some(&prefix_path)).is_ok());
    let (mut prefix, r) = Session::open(&prefix_path).unwrap();
    cut_geometry(&r, c, 20., [5., 0.]);
    marker(&prefix, "after", Some(c), Some(m2), 6, 7);
    let (p, volume, bbox) = app_edit_prefix(&prefix_path);
    assert_eq!(p.rollback, Some(6));
    assert_eq!(p.timeline[6].kind.consumed_body(), Some(c));
    assert_close(volume, 10. * 20. * 30. - 2. * 2. * 30., 1e-5, "app prefix (200-4)*30=5880");
    for (a, e) in bbox.into_iter().zip([0., -10., 0., 10., 10., 30.]) {
        assert_close(a, e, 2e-6, "app prefix top h=30 with +5X");
    }
    cut_geometry(&prefix.param_set("h", &30.0.into()).unwrap(), c, 30., [5., 0.]);
    let resumed = s.history_rollback(None, Some("end")).unwrap();
    assert!(resumed.changed);
    cut_geometry(&resumed.rebuild, m2, 20., [5., 7.]);
    let path = scratch("e7-inserted-cut.qcad");
    assert!(s.save(Some(&path)).is_ok());
    let (mut opened, r) = Session::open(&path).unwrap();
    cut_geometry(&r, m2, 20., [5., 7.]);
    assert_eq!(opened.project().timeline[6].kind.consumed_body(), Some(c));
    let (p, volume, bbox) = app_edit_prefix(&path);
    assert_eq!(p.rollback, None);
    assert_eq!(p.timeline[6].kind.consumed_body(), Some(c));
    assert_close(volume, 10. * 20. * 30. - 2. * 2. * 30., 1e-5, "app final (200-4)*30=5880");
    for (a, e) in bbox.into_iter().zip([0., -3., 0., 10., 17., 30.]) {
        assert_close(a, e, 2e-6, "app final top h=30 with +5X,+7Y");
    }
    cut_geometry(&opened.param_set("h", &30.0.into()).unwrap(), m2, 30., [5., 7.]);
    assert_eq!(opened.project().timeline[6].kind.consumed_body(), Some(c));
}

fn archive(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            if name == "document.ron" {
                b = String::from_utf8(b)
                    .unwrap()
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("next_id:"))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into_bytes();
            }
            (name, b)
        })
        .collect()
}
type ExactState = (String, BTreeMap<Id, Vec<u8>>, DocInfo, Option<Topology>, BTreeMap<String, Vec<u8>>);
fn snapshot(s: &mut Session, path: &Path) -> ExactState {
    let topo = if s.result_bodies().is_empty() { None } else { Some(s.topology(None, true).unwrap()) };
    assert!(s.save_with_options(Some(path), true).is_ok());
    let bytes = {
        let _g = qymcad_kernel::kernel_gate();
        s.project()
            .timeline
            .iter()
            .flat_map(|n| n.kind.bodies())
            .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
            .collect()
    };
    (ron::to_string(s.project()).unwrap(), bytes, s.info(), topo, archive(path))
}
#[test]
fn invalid_selectors_refuse_exactly_and_sketch_node_is_a_valid_boundary() {
    let (mut s, sk, _, m1, m2) = chain("refusals");
    let path = scratch("e7-selector-refusal.qcad");
    for (after, pos) in [(Some(m1), Some("start")), (None, None), (None, Some("middle")), (Some(u64::MAX), None)] {
        let before = snapshot(&mut s, &path);
        assert!(s.history_rollback(after, pos).is_err());
        assert_eq!(snapshot(&mut s, &path), before);
        assert!(s.undo().is_err());
    }
    let r = s.history_rollback(Some(sk), None).unwrap();
    assert!(r.changed);
    assert!(r.rebuild.errors.is_empty());
    assert!(r.rebuild.bodies.is_empty());
    marker(&s, "after", Some(sk), Some(s.project().timeline[1].id), 1, 4);
    let r = s.history_rollback(Some(m2), None).unwrap();
    assert!(r.changed);
    assert_eq!(s.project().rollback, None);
    marker(&s, "end", None, None, 4, 4);
    geometry(&r.rebuild, m2, 20., [5., 7.]);
}
#[test]
fn same_position_and_last_node_are_noops_and_undo_restores_exactly() {
    let (mut s, _, _, m1, m2) = chain("noop");
    let path = scratch("e7-undo.qcad");
    let before = snapshot(&mut s, &path);
    assert!(!s.history_rollback(None, Some("end")).unwrap().changed);
    assert!(!s.history_rollback(Some(m2), None).unwrap().changed);
    assert_eq!(s.project().rollback, None);
    assert_eq!(snapshot(&mut s, &path), before);
    assert!(s.undo().is_err());
    geometry(&s.history_rollback(Some(m1), None).unwrap().rebuild, m1, 20., [5., 0.]);
    let mid = snapshot(&mut s, &path);
    let r = s.history_rollback(Some(m1), None).unwrap();
    assert!(!r.changed);
    geometry(&r.rebuild, m1, 20., [5., 0.]);
    assert_eq!(snapshot(&mut s, &path), mid);
    let undone = s.undo().unwrap();
    assert_eq!(undone.call.unwrap().tool, "history_rollback");
    geometry(&undone.rebuild, m2, 20., [5., 7.]);
    assert_eq!(snapshot(&mut s, &path), before);
    assert!(s.undo().is_err());
}
fn rim_fixture() -> (Session, Id, Id, Id) {
    let (mut s, _, base) = prism();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("bore profile")).unwrap();
    assert!(s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).is_ok());
    let (cut, r) =
        s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":"h","op":"cut","target":base,"name":"bore"})).unwrap()).unwrap();
    assert!(r.errors.is_empty());
    assert_close(r.bodies[0].volume, 10. * 20. * 20. - std::f64::consts::PI * 2_f64.powi(2) * 20., 1e-5, "prism minus cylinder");
    let rim = s.project().regen_edges[&cut].iter().find(|e| (e.radius - 2.).abs() < 1e-7 && (e.center[2] - 20.).abs() < 1e-7).unwrap().id;
    let (fillet, r) = s.fillet(Some(cut), &Sel::Ids(vec![rim]), &0.5.into(), Some("rim")).unwrap();
    assert!(r.errors.is_empty());
    // Concave rim loss integral π∫[(R+f−sqrt(f²−u²))²−R²]du, u=0..f, R=2,f=.5.
    let (big_r, f) = (2., 0.5_f64);
    let loss = std::f64::consts::PI * (2. * big_r * f * f + 5. / 3. * f * f * f - std::f64::consts::PI / 2. * (big_r + f) * f * f);
    assert_close(r.bodies[0].volume, 4000. - 80. * std::f64::consts::PI - loss, 1e-5, "analytic rounded bore");
    (s, base, cut, fillet)
}
#[test]
fn inactive_tail_suppression_keeps_exact_result_and_resume_error_restores_everything() {
    let (mut s, base, cut, fillet) = rim_fixture();
    let r = s.history_rollback(Some(base), None).unwrap();
    geometry(&r.rebuild, base, 20., [0., 0.]);
    let bodies = s.result_bodies();
    let topo = s.topology(None, true).unwrap();
    let r = s.feature_suppress(cut, true).unwrap();
    assert!(r.changed && r.suppressed);
    assert_eq!(r.rebuild.bodies, bodies);
    assert_eq!(s.topology(None, true).unwrap(), topo);
    let path = scratch("e7-tail-refusal.qcad");
    let before = snapshot(&mut s, &path);
    let error = s.history_rollback(None, Some("end")).unwrap_err().to_string();
    assert!(error.contains("edge") || error.contains("Edge"), "{error}");
    assert_eq!(snapshot(&mut s, &path), before);
    assert!(s.project().timeline.iter().find(|n| n.id == fillet).is_some());
    // Exactly the two successful marker/suppress entries survive; refusal adds none.
    assert_eq!(s.undo().unwrap().call.unwrap().tool, "feature_suppress");
    assert_eq!(s.undo().unwrap().call.unwrap().tool, "history_rollback");
    assert!(s.undo().is_err());
}

fn piece_geometry(body: &BodyInfo, volume: f64, bbox: [f64; 6]) {
    assert_close(body.volume, volume, 1e-5, "hand-computed piece volume");
    for (actual, expected) in body.bbox.into_iter().zip(bbox) {
        assert_close(actual, expected, 2e-6, "hand-computed piece bounds");
    }
}

fn driven_move(s: &mut Session, body: Id) -> Id {
    let (moved, r) = s
        .move_body(&Move {
            body: Some(body),
            translation: Some([Num::Expr("x".into()), 0.0.into(), 0.0.into()]),
            rotation: None,
            name: None,
        })
        .unwrap();
    assert!(!moved.reused);
    assert!(r.errors.is_empty());
    moved.body
}

#[test]
fn rollback_piece_prefix_hides_split_and_keeps_active_identity_checks() {
    let (mut s, _, base) = prism();
    assert!(s.param_set("x", &0.0.into()).is_ok());
    let moved = driven_move(&mut s, base);
    let split = s
        .split(&SplitOptions {
            body: Some(moved),
            plane: PlaneRef::Base(BaseName::XY),
            offset: 10.0.into(),
            name: Some("halves".into()),
            piece_names: None,
        })
        .unwrap();
    assert!(split.rebuild.errors.is_empty());
    let pieces: Vec<_> = split.pieces.iter().map(|b| b.id).collect();
    let check_halves = |r: &Rebuild| {
        assert!(r.errors.is_empty());
        assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), pieces);
        // Each half has area 10*20 and height 10, with caps at z=0,10,20.
        piece_geometry(&r.bodies[0], 10. * 20. * 10., [-5., -10., 0., 5., 10., 10.]);
        piece_geometry(&r.bodies[1], 10. * 20. * 10., [-5., -10., 10., 5., 10., 20.]);
    };
    check_halves(&split.rebuild);
    let tail = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("inactive sketch")).unwrap();
    let path = scratch("e7-split-prefix.qcad");
    let original = snapshot(&mut s, &path);
    let rolled = s.history_rollback(Some(moved), None).unwrap();
    assert!(rolled.changed);
    geometry(&rolled.rebuild, moved, 20., [0., 0.]);
    marker(&s, "after", Some(moved), Some(split.feature), 3, 5);
    assert!(pieces.iter().all(|id| s.project().mesh_index(*id).is_none()));
    check_halves(&s.undo().unwrap().rebuild);
    assert_eq!(snapshot(&mut s, &path), original);
    geometry(&s.history_rollback(Some(moved), None).unwrap().rebuild, moved, 20., [0., 0.]);
    check_halves(&s.history_rollback(None, Some("end")).unwrap().rebuild);
    check_halves(&s.history_rollback(Some(split.feature), None).unwrap().rebuild);
    marker(&s, "after", Some(split.feature), Some(tail), 4, 5);
    let before = snapshot(&mut s, &path);
    // Translating both halves by 20 leaves zero overlap with their old X=[-5,5].
    // The active split's identity guard must still refuse, despite the inactive sketch tail.
    let error = s.param_set("x", &20.0.into()).unwrap_err().to_string();
    assert!(error.contains("split piece identity"), "{error}");
    assert_eq!(snapshot(&mut s, &path), before);
}

#[test]
fn rollback_piece_prefix_hides_boolean_and_keeps_active_identity_checks() {
    let (mut s, _, base) = prism();
    assert!(s.param_set("x", &0.0.into()).is_ok());
    let moved = driven_move(&mut s, base);
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("tool profile")).unwrap();
    assert!(s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &2.0.into(), &2.0.into(), false).is_ok());
    let (tool, r) = s.extrude(&serde_json::from_value(json!({"sketch":sketch,"height":20,"op":"new_body"})).unwrap()).unwrap();
    assert!(r.errors.is_empty());
    let tool = driven_move(&mut s, tool);
    let cut = s
        .body_boolean(&BodyBooleanOptions { a: moved, b: tool, op: BooleanOp::Cut, name: Some("bore".into()), piece_names: None })
        .unwrap();
    let check_cut = |r: &Rebuild| {
        assert!(r.errors.is_empty());
        assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![cut.body]);
        // Rectangular through-hole removes 2*2*20 from stock 10*20*20, retaining outer bounds.
        piece_geometry(&r.bodies[0], 10. * 20. * 20. - 2. * 2. * 20., [-5., -10., 0., 5., 10., 20.]);
    };
    check_cut(&cut.rebuild);
    let tail = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("inactive sketch")).unwrap();
    let path = scratch("e7-boolean-prefix.qcad");
    let original = snapshot(&mut s, &path);
    let rolled = s.history_rollback(Some(tool), None).unwrap();
    assert!(rolled.changed);
    assert!(rolled.rebuild.errors.is_empty());
    assert_eq!(rolled.rebuild.bodies.len(), 2);
    piece_geometry(rolled.rebuild.bodies.iter().find(|b| b.id == moved).unwrap(), 10. * 20. * 20., [-5., -10., 0., 5., 10., 20.]);
    piece_geometry(rolled.rebuild.bodies.iter().find(|b| b.id == tool).unwrap(), 2. * 2. * 20., [-1., -1., 0., 1., 1., 20.]);
    marker(&s, "after", Some(tool), Some(cut.feature), 6, 8);
    assert!(s.project().mesh_index(cut.body).is_none());
    check_cut(&s.undo().unwrap().rebuild);
    assert_eq!(snapshot(&mut s, &path), original);
    assert!(s.history_rollback(Some(tool), None).unwrap().changed);
    check_cut(&s.history_rollback(None, Some("end")).unwrap().rebuild);
    check_cut(&s.history_rollback(Some(cut.feature), None).unwrap().rebuild);
    marker(&s, "after", Some(cut.feature), Some(tail), 7, 8);
    let before = snapshot(&mut s, &path);
    // Both operands shift by 100, preserving the valid cut while its old/new boxes do not overlap.
    let error = s.param_set("x", &100.0.into()).unwrap_err().to_string();
    assert!(error.contains("body_boolean piece identity"), "{error}");
    assert_eq!(snapshot(&mut s, &path), before);
}
