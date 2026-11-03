//! E8 regression tests for the independently reproduced package E findings.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
};

fn block(w: f64, l: f64, h: f64) -> (Session, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &w.into(), &l.into(), false).unwrap();
    let (body, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":h,"name":"stock"})).unwrap()).unwrap();
    one_volume(&r, body, w * l * h); // Centred rectangle area times extrusion height.
    (s, body)
}

fn one_volume(r: &Rebuild, id: Id, expected: f64) {
    assert!(r.errors.is_empty(), "{r:?}");
    assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![id]);
    assert_close(r.bodies[0].volume, expected, 1e-5, "analytic solid volume");
}

fn piece_volumes(r: &Rebuild, expected: f64) {
    assert!(r.errors.is_empty(), "{r:?}");
    assert_eq!(r.bodies.len(), 2);
    for body in &r.bodies {
        assert_close(body.volume, expected, 1e-5, "analytic independent piece volume");
    }
}

type StateSnapshot = (DocInfo, Vec<Topology>, String, Vec<(Id, Vec<u8>)>);

fn snapshot(s: &mut Session) -> StateSnapshot {
    let ids = s.result_bodies().iter().map(|b| b.id).collect::<Vec<_>>();
    let topology = ids.into_iter().map(|id| s.topology(Some(id), true).unwrap()).collect();
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        s.project().bodies.iter().filter_map(|b| s.shape(b.id).map(|sh| (b.id, sh.to_brep_bytes().unwrap()))).collect()
    };
    (s.info(), topology, ron::to_string(s.project()).unwrap(), breps)
}

fn refuse_multi(s: &mut Session, target: Id, suppressed: bool, feature_name: &str) {
    let before = snapshot(s);
    let error = s.feature_suppress(target, suppressed).unwrap_err().to_string();
    assert!(error.contains(feature_name), "refusal names the multi-output feature: {error}");
    assert!(error.contains("pinned"), "refusal explains the pin limitation: {error}");
    assert!(error.contains("stale") || error.contains("overlapping") || error.contains("identity"), "native piece limitation: {error}");
    assert_eq!(snapshot(s), before, "full document, topology, recipe, and exact live B-reps");
}

fn split(s: &mut Session, body: Id) -> Id {
    let result = s
        .split(&SplitOptions {
            body: Some(body),
            plane: PlaneRef::Base(BaseName::XY),
            offset: 10.0.into(),
            name: Some("halves".into()),
            piece_names: None,
        })
        .unwrap();
    piece_volumes(&result.rebuild, 10. * 20. * (20. / 2.)); // Mid-Z cut: two 2000 mm³ pieces.
    assert_eq!(result.pieces.len(), 2);
    result.feature
}

#[test]
fn e8_split_and_base_cascade_suppression_refuse_exactly() {
    let (mut s, base) = block(10., 20., 20.);
    let feature = split(&mut s, base);
    refuse_multi(&mut s, feature, true, "halves");
    refuse_multi(&mut s, base, true, "halves");
    piece_volumes(&s.current_rebuild(), 10. * 20. * (20. / 2.));
}

fn slot(s: &mut Session, target: Option<Id>) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("slot_profile")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &2.0.into(), &30.0.into(), false).unwrap();
    let (body, r) = s
        .extrude(
            &serde_json::from_value(json!({
                "sketch":sk,"height":20,"op":if target.is_some(){"cut"}else{"new_body"},
                "target":target,"name":if target.is_some(){"slot_cut"}else{"slot_tool"}
            }))
            .unwrap(),
        )
        .unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    body
}

#[test]
fn e8_multipiece_cut_and_base_cascade_suppression_refuse_exactly() {
    let (mut s, base) = block(10., 20., 20.);
    let feature = slot(&mut s, Some(base));
    piece_volumes(&s.current_rebuild(), ((10. - 2.) / 2.) * 20. * 20.); // Two widths 4: 1600 each.
    refuse_multi(&mut s, feature, true, "slot_cut");
    refuse_multi(&mut s, base, true, "slot_cut");
    piece_volumes(&s.current_rebuild(), ((10. - 2.) / 2.) * 20. * 20.);
}

