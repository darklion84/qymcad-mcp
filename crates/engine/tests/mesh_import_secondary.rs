//! Files a mesh names inside itself (glTF buffer `uri`, OBJ `mtllib`) and ZIP inflation are confined and capped
//! before a native reader runs (ADR 0039, security addendum).

use qymcad_engine::Session;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn dir() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!("qymcad-mesh-secondary-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(d.join("mesh")).unwrap();
    d
}

/// Three vertices (0,0,0), (1,2,3), (0,2,0) in metres, Y-up: one triangle.
fn triangle_bin() -> Vec<u8> {
    [[0.0_f32, 0.0, 0.0], [1.0, 2.0, 3.0], [0.0, 2.0, 0.0]].iter().flatten().flat_map(|x| x.to_le_bytes()).collect()
}

fn write_gltf(path: &Path, uri: &str) {
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"buffers":[{{"uri":"{uri}","byteLength":36}}],"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],"nodes":[{{"mesh":0}}],"scenes":[{{"nodes":[0]}}],"scene":0}}"#
    );
    std::fs::write(path, json).unwrap();
}

/// A `.glb` whose JSON chunks name these buffer URIs in order (no BIN chunk).
fn glb_with_json_chunks(uris: &[&str]) -> Vec<u8> {
    let mut chunks = Vec::new();
    for uri in uris {
        let mut json = format!(r#"{{"asset":{{"version":"2.0"}},"buffers":[{{"uri":"{uri}","byteLength":36}}]}}"#).into_bytes();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        chunks.extend_from_slice(&(json.len() as u32).to_le_bytes());
        chunks.extend_from_slice(&0x4E4F_534A_u32.to_le_bytes());
        chunks.extend_from_slice(&json);
    }
    let mut out = b"glTF".to_vec();
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((12 + chunks.len()) as u32).to_le_bytes());
    out.extend_from_slice(&chunks);
    out
}

fn import(path: &Path) -> Result<qymcad_engine::MeshImportResult, String> {
    Session::new_part().import_mesh(path, None, None, 1.0, None).map_err(|e| e.to_string())
}

fn refused(path: &Path, needle: &str) {
    let error = import(path).expect_err("must be refused");
    assert!(error.contains(needle), "{error}");
}

