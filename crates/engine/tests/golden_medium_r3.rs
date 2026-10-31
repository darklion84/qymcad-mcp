//! R3: late reference scopes, general feature settling and native sketch solve order.
mod common;

mod a1a2 {
    fn r3_path(name: &str) -> String {
        super::common::scratch(&format!("r3-a1a2-{:?}-{name}", std::thread::current().id())).to_str().unwrap().to_string()
    }
    use super::common;
    use common::*;
    use qymcad_engine::*;
    use serde_json::json;
    fn extrusion(sk: Id) -> Extrude {
        serde_json::from_value(json!({"sketch":sk,"height":10,"op":"new_body"})).unwrap()
    }
    #[test]
    fn moved_source_chamfer_native_shift_preserves_reference_side() {
        let mut s = Session::new_part();
        s.param_set("dx", &5.0.into()).unwrap();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sk, &0.0.into(), &0.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
        let b = s.extrude(&extrusion(sk)).unwrap().0;
        let moved = s
            .move_body(&Move { body: Some(b), translation: Some(["dx".into(), 0.0.into(), 0.0.into()]), rotation: None, name: None })
            .unwrap()
            .0
            .body;
        let t = s.topology(Some(moved), true).unwrap();
        let x = t.faces.iter().find(|f| f.normal.unwrap()[0] > 0.99).unwrap().id;
        let y = t.faces.iter().find(|f| f.normal.unwrap()[1] > 0.99).unwrap().id;
        let e = t.edges.iter().find(|e| e.faces.unwrap().contains(&x) && e.faces.unwrap().contains(&y)).unwrap().id;
        s.chamfer_ex(&Chamfer {
            body: Some(moved),
            edges: Sel::Ids(vec![e]),
            dist: 2.0.into(),
            d2: None,
            angle: Some(30.0.into()),
            reference_face: Some(Sel::Ids(vec![y])),
            flip: false,
            name: None,
        })
        .unwrap();
        let path = &std::path::PathBuf::from(r3_path("moved-chamfer.qcad"));
        s.save(Some(path)).unwrap();
        let (v, b, faces, _) = gui_edit_param_model(path, "dx", "11");
        assert_close(v, 12000. - 20. * 30f64.to_radians().tan(), 1e-5, "chamfer volume");
        for (a, e) in b.into_iter().zip([-9., -15., 0., 31., 15., 10.]) {
            assert_close(a, e, 0.001, "shifted bbox");
        }
        let fy = faces.iter().find(|f| f.id == y).unwrap();
        assert_close(fy.area, 380., 0.001, "reference setback is 2");
        assert_close(fy.centroid.x, 10., 0.001, "shifted support centroid");
        let mut loaded = Session::open(path).unwrap().0;
        loaded.param_set("dx", &11.0.into()).unwrap();
        assert_close(loaded.result_bodies()[0].bbox[0], -9., 0.001, "engine shift");
    }
    #[test]
    fn down_on_tilted_sketch_with_points_follows_saved_tilt_and_back() {
        let mut s = Session::new_part();
        s.param_set("tilt", &30.0.into()).unwrap();
        s.param_set("back", &3.0.into()).unwrap();
        let plane =
            s.plane_offset_tilt(&PlaneRef::Base(BaseName::XY), &7.0.into(), &"tilt".into(), &0.0.into(), &0.0.into(), None).unwrap().0;
        let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
        s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
        s.sketch_point(sk, &20.0.into(), &20.0.into(), true).unwrap();
        let a: Extrude = serde_json::from_value(json!({"sketch":sk,"height":7,"down":"back","op":"new_body"})).unwrap();
        s.extrude(&a).unwrap();
        let path = &std::path::PathBuf::from(r3_path("tilted-two-sides.qcad"));
        s.save(Some(path)).unwrap();
        for (name, value, angle, down) in [("tilt", "40", 40f64, 3.), ("back", "5", 30f64, 5.)] {
            let (v, b, _, _) = gui_edit_param_model(path, name, value);
            assert_close(v, 60. * (7. + down), 1e-5, "two-side volume");
            let (sin, cos) = angle.to_radians().sin_cos();
            let n = [0., -sin, cos];
            let origin = n.map(|x| x * (7. * cos));
            let y = [0., cos, sin];
            let mut want = [f64::INFINITY; 6];
            want[3..].fill(f64::NEG_INFINITY);
            for x in [-5., 5.] {
                for local_y in [-3., 3.] {
                    for h in [-down, 7.] {
                        let p = [x, origin[1] + local_y * y[1] + h * n[1], origin[2] + local_y * y[2] + h * n[2]];
                        for i in 0..3 {
                            want[i] = want[i].min(p[i]);
                            want[i + 3] = want[i + 3].max(p[i]);
                        }
                    }
                }
            }
            for (a, e) in b.into_iter().zip(want) {
                assert_close(a, e, 0.001, "tilted span position");
            }
        }
    }
    fn source_stock(s: &mut Session, x: f64, width: Num) -> Id {
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sk, &x.into(), &15.0.into(), &width, &30.0.into(), false).unwrap();
        s.extrude(&extrusion(sk)).unwrap().0
    }
    fn projected_measurement() -> Session {
        use qymcad_core::model::Constraint;
        let mut s = Session::new_part();
        s.param_set("w", &40.0.into()).unwrap();
        let source = source_stock(&mut s, 20., "w".into());
        let projected = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("projected measurement")).unwrap();
        let edge = s.project().regen_edges[&source]
            .iter()
            .find(|e| e.a[1].abs() < 1e-6 && e.b[1].abs() < 1e-6 && (e.a[2] - 10.).abs() < 1e-6 && (e.b[2] - 10.).abs() < 1e-6)
            .unwrap()
            .id;
        let refs = s.sketch_project(projected, source, &[ProjSource::Edge(edge)]).unwrap().0.points;
        let mut p = s.project().clone();
        let si = p.sketch_index(projected).unwrap();
        p.sketches[si].constraints.push(Constraint::Distance {
            a: refs[0],
            b: refs[1],
            d: 40.,
            off: 0.,
            expr: String::new(),
            driven: true,
            axis: 1,
            at: None,
        });
        assert!(p.add_named_dim("measured_w".into(), projected, refs));
        p.solve_sketch(si);
        p.mark_sketch_dirty(projected);
        let breps = {
            let _gate = qymcad_kernel::kernel_gate();
            s.project()
                .timeline
                .iter()
                .flat_map(|n| n.kind.bodies())
                .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
                .collect::<Vec<_>>()
        };
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = r3_path(&format!("projected-source-{}.qcad", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        qymcad_io::save_project_guarded_with_brep(&p, &path, &breps).unwrap();
        Session::open(std::path::Path::new(&path)).unwrap().0
    }
    #[test]
    fn down_formula_follows_projected_reference_measurement() {
        let mut s = projected_measurement();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sk, &100.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
        let a: Extrude = serde_json::from_value(json!({"sketch":sk,"height":7,"down":"measured_w/10","op":"new_body"})).unwrap();
        let b = s.extrude(&a).unwrap().0;
        let path = &std::path::PathBuf::from(r3_path("projected-down.qcad"));
        s.save(Some(path)).unwrap();
        let (native_v, native_bb, _, p) = gui_edit_param_model(path, "w", "60");
        eprintln!("native: measured={}, v={native_v}, bbox={native_bb:?}", p.param_map()["measured_w"]);
        assert_close(p.param_map()["measured_w"], 60., 1e-6, "native refreshed reference");
        assert_close(native_v, 660., 1e-5, "F-120 native retains previous down=4");
        assert_close(native_bb[2], -4., 1e-6, "F-120 native previous back cap");
        let r = s.param_set("w", &60.0.into()).unwrap();
        assert!(r.errors.is_empty());
        let actual = s.result_bodies().into_iter().find(|bb| bb.id == b).unwrap();
        eprintln!("engine: measured={}, v={}, bbox={:?}", s.project().param_map()["measured_w"], actual.volume, actual.bbox);
        s.save(Some(&std::path::PathBuf::from(r3_path("stale-down-after.qcad")))).unwrap();
        let (opened, r) = Session::open(&std::path::PathBuf::from(r3_path("stale-down-after.qcad"))).unwrap();
        eprintln!(
            "reopen errors={:?}, v={}, scope_down={}",
            r.errors,
            opened.result_bodies().iter().find(|bb| bb.id == b).unwrap().volume,
            opened.project().param_map()["measured_w"] / 10.
        );
        assert_close(actual.volume, 780., 1e-5, "down=measured_w/10=6; height7");
        assert_close(actual.bbox[2], -6., 1e-5, "back cap");
        assert!(r.errors.is_empty(), "{r:?}");
        assert_close(opened.result_bodies().iter().find(|bb| bb.id == b).unwrap().volume, 780., 1e-5, "reopened down");
        let rebuilt = s.rebuild();
        assert!(rebuilt.errors.is_empty(), "{rebuilt:?}");
        assert_close(rebuilt.bodies.iter().find(|bb| bb.id == b).unwrap().bbox[2], -6., 1e-6, "rebuilt down");
    }
    #[test]
    fn datum_formula_follows_projected_reference_measurement() {
        let mut s = projected_measurement();
        let plane = s
            .plane_offset_tilt(&PlaneRef::Base(BaseName::XY), &7.0.into(), &"measured_w/2".into(), &0.0.into(), &0.0.into(), None)
            .unwrap()
            .0;
        let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
        s.sketch_rect(sk, &100.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
        let b = s.extrude(&extrusion(sk)).unwrap().0;
        let native_file = std::path::PathBuf::from(r3_path("tilt-native.qcad"));
        s.save(Some(&native_file)).unwrap();
        let (_, _, _, native) = gui_edit_param_model(&native_file, "w", "60");
        assert_close(native.param_map()["measured_w"], 60., 1e-6, "native reference");
        assert_close(native.planes.iter().find(|p| p.id == plane).unwrap().tilt.about_x, 20., 1e-6, "F-120 native previous tilt");
        let r = s.param_set("w", &60.0.into()).unwrap();
        assert!(r.errors.is_empty());
        let f = s.sketch_info(sk).unwrap().world_frame.unwrap();
        eprintln!(
            "datum measured={}, normal={:?}, bbox={:?}",
            s.project().param_map()["measured_w"],
            f.normal,
            s.result_bodies().iter().find(|bb| bb.id == b).unwrap().bbox
        );
        assert_close(f.normal[1], -30f64.to_radians().sin(), 1e-6, "datum angle=30");
        let (sin, cos) = 30f64.to_radians().sin_cos();
        let n = [0., -sin, cos];
        let y = [0., cos, sin];
        let origin = n.map(|x| x * (7. * cos));
        let mut expected = [f64::INFINITY; 6];
        expected[3..].fill(f64::NEG_INFINITY);
        for x in [95., 105.] {
            for local_y in [-3., 3.] {
                for h in [0., 10.] {
                    let p = [x, origin[1] + local_y * y[1] + h * n[1], origin[2] + local_y * y[2] + h * n[2]];
                    for i in 0..3 {
                        expected[i] = expected[i].min(p[i]);
                        expected[i + 3] = expected[i + 3].max(p[i]);
                    }
                }
            }
        }
        let actual = s.result_bodies().into_iter().find(|bb| bb.id == b).unwrap();
        for (a, e) in actual.bbox.into_iter().zip(expected) {
            assert_close(a, e, 1e-5, "tilted solid transformed corner bound");
        }
        let file = std::path::PathBuf::from(r3_path("tilt-after.qcad"));
        s.save(Some(&file)).unwrap();
        let (mut opened, r) = Session::open(&file).unwrap();
        assert!(r.errors.is_empty(), "{r:?}");
        let r = opened.rebuild();
        assert!(r.errors.is_empty(), "{r:?}");
        assert_close(opened.sketch_info(sk).unwrap().world_frame.unwrap().normal[1], -0.5, 1e-6, "reopened/rebuilt tilt");
    }
    pub(super) fn archive_bytes(s: &mut Session, name: &str) -> std::collections::BTreeMap<String, Vec<u8>> {
        s.save_with_options(Some(std::path::Path::new(name)), true).unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(name).unwrap()).unwrap();
        (0..z.len())
            .map(|i| {
                let mut e = z.by_index(i).unwrap();
                let n = e.name().to_string();
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut e, &mut bytes).unwrap();
                (n, bytes)
            })
            .collect()
    }
    #[test]
    fn late_projected_invalid_down_refuses_exactly() {
        let mut s = projected_measurement();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sk, &100.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
        let a: Extrude = serde_json::from_value(json!({"sketch":sk,"height":7,"down":"50-measured_w","op":"new_body"})).unwrap();
        s.extrude(&a).unwrap();
        let before = archive_bytes(&mut s, &r3_path("late-guard-before.qcad"));
        let e = s.param_set("w", &60.0.into()).unwrap_err().to_string();
        eprintln!("late guard error: {e}");
        assert!(e.contains("down"));
        assert_eq!(archive_bytes(&mut s, &r3_path("late-guard-after.qcad")), before);
    }
    #[test]
    fn baseline_height_formula_follows_projected_reference_measurement() {
        let mut s = projected_measurement();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sk, &100.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
        let a: Extrude = serde_json::from_value(json!({"sketch":sk,"height":"measured_w/10","op":"new_body"})).unwrap();
        let b = s.extrude(&a).unwrap().0;
        let native_file = std::path::PathBuf::from(r3_path("height-native.qcad"));
        s.save(Some(&native_file)).unwrap();
        let (native_v, _, _, native) = gui_edit_param_model(&native_file, "w", "60");
        assert_close(native.param_map()["measured_w"], 60., 1e-6, "native reference");
        assert_close(native_v, 240., 1e-5, "F-120 native inherited height control");
        let r = s.param_set("w", &60.0.into()).unwrap();
        assert!(r.errors.is_empty());
        let actual = s.result_bodies().into_iter().find(|bb| bb.id == b).unwrap();
        eprintln!("baseline height: measured={}, v={}, bbox={:?}", s.project().param_map()["measured_w"], actual.volume, actual.bbox);
        assert_close(actual.volume, 360., 1e-5, "height=measured_w/10=6");
        let file = std::path::PathBuf::from(r3_path("height-after.qcad"));
        s.save(Some(&file)).unwrap();
        let (mut opened, r) = Session::open(&file).unwrap();
        assert!(r.errors.is_empty(), "{r:?}");
        let r = opened.rebuild();
        assert!(r.errors.is_empty(), "{r:?}");
        assert_close(r.bodies.iter().find(|bb| bb.id == b).unwrap().bbox[5], 6., 1e-6, "reopened/rebuilt height");
    }
    #[test]
    fn chamfer_formula_follows_projected_reference_measurement() {
        let mut s = projected_measurement();
        let stock = source_stock(&mut s, 100., 40.0.into());
        let t = s.topology(Some(stock), true).unwrap();
        let x = t.faces.iter().find(|f| f.normal.unwrap()[0] > 0.99).unwrap().id;
        let y = t.faces.iter().find(|f| f.normal.unwrap()[1] > 0.99).unwrap().id;
        let e = t.edges.iter().find(|e| e.faces.unwrap().contains(&x) && e.faces.unwrap().contains(&y)).unwrap().id;
        let id = s
            .chamfer_ex(&Chamfer {
                body: Some(stock),
                edges: Sel::Ids(vec![e]),
                dist: 2.0.into(),
                d2: None,
                angle: Some("measured_w/2".into()),
                reference_face: Some(Sel::Ids(vec![y])),
                flip: false,
                name: None,
            })
            .unwrap()
            .0;
        let file = std::path::PathBuf::from(r3_path("chamfer-native.qcad"));
        s.save(Some(&file)).unwrap();
        let (v, _, _, native) = gui_edit_param_model(&file, "w", "60");
        assert_close(native.param_map()["measured_w"], 60., 1e-6, "native reference");
        assert_close(v, 12000. - 20. * 20f64.to_radians().tan(), 1e-5, "F-120 native previous chamfer angle");
        let r = s.param_set("w", &60.0.into()).unwrap();
        assert!(r.errors.is_empty(), "{r:?}");
        let want = 12000. - 20. * 30f64.to_radians().tan();
        assert_close(s.result_bodies().iter().find(|b| b.id == id).unwrap().volume, want, 1e-5, "updated chamfer angle=30");
        s.save(Some(&file)).unwrap();
        let (mut opened, r) = Session::open(&file).unwrap();
        assert!(r.errors.is_empty(), "{r:?}");
        let r = opened.rebuild();
        assert!(r.errors.is_empty(), "{r:?}");
        assert_close(r.bodies.iter().find(|b| b.id == id).unwrap().volume, want, 1e-5, "reopened/rebuilt chamfer");
    }
    fn assert_open_refusal(file: &str, node: Id, field: &str) {
        let bytes = std::fs::read(file).unwrap();
        let error = match Session::open(std::path::Path::new(file)) {
            Err(e) => e,
            Ok(_) => panic!("open accepted late invalid {field}"),
        };
        assert!(matches!(error, Error::Invalid(_)), "{error}");
        let text = error.to_string();
        assert!(text.contains(&node.to_string()) && text.contains(field), "{text}");
        assert_eq!(std::fs::read(file).unwrap(), bytes, "open refusal rewrote input archive");
    }
    #[test]
    fn open_must_recheck_late_projected_invalid_down() {
        let mut s = projected_measurement();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sk, &100.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
        let a: Extrude = serde_json::from_value(json!({"sketch":sk,"height":7,"down":"50-measured_w","op":"new_body"})).unwrap();
        let b = s.extrude(&a).unwrap().0;
        let before = &std::path::PathBuf::from(r3_path("pending-down-before.qcad"));
        s.save(Some(before)).unwrap();
        let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(before.to_str().unwrap()).unwrap();
        project.parameters.iter_mut().find(|p| p.name == "w").unwrap().expr = "60".into();
        project.eval_parameters();
        project.mark_all_dirty();
        eprintln!("before open: measured={}, down_eval={}", project.param_map()["measured_w"], 50. - project.param_map()["measured_w"]);
        assert_close(50. - project.param_map()["measured_w"], 10., 1e-6, "pre-open down valid");
        qymcad_io::save_project_guarded_with_brep(&project, &r3_path("pending-down-open.qcad"), &breps).unwrap();
        assert_open_refusal(&r3_path("pending-down-open.qcad"), b, "down");
    }
    #[test]
    fn open_must_recheck_late_projected_invalid_datum() {
        let mut s = projected_measurement();
        let plane = s
            .plane_offset_tilt(&PlaneRef::Base(BaseName::XY), &7.0.into(), &"8*measured_w".into(), &0.0.into(), &0.0.into(), None)
            .unwrap()
            .0;
        let sk = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
        s.sketch_rect(sk, &100.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).unwrap();
        s.extrude(&extrusion(sk)).unwrap();
        let before = &std::path::PathBuf::from(r3_path("pending-datum-before.qcad"));
        s.save(Some(before)).unwrap();
        let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(before.to_str().unwrap()).unwrap();
        project.parameters.iter_mut().find(|p| p.name == "w").unwrap().expr = "60".into();
        project.eval_parameters();
        project.mark_all_dirty();
        eprintln!(
            "before datum open: measured={}, tilt_eval={}",
            project.param_map()["measured_w"],
            8. * project.param_map()["measured_w"]
        );
        assert_close(8. * project.param_map()["measured_w"], 320., 1e-6, "pre-open tilt valid");
        qymcad_io::save_project_guarded_with_brep(&project, &r3_path("pending-datum-open.qcad"), &breps).unwrap();
        assert_open_refusal(&r3_path("pending-datum-open.qcad"), plane, "tilt_x");
    }
    #[test]
    fn open_must_recheck_late_projected_invalid_chamfer_angle() {
        let mut s = projected_measurement();
        let stock = source_stock(&mut s, 100., 40.0.into());
        let t = s.topology(Some(stock), true).unwrap();
        let x = t.faces.iter().find(|f| f.normal.unwrap()[0] > 0.99).unwrap().id;
        let y = t.faces.iter().find(|f| f.normal.unwrap()[1] > 0.99).unwrap().id;
        let e = t.edges.iter().find(|e| e.faces.unwrap().contains(&x) && e.faces.unwrap().contains(&y)).unwrap().id;
        let chamfer = s
            .chamfer_ex(&Chamfer {
                body: Some(stock),
                edges: Sel::Ids(vec![e]),
                dist: 2.0.into(),
                d2: None,
                angle: Some("measured_w/2".into()),
                reference_face: Some(Sel::Ids(vec![y])),
                flip: false,
                name: None,
            })
            .unwrap()
            .0;
        let before = &std::path::PathBuf::from(r3_path("pending-chamfer-before.qcad"));
        s.save(Some(before)).unwrap();
        let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(before.to_str().unwrap()).unwrap();
        project.parameters.iter_mut().find(|p| p.name == "w").unwrap().expr = "200".into();
        project.eval_parameters();
        project.mark_all_dirty();
        eprintln!(
            "before chamfer open: measured={}, angle_eval={}",
            project.param_map()["measured_w"],
            project.param_map()["measured_w"] / 2.
        );
        assert_close(project.param_map()["measured_w"] / 2., 20., 1e-6, "pre-open angle valid");
        qymcad_io::save_project_guarded_with_brep(&project, &r3_path("pending-chamfer-open.qcad"), &breps).unwrap();
        assert_open_refusal(&r3_path("pending-chamfer-open.qcad"), chamfer, "angle");
    }
}

