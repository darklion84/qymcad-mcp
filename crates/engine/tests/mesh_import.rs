mod common;

use qymcad_engine::{BaseName, BodyBooleanOptions, BooleanOp, Direction, ExportFormat, Extrude, Move, Op, PlaneRef, Quality, Session};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn cube_stl() -> Vec<u8> {
    let vertices = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, 0.0, 10.0],
        [10.0, 0.0, 10.0],
        [10.0, 10.0, 10.0],
        [0.0, 10.0, 10.0],
    ];
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
    let mut out = vec![0; 84 + tris.len() * 50];
    out[80..84].copy_from_slice(&(tris.len() as u32).to_le_bytes());
    for (k, [a, b, c]) in tris.into_iter().enumerate() {
        for (j, v) in [vertices[a], vertices[b], vertices[c]].into_iter().enumerate() {
            for axis in 0..3 {
                out[84 + k * 50 + 12 + j * 12 + axis * 4..84 + k * 50 + 16 + j * 12 + axis * 4]
                    .copy_from_slice(&(v[axis] as f32).to_le_bytes());
            }
        }
    }
    out
}

fn cylinder_stl(segments: usize, radius: f64, height: f64) -> Vec<u8> {
    cylinder_stl_rows(segments, radius, height, 1)
}

fn write_closed_cylinder_with_triangles(path: &std::path::Path, triangles: usize) -> (usize, usize) {
    let segments = 100;
    assert_eq!(triangles % (segments * 2), 0);
    let rows = triangles / (segments * 2) - 1;
    std::fs::write(path, cylinder_stl_rows(segments, 10.0, 40.0, rows)).unwrap();
    (segments, rows)
}

fn cylinder_stl_rows(segments: usize, radius: f64, height: f64, rows: usize) -> Vec<u8> {
    let mut tris = Vec::with_capacity(segments * (2 * rows + 2));
    for i in 0..segments {
        let j = (i + 1) % segments;
        let a = i as f64 * std::f64::consts::TAU / segments as f64;
        let b = j as f64 * std::f64::consts::TAU / segments as f64;
        let points = |z: f64| [[radius * a.cos(), radius * a.sin(), z], [radius * b.cos(), radius * b.sin(), z]];
        let [a0, a1] = points(0.0);
        let [b0, b1] = points(height);
        tris.push([[0.0, 0.0, 0.0], a1, a0]);
        tris.push([[0.0, 0.0, height], b0, b1]);
        for row in 0..rows {
            let z0 = height * row as f64 / rows as f64;
            let z1 = height * (row + 1) as f64 / rows as f64;
            let [a0, a1] = points(z0);
            let [b0, b1] = points(z1);
            tris.push([a0, a1, b1]);
            tris.push([a0, b1, b0]);
        }
    }
    let mut out = vec![0; 84 + tris.len() * 50];
    out[80..84].copy_from_slice(&(tris.len() as u32).to_le_bytes());
    for (k, tri) in tris.into_iter().enumerate() {
        for (j, vertex) in tri.into_iter().enumerate() {
            for (axis, coordinate) in vertex.into_iter().enumerate() {
                let at = 84 + k * 50 + 12 + j * 12 + axis * 4;
                out[at..at + 4].copy_from_slice(&(coordinate as f32).to_le_bytes());
            }
        }
    }
    out
}

fn fixture() -> (PathBuf, PathBuf) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("qymcad-mesh-import-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir(&dir).unwrap();
    let input = dir.join("cube.stl");
    std::fs::write(&input, cube_stl()).unwrap();
    (dir, input)
}

fn assert_bbox(actual: [f64; 6], expected: [f64; 6]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-9, "bbox {actual} != {expected}");
    }
}

fn ascii_stl() -> String {
    let mut out = String::from("solid cube\n");
    let binary = cube_stl();
    for k in 0..12 {
        let at = 84 + k * 50;
        let v = |j: usize| {
            let p = at + 12 + j * 12;
            format!(
                "{} {} {}",
                f32::from_le_bytes(binary[p..p + 4].try_into().unwrap()),
                f32::from_le_bytes(binary[p + 4..p + 8].try_into().unwrap()),
                f32::from_le_bytes(binary[p + 8..p + 12].try_into().unwrap())
            )
        };
        out.push_str(&format!(
            " facet normal 0 0 0\n  outer loop\n   vertex {}\n   vertex {}\n   vertex {}\n  endloop\n endfacet\n",
            v(0),
            v(1),
            v(2)
        ));
    }
    out.push_str("endsolid cube\n");
    out
}

fn write_cube_ply(path: &std::path::Path) {
    let vertices = "0 0 0\n10 0 0\n10 10 0\n0 10 0\n0 0 10\n10 0 10\n10 10 10\n0 10 10\n";
    let faces = "3 0 2 1\n3 0 3 2\n3 4 5 6\n3 4 6 7\n3 0 1 5\n3 0 5 4\n3 1 2 6\n3 1 6 5\n3 2 3 7\n3 2 7 6\n3 3 0 4\n3 3 4 7\n";
    let text = format!("ply\nformat ascii 1.0\nelement vertex 8\nproperty float x\nproperty float y\nproperty float z\nelement face 12\nproperty list uchar int vertex_indices\nend_header\n{vertices}{faces}");
    std::fs::write(path, text).unwrap();
}

