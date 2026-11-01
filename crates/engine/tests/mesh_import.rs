mod common;

use qymcad_engine::{ExportFormat, Move, Quality, Session};
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