#[test]
fn e8_multipiece_boolean_and_base_cascade_suppression_refuse_exactly() {
    let (mut s, base) = block(10., 20., 20.);
    let tool = slot(&mut s, None);
    // Tool is a separate 2×30×20 prism before the boolean consumes it.
    assert_close(s.result_bodies().iter().find(|b| b.id == tool).unwrap().volume, 2. * 30. * 20., 1e-5, "tool volume");
    let result = s
        .body_boolean(&BodyBooleanOptions { a: base, b: tool, op: BooleanOp::Cut, name: Some("body_slot".into()), piece_names: None })
        .unwrap();
    piece_volumes(&result.rebuild, ((10. - 2.) / 2.) * 20. * 20.); // Two widths 4: 1600 each.
    assert_eq!(result.pieces.len(), 2);
    refuse_multi(&mut s, result.feature, true, "body_slot");
    refuse_multi(&mut s, base, true, "body_slot");
    piece_volumes(&s.current_rebuild(), ((10. - 2.) / 2.) * 20. * 20.);
}

fn save_native(p: &mut qymcad_core::model::Project, shapes: &std::collections::HashMap<Id, qymcad_kernel::Shape>, path: &Path) {
    for b in &mut p.bodies {
        if let Some(faces) = p.regen_faces.get(&b.id) {
            b.faces = faces.clone();
        }
    }
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(&id, shape)| (id, shape.to_brep_bytes().unwrap())).collect::<Vec<_>>()
    };
    qymcad_io::save_project_with_brep(p, path.to_str().unwrap(), &breps).unwrap();
}

#[test]
fn e8_enabling_base_with_native_suppressed_split_refuses_exactly() {
    let (mut s, base) = block(10., 20., 20.);
    let feature = split(&mut s, base);
    let path = scratch("e8-native-suppressed-split.qcad");
    s.save(Some(&path)).unwrap();
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    let ti = p.timeline.iter().position(|node| node.id == base).unwrap();
    assert!(p.set_feature_suppressed(ti, true));
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, restore_shapes(loaded.breps));
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    save_native(&mut p, &shapes, &path);
    let (mut s, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    assert!(r.bodies.is_empty());
    assert!(s.info().timeline.iter().find(|node| node.id == base).unwrap().suppressed);
    assert!(!s.info().timeline.iter().find(|node| node.id == feature).unwrap().suppressed);
    refuse_multi(&mut s, base, false, "halves");
    assert!(s.current_rebuild().bodies.is_empty());
}

fn asymmetric(s: &mut Session, base: Id) -> Id {
    let topo = s.topology(Some(base), true).unwrap();
    let edge = &topo.edges[0]; // Same first-edge reproduction as E-review/history_probe.py chamfer.
    assert_close(edge.length, 20., 1e-7, "first edge spans the 20 mm stock height");
    let (body, r) = s.chamfer(Some(base), &Sel::Ids(vec![edge.id]), &1.0.into(), Some(&2.0.into()), Some("bevel")).unwrap();
    one_volume(&r, body, 10. * 20. * 20. - 0.5 * 1. * 2. * 20.); // Triangular prism loss d*d2/2 * edge length: 3980.
    body
}

#[test]
fn e8_inactive_asymmetric_chamfer_allows_empty_start_and_exact_resume() {
    let (mut s, base) = block(10., 20., 20.);
    let chamfer = asymmetric(&mut s, base);
    let r = s.history_rollback(None, Some("start")).unwrap();
    assert!(r.changed);
    assert_eq!(r.marker.position, "start");
    assert_eq!(r.marker.active_nodes, 0);
    assert_eq!(r.marker.total_nodes, 3); // Profile, stock, chamfer.
    assert!(r.rebuild.errors.is_empty());
    assert!(r.rebuild.bodies.is_empty());
    assert!(s.result_bodies().is_empty());
    let r = s.history_rollback(None, Some("end")).unwrap();
    assert!(r.changed);
    assert_eq!(r.marker.position, "end");
    assert_eq!(r.marker.active_nodes, 3);
    one_volume(&r.rebuild, chamfer, 10. * 20. * 20. - 0.5 * 1. * 2. * 20.); // 3980.
}

#[test]
fn e8_suppression_cascade_ignores_unbuilt_asymmetric_chamfer() {
    let (mut s, base) = block(10., 20., 20.);
    let chamfer = asymmetric(&mut s, base);
    let r = s.feature_suppress(base, true).unwrap();
    assert!(r.changed && r.suppressed);
    assert_eq!(r.unbuilt, vec![base, chamfer]);
    assert!(r.rebuild.errors.is_empty());
    assert!(r.rebuild.bodies.is_empty());
    let r = s.feature_suppress(base, false).unwrap();
    assert!(r.changed && !r.suppressed);
    assert!(r.unbuilt.is_empty());
    one_volume(&r.rebuild, chamfer, 10. * 20. * 20. - 0.5 * 1. * 2. * 20.); // 3980.
}

