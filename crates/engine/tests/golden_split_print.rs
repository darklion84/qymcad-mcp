//! Independent volume, persistence and selected-export checks for fixed-count print splits (ADR 0024).

mod common;
use common::*;
use qymcad_engine::*;
use std::f64::consts::PI;

fn opts(body: Id, plane: PlaneRef, offset: Num) -> SplitOptions {
    SplitOptions { body: Some(body), plane, offset, name: Some("print split".into()), piece_names: None }
}

fn solid(s: &mut Session, sketch: Id, height: f64, name: &str) -> Id {
    s.extrude(&Extrude {
        down: None,
        sketch,
        profiles: None,
        height: height.into(),
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: Some(name.into()),
    })
    .unwrap()
    .0
}

fn box20(s: &mut Session) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &10.0.into(), &10.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    solid(s, sk, 20.0, "stock")
}

fn u_body(s: &mut Session) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let vertices = [(-10.0, 0.0), (10.0, 0.0), (10.0, 20.0), (6.0, 20.0), (6.0, 4.0), (-6.0, 4.0), (-6.0, 20.0), (-10.0, 20.0)];
    s.sketch_polyline(
        sk,
        &PolylineSpec {
            points: vertices.into_iter().map(|(x, y)| Xy(x.into(), y.into())).collect(),
            closed: true,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    solid(s, sk, 10.0, "U stock")
}

fn volumes(bodies: &[BodyInfo], expected: &[f64]) {
    assert_eq!(bodies.len(), expected.len(), "every split piece is a body");
    for (body, volume) in bodies.iter().zip(expected) {
        assert_close(body.volume, *volume, 1e-5, "independent piece volume");
    }
    assert_close(bodies.iter().map(|b| b.volume).sum(), expected.iter().sum(), 1e-5, "pieces sum to the whole");
}

fn bbox(actual: [f64; 6], expected: [f64; 6]) {
    for (a, e) in actual.into_iter().zip(expected) {
        assert_close(a, e, 0.001, "piece bound");
    }
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

#[test]
fn split_box_offsets_survive_edit_reopen_and_app() {
    let mut s = Session::new_part();
    s.param_set("z", &10.0.into()).unwrap();
    let body = box20(&mut s);
    let result = s.split(&opts(body, PlaneRef::Base(BaseName::XY), Num::Expr("z".into()))).unwrap();
    volumes(&result.pieces, &[4000.0, 4000.0]);
    assert_eq!(result.body, result.feature);
    assert_eq!(result.name, "print split");
    assert_eq!(result.offset, 10.0);
    assert!(result.rebuild.errors.is_empty());
    assert_eq!(s.project().feat_dims[&result.feature]["offset"], "z");
    bbox(result.pieces[0].bbox, [0.0, 0.0, 0.0, 20.0, 20.0, 10.0]);
    bbox(result.pieces[1].bbox, [0.0, 0.0, 10.0, 20.0, 20.0, 20.0]);
    let path = scratch("split-box-print.qcad");
    s.save(Some(&path)).unwrap();
    let (mut reopened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    assert_eq!(reopened.result_bodies(), result.pieces);
    reopened.param_set("z", &15.0.into()).unwrap();
    volumes(&reopened.result_bodies(), &[6000.0, 2000.0]);
    assert_eq!(reopened.result_bodies().iter().map(|b| b.id).collect::<Vec<_>>(), result.pieces.iter().map(|b| b.id).collect::<Vec<_>>());
    let (v, bb, faces) = gui_edit_param_faces(&path, "z", "15");
    assert_close(v, 2000.0, 1e-6, "app's second piece");
    for (a, e) in bb.into_iter().zip([0.0, 0.0, 15.0, 20.0, 20.0, 20.0]) {
        assert_close(a, e, 0.05, "app piece bound");
    }
    assert!(faces.iter().any(|f| (f.centroid.z - 15.0).abs() < 1e-6), "app rebuild has moved the split cap");
}

#[test]
fn split_u_xz_positive_y_keeps_three_bodies_and_refuses_count_changes() {
    let mut s = Session::new_part();
    s.param_set("y", &10.0.into()).unwrap();
    let body = u_body(&mut s);
    let split = s.split(&opts(body, PlaneRef::Base(BaseName::XZ), Num::Expr("y".into()))).unwrap();
    volumes(&split.pieces, &[1280.0, 400.0, 400.0]);
    bbox(split.pieces[0].bbox, [-10.0, 0.0, 0.0, 10.0, 10.0, 10.0]);
    let path = scratch("split-u-print.qcad");
    s.save(Some(&path)).unwrap();
    let (mut reopened, _) = Session::open(&path).unwrap();
    assert_eq!(reopened.result_bodies(), split.pieces);
    reopened.param_set("y", &12.0.into()).unwrap();
    volumes(&reopened.result_bodies(), &[1440.0, 320.0, 320.0]);
    let before = reopened.info();
    let bytes = breps(&reopened);
    let err = reopened.param_set("y", &2.0.into()).unwrap_err().to_string();
    assert!(err.contains('2') && err.contains('3'), "count-changing reason: {err}");
    assert_eq!(reopened.info(), before, "failed edit restores parameter, ids, names, diagnostics and every piece");
    assert_eq!(breps(&reopened), bytes, "failed edit restores exact B-reps");
    let (v, bb, _) = gui_edit_param_faces(&path, "y", "12");
    assert_close(v, 320.0, 1e-6, "app's final U limb");
    assert_close(bb[1], 12.0, 0.05, "app's native XZ +Y offset");
    assert_close(bb[4], 20.0, 0.05, "U limb end");
    let mut two = Session::new_part();
    two.param_set("y", &2.0.into()).unwrap();
    let body = u_body(&mut two);
    two.split(&opts(body, PlaneRef::Base(BaseName::XZ), Num::Expr("y".into()))).unwrap();
    let before = two.info();
    let err = two.param_set("y", &10.0.into()).unwrap_err().to_string();
    assert!(err.contains('2') && err.contains('3'), "reverse changed-count reason: {err}");
    assert_eq!(two.info(), before);
}

#[test]
fn split_every_piece_resolves_by_numeric_and_native_name() {
    let mut s = Session::new_part();
    let src = u_body(&mut s);
    let mut args = opts(src, PlaneRef::Base(BaseName::XZ), 10.0.into());
    args.piece_names = Some(vec!["bridge".into(), "limb A".into(), "limb B".into()]);
    let split = s.split(&args).unwrap();
    for (piece, name) in split.pieces.iter().zip(["bridge", "limb A", "limb B"]) {
        assert_eq!(piece.name, name);
        assert_eq!(s.resolve(&piece.id.to_string()).unwrap(), piece.id);
        assert_eq!(s.resolve(name).unwrap(), piece.id);
        let index = s.project().mesh_index(piece.id).unwrap();
        assert_eq!(s.project().bodies[index].name, name, "native body name");
    }
    assert_eq!(s.resolve("print split").unwrap(), split.feature, "feature alias retained");
    assert_eq!(s.resolve("U stock").unwrap(), src, "ordinary consumed feature alias retained");
    assert_eq!(s.info().bodies, split.pieces);
    let path = scratch("split-native-names-print.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, _) = Session::open(&path).unwrap();
    for piece in &split.pieces {
        assert_eq!(opened.resolve(&piece.name).unwrap(), piece.id);
    }
    // Another feature may keep its own alias while colliding with a native body alias.
    opened.sketch_create(&PlaneRef::Base(BaseName::XY), Some("limb B")).unwrap();
    assert!(opened.resolve("limb B").unwrap_err().to_string().contains("several"));
    let original = Session::new_part();
    assert!(original.resolve("999999").is_err());
}

#[test]
fn split_open_restores_a_missing_secondary_brep() {
    let mut s = Session::new_part();
    let src = u_body(&mut s);
    let split = s.split(&opts(src, PlaneRef::Base(BaseName::XZ), 10.0.into())).unwrap();
    let path = scratch("split-secondary-brep-print.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { project, mut breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let last = split.pieces[2].id;
    breps.retain(|(id, _)| *id != last);
    assert!(breps.iter().any(|(id, _)| *id == split.body));
    assert!(project.timeline.iter().all(|n| !n.dirty), "missing piece is the only rebuild trigger");
    qymcad_io::save_project_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty());
    volumes(&opened.result_bodies(), &[1280.0, 400.0, 400.0]);
    assert!(opened.shape(last).is_some(), "secondary B-rep is rebuilt");
}

fn triangles_volume(triangles: &[[[f64; 3]; 3]]) -> f64 {
    triangles
        .iter()
        .map(|[a, b, c]| {
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
        })
        .sum()
}

fn triangles_bbox(triangles: &[[[f64; 3]; 3]]) -> [f64; 6] {
    let mut bounds = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for point in triangles.iter().flatten() {
        for axis in 0..3 {
            bounds[axis] = bounds[axis].min(point[axis]);
            bounds[axis + 3] = bounds[axis + 3].max(point[axis]);
        }
    }
    bounds
}

fn read_stl(path: &std::path::Path) -> Vec<[[f64; 3]; 3]> {
    let bytes = std::fs::read(path).unwrap();
    let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), 84 + count * 50);
    let number = |at| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as f64;
    (0..count)
        .map(|i| {
            [0, 1, 2].map(|v| {
                let at = 84 + i * 50 + 12 + v * 12;
                [number(at), number(at + 4), number(at + 8)]
            })
        })
        .collect()
}

#[test]
fn split_selected_piece_exports_alone_in_stl_3mf_step() {
    let mut s = Session::new_part();
    let src = u_body(&mut s);
    let split = s.split(&opts(src, PlaneRef::Base(BaseName::XZ), 10.0.into())).unwrap();
    for (index, piece) in split.pieces.iter().enumerate() {
        let expected_volume = if index == 0 { 1280.0 } else { 400.0 };
        let expected_bbox = if index == 0 {
            [-10.0, 0.0, 0.0, 10.0, 10.0, 10.0]
        } else if piece.bbox[0] < 0.0 {
            [-10.0, 10.0, 0.0, -6.0, 20.0, 10.0]
        } else {
            [6.0, 10.0, 0.0, 10.0, 20.0, 10.0]
        };
        for (format, ext) in [(ExportFormat::Stl, "stl"), (ExportFormat::ThreeMf, "3mf"), (ExportFormat::Step, "step")] {
            let path = scratch(&format!("split-piece-{index}.{ext}"));
            let selected = s.resolve(&piece.name).unwrap();
            let report = s.export(format, &path, Quality::Standard, Some(&[selected])).unwrap();
            assert_eq!(report.bodies.len(), 1);
            assert_eq!(report.bodies[0].id, piece.id);
            match format {
                ExportFormat::Step => {
                    let _gate = qymcad_kernel::kernel_gate();
                    let (_, shapes) = qymcad_kernel::read_exact(qymcad_kernel::ExactFormat::Step, path.to_str().unwrap(), 0.5).unwrap();
                    assert_eq!(shapes.len(), 1, "exactly one piece in STEP");
                    assert_close(shapes[0].volume(), expected_volume, 1e-5, "selected STEP piece volume");
                    for (a, e) in shapes[0].bbox().unwrap().into_iter().zip(expected_bbox) {
                        assert_close(a, e, 0.05, "STEP bound");
                    }
                }
                ExportFormat::Stl => {
                    let triangles = read_stl(&path);
                    assert_close(triangles_volume(&triangles), expected_volume, 1e-5, "selected STL piece volume");
                    bbox(triangles_bbox(&triangles), expected_bbox);
                }
                ExportFormat::ThreeMf => {
                    let meshes = qymcad_io::import_3mf(path.to_str().unwrap()).unwrap();
                    assert_eq!(meshes.len(), 1, "exactly one piece object in 3MF");
                    let mesh = &meshes[0].mesh;
                    let triangles: Vec<_> = mesh
                        .tris
                        .iter()
                        .map(|t| {
                            t.map(|i| {
                                let p = mesh.verts[i as usize];
                                [p.x, p.y, p.z]
                            })
                        })
                        .collect();
                    assert_close(triangles_volume(&triangles), expected_volume, 1e-5, "selected 3MF piece volume");
                    bbox(triangles_bbox(&triangles), expected_bbox);
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn split_datum_face_and_yz_planes_use_native_normals() {
    let mut datum = Session::new_part();
    let body = box20(&mut datum);
    let (plane, _) = datum.plane_offset(&PlaneRef::Base(BaseName::XY), &5.0.into(), Some("cut datum")).unwrap();
    let split = datum.split(&opts(body, PlaneRef::Plane(plane), 5.0.into())).unwrap();
    volumes(&split.pieces, &[4000.0, 4000.0]);
    let mut face = Session::new_part();
    let body = box20(&mut face);
    let top = face.topology(Some(body), false).unwrap().faces.into_iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    let split = face.split(&opts(body, PlaneRef::Face { body, face: top }, (-5.0).into())).unwrap();
    volumes(&split.pieces, &[6000.0, 2000.0]);
    assert!(
        matches!(&face.project().timeline.iter().find(|n|n.id==split.feature).unwrap().kind,qymcad_core::feature::FeatureKind::SplitBody{face:Some((b,_)),..}if *b==body)
    );
    assert!(face.project().planes.is_empty(), "face reference creates no auxiliary datum");
    let mut yz = Session::new_part();
    let body = box20(&mut yz);
    let split = yz.split(&opts(body, PlaneRef::Base(BaseName::YZ), 5.0.into())).unwrap();
    volumes(&split.pieces, &[2000.0, 6000.0]);
    bbox(split.pieces[0].bbox, [0.0, 0.0, 0.0, 5.0, 20.0, 20.0]);
}

#[test]
fn split_refuses_noncutting_planes_bad_references_and_bad_names_atomically() {
    let mut s = Session::new_part();
    let body = box20(&mut s);
    for offset in [-1.0, 0.0, 20.0, 21.0] {
        let before = s.info();
        let err = s.split(&opts(body, PlaneRef::Base(BaseName::XY), offset.into())).expect_err("no cut must refuse").to_string();
        assert!(err.contains("cut") || err.contains("split"), "clear no-cut reason: {err}");
        assert_eq!(s.info(), before);
    }
    for names in [vec!["one"], vec!["same", "same"], vec!["good", " bad "]] {
        let mut args = opts(body, PlaneRef::Base(BaseName::XY), 10.0.into());
        args.piece_names = Some(names.into_iter().map(str::to_owned).collect());
        let before = s.info();
        assert!(s.split(&args).is_err());
        assert_eq!(s.info(), before);
    }
    for plane in [PlaneRef::Plane(99999), PlaneRef::Face { body, face: u32::MAX }, PlaneRef::Face { body: 99999, face: 1 }] {
        let before = s.info();
        assert!(s.split(&opts(body, plane, 10.0.into())).is_err());
        assert_eq!(s.info(), before);
    }
    assert!(s.split(&opts(99999, PlaneRef::Base(BaseName::XY), 10.0.into())).is_err());
}

#[test]
fn split_cap_fillet_survives_parameter_edit_reopen_and_app() {
    let mut s = Session::new_part();
    s.param_set("z", &10.0.into()).unwrap();
    s.param_set("r", &1.0.into()).unwrap();
    let body = box20(&mut s);
    let split = s.split(&opts(body, PlaneRef::Base(BaseName::XY), Num::Expr("z".into()))).unwrap();
    let first = split.pieces[0].id;
    let edge = s
        .topology(Some(first), false)
        .unwrap()
        .edges
        .into_iter()
        .find(|e| e.kind == EdgeKind::Line && (e.a[2] - 10.0).abs() < 1e-6 && (e.b[2] - 10.0).abs() < 1e-6)
        .expect("new split cap edge");
    let (fillet, report) = s.fillet(Some(first), &Sel::Ids(vec![edge.id]), &Num::Expr("r".into()), Some("split cap fillet")).unwrap();
    assert!(report.errors.is_empty());
    let removal = (1.0 - PI / 4.0) * 20.0;
    let volume = |s: &Session| s.result_bodies().iter().find(|b| b.id == fillet).unwrap().volume;
    assert_close(volume(&s), 4000.0 - removal, 1e-5, "one straight cap edge analytic fillet");
    let path = scratch("split-fillet-print.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, _) = Session::open(&path).unwrap();
    assert_close(volume(&opened), 4000.0 - removal, 1e-5, "saved cap fillet");
    opened.param_set("z", &12.0.into()).unwrap();
    assert_close(volume(&opened), 4800.0 - removal, 1e-5, "moved split cap retains fillet");
    let (v, _, _) = gui_edit_param_faces(&path, "z", "12");
    assert_close(v, 4800.0 - removal, 1e-5, "app moved split-cap fillet");
    let before = opened.info();
    let err = opened.param_set("r", &30.0.into()).unwrap_err().to_string();
    assert!(err.contains("fillet") || err.contains("blend") || err.contains("radius"), "fillet refusal: {err}");
    assert_eq!(opened.info(), before, "unbuildable cap fillet rolls back");
}

#[test]
fn deleting_split_removes_siblings_and_secondary_dependents_then_undo_restores_them() {
    let mut s = Session::new_part();
    let src = u_body(&mut s);
    let split = s.split(&opts(src, PlaneRef::Base(BaseName::XZ), 10.0.into())).unwrap();
    let last = split.pieces[2].id;
    let topo = s.topology(Some(last), false).unwrap();
    let edge =
        topo.edges.iter().find(|e| e.kind == EdgeKind::Line && (e.a[2] - 10.0).abs() < 1e-6 && (e.b[2] - 10.0).abs() < 1e-6).unwrap();
    let (fillet, _) = s.fillet(Some(last), &Sel::Ids(vec![edge.id]), &0.5.into(), Some("limb fillet")).unwrap();
    let before = s.info();
    assert!(s.feature_delete(last, true).is_err(), "a secondary body is not a standalone timeline feature");
    assert!(s.feature_delete(split.feature, false).is_err(), "secondary-body dependencies block noncascade deletion");
    let mut snapshot = s.begin_tool_edit().unwrap();
    snapshot.record_call("feature_delete", serde_json::json!({"feature":split.feature,"cascade":true}));
    s.feature_delete(split.feature, true).unwrap();
    s.finish_tool_edit(snapshot, true);
    assert_eq!(s.result_bodies().iter().map(|b| b.id).collect::<Vec<_>>(), vec![src]);
    for id in split.pieces.iter().map(|b| b.id).chain([fillet]) {
        assert!(s.shape(id).is_none());
    }
    s.undo().unwrap();
    assert_eq!(s.info(), before);
}