#[test]
fn gltf_buffer_beside_the_file_is_read_and_one_outside_is_refused() {
    let d = dir();
    std::fs::write(d.join("mesh/tri.bin"), triangle_bin()).unwrap();
    std::fs::write(d.join("outside.bin"), triangle_bin()).unwrap();
    let ok = d.join("mesh/ok.gltf");
    write_gltf(&ok, "tri.bin");
    // metres, Y-up -> mm, Z-up: [x,y,z] -> [1000x, -1000z, 1000y]; bbox from the three vertices.
    let result = import(&ok).unwrap();
    assert_eq!(result.pieces[0].triangles, 1);
    let expected = [0.0, -3000.0, 0.0, 1000.0, 0.0, 2000.0];
    for (a, e) in result.pieces[0].bbox.into_iter().zip(expected) {
        assert!((a - e).abs() < 1e-9, "{a} != {e}");
    }
    let parent = d.join("mesh/parent.gltf");
    write_gltf(&parent, "../outside.bin");
    refused(&parent, "not a plain relative path");
    let absolute = d.join("mesh/absolute.gltf");
    write_gltf(&absolute, d.join("outside.bin").to_str().unwrap());
    refused(&absolute, "not a plain relative path");
    std::os::unix::fs::symlink(d.join("outside.bin"), d.join("mesh/link.bin")).unwrap();
    let link = d.join("mesh/link.gltf");
    write_gltf(&link, "link.bin");
    refused(&link, "a symbolic link");
    std::fs::create_dir(d.join("mesh/sub")).unwrap();
    std::os::unix::fs::symlink(d.as_path(), d.join("mesh/sub/up")).unwrap();
    let through = d.join("mesh/through.gltf");
    write_gltf(&through, "sub/up/outside.bin");
    refused(&through, "in a subfolder");
    // A FIFO must neither block the import nor be read.
    assert!(std::process::Command::new("mkfifo").arg(d.join("mesh/fifo.bin")).status().unwrap().success());
    let fifo = d.join("mesh/fifo.gltf");
    write_gltf(&fifo, "fifo.bin");
    refused(&fifo, "not a regular file");
    // The native GLB reader keeps the LAST JSON chunk: a harmless first one must not hide a second that escapes.
    let two = d.join("mesh/two.glb");
    std::fs::write(&two, glb_with_json_chunks(&["tri.bin", "../outside.bin"])).unwrap();
    refused(&two, "`../outside.bin`: not a plain relative path");
    // 16 MiB buffer + the .gltf itself > 16 MiB.
    std::fs::write(d.join("mesh/big.bin"), vec![0u8; 16 * 1024 * 1024]).unwrap();
    let big = d.join("mesh/big.gltf");
    write_gltf(&big, "big.bin");
    refused(&big, "with its secondary files");
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn obj_material_library_must_lie_beside_the_file() {
    let d = dir();
    let body = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    std::fs::write(d.join("outside.mtl"), "newmtl red\nKd 1 0 0\n").unwrap();
    std::fs::write(d.join("mesh/red.mtl"), "newmtl red\nKd 1 0 0\n").unwrap();
    let ok = d.join("mesh/ok.obj");
    std::fs::write(&ok, format!("mtllib red.mtl\n{body}")).unwrap();
    assert_eq!(import(&ok).unwrap().pieces[0].triangles, 1);
    // A missing library is skipped by the native reader; so it is here.
    let missing = d.join("mesh/missing.obj");
    std::fs::write(&missing, format!("mtllib nothing.mtl\n{body}")).unwrap();
    assert_eq!(import(&missing).unwrap().pieces[0].triangles, 1);
    let parent = d.join("mesh/parent.obj");
    std::fs::write(&parent, format!("mtllib ../outside.mtl\n{body}")).unwrap();
    refused(&parent, "not a plain relative path");
    let device = d.join("mesh/device.obj");
    std::fs::write(&device, format!("mtllib /dev/zero\n{body}")).unwrap();
    refused(&device, "not a plain relative path");
    // The reader joins a backslash-continued line and cuts `#` comments: "mtllib \\\n/dev/zero" opens /dev/zero.
    let continued = d.join("mesh/continued.obj");
    std::fs::write(&continued, format!("mtllib \\\n/dev/zero\n{body}")).unwrap();
    refused(&continued, "`/dev/zero`: not a plain relative path");
    let commented = d.join("mesh/commented.obj");
    std::fs::write(&commented, format!("mtllib ../outside.mtl # colours\n{body}")).unwrap();
    refused(&commented, "`../outside.mtl`: not a plain relative path");
    std::os::unix::fs::symlink("/dev/zero", d.join("mesh/zero.mtl")).unwrap();
    let zero = d.join("mesh/zero.obj");
    std::fs::write(&zero, format!("mtllib zero.mtl\n{body}")).unwrap();
    refused(&zero, "a symbolic link");
    std::fs::remove_dir_all(d).unwrap();
}

/// One stored-name entry of `size` spaces, deflated (a few KiB on disk).
fn write_zip(path: &Path, entry: &str, prefix: &str, size: usize) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    zip.start_file(entry, zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(prefix.as_bytes()).unwrap();
    let chunk = vec![b' '; 1 << 20];
    let mut left = size - prefix.len();
    while left > 0 {
        let n = left.min(chunk.len());
        zip.write_all(&chunk[..n]).unwrap();
        left -= n;
    }
    zip.finish().unwrap();
}

/// Rewrite the declared uncompressed size in both the local header and the central directory.
fn lie_about_size(path: &Path, size: u32) {
    let mut bytes = std::fs::read(path).unwrap();
    let local = bytes.windows(4).position(|w| w == [0x50, 0x4b, 0x03, 0x04]).unwrap();
    bytes[local + 22..local + 26].copy_from_slice(&size.to_le_bytes());
    let central = bytes.windows(4).position(|w| w == [0x50, 0x4b, 0x01, 0x02]).unwrap();
    bytes[central + 24..central + 28].copy_from_slice(&size.to_le_bytes());
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn zip_containers_are_capped_by_inflated_bytes_not_by_their_word() {
    let d = dir();
    let over = 64 * 1024 * 1024 + 1;
    let honest_amf = d.join("mesh/honest.amf");
    write_zip(&honest_amf, "honest.amf", "<?xml version=\"1.0\"?>", over);
    refused(&honest_amf, "declared uncompressed limit");
    let lying_amf = d.join("mesh/lying.amf");
    write_zip(&lying_amf, "lying.amf", "<?xml version=\"1.0\"?>", over);
    lie_about_size(&lying_amf, 100);
    let started = std::time::Instant::now();
    // The zip reader does not stop at the declared size: only the counted inflation refuses it.
    refused(&lying_amf, "uncompressed limit is 67108864, inflated at least 67108865");
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    let lying_3mf = d.join("mesh/lying.3mf");
    write_zip(&lying_3mf, "3D/3dmodel.model", "<?xml version=\"1.0\"?>", over);
    lie_about_size(&lying_3mf, 100);
    refused(&lying_3mf, "uncompressed limit is 67108864, inflated at least 67108865");
    std::fs::remove_dir_all(d).unwrap();
}