#[test]
fn e8_unbuilt_sketch_hole_site_checks_respect_prefix_and_suppression() {
    let (mut s, base) = block(40., 30., 10.);
    let (plane, r) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &10.0.into(), Some("hole_top")).unwrap();
    assert!(r.errors.is_empty());
    let sk = s.sketch_create(&PlaneRef::Plane(plane), Some("hole_sites")).unwrap();
    s.sketch_point(sk, &0.0.into(), &0.0.into(), true).unwrap();
    let (hole, r) = s
        .hole_by_sketch(&HoleSketch {
            body: Some(base),
            sketch: sk,
            points: None,
            flip: false,
            diameter: 4.0.into(),
            depth: None,
            kind: HoleKind::Plain,
            dia2: None,
            depth2: None,
            name: Some("central_bore".into()),
        })
        .unwrap();
    let expected = 40. * 30. * 10. - std::f64::consts::PI * 2_f64.powi(2) * 10.; // Stock minus r=2 through cylinder: 12000−40π.
    one_volume(&r, hole, expected);
    let r = s.history_rollback(None, Some("start")).unwrap();
    assert_eq!(r.marker.active_nodes, 0);
    assert!(r.rebuild.errors.is_empty());
    assert!(r.rebuild.bodies.is_empty());
    one_volume(&s.history_rollback(None, Some("end")).unwrap().rebuild, hole, expected);
    let r = s.feature_suppress(base, true).unwrap();
    assert!(r.changed && r.suppressed);
    assert!(r.rebuild.errors.is_empty());
    assert!(r.rebuild.bodies.is_empty());
    one_volume(&s.feature_suppress(base, false).unwrap().rebuild, hole, expected);
}

#[test]
fn e8_active_invalid_chamfer_reference_is_still_refused_exactly() {
    use qymcad_core::feature::FeatureKind;
    let (mut s, base) = block(10., 20., 20.);
    let chamfer = asymmetric(&mut s, base);
    let path = scratch("e8-inactive-invalid-chamfer.qcad");
    s.save(Some(&path)).unwrap();
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let mut p = loaded.project;
    let ti = p.timeline.iter().position(|node| node.id == chamfer).unwrap();
    let FeatureKind::Chamfer { ref_face, .. } = &mut p.timeline[ti].kind else { panic!("chamfer fixture") };
    *ref_face = u32::MAX; // Deliberately impossible support id, never an observed expected value.
    p.set_rollback(Some(ti));
    let (r, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut p, restore_shapes(loaded.breps));
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    save_native(&mut p, &shapes, &path);
    let (mut s, r) = Session::open(&path).unwrap();
    one_volume(&r, base, 10. * 20. * 20.); // Only the active stock is built: 4000.
    let before = snapshot(&mut s);
    let error = s.history_rollback(None, Some("end")).unwrap_err().to_string();
    assert!(error.contains("chamfer reference face") && error.contains("no longer exists"), "{error}");
    assert_eq!(snapshot(&mut s), before);
    one_volume(&s.current_rebuild(), base, 10. * 20. * 20.);
}

fn moved(s: &mut Session, base: Id) -> Id {
    let (moved, r) = s
        .move_body(&Move {
            body: Some(base),
            translation: Some([5.0.into(), 0.0.into(), 0.0.into()]),
            rotation: None,
            name: Some("tail".into()),
        })
        .unwrap();
    one_volume(&r, moved.body, 10. * 20. * 20.); // Rigid translation preserves volume 4000.
    moved.body
}

fn lost(result: impl serde::Serialize) -> Value {
    serde_json::to_value(result).unwrap()["lost_body_metadata"].clone()
}

fn metadata(s: &mut Session, body: Id) {
    let r = s.rename(RenameTarget::Body(body), "custom-body").unwrap();
    assert_eq!(r.name, "custom-body");
    let r = s.appearance(body, Some(false), Some(Some([12, 34, 56]))).unwrap();
    assert!(r.changed);
    let b = &s.info().bodies[0];
    assert_eq!(b.id, body);
    assert_eq!(b.body_name, "custom-body");
    assert!(!b.visible);
    assert_eq!(b.color_rgb, [12, 34, 56]);
}