fn write_obj(path: &std::path::Path) {
    std::fs::write(path, "o first\nv 0 0 0\nv 10 0 0\nv 0 10 0\nf 1 2 3\no second\nv 0 0 10\nv 10 0 10\nv 0 10 10\nf 4 5 6\n").unwrap();
}

fn write_many_triangle_obj(path: &std::path::Path, count: usize) {
    let mut text = String::from("v 0 0 0\nv 1 0 0\nv 0 1 0\n");
    text.push_str(&"f 1 2 3\n".repeat(count));
    std::fs::write(path, text).unwrap();
}

fn write_amf(path: &std::path::Path, unit: &str) {
    std::fs::write(path, format!("<amf unit=\"{unit}\" version=\"1.1\"><object id=\"1\"><metadata type=\"name\">amf cube</metadata><mesh><vertices><vertex><coordinates><x>0</x><y>0</y><z>0</z></coordinates></vertex><vertex><coordinates><x>1</x><y>0</y><z>0</z></coordinates></vertex><vertex><coordinates><x>0</x><y>1</y><z>0</z></coordinates></vertex><vertex><coordinates><x>0</x><y>0</y><z>1</z></coordinates></vertex></vertices><volume><triangle><v1>0</v1><v2>1</v2><v3>2</v3></triangle><triangle><v1>0</v1><v2>3</v2><v3>1</v3></triangle><triangle><v1>0</v1><v2>2</v2><v3>3</v3></triangle><triangle><v1>1</v1><v2>3</v2><v3>2</v3></triangle></volume></mesh></object></amf>")).unwrap();
}

fn write_3mf(path: &std::path::Path, unit: &str, declared_size: Option<u64>) {
    let model = format!("<?xml version=\"1.0\"?><model unit=\"{unit}\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\"><resources><object id=\"1\" name=\"3mf cube\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"1\" y=\"0\" z=\"0\"/><vertex x=\"0\" y=\"1\" z=\"0\"/></vertices><triangles><triangle v1=\"0\" v2=\"1\" v3=\"2\"/></triangles></mesh></object></resources><build><item objectid=\"1\"/></build></model>");
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("3D/3dmodel.model", options).unwrap();
    zip.write_all(model.as_bytes()).unwrap();
    zip.finish().unwrap();
    if let Some(size) = declared_size {
        let mut bytes = std::fs::read(path).unwrap();
        let sig = [0x50, 0x4b, 0x01, 0x02];
        let at = bytes.windows(4).position(|w| w == sig).unwrap();
        bytes[at + 24..at + 28].copy_from_slice(&(size as u32).to_le_bytes());
        std::fs::write(path, bytes).unwrap();
    }
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { ALPHABET[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[(n & 63) as usize] as char } else { '=' });
    }
    out
}

fn gltf_json(buffer: &str) -> String {
    format!(
        r#"{{"asset":{{"version":"2.0"}},"buffers":[{{{buffer}}}],"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],"nodes":[{{"mesh":0}}],"scenes":[{{"nodes":[0]}}],"scene":0}}"#
    )
}

fn gltf_binary() -> Vec<u8> {
    let mut bin = Vec::new();
    for v in [[0.0_f32, 0.0, 0.0], [1.0, 2.0, 3.0], [0.0, 2.0, 0.0]] {
        for x in v {
            bin.extend_from_slice(&x.to_le_bytes());
        }
    }
    bin
}

fn write_gltf(path: &std::path::Path) {
    let bin = gltf_binary();
    let json = gltf_json(&format!("\"uri\":\"data:application/octet-stream;base64,{}\",\"byteLength\":36", base64(&bin)));
    std::fs::write(path, json).unwrap();
}

fn write_glb(path: &std::path::Path) {
    let bin = gltf_binary();
    let json = gltf_json("\"byteLength\":36");
    let mut json_bytes = json.into_bytes();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let mut bin_bytes = bin;
    while !bin_bytes.len().is_multiple_of(4) {
        bin_bytes.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin_bytes.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4e4f534au32.to_le_bytes());
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004e4942u32.to_le_bytes());
    out.extend_from_slice(&bin_bytes);
    std::fs::write(path, out).unwrap();
}

