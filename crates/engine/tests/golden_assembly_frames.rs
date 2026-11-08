//! Package A: placements leave local geometry intact and enter output exactly once.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::{json, Value};

const LOCAL: [f64; 6] = [0., 0., 0., 10., 20., 30.];
const TRANSLATED: [f64; 6] = [100., 200., 300., 110., 220., 330.];
const ROTATED: [f64; 6] = [80., 200., 300., 100., 210., 330.];

fn stock(mesh: bool) -> Session {
    let mut s = Session::new_part();
    if mesh {
        // Eight box vertices and twelve outward triangles: V=10*20*30=6000 mm³.
        let path = scratch("a-frames-box.ply");
        std::fs::write(&path,"ply\nformat ascii 1.0\nelement vertex 8\nproperty float x\nproperty float y\nproperty float z\nelement face 12\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n10 0 0\n10 20 0\n0 20 0\n0 0 30\n10 0 30\n10 20 30\n0 20 30\n3 0 2 1\n3 0 3 2\n3 4 5 6\n3 4 6 7\n3 0 1 5\n3 0 5 4\n3 1 2 6\n3 1 6 5\n3 2 3 7\n3 2 7 6\n3 3 0 4\n3 3 4 7\n").unwrap();
        s.import_mesh(&path, None, None, 1., Some("box mesh")).unwrap();
    } else {
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
        s.sketch_rect(sk, &5.0.into(), &10.0.into(), &10.0.into(), &20.0.into(), false).unwrap();
        let ex: Extrude = serde_json::from_value(json!({"sketch":sk,"height":30,"name":"box"})).unwrap();
        s.extrude(&ex).unwrap();
    }
    s
}

fn placed(mut s: Session, rotated: bool, label: &str) -> Session {
    let path = scratch(&format!("a-frames-{label}.qcad"));
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let owner = project.body_owner(s.info().bodies[0].id).unwrap();
    project.components.iter_mut().find(|c| c.id == owner).unwrap().transform = if rotated {
        [0., -1., 0., 100., 1., 0., 0., 200., 0., 0., 1., 300.]
    } else {
        [1., 0., 0., 100., 0., 1., 0., 200., 0., 0., 1., 300.]
    };
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (s, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    s
}

fn bbox(actual: [f64; 6], expected: [f64; 6]) {
    for (a, e) in actual.into_iter().zip(expected) {
        assert_close(a, e, 1e-4, "world placement exactly once");
    }
}

fn json_bbox(actual: &Value, expected: [f64; 6]) {
    assert!(actual.is_array(), "missing bounds: {actual}");
    bbox(std::array::from_fn(|i| actual[i].as_f64().unwrap()), expected);
}

#[test]
fn assembly_body_reporting_keeps_local_and_world_frames_and_owners() {
    for mesh in [false, true] {
        for rotated in [false, true] {
            let mut s = placed(stock(mesh), rotated, &format!("report-{mesh}-{rotated}"));
            let info = serde_json::to_value(s.info()).unwrap();
            let b = &info["bodies"][0];
            let owner = s.project().body_owner(b["id"].as_u64().unwrap()).unwrap();
            assert_eq!(b["owner_part"], owner);
            json_bbox(&b["bbox_local"], LOCAL);
            json_bbox(&b["bbox_world"], if rotated { ROTATED } else { TRANSLATED });
            json_bbox(
                &b["bbox"],
                if mesh {
                    if rotated {
                        ROTATED
                    } else {
                        TRANSLATED
                    }
                } else {
                    LOCAL
                },
            );
            assert_eq!(b["bbox_frame"], if mesh { "world" } else { "part_local" });
            assert_close(b["volume_mm3"].as_f64().unwrap(), 6000., 1e-6, "10*20*30");
            assert_eq!(b["world_transform"][3], 100.);
            assert_eq!(info["active_component"], json!(s.project().active_component));
            assert!(info["components"].as_array().unwrap().iter().any(|c| c["id"] == owner));
            if !mesh {
                assert_eq!(info["sketches"][0]["owner_part"], owner);
                assert!(info["timeline"].as_array().unwrap().iter().all(|n| n["owner_part"] == owner));
                let t = s.topology(None, true).unwrap();
                assert_eq!(serde_json::to_value(&t).unwrap()["coordinate_frame"], "part_local");
                assert!(t.faces.iter().all(|f| f.centroid[0] <= 10.0001 && f.centroid[1] <= 20.0001));
            }
        }
    }
}

#[test]
fn assembly_mesh_render_bounds_apply_world_transform_once() {
    for rotated in [false, true] {
        let s = placed(stock(true), rotated, &format!("render-{rotated}"));
        bbox(s.render(View::Top, 64, 64, None).unwrap().bbox, if rotated { ROTATED } else { TRANSLATED });
    }
}

#[test]
fn assembly_brep_and_mesh_exports_read_back_world_once() {
    for mesh in [false, true] {
        for rotated in [false, true] {
            let s = placed(stock(mesh), rotated, &format!("export-{mesh}-{rotated}"));
            let want = if rotated { ROTATED } else { TRANSLATED };
            bbox(s.render(View::Top, 64, 64, None).unwrap().bbox, want);
            let path = scratch(&format!("a-world-{mesh}-{rotated}.3mf"));
            s.export(ExportFormat::ThreeMf, &path, Quality::Standard, None).unwrap();
            let imported = qymcad_io::import_3mf(path.to_str().unwrap()).unwrap();
            assert_eq!(imported.len(), 1);
            let mut actual = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
            for v in &imported[0].mesh.verts {
                for (k, x) in [v.x, v.y, v.z].into_iter().enumerate() {
                    actual[k] = actual[k].min(x);
                    actual[k + 3] = actual[k + 3].max(x);
                }
            }
            bbox(actual, want);
            assert_close(imported[0].mesh.volume(), 6000., 1e-4, "3MF box volume");
            if !mesh {
                let path = scratch(&format!("a-world-{rotated}.step"));
                s.export(ExportFormat::Step, &path, Quality::Standard, None).unwrap();
                let mut back = Session::new_part();
                back.import(&path, 1., None, None).unwrap();
                bbox(back.render(View::Top, 64, 64, None).unwrap().bbox, want);
                assert_close(back.info().bodies[0].volume, 6000., 1e-5, "STEP box volume");
            }
        }
    }
}