fn restored_metadata(s: &Session, body: Id) {
    one_volume(&s.current_rebuild(), body, 10. * 20. * 20.); // Native recreated prism: 4000.
    let b = &s.info().bodies[0];
    assert_eq!(b.body_name, "Body 1"); // Native default for this sole recreated body (F-136).
    assert!(b.visible);
    assert_eq!(b.color_rgb, [12, 34, 56]);
    assert_eq!(b.color_source, "manual");
    assert_eq!(s.project().lineage_root(body), s.info().timeline.iter().find(|n| n.name == "stock").unwrap().id);
}

#[test]
fn e8_rollback_reports_exact_lost_body_metadata_and_preserves_lineage_colour() {
    let (mut s, base) = block(10., 20., 20.);
    let body = moved(&mut s, base);
    metadata(&mut s, body);
    let r = s.history_rollback(Some(base), None).unwrap();
    one_volume(&r.rebuild, base, 10. * 20. * 20.);
    assert_eq!(lost(&r), json!([{"body":body,"body_name":"custom-body","visible":false}]));
    assert_eq!(lost(s.history_rollback(Some(base), None).unwrap()), json!([]));
    let r = s.history_rollback(None, Some("end")).unwrap();
    one_volume(&r.rebuild, body, 10. * 20. * 20.);
    assert_eq!(lost(&r), json!([]));
    restored_metadata(&s, body);
}

#[test]
fn e8_suppress_reports_exact_lost_body_metadata_and_preserves_lineage_colour() {
    let (mut s, base) = block(10., 20., 20.);
    let body = moved(&mut s, base);
    metadata(&mut s, body);
    let r = s.feature_suppress(base, true).unwrap();
    assert!(r.changed && r.suppressed);
    assert!(r.rebuild.errors.is_empty());
    assert!(r.rebuild.bodies.is_empty());
    assert_eq!(lost(&r), json!([{"body":body,"body_name":"custom-body","visible":false}]));
    assert_eq!(lost(s.feature_suppress(base, true).unwrap()), json!([]));
    let r = s.feature_suppress(base, false).unwrap();
    one_volume(&r.rebuild, body, 10. * 20. * 20.);
    assert_eq!(lost(&r), json!([]));
    restored_metadata(&s, body);
}

#[test]
fn e8_plain_history_calls_report_empty_lost_body_metadata() {
    let (mut s, base) = block(10., 20., 20.);
    let body = moved(&mut s, base);
    let r = s.history_rollback(Some(base), None).unwrap();
    one_volume(&r.rebuild, base, 10. * 20. * 20.);
    assert_eq!(lost(&r), json!([]));
    let r = s.history_rollback(None, Some("end")).unwrap();
    one_volume(&r.rebuild, body, 10. * 20. * 20.);
    assert_eq!(lost(&r), json!([]));
    let r = s.feature_suppress(base, true).unwrap();
    assert!(r.rebuild.errors.is_empty());
    assert!(r.rebuild.bodies.is_empty());
    assert_eq!(lost(&r), json!([]));
    let r = s.feature_suppress(base, false).unwrap();
    one_volume(&r.rebuild, body, 10. * 20. * 20.);
    assert_eq!(lost(&r), json!([]));
}

fn archive(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut entry = zip.by_index(i).unwrap();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (entry.name().to_string(), bytes)
        })
        .collect()
}

#[test]
fn e8_refused_measure_only_restores_missing_derived_face_cache() {
    let (mut s, body) = block(12., 8., 5.);
    let original = scratch("e8-measure-with-faces.qcad");
    s.save(Some(&original)).unwrap();
    let legacy = scratch("e8-measure-no-faces.qcad");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&legacy).unwrap());
    for (name, bytes) in archive(&original) {
        if !name.starts_with("faces/") {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(&bytes).unwrap();
        }
    }
    zip.finish().unwrap();
    let (mut s, r) = Session::open(&legacy).unwrap();
    one_volume(&r, body, 12. * 8. * 5.); // Independent rectangular prism volume: 480.
    let before_path = scratch("e8-measure-before.qcad");
    s.save(Some(&before_path)).unwrap();
    let before = archive(&before_path);
    assert!(!before.keys().any(|name| name.starts_with("faces/")));
    let info = s.info(); // Do not call topology: the measurement must restore the cache.
    let error =
        s.measure(MeasureOperand::Face { body, selection: Sel::Facing { dir: [0., 0., 1.], tol_deg: 1. } }, None).unwrap_err().to_string();
    assert!(error.contains("point or planar face alone"), "{error}");
    assert_eq!(s.info(), info, "diagnostics and full doc_info remain unchanged");
    let after_path = scratch("e8-measure-after.qcad");
    s.save(Some(&after_path)).unwrap();
    let mut after = archive(&after_path);
    let restored = after.remove(&format!("faces/{body}.ron")).expect("derived face cache restored after open");
    assert!(!restored.is_empty());
    assert_eq!(after, before, "recipe, B-reps, meshes, meta: every pre-existing archive entry is byte-identical");
}

