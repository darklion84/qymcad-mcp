//! Exact fixtures from the old release's public constructors.
//! These tests need intentionally chosen native names and non-manifold fixtures,
//! which the new release no longer exposes constructors for in normal builds.
#![allow(dead_code)]
use qymcad_kernel::Shape;

/// Read an exact, checked-in test fixture.
pub fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/brep").join(format!("{name}.brep"))).unwrap()
}

/// Restore a trusted fixture through the self-gating public import API.
pub fn restore(bytes: Vec<u8>) -> Shape {
    qymcad_kernel::imports::restore(vec![qymcad_kernel::imports::SavedBody { body: 1, bytes }]).remove(0).shape
}

/// Load the named exact fixture as an independent shape.
pub fn fixture(name: &str) -> Shape {
    restore(bytes(name))
}

/// QYMB v2/v3 stores name tables ahead of the binary OCCT body (occt_io.cpp).
/// Mutating these fixture tables replaces the removed test-only native renamers.
pub fn rename_names(bytes: &mut [u8], faces: &[(u32, u32)], unnamed_edges: bool) {
    assert_eq!(&bytes[..4], b"QYMB");
    let get = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    let (version, nf, ne) = (get(4), get(8), get(12));
    assert!(version == 2 || version == 3);
    for value in bytes[16..16 + nf * 4].as_chunks_mut::<4>().0 {
        let id = u32::from_le_bytes(*value);
        if let Some((_, next)) = faces.iter().find(|(old, _)| *old == id) {
            value.copy_from_slice(&next.to_le_bytes());
        }
    }
    if unnamed_edges {
        bytes[16 + 3 * nf * 4..16 + (3 * nf + ne) * 4].fill(0);
    }
}

/// Restore a sphere with the requested persistent face names.
pub fn sphere_named(radius: f64, names: [u32; 3]) -> Shape {
    named(&format!("sphere-{radius}"), names)
}
/// Restore a cylinder with the requested persistent face names.
pub fn cylinder_named(radius: f64, height: f64, names: [u32; 3]) -> Shape {
    named(&format!("cylinder-{radius}-{height}"), names)
}
/// Restore a cone fixture with the requested persistent face names.
pub fn cone_named(r1: f64, r2: f64, height: f64, names: [u32; 3]) -> Shape {
    assert_eq!((r1, r2), (10.0, 5.0));
    named(&format!("cone-{height}"), names)
}
fn named(name: &str, names: [u32; 3]) -> Shape {
    let mut bytes = bytes(name);
    rename_names(&mut bytes, &[(1, names[0]), (2, names[1]), (3, names[2])], false);
    restore(bytes)
}