mod move_probe {
    fn r3_path(name: &str) -> String {
        super::common::scratch(&format!("r3-move-{:?}-{name}", std::thread::current().id())).to_str().unwrap().to_string()
    }
    use super::common;
    #[cfg(test)]
    mod tests {
        use super::{common, r3_path};
        use qymcad_core::model::{Constraint, ProjSource};
        use qymcad_engine::*;
        use std::path::Path;

        fn stock(s: &mut Session, x: f64, width: Num) -> Id {
            let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
            s.sketch_rect(sk, &x.into(), &15.0.into(), &width, &30.0.into(), false).unwrap();
            s.extrude(&serde_json::from_value(serde_json::json!({"sketch":sk,"height":10,"op":"new_body"})).unwrap()).unwrap().0
        }
        fn breps(s: &Session) -> Vec<(Id, Vec<u8>)> {
            let _gate = qymcad_kernel::kernel_gate();
            s.project()
                .timeline
                .iter()
                .flat_map(|n| n.kind.bodies())
                .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
                .collect()
        }
        fn projected_measurement() -> (Session, Id) {
            let mut s = Session::new_part();
            s.param_set("w", &40.0.into()).unwrap();
            let source = stock(&mut s, 20.0, "w".into());
            let target = stock(&mut s, 100.0, 10.0.into());
            let projected = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("projected measurement")).unwrap();
            let edge = s.project().regen_edges[&source]
                .iter()
                .find(|e| e.a[1].abs() < 1e-6 && e.b[1].abs() < 1e-6 && (e.a[2] - 10.).abs() < 1e-6 && (e.b[2] - 10.).abs() < 1e-6)
                .unwrap()
                .id;
            let refs = s.sketch_project(projected, source, &[ProjSource::Edge(edge)]).unwrap().0.points;
            let mut p = s.project().clone();
            let si = p.sketch_index(projected).unwrap();
            p.sketches[si].constraints.push(Constraint::Distance {
                a: refs[0],
                b: refs[1],
                d: 40.,
                off: 0.,
                expr: String::new(),
                driven: true,
                axis: 1,
                at: None,
            });
            assert!(p.add_named_dim("measured_w".into(), projected, refs));
            p.solve_sketch(si);
            p.mark_sketch_dirty(projected);
            let path = &r3_path("projected-source.qcad");
            qymcad_io::save_project_guarded_with_brep(&p, path, &breps(&s)).unwrap();
            let (s, r) = Session::open(Path::new(path)).unwrap();
            assert!(r.errors.is_empty(), "{r:?}");
            (s, target)
        }
        #[test]
        fn projected_named_source_move_updates_after_engine_parameter_edit() {
            let (mut s, target) = projected_measurement();
            let (m, r) = s
                .move_body(&Move {
                    body: Some(target),
                    translation: Some(["measured_w/4".into(), 0.0.into(), 0.0.into()]),
                    rotation: None,
                    name: None,
                })
                .unwrap();
            assert!(r.errors.is_empty());
            let before = s.result_bodies().into_iter().find(|b| b.id == m.body).unwrap().bbox;
            s.save(Some(&std::path::PathBuf::from(r3_path("projected-move.qcad")))).unwrap();
            let (_, native_bbox, _, native_p) =
                common::gui_edit_param_model(&std::path::PathBuf::from(r3_path("projected-move.qcad")), "w", "60");
            eprintln!("native measurement={}, native_bbox={native_bbox:?}", native_p.param_map()["measured_w"]);
            common::assert_close(native_p.param_map()["measured_w"], 60., 1e-6, "native refreshed reference");
            common::assert_close(native_bbox[0], 105., 1e-6, "F-120 native previous Move");
            let r = s.param_set("w", &60.0.into()).unwrap();
            assert!(r.errors.is_empty(), "{r:?}");
            let after = s.result_bodies().into_iter().find(|b| b.id == m.body).unwrap().bbox;
            eprintln!("measurement={}, before={before:?}, after={after:?}", s.project().param_map()["measured_w"]);
            let rebuilt = s.rebuild();
            eprintln!(
                "second rebuild errors={:?}, body={:?}",
                rebuilt.errors,
                rebuilt.bodies.iter().find(|b| b.id == m.body).unwrap().bbox
            );
            s.save(Some(&std::path::PathBuf::from(r3_path("projected-move-after.qcad")))).unwrap();
            let (opened, report) = Session::open(&std::path::PathBuf::from(r3_path("projected-move-after.qcad"))).unwrap();
            eprintln!("reopen errors={:?}, body={:?}", report.errors, opened.result_bodies().iter().find(|b| b.id == m.body).unwrap().bbox);
            assert!(report.errors.is_empty(), "{report:?}");
            assert!(rebuilt.errors.is_empty(), "{rebuilt:?}");
            assert!((rebuilt.bodies.iter().find(|b| b.id == m.body).unwrap().bbox[0] - 110.).abs() < 1e-6);
            assert!((opened.result_bodies().iter().find(|b| b.id == m.body).unwrap().bbox[0] - 110.).abs() < 1e-6);
            assert!((after[0] - 110.).abs() < 1e-5, "Move should follow measured_w/4=15 after w=60, target min=95 plus shift15=110");
        }
    }
}