#[test]
fn ascii_stl_matches_binary_stl_exactly() {
    let (dir, binary) = fixture();
    let ascii = dir.join("cube.ascii.stl");
    std::fs::write(&ascii, ascii_stl()).unwrap();
    let a = Session::new_part().import_mesh(&ascii, None, None, 1.0, None).unwrap();
    let b = Session::new_part().import_mesh(&binary, None, None, 1.0, None).unwrap();
    assert_eq!(a.pieces[0].triangles, 12);
    assert_eq!(a.pieces[0].triangles, b.pieces[0].triangles);
    assert_bbox(a.pieces[0].bbox, [0.0, 0.0, 0.0, 10.0, 10.0, 10.0]);
    assert_bbox(a.pieces[0].bbox, b.pieces[0].bbox);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn obj_objects_and_ply_cube_are_separate_import_pieces() {
    let (dir, _) = fixture();
    let obj = dir.join("parts.obj");
    write_obj(&obj);
    let result = Session::new_part().import_mesh(&obj, None, None, 1.0, None).unwrap();
    assert_eq!(result.pieces.len(), 2);
    assert_eq!(result.pieces.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["first", "second"]);
    assert_eq!(result.pieces.iter().map(|p| p.triangles).sum::<usize>(), 2);
    let ply = dir.join("cube.ply");
    write_cube_ply(&ply);
    let result = Session::new_part().import_mesh(&ply, None, None, 1.0, None).unwrap();
    assert_eq!(result.pieces.len(), 1);
    assert_eq!(result.pieces[0].triangles, 12);
    assert_bbox(result.pieces[0].bbox, [0.0, 0.0, 0.0, 10.0, 10.0, 10.0]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn embedded_formats_apply_declared_units_and_gltf_axes() {
    let (dir, _) = fixture();
    let gltf = dir.join("triangle.gltf");
    write_gltf(&gltf);
    let result = Session::new_part().import_mesh(&gltf, None, None, 1.0, None).unwrap();
    let glb = dir.join("triangle.glb");
    write_glb(&glb);
    let glb_result = Session::new_part().import_mesh(&glb, None, None, 1.0, None).unwrap();
    assert_eq!(glb_result.pieces[0].triangles, 1);
    assert_bbox(glb_result.pieces[0].bbox, [0.0, -3000.0, 0.0, 1000.0, 0.0, 2000.0]);
    assert_eq!(result.pieces[0].triangles, 1);
    // glTF inward([x,y,z]) = [1000x,-1000z,1000y], so the hand-computed bbox is [0,-3000,0,1000,0,2000].
    assert_bbox(result.pieces[0].bbox, [0.0, -3000.0, 0.0, 1000.0, 0.0, 2000.0]);
    let amf = dir.join("inch.amf");
    write_amf(&amf, "inch");
    let result = Session::new_part().import_mesh(&amf, None, None, 1.0, None).unwrap();
    assert_bbox(result.pieces[0].bbox, [0.0, 0.0, 0.0, 25.4, 25.4, 25.4]);
    let three = dir.join("inch.3mf");
    write_3mf(&three, "inch", None);
    let result = Session::new_part().import_mesh(&three, None, None, 1.0, None).unwrap();
    assert_bbox(result.pieces[0].bbox, [0.0, 0.0, 0.0, 25.4, 25.4, 0.0]);
    assert!(Session::new_part().import_mesh(&gltf, None, Some("mm"), 1.0, None).is_err());
    let explicit_archive = dir.join("explicit.qcad");
    let mut explicit_session = Session::new_part();
    explicit_session.save(Some(&explicit_archive)).unwrap();
    let explicit_before = std::fs::read(&explicit_archive).unwrap();
    assert!(explicit_session.import_mesh(&gltf, None, Some("mm"), 1.0, None).is_err());
    explicit_session.save(Some(&explicit_archive)).unwrap();
    assert_eq!(std::fs::read(&explicit_archive).unwrap(), explicit_before);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn caps_and_refusals_are_atomic() {
    let (dir, path) = fixture();
    let saved = dir.join("before.qcad");
    let mut session = Session::new_part();
    session.save(Some(&saved)).unwrap();
    let before = std::fs::read(&saved).unwrap();
    let mut claimed = cube_stl();
    claimed[80..84].copy_from_slice(&(300_000_u32).to_le_bytes());
    std::fs::write(&path, claimed).unwrap();
    let started = std::time::Instant::now();
    let error = session.import_mesh(&path, None, None, 1.0, None).unwrap_err().to_string();
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert!(error.contains("triangle limit"));
    session.save(Some(&saved)).unwrap();
    assert_eq!(std::fs::read(&saved).unwrap(), before);
    let oversized = dir.join("oversized.stl");
    std::fs::write(&oversized, vec![0u8; 16 * 1024 * 1024 + 1]).unwrap();
    let refusal_archive = |path: &std::path::Path, format: Option<&str>, unit: Option<&str>| {
        let mut s = Session::new_part();
        let archive = path.with_extension("qcad");
        s.save(Some(&archive)).unwrap();
        let before = std::fs::read(&archive).unwrap();
        assert!(s.import_mesh(path, format, unit, 1.0, None).is_err());
        s.save(Some(&archive)).unwrap();
        assert_eq!(std::fs::read(&archive).unwrap(), before);
        std::fs::remove_file(archive).unwrap();
    };
    refusal_archive(&oversized, None, None);
    let oversized_error = Session::new_part().import_mesh(&oversized, None, None, 1.0, None).unwrap_err().to_string();
    assert!(oversized_error.contains("byte limit"), "{oversized_error}");
    // A text format has no header count: 200 001 faces over three vertices must hit the decoded cap.
    let many = dir.join("many.obj");
    let mut text = String::from("v 0 0 0\nv 1 0 0\nv 0 1 0\n");
    text.push_str(&"f 1 2 3\n".repeat(200_001));
    std::fs::write(&many, text).unwrap();
    let many_error = Session::new_part().import_mesh(&many, None, None, 1.0, None).unwrap_err().to_string();
    assert!(many_error.contains("triangle limit") && many_error.contains("200001"), "{many_error}");
    refusal_archive(&many, None, None);
    let zip = dir.join("large.3mf");
    write_3mf(&zip, "millimeter", Some(64 * 1024 * 1024 + 1));
    assert!(Session::new_part().import_mesh(&zip, None, None, 1.0, None).unwrap_err().to_string().contains("declared uncompressed limit"));
    refusal_archive(&zip, None, None);
    let missing = dir.join("missing.stl");
    let missing_archive = dir.join("missing.qcad");
    let mut missing_session = Session::new_part();
    missing_session.save(Some(&missing_archive)).unwrap();
    let missing_before = std::fs::read(&missing_archive).unwrap();
    assert!(missing_session.import_mesh(&missing, None, None, 1.0, None).is_err());
    missing_session.save(Some(&missing_archive)).unwrap();
    assert_eq!(std::fs::read(&missing_archive).unwrap(), missing_before);
    let unknown = dir.join("unknown.xyz");
    std::fs::write(&unknown, b"x").unwrap();
    refusal_archive(&unknown, None, None);
    let mismatch = dir.join("mismatch.obj");
    std::fs::write(&mismatch, cube_stl()).unwrap();
    refusal_archive(&mismatch, Some("stl"), None);
    let nan = dir.join("nan.obj");
    std::fs::write(&nan, "v NaN 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    refusal_archive(&nan, None, None);
    let empty = dir.join("empty.obj");
    std::fs::write(&empty, "v 0 0 0\n").unwrap();
    refusal_archive(&empty, None, None);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mesh_export_roundtrip_and_move_are_exact_and_brep_tools_refuse() {
    let (dir, path) = fixture();
    let mut session = Session::new_part();
    let imported = session.import_mesh(&path, None, None, 1.0, None).unwrap();
    let body = imported.pieces[0].body;
    let moved = session
        .move_body(&Move { body: Some(body), translation: Some([5.0.into(), (-2.0).into(), 3.0.into()]), rotation: None, name: None })
        .unwrap();
    assert_eq!([moved.0.transform[3], moved.0.transform[7], moved.0.transform[11]], [5.0, -2.0, 3.0]);
    assert_bbox(session.info().bodies[0].bbox, [5.0, -2.0, 3.0, 15.0, 8.0, 13.0]);
    let stl = dir.join("roundtrip.stl");
    session.export(ExportFormat::Stl, &stl, Quality::Draft, Some(&[moved.0.body])).unwrap();
    let roundtrip = Session::new_part().import_mesh(&stl, None, None, 1.0, None).unwrap();
    assert_eq!(roundtrip.pieces[0].triangles, 12);
    assert_bbox(roundtrip.pieces[0].bbox, [5.0, -2.0, 3.0, 15.0, 8.0, 13.0]);
    // Every B-rep operation on the current (moved) mesh body, explicit or by default, gets the one mesh refusal.
    let current = moved.0.body;
    let mesh_refusal = |error: String| {
        assert!(error.contains(&format!("body {current} is an imported mesh")), "{error}");
    };
    let ids = qymcad_engine::Sel::Ids(vec![1]);
    mesh_refusal(session.fillet(Some(current), &ids, &1.0.into(), None).unwrap_err().to_string());
    mesh_refusal(session.fillet(None, &ids, &1.0.into(), None).unwrap_err().to_string());
    mesh_refusal(session.chamfer(None, &ids, &1.0.into(), None, None).unwrap_err().to_string());
    mesh_refusal(session.shell(None, &ids, &1.0.into(), qymcad_engine::Side::Inward, None).unwrap_err().to_string());
    mesh_refusal(session.topology(None, false).unwrap_err().to_string());
    mesh_refusal(session.select(None, qymcad_engine::Element::Faces, &ids).unwrap_err().to_string());
    mesh_refusal(session.mirror(None, &qymcad_engine::PlaneRef::Base(qymcad_engine::BaseName::YZ), true, None).unwrap_err().to_string());
    assert_eq!(session.info().bodies.len(), 1, "refusals must not add features");
    let other = session.import_mesh(&path, None, None, 1.0, None).unwrap().pieces[0].body;
    mesh_refusal(
        session
            .body_boolean(&qymcad_engine::BodyBooleanOptions {
                a: current,
                b: other,
                op: qymcad_engine::BooleanOp::Union,
                name: None,
                piece_names: None,
            })
            .unwrap_err()
            .to_string(),
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn binary_stl_at_decoded_cap_is_accepted() {
    let (dir, path) = fixture();
    let mut bytes = vec![0u8; 84 + 199_999 * 50];
    bytes[80..84].copy_from_slice(&(199_999_u32).to_le_bytes());
    let tri = &cube_stl()[84..134];
    for chunk in bytes[84..].as_chunks_mut::<50>().0 {
        chunk.copy_from_slice(tri);
    }
    std::fs::write(&path, bytes).unwrap();
    let result = Session::new_part().import_mesh(&path, None, None, 1.0, None).unwrap();
    assert_eq!(result.pieces[0].triangles, 199_999);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recognise_polyhedron_cube_is_a_consuming_solid() {
    let (dir, path) = fixture();
    let mut session = Session::new_part();
    let imported = session.import_mesh(&path, None, None, 1.0, None).unwrap();
    let source = imported.pieces[0].body;
    let (result, rebuild) = session
        .recognise_mesh(Some(source), qymcad_engine::MeshRecogniseMode::Polyhedron, &1.0.into(), &30.0.into(), &0.0.into(), None)
        .unwrap();
    assert!(rebuild.errors.is_empty());
    assert_eq!(result.faces, 6);
    assert_eq!(result.solids, 1);
    assert_eq!(result.shells, 1);
    let expected_volume = 10.0_f64.powi(3); // hand formula: S^3
    assert!((result.volume - expected_volume).abs() < 1e-9, "{} != {expected_volume}", result.volume);
    assert_eq!(result.bbox, [0.0, 0.0, 0.0, 10.0, 10.0, 10.0]);
    assert_eq!(session.info().bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![result.body]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recognise_exact_and_polyhedron_cylinder_have_hand_computed_metrics() {
    let dir = std::env::temp_dir().join(format!("qymcad-cylinder-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("cylinder.stl");
    std::fs::write(&path, cylinder_stl(200, 10.0, 40.0)).unwrap();
    let mut exact = Session::new_part();
    let source = exact.import_mesh(&path, None, None, 1.0, None).unwrap().pieces[0].body;
    let (result, rebuild) =
        exact.recognise_mesh(Some(source), qymcad_engine::MeshRecogniseMode::Exact, &1.0.into(), &30.0.into(), &0.0.into(), None).unwrap();
    assert!(rebuild.errors.is_empty());
    assert_eq!(result.faces, 3);
    assert_eq!(result.solids, 1);
    let expected_exact = std::f64::consts::PI * 10.0_f64.powi(2) * 40.0; // hand formula: pi*r^2*h
    assert!((result.volume - expected_exact).abs() < 0.5, "{} != {expected_exact}", result.volume);
    assert_eq!(result.bbox, [-10.0, -10.0, 0.0, 10.0, 10.0, 40.0]);

    let mut poly = Session::new_part();
    let source = poly.import_mesh(&path, None, None, 1.0, None).unwrap().pieces[0].body;
    let (result, rebuild) = poly
        .recognise_mesh(Some(source), qymcad_engine::MeshRecogniseMode::Polyhedron, &1.0.into(), &30.0.into(), &0.0.into(), None)
        .unwrap();
    assert!(rebuild.errors.is_empty());
    assert_eq!(result.faces, 202);
    let expected_poly = (200.0 / 2.0) * 10.0_f64.powi(2) * (std::f64::consts::TAU / 200.0).sin() * 40.0; // hand formula: (n/2)r^2 sin(2pi/n)h
    assert!((result.volume - expected_poly).abs() < 1e-3, "{} != {expected_poly}", result.volume); // f32 STL vertices add <1e-3 mm3 quantisation
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recognised_cube_supports_fillet_step_roundtrip_and_boolean_union() {
    let (dir, path) = fixture();
    let mut session = Session::new_part();
    let source = session.import_mesh(&path, None, None, 1.0, None).unwrap().pieces[0].body;
    let (_, _) = session
        .recognise_mesh(Some(source), qymcad_engine::MeshRecogniseMode::Polyhedron, &1.0.into(), &30.0.into(), &0.0.into(), None)
        .unwrap();
    let cube = session.info().bodies[0].id;
    let topology = session.topology(Some(cube), false).unwrap();
    let vertical = topology.edges.iter().find(|edge| (edge.b[2] - edge.a[2]).abs() > 9.9).unwrap().id;
    let (fillet, _) = session.fillet(Some(cube), &qymcad_engine::Sel::Ids(vec![vertical]), &1.0.into(), None).unwrap();
    let expected_fillet = 1000.0 - (1.0 - std::f64::consts::PI / 4.0) * 1.0_f64.powi(2) * 10.0; // hand formula: cube minus one quarter-cylinder corner
    assert!((session.info().bodies[0].volume - expected_fillet).abs() < 1e-6);
    let step = dir.join("recognised.step");
    session.export(ExportFormat::Step, &step, Quality::Standard, Some(&[fillet])).unwrap();
    let imported = Session::new_part().import(&step, 1.0, None, None).unwrap();
    assert!((imported.bodies[0].volume - expected_fillet).abs() < 1e-3);

    let mut session = Session::new_part();
    let sketch_a = session.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    session.sketch_rect(sketch_a, &5.0.into(), &5.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    let cube_a = session
        .extrude(&Extrude {
            sketch: sketch_a,
            profiles: None,
            profiles_at: None,
            height: 10.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    let sketch_b = session.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    session.sketch_rect(sketch_b, &10.0.into(), &5.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    let recognised = session
        .extrude(&Extrude {
            sketch: sketch_b,
            profiles: None,
            profiles_at: None,
            height: 10.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    let union = session
        .body_boolean(&BodyBooleanOptions { a: cube_a, b: recognised, op: BooleanOp::Union, name: None, piece_names: None })
        .unwrap();
    assert_eq!(union.pieces.len(), 1);
    let expected_union = 2.0 * 10.0_f64.powi(3) - 5.0 * 10.0 * 10.0; // hand formula: 2*S^3 - overlap_width*S^2
    assert!((union.pieces[0].volume - expected_union).abs() < 1e-6, "{} != {expected_union}", union.pieces[0].volume);
    assert_eq!(union.pieces[0].bbox, [0.0, 0.0, 0.0, 15.0, 10.0, 10.0]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recognition_persists_and_edits_existing_feature_formula() {
    let (dir, _path) = fixture();
    let archive = dir.join("recognised.qcad");
    let cylinder = dir.join("cylinder.stl");
    std::fs::write(&cylinder, cylinder_stl(200, 10.0, 40.0)).unwrap();
    let mut session = Session::new_part();
    session.param_set("tol", &1.0.into()).unwrap();
    session.param_set("sharp", &30.0.into()).unwrap();
    session.param_set("simplify", &0.0.into()).unwrap();
    let source = session.import_mesh(&cylinder, None, None, 1.0, None).unwrap().pieces[0].body;
    let (recognised, _) = session
        .recognise_mesh(Some(source), qymcad_engine::MeshRecogniseMode::Exact, &"tol".into(), &"sharp".into(), &"simplify".into(), None)
        .unwrap();
    assert_eq!(recognised.tolerance_formula, "tol");
    assert_eq!(session.project().feat_dim(recognised.feature, "simplify"), Some("simplify"));
    session.save(Some(&archive)).unwrap();
    std::fs::remove_file(cylinder).unwrap();
    let (mut reopened, report) = Session::open(&archive).unwrap();
    assert!(report.errors.is_empty());
    assert_eq!(reopened.info().timeline.iter().filter(|node| node.kind.starts_with("meshrecognised")).count(), 1);
    assert_eq!(reopened.info().bodies[0].volume, session.info().bodies[0].volume);
    let rebuild = reopened.param_set("tol", &"2".into()).unwrap();
    assert!(rebuild.errors.is_empty());
    assert_eq!(reopened.info().timeline.iter().filter(|node| node.kind.starts_with("meshrecognised")).count(), 1);
    let expected_cylinder = std::f64::consts::PI * 10.0_f64.powi(2) * 40.0; // hand formula: pi*r^2*h
    assert!((reopened.info().bodies[0].volume - expected_cylinder).abs() < 0.5);
    reopened.save(Some(&archive)).unwrap();
    let archive_before_sharp = std::fs::read(&archive).unwrap();
    let params_before_sharp = reopened.params();
    let sharp = qymcad_engine::DimensionTarget::Feature { feature: recognised.feature, key: "sharp".into() };
    assert!(reopened.param_set("sharp", &100.0.into()).is_err());
    assert_eq!(reopened.params(), params_before_sharp);
    reopened.save(Some(&archive)).unwrap();
    assert_eq!(std::fs::read(&archive).unwrap(), archive_before_sharp);
    let archive_before_dimension = std::fs::read(&archive).unwrap();
    let params_before_dimension = reopened.params();
    assert!(reopened.dimension_set(&sharp, Some(&100.0.into()), None).is_err());
    assert_eq!(reopened.params(), params_before_dimension);
    reopened.save(Some(&archive)).unwrap();
    assert_eq!(std::fs::read(&archive).unwrap(), archive_before_dimension);
    let simplify = qymcad_engine::DimensionTarget::Feature { feature: recognised.feature, key: "simplify".into() };
    let edit = reopened.dimension_set(&simplify, Some(&0.0.into()), None).unwrap();
    assert!(edit.rebuild.errors.is_empty());
    let gui = common::gui_edit_param(&archive, "tol", "1");
    reopened.param_set("tol", &"1".into()).unwrap();
    assert!((reopened.info().bodies[0].volume - gui).abs() < 1e-3);
    assert_eq!(reopened.params().iter().find(|param| param.name == "tol").unwrap().value, 1.0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recognition_refusals_are_atomic_and_undo_restores_mesh() {
    let (dir, path) = fixture();
    let archive = dir.join("before.qcad");
    let mut session = Session::new_part();
    let source = session.import_mesh(&path, None, None, 1.0, None).unwrap().pieces[0].body;
    session.save(Some(&archive)).unwrap();
    let refuse_unchanged = |session: &mut Session, message: &str, call: &mut dyn FnMut(&mut Session) -> String| {
        session.save(Some(&archive)).unwrap();
        let before = std::fs::read(&archive).unwrap();
        let error = call(session);
        assert!(error.contains(message), "{error}");
        session.save(Some(&archive)).unwrap();
        assert_eq!(std::fs::read(&archive).unwrap(), before);
    };
    let sketch = session.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    session.sketch_rect(sketch, &0.0.into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    let brep = session
        .extrude(&Extrude {
            sketch,
            profiles: None,
            profiles_at: None,
            height: 10.0.into(),
            down: None,
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap()
        .0;
    refuse_unchanged(&mut session, "already a B-rep", &mut |s| {
        s.recognise_mesh(Some(brep), qymcad_engine::MeshRecogniseMode::Exact, &1.0.into(), &30.0.into(), &0.0.into(), None)
            .unwrap_err()
            .to_string()
    });
    let (recognised, _) = session
        .recognise_mesh(Some(source), qymcad_engine::MeshRecogniseMode::Polyhedron, &1.0.into(), &30.0.into(), &0.0.into(), None)
        .unwrap();
    session.save(Some(&archive)).unwrap();
    refuse_unchanged(&mut session, "consumed", &mut |s| {
        s.recognise_mesh(Some(source), qymcad_engine::MeshRecogniseMode::Exact, &1.0.into(), &30.0.into(), &0.0.into(), None)
            .unwrap_err()
            .to_string()
    });
    let mut fresh = Session::new_part();
    let fresh_source = fresh.import_mesh(&path, None, None, 1.0, None).unwrap().pieces[0].body;
    fresh.save(Some(&archive)).unwrap();
    refuse_unchanged(&mut fresh, "polyhedron", &mut |s| {
        s.recognise_mesh(Some(fresh_source), qymcad_engine::MeshRecogniseMode::Polyhedron, &2.0.into(), &30.0.into(), &0.0.into(), None)
            .unwrap_err()
            .to_string()
    });
    let open = dir.join("open.stl");
    let mut open_bytes = cube_stl();
    open_bytes[80..84].copy_from_slice(&11u32.to_le_bytes());
    open_bytes.truncate(84 + 11 * 50);
    std::fs::write(&open, open_bytes).unwrap();
    let mut open_session = Session::new_part();
    let open_source = open_session.import_mesh(&open, None, None, 1.0, None).unwrap().pieces[0].body;
    open_session.save(Some(&archive)).unwrap();
    refuse_unchanged(&mut open_session, "shells", &mut |s| {
        s.recognise_mesh(Some(open_source), qymcad_engine::MeshRecogniseMode::Exact, &1.0.into(), &30.0.into(), &0.0.into(), None)
            .unwrap_err()
            .to_string()
    });
    assert!(session.info().timeline.iter().any(|node| node.id == recognised.feature));
    let mut undo_session = Session::new_part();
    let undo_source = undo_session.import_mesh(&path, None, None, 1.0, None).unwrap().pieces[0].body;
    let before = undo_session.info().bodies.clone();
    let before_ids = before.iter().map(|body| body.id).collect::<Vec<_>>();
    let mut undo_snapshot = undo_session.begin_tool_edit().unwrap();
    undo_snapshot.record_call("recognise", serde_json::json!({}));
    let (undo_result, _) = undo_session
        .recognise_mesh(Some(undo_source), qymcad_engine::MeshRecogniseMode::Polyhedron, &1.0.into(), &30.0.into(), &0.0.into(), None)
        .unwrap();
    undo_snapshot.record_created_node(undo_result.feature);
    undo_session.finish_tool_edit(undo_snapshot, true);
    undo_session.undo().unwrap();
    assert_eq!(undo_session.info().bodies.iter().map(|body| body.id).collect::<Vec<_>>(), before_ids);
    let topology_error = undo_session.topology(Some(undo_source), false).unwrap_err().to_string();
    assert!(topology_error.contains("is an imported mesh"), "{topology_error}");
    let (_, rebuild) = undo_session
        .recognise_mesh(Some(undo_source), qymcad_engine::MeshRecogniseMode::Polyhedron, &1.0.into(), &30.0.into(), &0.0.into(), None)
        .unwrap();
    assert!(rebuild.errors.is_empty());
    assert_eq!(undo_result.source_mesh_body, undo_source);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recognition_caps_ranges_and_open_mesh_refusals_preserve_archive() {
    let (dir, path) = fixture();
    let archive = dir.join("guard.qcad");
    let mut session = Session::new_part();
    let source = session.import_mesh(&path, None, None, 1.0, None).unwrap().pieces[0].body;
    session.save(Some(&archive)).unwrap();
    let before = std::fs::read(&archive).unwrap();
    for (tolerance, sharp, simplify) in [(0.05, 30.0, 0.0), (1.0, 4.0, 0.0), (1.0, 91.0, 0.0), (1.0, 30.0, 11.0), (f64::NAN, 30.0, 0.0)] {
        assert!(session
            .recognise_mesh(Some(source), qymcad_engine::MeshRecogniseMode::Exact, &tolerance.into(), &sharp.into(), &simplify.into(), None)
            .is_err());
        session.save(Some(&archive)).unwrap();
        assert_eq!(std::fs::read(&archive).unwrap(), before);
    }
    let many = dir.join("many.obj");
    write_many_triangle_obj(&many, 50_001);
    let mut capped = Session::new_part();
    let capped_source = capped.import_mesh(&many, None, None, 1.0, None).unwrap().pieces[0].body;
    let capped_archive = dir.join("capped.qcad");
    capped.save(Some(&capped_archive)).unwrap();
    let capped_before = std::fs::read(&capped_archive).unwrap();
    let started = std::time::Instant::now();
    let capped_error = capped
        .recognise_mesh(Some(capped_source), qymcad_engine::MeshRecogniseMode::Exact, &1.0.into(), &30.0.into(), &0.0.into(), None)
        .unwrap_err()
        .to_string();
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert!(capped_error.contains("triangle limit") && capped_error.contains("50001"));
    capped.save(Some(&capped_archive)).unwrap();
    assert_eq!(std::fs::read(&capped_archive).unwrap(), capped_before);
    let exact_path = dir.join("exact-50000.stl");
    let (segments, rows) = write_closed_cylinder_with_triangles(&exact_path, 50_000);
    assert_eq!(segments * (2 * rows + 2), 50_000); // hand formula: n*(2*rows+2) = 100*(2*249+2)
    let mut exact_cap = Session::new_part();
    let exact_source = exact_cap.import_mesh(&exact_path, None, None, 1.0, None).unwrap().pieces[0].body;
    let started = std::time::Instant::now();
    let (exact_result, exact_rebuild) = exact_cap
        .recognise_mesh(Some(exact_source), qymcad_engine::MeshRecogniseMode::Exact, &1.0.into(), &30.0.into(), &0.0.into(), None)
        .unwrap();
    assert!(exact_rebuild.errors.is_empty());
    assert_eq!(exact_result.solids, 1);
    assert!(started.elapsed() < std::time::Duration::from_secs(60));
    let poly_path = dir.join("poly-200000.stl");
    let (segments, rows) = write_closed_cylinder_with_triangles(&poly_path, 200_000);
    assert_eq!(segments * (2 * rows + 2), 200_000); // hand formula: n*(2*rows+2) = 100*(2*999+2)
    let mut poly_cap = Session::new_part();
    let poly_source = poly_cap.import_mesh(&poly_path, None, None, 1.0, None).unwrap().pieces[0].body;
    let (poly_result, poly_rebuild) = poly_cap
        .recognise_mesh(Some(poly_source), qymcad_engine::MeshRecogniseMode::Polyhedron, &1.0.into(), &30.0.into(), &0.0.into(), None)
        .unwrap();
    assert!(poly_rebuild.errors.is_empty());
    assert_eq!(poly_result.faces, (segments + 2) as u32);
    let expected_poly_cap = (segments as f64 / 2.0) * 10.0_f64.powi(2) * (std::f64::consts::TAU / segments as f64).sin() * 40.0; // hand formula: (n/2)*r^2*sin(2pi/n)*h
    assert!((poly_result.volume - expected_poly_cap).abs() < 1e-3);
    let open = dir.join("open.stl");
    let mut open_bytes = cube_stl();
    open_bytes[80..84].copy_from_slice(&11u32.to_le_bytes());
    open_bytes.truncate(84 + 11 * 50);
    std::fs::write(&open, open_bytes).unwrap();
    let mut open_session = Session::new_part();
    let open_source = open_session.import_mesh(&open, None, None, 1.0, None).unwrap().pieces[0].body;
    let error = open_session
        .recognise_mesh(Some(open_source), qymcad_engine::MeshRecogniseMode::Exact, &1.0.into(), &30.0.into(), &0.0.into(), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("shells") && error.contains("solids"));
    assert!(open_session.info().timeline.iter().all(|node| !node.kind.starts_with("meshrecognised")));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn binary_stl_units_scale_and_embedded_reopen() {
    let (dir, path) = fixture();
    let mut session = Session::new_part();
    let result = session.import_mesh(&path, None, None, 1.0, Some("cube")).unwrap();
    assert_eq!(result.pieces.len(), 1);
    assert_eq!(result.pieces[0].triangles, 12);
    assert_eq!(result.pieces[0].vertices, 8);
    assert_eq!(result.pieces[0].unit, qymcad_engine::MeshUnit::Mm);
    assert_bbox(result.pieces[0].bbox, [0.0, 0.0, 0.0, 10.0, 10.0, 10.0]);

    let cm = Session::new_part().import_mesh(&path, None, Some("cm"), 1.0, None).unwrap();
    assert_bbox(cm.pieces[0].bbox, [0.0, 0.0, 0.0, 100.0, 100.0, 100.0]);
    let scaled = Session::new_part().import_mesh(&path, None, None, 2.0, None).unwrap();
    assert_bbox(scaled.pieces[0].bbox, [0.0, 0.0, 0.0, 20.0, 20.0, 20.0]);

    let saved = dir.join("mesh.qcad");
    session.save(Some(&saved)).unwrap();
    std::fs::remove_file(path).unwrap();
    let (opened, report) = Session::open(&saved).unwrap();
    assert!(report.errors.is_empty(), "open errors: {:?}", report.errors);
    assert_eq!(opened.info().bodies.len(), 1);
    assert_bbox(opened.info().bodies[0].bbox, [0.0, 0.0, 0.0, 10.0, 10.0, 10.0]);
    assert!(opened.project().sources.iter().any(|source| source.id == result.source && source.data == cube_stl()));
    std::fs::remove_dir_all(dir).unwrap();
}