#[test]
fn e8_unbuilt_thread_stock_checks_respect_prefix_and_suppression() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("rod_profile")).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &8.0.into(), false).unwrap();
    let (base, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":20,"name":"rod"})).unwrap()).unwrap();
    one_volume(&r, base, std::f64::consts::PI * 4_f64.powi(2) * 20.); // Radius-4 stock cylinder: 320π.
    let topo = s.topology(Some(base), false).unwrap();
    let edge = topo.edges.iter().find(|e| e.kind == EdgeKind::Circle && e.center.is_some_and(|c| c[2].abs() < 1e-8)).unwrap().id;
    let r = s
        .thread(
            Some(base),
            &Sel::Ids(vec![edge]),
            &ThreadOptions {
                size: Some("M8".into()),
                length: 20.0.into(),
                fit: Some(0.1.into()),
                lead_in: Some(0.0.into()),
                lead_out: Some(0.0.into()),
                name: Some("external_thread".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let body = r.body;
    assert_eq!(r.source, base);
    assert_eq!(r.edge, edge);
    assert_eq!(r.kind, "external");
    assert_eq!(r.standard, "metric_iso");
    assert_eq!(r.nominal, 8.);
    assert_eq!(r.pitch, 1.25); // Standard M8 coarse pitch.
    assert_eq!(r.lead, 1.25); // One start: lead = pitch.
    assert_eq!(r.length, 20.);
    assert_eq!(r.fit, 0.1);
    assert_eq!(r.basic_major_diameter, 8.);
    // ISO fundamental triangle H=√3*P/2: pitch reduction 3H/4; external minor reduction 17H/12.
    assert_close(r.basic_pitch_diameter, 8. - 3. * 3_f64.sqrt() * 1.25 / 8., 2e-6, "ISO pitch diameter");
    assert_close(r.basic_minor_diameter, 8. - 17. * 3_f64.sqrt() * 1.25 / 24., 2e-6, "ISO external minor diameter");
    let relief = (0.1 - 0.0525 * 1.25) / 30_f64.to_radians().tan(); // Capped axial groove widening converted to radial relief.
    assert_close(r.radial_relief, relief, 1e-9, "radial relief from the profile geometry");
    assert!(r.rebuild.errors.is_empty());
    assert_eq!(r.rebuild.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![body]);
    let appearance = s.appearance(body, None, Some(Some([12, 34, 56]))).unwrap();
    assert!(appearance.changed);
    let assert_recreated = |s: &Session, rebuild: &Rebuild| {
        assert!(rebuild.errors.is_empty(), "{rebuild:?}");
        assert_eq!(rebuild.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![body]);
        let b = &rebuild.bodies[0];
        assert_eq!(b.name, "external_thread");
        assert_eq!(b.body_name, "Body 1");
        assert!(b.visible);
        assert_eq!(b.color_rgb, [12, 34, 56]);
        assert_eq!(b.color_source, "manual");
        assert_eq!(s.project().lineage_root(body), base);
    };
    let r = s.history_rollback(None, Some("start")).unwrap();
    assert!(r.changed);
    assert_eq!(r.marker.position, "start");
    assert_eq!(r.marker.active_nodes, 0);
    assert_eq!(r.marker.total_nodes, 3); // Profile, rod, thread.
    assert!(r.rebuild.errors.is_empty());
    assert!(r.rebuild.bodies.is_empty());
    assert_eq!(lost(&r), json!([]));
    let r = s.history_rollback(None, Some("end")).unwrap();
    assert!(r.changed);
    assert_eq!(r.marker.active_nodes, 3);
    assert_eq!(lost(&r), json!([]));
    assert_recreated(&s, &r.rebuild);
    let r = s.feature_suppress(base, true).unwrap();
    assert!(r.changed && r.suppressed);
    assert_eq!(r.unbuilt, vec![base, body]);
    assert!(r.rebuild.errors.is_empty());
    assert!(r.rebuild.bodies.is_empty());
    assert_eq!(lost(&r), json!([]));
    let r = s.feature_suppress(base, false).unwrap();
    assert!(r.changed && !r.suppressed);
    assert!(r.unbuilt.is_empty());
    assert_eq!(lost(&r), json!([]));
    assert_recreated(&s, &r.rebuild);
}