mod order {
    fn r3_path(name: &str) -> String {
        super::common::scratch(&format!("r3-order-{:?}-{name}", std::thread::current().id())).to_str().unwrap().to_string()
    }
    use super::common;
    #[cfg(test)]
    mod tests {
        use super::{common, r3_path};
        use qymcad_core::model::Constraint;
        use qymcad_engine::*;
        fn ext(s: &mut Session, sk: Id) -> Id {
            s.extrude(&Extrude {
                sketch: sk,
                profiles: None,
                height: 10.0.into(),
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
        fn fixture(source_first: bool) -> (Session, Id, Id, Id, Id, usize, std::path::PathBuf) {
            let mut s = Session::new_part();
            s.param_set("w", &40.0.into()).unwrap();
            let stocksk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("stock profile")).unwrap();
            s.sketch_rect(stocksk, &20.0.into(), &15.0.into(), &40.0.into(), &30.0.into(), false).unwrap();
            let stock = ext(&mut s, stocksk);
            let marker = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("early marker")).unwrap();
            let point = s.sketch_point(marker, &10.0.into(), &6.0.into(), true).unwrap().point;
            let source = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("later world measurement")).unwrap();
            s.sketch_rect(source, &20.0.into(), &15.0.into(), &"w".into(), &30.0.into(), false).unwrap();
            let mut p = s.project().clone();
            let si = p.sketch_index(source).unwrap();
            let pts: Vec<_> = p.sketches[si].points.iter().filter(|pt| !p.sketches[si].system_ids().contains(&pt.id)).collect();
            let a = pts.iter().find(|pt| pt.x.abs() < 1e-6 && pt.y.abs() < 1e-6).unwrap().id;
            let b = pts.iter().find(|pt| (pt.x - 40.).abs() < 1e-6 && (pt.y - 30.).abs() < 1e-6).unwrap().id;
            if source_first {
                // R5: references are unsupported even earlier; retain X15 with a genuine driving width.
                let c = p.sketches[si].constraints.iter().find(|c| c.expr() == Some("w")).unwrap();
                assert!(p.add_named_dim("measured_w".into(), source, qymcad_core::model::Project::dim_refs(c).unwrap()));
            } else {
                p.sketches[si].constraints.push(Constraint::Distance {
                    a,
                    b,
                    d: 40.,
                    off: 0.,
                    expr: String::new(),
                    driven: true,
                    axis: 1,
                    at: None,
                });
                assert!(p.add_named_dim("measured_w".into(), source, vec![a, b]));
            }
            p.solve_sketch(si);
            let breps = {
                let _g = qymcad_kernel::kernel_gate();
                p.timeline
                    .iter()
                    .flat_map(|n| n.kind.bodies())
                    .filter_map(|id| s.shape(id).map(|sh| (id, sh.to_brep_bytes().unwrap())))
                    .collect::<Vec<_>>()
            };
            if source_first {
                let marker_index = p.sketch_index(marker).unwrap();
                let source_index = p.sketch_index(source).unwrap();
                p.sketches.swap(marker_index, source_index);
            }
            let file = std::path::PathBuf::from(r3_path(if source_first { "source-first.qcad" } else { "order-native.qcad" }));
            qymcad_io::save_project_guarded_with_brep(&p, file.to_str().unwrap(), &breps).unwrap();
            let (s, r) = Session::open(&file).unwrap();
            assert!(r.errors.is_empty(), "{r:?}");
            let ci = s
                .sketch_detail(marker)
                .unwrap()
                .constraints
                .into_iter()
                .find(|c| c.kind == "distance_point_line" && c.points.contains(&point))
                .unwrap()
                .index;
            (s, stock, marker, point, source, ci, file)
        }

        fn hole(stock: Id, marker: Id) -> HoleSketch {
            HoleSketch {
                body: Some(stock),
                sketch: marker,
                points: None,
                flip: true,
                diameter: 4.0.into(),
                depth: Some(4.0.into()),
                kind: HoleKind::Plain,
                dia2: None,
                depth2: None,
                name: None,
            }
        }
        fn archive(s: &mut Session) -> std::collections::BTreeMap<String, Vec<u8>> {
            super::super::a1a2::archive_bytes(s, &r3_path("archive.qcad"))
        }
        fn refusal(e: String, source: Id) {
            assert!(
                e.contains("later world measurement") && e.contains(&source.to_string()) && e.contains("reference measurement measured_w"),
                "{e}"
            );
        }
        #[test]
        fn later_world_measurement_source_refuses_creation_atomically() {
            let (mut s, stock, marker, _, source, ci, _) = fixture(false);
            s.param_set("alias1", &"measured_w/4".into()).unwrap();
            s.param_set("alias2", &"alias1+0".into()).unwrap();
            s.dimension_set(&DimensionTarget::Sketch { sketch: marker, constraint: ci }, Some(&"alias2".into()), None).unwrap();
            let before = archive(&mut s);
            refusal(s.hole_by_sketch(&hole(stock, marker)).unwrap_err().to_string(), source);
            assert_eq!(archive(&mut s), before);
        }
        #[test]
        fn later_world_measurement_source_refuses_coordinate_edit_atomically() {
            let (mut s, stock, marker, _, source, ci, _) = fixture(false);
            s.hole_by_sketch(&hole(stock, marker)).unwrap();
            let before = archive(&mut s);
            refusal(
                s.dimension_set(&DimensionTarget::Sketch { sketch: marker, constraint: ci }, Some(&"measured_w/4".into()), None)
                    .unwrap_err()
                    .to_string(),
                source,
            );
            assert_eq!(archive(&mut s), before);
        }
        #[test]
        fn later_world_measurement_source_refuses_open_without_rewriting() {
            let (mut s, stock, marker, _, source, ci, file) = fixture(false);
            s.hole_by_sketch(&hole(stock, marker)).unwrap();
            s.save(Some(&file)).unwrap();
            let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(file.to_str().unwrap()).unwrap();
            let si = project.sketch_index(marker).unwrap();
            *project.sketches[si].constraints[ci].expr_mut().unwrap() = "measured_w/4".into();
            qymcad_io::save_project_guarded_with_brep(&project, file.to_str().unwrap(), &breps).unwrap();
            let before = std::fs::read(&file).unwrap();
            let e = match Session::open(&file) {
                Ok(_) => panic!("accepted late source"),
                Err(e) => e,
            };
            refusal(e.to_string(), source);
            assert_eq!(std::fs::read(file).unwrap(), before);
        }
        #[test]
        fn earlier_solve_order_world_driving_control() {
            let (mut s, stock, marker, point, _, ci, file) = fixture(true);
            s.dimension_set(&DimensionTarget::Sketch { sketch: marker, constraint: ci }, Some(&"measured_w/4".into()), None).unwrap();
            let (hole, r) = s.hole_by_sketch(&hole(stock, marker)).unwrap();
            assert!(r.errors.is_empty(), "{r:?}");
            s.save(Some(&file)).unwrap();
            let (_, _, faces, native) = common::gui_edit_param_model(&file, "w", "60");
            let face = faces
                .iter()
                .find(|f| {
                    native.names.get(f.id).is_some_and(|n| n.feature == hole && n.src == point && n.role == qymcad_core::names::Role::Hole)
                })
                .unwrap();
            assert!((face.centroid.x - 15.).abs() < 1e-6, "saved native app bore X={}", face.centroid.x);
            s.param_set("w", &60.0.into()).unwrap();
            let t = s.topology(Some(hole), false).unwrap();
            let face = t
                .faces
                .iter()
                .find(|f| {
                    s.project()
                        .names
                        .get(f.id)
                        .is_some_and(|n| n.feature == hole && n.src == point && n.role == qymcad_core::names::Role::Hole)
                })
                .unwrap();
            assert!((face.centroid[0] - 15.).abs() < 1e-6, "engine bore X={}", face.centroid[0]);
        }
    }
}
