//! Export and render of a known part, checked by reading the files back.
//!
//! Part: the golden plate 60 × 40 × 6 with four Ø4.5 through holes at (±22, ±12) and a 30 × 16 × 3 top pocket
//! (crates/engine/tests/golden_plate.rs), plus optionally a separate 10 × 10 × 5 block centred at (50, 0).

mod common;
use common::*;
use qymcad_engine::*;
use serde_json::Value;
use std::f64::consts::PI;

const PLATE: f64 = 60.0 * 40.0 * 6.0 - 4.0 * PI * 2.25 * 2.25 * 6.0 - 30.0 * 16.0 * 3.0;
const BLOCK: f64 = 10.0 * 10.0 * 5.0;

fn n(v: f64) -> Num {
    Num::Value(v)
}

fn extrude(sketch: Id, h: f64, op: Op, direction: Direction, name: &str) -> Extrude {
    Extrude { sketch, profiles: None, height: n(h), op, direction, through: false, target: None, name: Some(name.into()) }
}

/// Returns the session and the ids of (plate body, block body or 0).
fn build(with_block: bool) -> (Session, Id, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("plate sketch")).unwrap();
    s.sketch_rect(sk, &n(0.0), &n(0.0), &n(60.0), &n(40.0), false).unwrap();
    for (x, y) in [(22.0, 12.0), (-22.0, 12.0), (-22.0, -12.0), (22.0, -12.0)] {
        s.sketch_circle(sk, &n(x), &n(y), &n(4.5), false).unwrap();
    }
    s.extrude(&extrude(sk, 6.0, Op::Add, Direction::Normal, "plate")).unwrap();
    let (top, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &n(6.0), Some("top")).unwrap();
    let pk = s.sketch_create(&PlaneRef::Plane(top), Some("pocket sketch")).unwrap();
    s.sketch_rect(pk, &n(0.0), &n(0.0), &n(30.0), &n(16.0), false).unwrap();
    let (plate, _) = s.extrude(&extrude(pk, 3.0, Op::Cut, Direction::Reverse, "pocket")).unwrap();
    let mut block = 0;
    if with_block {
        let bk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("block sketch")).unwrap();
        s.sketch_rect(bk, &n(50.0), &n(0.0), &n(10.0), &n(10.0), false).unwrap();
        block = s.extrude(&extrude(bk, 5.0, Op::NewBody, Direction::Normal, "block")).unwrap().0;
    }
    let bodies = s.result_bodies();
    assert_eq!(bodies.len(), if with_block { 2 } else { 1 }, "{bodies:?}");
    assert_close(bodies[0].volume, PLATE, 1e-3, "plate B-rep volume");
    (s, plate, block)
}

/// Signed-tetrahedra volume of a closed triangle soup, mm³.
fn tri_volume(tris: &[[[f64; 3]; 3]]) -> f64 {
    tris.iter()
        .map(|[a, b, c]| {
            (a[0] * (b[1] * c[2] - c[1] * b[2]) - a[1] * (b[0] * c[2] - c[0] * b[2]) + a[2] * (b[0] * c[1] - c[0] * b[1])) / 6.0
        })
        .sum()
}

fn tri_bbox(tris: &[[[f64; 3]; 3]]) -> [f64; 6] {
    let mut bb = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in tris.iter().flatten() {
        for k in 0..3 {
            bb[k] = bb[k].min(p[k]);
            bb[k + 3] = bb[k + 3].max(p[k]);
        }
    }
    bb
}

/// A binary STL, parsed by hand: (triangles, count in the header).
fn read_stl(path: &std::path::Path) -> Vec<[[f64; 3]; 3]> {
    let b = std::fs::read(path).unwrap();
    let count = u32::from_le_bytes(b[80..84].try_into().unwrap()) as usize;
    assert_eq!(b.len(), 84 + 50 * count, "binary STL size for {count} triangles");
    let f = |at: usize| f32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as f64;
    (0..count)
        .map(|i| {
            let t = 84 + 50 * i + 12; // skip the normal
            [0, 1, 2].map(|v| [f(t + 12 * v), f(t + 12 * v + 4), f(t + 12 * v + 8)])
        })
        .collect()
}

fn mesh_tris(m: &qymcad_core::geom::Mesh) -> Vec<[[f64; 3]; 3]> {
    m.tris.iter().map(|t| t.map(|i| m.verts[i as usize]).map(|p| [p.x, p.y, p.z])).collect()
}

fn assert_bbox(bb: [f64; 6], want: [f64; 6], tol: f64, what: &str) {
    for k in 0..6 {
        assert_close(bb[k], want[k], tol, &format!("{what} bbox[{k}]"));
    }
}

const PLATE_BBOX: [f64; 6] = [-30.0, -20.0, 0.0, 30.0, 20.0, 6.0];

#[test]
fn step_reads_back_with_the_same_volume() {
    let (s, plate, block) = build(true);
    let path = scratch("export.step");
    let r = s.export(ExportFormat::Step, &path, Quality::Standard, None).unwrap();
    assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), [plate, block]);
    assert!(r.triangles.is_none() && r.deflection_mm.is_none());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("ISO-10303-21;"), "a STEP file: {}", &text[..40.min(text.len())]);
    assert!(text.contains("MILLI") || text.contains("milli"), "units are millimetres");
    let _gate = qymcad_kernel::kernel_gate();
    let (_, shapes) = qymcad_kernel::read_exact(qymcad_kernel::ExactFormat::Step, path.to_str().unwrap(), 0.5).unwrap();
    assert_eq!(shapes.len(), 2, "one solid per body");
    let mut vols: Vec<f64> = shapes.iter().map(|s| s.volume()).collect();
    vols.sort_by(f64::total_cmp);
    assert_close(vols[0], BLOCK, 1e-3, "block volume read back");
    assert_close(vols[1], PLATE, 1e-3, "plate volume read back");
    let bb = shapes.iter().map(|s| s.bbox().unwrap()).find(|b| b[3] - b[0] > 50.0).unwrap();
    assert_bbox(bb, PLATE_BBOX, 0.05, "plate read back");
}

#[test]
fn stl_mesh_matches_the_brep() {
    let (s, _, _) = build(false);
    let path = scratch("export.stl");
    let r = s.export(ExportFormat::Stl, &path, Quality::Standard, None).unwrap();
    let tris = read_stl(&path);
    assert!(!tris.is_empty());
    assert_eq!(Some(tris.len()), r.triangles);
    assert_eq!(r.deflection_mm, Some(0.05));
    let v = tri_volume(&tris);
    // 0.1 %: tight enough that a missing Ø4.5 hole (95 mm³ = 0.76 %) fails; standard quality is ~0.01 %.
    assert!((v - PLATE).abs() / PLATE < 0.001, "STL volume {v} vs B-rep {PLATE}");
    assert_close(r.bodies[0].mesh_volume_mm3.unwrap(), v, 0.05, "reported mesh volume");
    assert_bbox(tri_bbox(&tris), PLATE_BBOX, 1e-3, "STL");
}

/// Finer quality never means fewer triangles or a larger volume error (inscribed polygons in the holes). On these
/// Ø4.5 holes draft/standard/high give the same mesh: the kernel's fixed 0.3 rad angular deflection decides on small
/// radii (FINDINGS F-021); only max refines them.
#[test]
fn finer_quality_never_loses_accuracy() {
    let (s, _, _) = build(false);
    let mut first: Option<usize> = None;
    let mut last: Option<(usize, f64)> = None;
    for q in [Quality::Draft, Quality::Standard, Quality::High, Quality::Max] {
        let path = scratch(&format!("quality_{q:?}.stl"));
        let r = s.export(ExportFormat::Stl, &path, q, None).unwrap();
        let tris = read_stl(&path);
        let err = (tri_volume(&tris) - PLATE).abs();
        if let Some((n0, e0)) = last {
            assert!(tris.len() >= n0, "{q:?}: {} triangles, fewer than {n0}", tris.len());
            assert!(err <= e0 + 1e-6, "{q:?}: volume error {err} grew from {e0}");
        }
        assert_eq!(r.deflection_mm, Some(q.deflection()));
        first.get_or_insert(tris.len());
        last = Some((tris.len(), err));
    }
    let (max_tris, max_err) = last.unwrap();
    assert!(max_tris > first.unwrap(), "max quality refines the holes: {max_tris} vs draft {first:?}");
    // Derived bound: the only curved faces are the four Ø4.5 × 6 holes. An inscribed chord with sagitta s ≤ d cuts off
    // a circular segment of area ≤ (2/3)·(its arc length)·s (the ratio is 2/3 for small arcs and falls to 1/2 for a
    // half circle), so the polygon misses at most (2/3)·perimeter·d of each hole's section.
    let bound = 4.0 * (2.0 / 3.0) * (2.0 * PI * 2.25) * Quality::Max.deflection() * 6.0;
    assert!(max_err <= bound, "max quality volume error {max_err} mm³ exceeds the chord bound {bound}");
}

#[test]
fn threemf_is_in_millimetres() {
    let (s, _, _) = build(true);
    let path = scratch("export.3mf");
    let r = s.export(ExportFormat::ThreeMf, &path, Quality::Standard, None).unwrap();
    let meshes = qymcad_io::import_3mf(path.to_str().unwrap()).unwrap();
    assert_eq!(meshes.len(), 2, "one object per body");
    let total: usize = meshes.iter().map(|m| m.mesh.tris.len()).sum();
    assert_eq!(Some(total), r.triangles);
    let plate = meshes.iter().map(|m| mesh_tris(&m.mesh)).find(|t| tri_bbox(t)[3] - tri_bbox(t)[0] > 50.0).unwrap();
    let v = tri_volume(&plate);
    assert!((v - PLATE).abs() / PLATE < 0.001, "3MF plate volume {v}");
    assert_bbox(tri_bbox(&plate), PLATE_BBOX, 1e-3, "3MF plate");
}

/// A closed manifold: triangles exist, their corners are distinct points of the list, every edge has exactly two
/// triangles running it in opposite directions, and the triangles around every point form one fan.
fn assert_closed_manifold(what: &str, points: usize, tris: &[[usize; 3]]) {
    assert!(points > 0 && !tris.is_empty(), "{what}: empty");
    let mut directed = std::collections::HashMap::<(usize, usize), usize>::new();
    let mut around = vec![Vec::new(); points];
    for (k, t) in tris.iter().enumerate() {
        assert!(t.iter().all(|&i| i < points) && t[0] != t[1] && t[1] != t[2] && t[2] != t[0], "{what}: triangle {t:?}");
        for j in 0..3 {
            *directed.entry((t[j], t[(j + 1) % 3])).or_default() += 1;
            around[t[j]].push(k);
        }
    }
    let bad: Vec<_> = directed.iter().filter(|&(&(a, b), &k)| k != 1 || directed.get(&(b, a)) != Some(&1)).collect();
    assert!(bad.is_empty(), "{what}: {} of {} edges are open or doubled, e.g. {:?}", bad.len(), directed.len(), &bad[..bad.len().min(3)]);
    for (v, fan) in around.iter().enumerate().filter(|(_, f)| !f.is_empty()) {
        // walk the fan from triangle to triangle across the edge that leaves v
        let (start, mut k, mut seen) = (fan[0], fan[0], 1);
        loop {
            let t = tris[k];
            let out = t[(t.iter().position(|&i| i == v).unwrap() + 2) % 3];
            k = *fan.iter().find(|&&u| (0..3).any(|j| tris[u][j] == v && tris[u][(j + 1) % 3] == out)).unwrap();
            if k == start {
                break;
            }
            seen += 1;
        }
        assert_eq!(seen, fan.len(), "{what}: point {v} is shared by more than one fan");
    }
}

/// Every mesh format writes each body as a closed manifold. The kernel meshes faces separately; unwelded, a slicer
/// reported every edge between faces as non-manifold (F-093). The files are read here by hand, not through an importer
/// that might weld: indices as written for 3MF, OBJ and GLB, and for STL the points joined by their exact float32
/// coordinates, as a slicer joins them.
#[test]
fn mesh_exports_are_closed_manifolds() {
    let (mut s, _, _) = build(true);
    s.fillet(None, &Sel::Convex, &n(0.5), Some("round")).unwrap(); // rounded corners: curved faces and degenerate poles

    let path = scratch("export_closed.3mf");
    s.export(ExportFormat::ThreeMf, &path, Quality::Standard, None).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut model = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("3D/3dmodel.model").unwrap(), &mut model).unwrap();
    let objects: Vec<&str> = model.split("<object ").skip(1).collect();
    assert_eq!(objects.len(), 2, "one object per body");
    let attr = |tag: &str, name: &str| -> String {
        let at = tag.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
        tag[at..at + tag[at..].find('"').unwrap()].to_string()
    };
    for object in objects {
        let verts: Vec<[String; 3]> =
            object.split("<vertex ").skip(1).map(|v| ["x", "y", "z"].map(|k| attr(&format!(" {v}"), k))).collect();
        let tris: Vec<[usize; 3]> =
            object.split("<triangle ").skip(1).map(|t| ["v1", "v2", "v3"].map(|k| attr(&format!(" {t}"), k).parse().unwrap())).collect();
        let distinct: std::collections::HashSet<&[String; 3]> = verts.iter().collect();
        assert_eq!(distinct.len(), verts.len(), "3MF: every point is written once");
        assert_closed_manifold("3MF", verts.len(), &tris);
    }

    let path = scratch("export_closed.obj");
    s.export(ExportFormat::Obj, &path, Quality::Standard, None).unwrap();
    let (mut points, mut objects) = (0usize, Vec::<(usize, Vec<[usize; 3]>)>::new());
    for line in std::fs::read_to_string(&path).unwrap().lines() {
        if line.starts_with("o ") {
            objects.push((points, Vec::new()));
        } else if line.starts_with("v ") {
            points += 1;
        } else if let Some(f) = line.strip_prefix("f ") {
            let (base, tris) = objects.last_mut().unwrap();
            let ids: Vec<usize> = f.split_whitespace().map(|w| w.parse::<usize>().unwrap() - 1 - *base).collect();
            tris.push(ids.try_into().unwrap());
        }
    }
    assert_eq!(objects.len(), 2);
    for (k, (base, tris)) in objects.iter().enumerate() {
        let end = objects.get(k + 1).map_or(points, |o| o.0);
        assert_closed_manifold("OBJ", end - base, tris);
    }

    let path = scratch("export_closed.glb");
    s.export(ExportFormat::Glb, &path, Quality::Standard, None).unwrap();
    let b = std::fs::read(&path).unwrap();
    let json_len = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
    let doc: Value = serde_json::from_slice(&b[20..20 + json_len]).unwrap();
    let bin = &b[20 + json_len + 8..];
    let meshes = doc["meshes"].as_array().unwrap();
    assert_eq!(meshes.len(), 2);
    for mesh in meshes {
        let prims = mesh["primitives"].as_array().unwrap();
        let points = doc["accessors"][prims[0]["attributes"]["POSITION"].as_u64().unwrap() as usize]["count"].as_u64().unwrap() as usize;
        let mut tris = Vec::new();
        for prim in prims {
            assert_eq!(prim["attributes"]["POSITION"], prims[0]["attributes"]["POSITION"], "one point list per body");
            let acc = &doc["accessors"][prim["indices"].as_u64().unwrap() as usize];
            assert_eq!(acc["componentType"], 5125, "u32 indices");
            let view = &doc["bufferViews"][acc["bufferView"].as_u64().unwrap() as usize];
            let at = view["byteOffset"].as_u64().unwrap_or(0) as usize + acc["byteOffset"].as_u64().unwrap_or(0) as usize;
            let index = |i: usize| u32::from_le_bytes(bin[at + 4 * i..at + 4 * i + 4].try_into().unwrap()) as usize;
            tris.extend((0..acc["count"].as_u64().unwrap() as usize / 3).map(|t| [index(3 * t), index(3 * t + 1), index(3 * t + 2)]));
        }
        assert_closed_manifold("GLB", points, &tris);
    }

    // STL holds no shared points: join them by exact coordinates, as a slicer does, and check both bodies together;
    // they stand apart, so they are two separate fans everywhere.
    let path = scratch("export_closed.stl");
    s.export(ExportFormat::Stl, &path, Quality::Standard, None).unwrap();
    let mut at = std::collections::HashMap::new();
    let tris: Vec<[usize; 3]> = read_stl(&path)
        .iter()
        .map(|t| {
            t.map(|p| {
                let n = at.len();
                *at.entry(p.map(|c| (c as f32).to_bits())).or_insert(n)
            })
        })
        .collect();
    assert_closed_manifold("STL", at.len(), &tris);
}

#[test]
fn glb_is_in_metres_with_y_up() {
    // Deliberately offset in +X/+Y: symmetric bounds cannot distinguish +Y from -Y.
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let (cx, cy, width, length, height) = (11.0, 17.0, 6.0, 10.0, 8.0);
    s.sketch_rect(sk, &n(cx), &n(cy), &n(width), &n(length), false).unwrap();
    let (body, _) = s.extrude(&extrude(sk, height, Op::Add, Direction::Normal, "asymmetric block")).unwrap();
    assert_close(s.shape(body).unwrap().volume(), width * length * height, 1e-6, "asymmetric fixture volume");
    let path = scratch("export.glb");
    let report = s.export(ExportFormat::Glb, &path, Quality::Standard, None).unwrap();
    let b = std::fs::read(&path).unwrap();
    assert_eq!(&b[0..4], b"glTF");
    assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()), 2, "GLB version");
    assert_eq!(u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize, b.len());
    let json_len = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
    assert_eq!(&b[16..20], b"JSON");
    let doc: Value = serde_json::from_slice(&b[20..20 + json_len]).unwrap();
    let pos = doc["meshes"][0]["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap() as usize;
    let acc = &doc["accessors"][pos];
    let v = |k: &str| -> Vec<f64> { acc[k].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect() };
    let (mn, mx) = (v("min"), v("max"));
    // ours (x, y, z) mm, Z up  ->  glTF (x, z, −y) m, Y up
    let want_min = [(cx - width / 2.0) / 1000.0, 0.0, -(cy + length / 2.0) / 1000.0];
    let want_max = [(cx + width / 2.0) / 1000.0, height / 1000.0, -(cy - length / 2.0) / 1000.0];
    let bin_header = 20 + json_len;
    assert_eq!(&b[bin_header + 4..bin_header + 8], b"BIN\0");
    let bin_len = u32::from_le_bytes(b[bin_header..bin_header + 4].try_into().unwrap()) as usize;
    let bin = &b[bin_header + 8..bin_header + 8 + bin_len];
    // Decode independently of QymCAD's importer: accessor and buffer-view offsets both apply.
    let data = |accessor: &Value| {
        let view = &doc["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
        assert_eq!(view["buffer"], 0);
        let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize + accessor["byteOffset"].as_u64().unwrap_or(0) as usize;
        (&bin[offset..], view["byteStride"].as_u64().map(|v| v as usize))
    };
    assert_eq!(acc["componentType"], 5126, "float32 positions");
    assert_eq!(acc["type"], "VEC3");
    let (bytes, stride) = data(acc);
    let vertices: Vec<[f64; 3]> = (0..acc["count"].as_u64().unwrap() as usize)
        .map(|i| {
            std::array::from_fn(|k| {
                let at = i * stride.unwrap_or(12) + 4 * k;
                f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as f64
            })
        })
        .collect();
    let index = doc["meshes"][0]["primitives"][0]["indices"].as_u64().unwrap() as usize;
    let indices = &doc["accessors"][index];
    assert_eq!(indices["componentType"], 5125, "uint32 indices");
    assert_eq!(indices["type"], "SCALAR");
    let (bytes, stride) = data(indices);
    let ids: Vec<usize> = (0..indices["count"].as_u64().unwrap() as usize)
        .map(|i| {
            let at = i * stride.unwrap_or(4);
            u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
        })
        .collect();
    assert_eq!(ids.len() % 3, 0);
    assert!(ids.iter().all(|&i| i < vertices.len()), "all GLB indices reference exported vertices");
    let triangles: Vec<_> = ids.as_chunks::<3>().0.iter().map(|t| t.map(|i| vertices[i])).collect();
    assert_eq!(Some(triangles.len()), report.triangles);
    assert_bbox(tri_bbox(&triangles), [want_min[0], want_min[1], want_min[2], want_max[0], want_max[1], want_max[2]], 1e-9, "GLB binary");
    for k in 0..3 {
        assert_close(mn[k], want_min[k], 1e-6, &format!("GLB min[{k}]"));
        assert_close(mx[k], want_max[k], 1e-6, &format!("GLB max[{k}]"));
    }
    // The axis conversion has determinant +1, so winding stays outward and signed volume stays positive.
    assert_close(tri_volume(&triangles), width * length * height / 1000.0_f64.powi(3), 1e-12, "GLB signed volume in metres cubed");
    let center = [cx / 1000.0, height / 2000.0, -cy / 1000.0];
    for [a, b, c] in triangles {
        let ab = std::array::from_fn::<_, 3, _>(|k| b[k] - a[k]);
        let ac = std::array::from_fn::<_, 3, _>(|k| c[k] - a[k]);
        let normal = [ab[1] * ac[2] - ab[2] * ac[1], ab[2] * ac[0] - ab[0] * ac[2], ab[0] * ac[1] - ab[1] * ac[0]];
        let outward: f64 = (0..3).map(|k| normal[k] * ((a[k] + b[k] + c[k]) / 3.0 - center[k])).sum();
        assert!(outward > 0.0, "GLB triangle winding must face away from the box centre: {outward}");
    }
}

#[test]
fn obj_multiple_bodies_use_global_vertex_offsets_and_preserve_each_solid() {
    let (s, plate, block) = build(true);
    let path = scratch("export_multiple.obj");
    let report = s.export(ExportFormat::Obj, &path, Quality::Standard, None).unwrap();
    assert_eq!(report.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), [plate, block]);
    let mut vertices = Vec::<[f64; 3]>::new();
    let mut objects = Vec::<(String, usize, Vec<[usize; 3]>)>::new();
    for line in std::fs::read_to_string(&path).unwrap().lines() {
        if let Some(name) = line.strip_prefix("o ") {
            objects.push((name.into(), vertices.len(), Vec::new()));
        } else if let Some(coords) = line.strip_prefix("v ") {
            let values: Vec<f64> = coords.split_whitespace().map(|w| w.parse().unwrap()).collect();
            vertices.push(values.try_into().unwrap());
        } else if let Some(face) = line.strip_prefix("f ") {
            let ids: Vec<usize> = face.split_whitespace().map(|w| w.parse::<usize>().unwrap() - 1).collect();
            objects.last_mut().unwrap().2.push(ids.try_into().unwrap());
        }
    }
    assert_eq!(objects.iter().map(|o| o.0.as_str()).collect::<Vec<_>>(), ["body_1", "body_2"]);
    let expected = [(PLATE, PLATE_BBOX), (BLOCK, [45.0, -5.0, 0.0, 55.0, 5.0, 5.0])];
    for (k, (name, start, indices)) in objects.iter().enumerate() {
        let end = objects.get(k + 1).map_or(vertices.len(), |o| o.1);
        assert!(!indices.is_empty(), "{name} has triangles");
        assert!(
            indices.iter().flatten().all(|&i| i >= *start && i < end),
            "{name} indices must use its global vertex range {start}..{end}"
        );
        let triangles: Vec<_> = indices.iter().map(|t| t.map(|i| vertices[i])).collect();
        assert_bbox(tri_bbox(&triangles), expected[k].1, 1e-3, name);
        // The plate's circles are inscribed polygons; the independent block is exact.
        let tolerance = if k == 0 { expected[k].0 * 0.001 } else { 1e-6 };
        assert_close(tri_volume(&triangles), expected[k].0, tolerance, &format!("{name} signed volume"));
    }
    assert_eq!(Some(objects.iter().map(|o| o.2.len()).sum()), report.triangles);
}

#[test]
fn obj_is_the_same_solid_in_millimetres() {
    let (s, _, _) = build(false);
    let path = scratch("export.obj");
    let r = s.export(ExportFormat::Obj, &path, Quality::Draft, None).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let num = |w: &str| w.parse::<f64>().unwrap();
    let verts: Vec<[f64; 3]> = text
        .lines()
        .filter_map(|l| l.strip_prefix("v "))
        .map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            [num(w[0]), num(w[1]), num(w[2])]
        })
        .collect();
    let tris: Vec<[[f64; 3]; 3]> = text
        .lines()
        .filter_map(|l| l.strip_prefix("f "))
        .map(|l| {
            let i: Vec<usize> = l.split_whitespace().map(|w| w.parse::<usize>().unwrap() - 1).collect();
            [verts[i[0]], verts[i[1]], verts[i[2]]]
        })
        .collect();
    assert_eq!(Some(tris.len()), r.triangles);
    // millimetres, Z up, world placement: the same solid as the STL
    assert_bbox(tri_bbox(&tris), PLATE_BBOX, 1e-3, "OBJ");
    assert!((tri_volume(&tris) - PLATE).abs() / PLATE < 0.001, "OBJ volume {}", tri_volume(&tris));
}

#[test]
fn body_selection_and_refusals() {
    let (s, plate, block) = build(true);
    // one body by id
    let path = scratch("only_block.stl");
    let r = s.export(ExportFormat::Stl, &path, Quality::Standard, Some(&[block])).unwrap();
    assert_eq!(r.bodies.len(), 1);
    assert_close(tri_volume(&read_stl(&path)), BLOCK, 1e-6, "block only");
    assert_bbox(tri_bbox(&read_stl(&path)), [45.0, -5.0, 0.0, 55.0, 5.0, 5.0], 1e-4, "block only");
    // the plate's first feature was consumed by the pocket: not a result body
    let first = s.resolve("plate").unwrap();
    assert_ne!(first, plate);
    let e = s.export(ExportFormat::Stl, &path, Quality::Standard, Some(&[first])).unwrap_err();
    assert!(e.to_string().contains("not a current result body"), "{e}");
    // a sketch is not a body
    let sk = s.resolve("plate sketch").unwrap();
    assert!(s.export(ExportFormat::Stl, &path, Quality::Standard, Some(&[sk])).is_err());
    // the extension must match the format
    let e = s.export(ExportFormat::Step, &scratch("wrong.stl"), Quality::Standard, None).unwrap_err();
    assert!(e.to_string().contains(".step"), "{e}");
    // nothing to export in an empty document
    assert!(Session::new_part().export(ExportFormat::Stl, &path, Quality::Standard, None).is_err());
}

// ---------------------------------------------------------------- render

struct Img {
    w: usize,
    h: usize,
    rgb: Vec<u8>,
}

impl Img {
    fn decode(png_bytes: &[u8]) -> Img {
        let mut dec = png::Decoder::new(std::io::Cursor::new(png_bytes)).read_info().unwrap();
        let mut buf = vec![0; dec.output_buffer_size().unwrap()];
        let info = dec.next_frame(&mut buf).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgb);
        buf.truncate(info.buffer_size());
        Img { w: info.width as usize, h: info.height as usize, rgb: buf }
    }

    fn px(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.w + x) * 3;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }

    fn is_bg(&self, x: usize, y: usize) -> bool {
        self.px(x, y) == RENDER_BACKGROUND
    }

    fn coverage(&self) -> f64 {
        let n = (0..self.h).flat_map(|y| (0..self.w).map(move |x| (x, y))).filter(|&(x, y)| !self.is_bg(x, y)).count();
        n as f64 / (self.w * self.h) as f64
    }

    /// `[xmin, ymin, xmax, ymax]` of the non-background pixels (inclusive).
    fn covered(&self) -> [usize; 4] {
        let mut c = [usize::MAX, usize::MAX, 0, 0];
        for y in 0..self.h {
            for x in 0..self.w {
                if !self.is_bg(x, y) {
                    c = [c[0].min(x), c[1].min(y), c[2].max(x), c[3].max(y)];
                }
            }
        }
        c
    }
}

fn save_for_inspection(name: &str, png: &[u8]) {
    // left in the scratch dir so a person (or an agent) can look at the pictures after a test run
    std::fs::write(scratch(name), png).unwrap();
}

#[test]
fn render_top_view_shows_the_plate_and_its_holes() {
    let (s, _, _) = build(false);
    let r = s.render(View::Top, 512, 384, None).unwrap();
    save_for_inspection("render_top.png", &r.png);
    let img = Img::decode(&r.png);
    assert_eq!((img.w, img.h), (512, 384));
    let [x0, y0, x1, y1] = img.covered();
    let (cw, ch) = ((x1 - x0 + 1) as f64, (y1 - y0 + 1) as f64);
    // 60 × 40 into 512 × 384 is limited by the width: the plate spans the frame minus the 7 % margins
    assert_close(cw, 0.86 * 512.0, 2.0, "plate width in pixels");
    assert_close(cw / ch, 1.5, 0.02, "aspect of the 60 × 40 plate from the top");
    // world (x, y) -> pixel; +Y is up in the image
    let at = |x: f64, y: f64| (x0 as f64 + (x + 30.0) / 60.0 * cw, y0 as f64 + (20.0 - y) / 40.0 * ch);
    for (hx, hy) in [(22.0, 12.0), (-22.0, 12.0), (-22.0, -12.0), (22.0, -12.0)] {
        let (px, py) = at(hx, hy);
        assert!(img.is_bg(px as usize, py as usize), "hole at ({hx}, {hy}) shows the background");
    }
    let (px, py) = at(0.0, 0.0);
    assert!(!img.is_bg(px as usize, py as usize), "pocket floor is drawn");
    let (px, py) = at(-26.0, 0.0);
    assert!(!img.is_bg(px as usize, py as usize), "plate top is drawn");
    // the pocket outline is a dark edge between the top face and the pocket walls/floor
    let (ex, ey) = at(-15.0, 0.0);
    let dark = (ex as usize - 2..=ex as usize + 2).any(|x| img.px(x, ey as usize).iter().all(|c| *c < 120));
    assert!(dark, "pocket edge at x=-15 is dark");
    assert_bbox(r.bbox, PLATE_BBOX, 0.05, "render bbox");
}

#[test]
fn render_side_and_iso_views() {
    let (s, _, block) = build(true);
    let front = s.render(View::Front, 512, 384, None).unwrap();
    save_for_inspection("render_front.png", &front.png);
    let img = Img::decode(&front.png);
    let [x0, y0, x1, y1] = img.covered();
    // from the front: plate x −30..30 and block x 45..55, 85 mm wide in all, limited by the width
    let cw = (x1 - x0 + 1) as f64;
    assert_close(cw, 0.86 * 512.0, 2.0, "front width in pixels");
    // and the height: 6 mm at the same scale, 0.86·512·6/85 ≈ 31.1 px (±1.5 px of raster edges)
    assert_close((y1 - y0 + 1) as f64, 0.86 * 512.0 * 6.0 / 85.0, 1.5, "front height in pixels");
    // the empty columns between them are x 30..45: they place the block to a fraction of a millimetre
    let mm = |x: usize| -30.0 + (x - x0) as f64 / cw * 85.0;
    let empty: Vec<usize> = (x0..=x1).filter(|&x| (y0..=y1).all(|y| img.is_bg(x, y))).collect();
    assert!(!empty.is_empty(), "a gap between plate and block");
    assert_close(mm(empty[0]), 30.0, 0.5, "plate ends at x=30");
    assert_close(mm(*empty.last().unwrap() + 1), 45.0, 0.5, "block starts at x=45");

    let iso = s.render(View::Iso, 320, 240, None).unwrap();
    save_for_inspection("render_iso.png", &iso.png);
    let img = Img::decode(&iso.png);
    assert_eq!((img.w, img.h), (320, 240));
    // smoke only (something shaded with edges); the geometry is checked by the top and front views
    assert!(img.coverage() > 0.15, "iso coverage {}", img.coverage());
    let dark = img.rgb.chunks(3).filter(|c| c.iter().all(|v| *v < 90)).count();
    assert!(dark > 200, "edges are drawn: {dark} dark pixels");
    let shades: std::collections::HashSet<&[u8]> = img.rgb.chunks(3).collect();
    assert!(shades.len() > 5, "faces are shaded differently");

    // one body only
    let only = s.render(View::Top, 200, 200, Some(&[block])).unwrap();
    let img = Img::decode(&only.png);
    let [x0, y0, x1, y1] = img.covered();
    assert_close((x1 - x0 + 1) as f64 / (y1 - y0 + 1) as f64, 1.0, 0.02, "square block from the top");
    assert_eq!(only.bodies, [block]);

    assert!(s.render(View::Iso, 10, 384, None).is_err(), "too small");
    assert!(s.render(View::Iso, 512, 5000, None).is_err(), "too large");
    assert!(Session::new_part().render(View::Iso, 512, 384, None).is_err(), "nothing to draw");
}

/// Review finding (Codex, phase 3C): writing the target in place would truncate a hard-linked file elsewhere.
/// The export goes to a fresh temporary file and is renamed over the target, so the other link keeps its data.
#[test]
fn export_does_not_write_through_a_hard_link() {
    let (s, _, _) = build(false);
    let dir = scratch("hardlink");
    std::fs::create_dir_all(&dir).unwrap();
    let victim = dir.join("victim.txt");
    std::fs::write(&victim, "keep me").unwrap();
    let target = dir.join("target.stl");
    let _ = std::fs::remove_file(&target);
    std::fs::hard_link(&victim, &target).unwrap();
    s.export(ExportFormat::Stl, &target, Quality::Standard, None).unwrap();
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me", "the hard-linked file must be untouched");
    assert!(!read_stl(&target).is_empty(), "the target now holds the export");
    let leftovers: Vec<_> = std::fs::read_dir(target.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".qymcad-mcp-"))
        .collect();
    assert!(leftovers.is_empty(), "no staging directory left behind");
}

/// Review round 2 (Codex): a part placed in its parent (turned 90° about Z, moved +100 in x) renders and exports in
/// world space. Plate [−30, −20, 0, 30, 20, 6] → x' = 100 − y, y' = x → [80, −30, 0, 120, 30, 6].
#[test]
fn a_placed_part_renders_and_exports_in_world_space() {
    let (mut s, _, _) = build(false);
    let path = scratch("placed.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let parts: Vec<usize> = project.components.iter().enumerate().filter(|(_, c)| c.parent.is_some()).map(|(i, _)| i).collect();
    assert_eq!(parts.len(), 1, "one part under the root");
    project.components[parts[0]].transform = [0.0, -1.0, 0.0, 100.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (o, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let want = [80.0, -30.0, 0.0, 120.0, 30.0, 6.0];
    assert_bbox(o.render(View::Top, 256, 256, None).unwrap().bbox, want, 0.05, "render bbox of the placed part");
    let stl = scratch("placed.stl");
    o.export(ExportFormat::Stl, &stl, Quality::Standard, None).unwrap();
    assert_bbox(tri_bbox(&read_stl(&stl)), want, 1e-3, "STL of the placed part");
}

/// Review finding (Codex, phase 3C): a document whose feature failed (e.g. opened from a file) shows the failed
/// feature's source body unchanged (F-008); export and render must refuse instead of silently dropping it.
#[test]
fn export_and_render_refuse_a_document_with_failed_features() {
    // Build such a document with QymCAD directly: the engine itself never keeps a failed feature.
    let mut p = qymcad_core::model::Project::default();
    p.new_document();
    let b = p.add_box(10.0, 10.0, 10.0);
    let _ = qymcad_testkit::regenerate(&mut p);
    let edge = p.regen_edges[&b][0].id;
    let f = p.add_fillet(b, 50.0, vec![edge]); // far too big for a 10 mm box
    let (report, shapes) = qymcad_testkit::regenerate(&mut p);
    assert!(report.errors.iter().any(|(id, _)| *id == f), "the fillet must fail: {:?}", report.errors);
    let breps: Vec<_> = shapes.iter().filter_map(|(id, s)| s.to_brep_bytes().map(|b| (*id, b))).collect();
    let path = scratch("failed_feature.qcad");
    qymcad_io::save_project_with_brep(&p, path.to_str().unwrap(), &breps).unwrap();

    let (s, r) = Session::open(&path).unwrap();
    assert!(!r.errors.is_empty(), "the reopened document still has the failed fillet");
    let e = s.export(ExportFormat::Stl, &scratch("failed_feature.stl"), Quality::Standard, None).unwrap_err();
    assert!(e.to_string().contains("did not build"), "{e}");
    let e = s.render(View::Iso, 128, 96, None).unwrap_err();
    assert!(e.to_string().contains("did not build"), "{e}");
}

/// Item 10 of ADR 0022: every result body is written, not only the active one. Two separate prisms, the second H
/// high; after the app's own edit of H, saved and reopened, STEP, 3MF and GLB each read back two 600 mm³ bodies.
#[test]
fn every_body_survives_glb_and_saved_app_edit() {
    let mut s = Session::new_part();
    s.param_set("H", &n(5.)).unwrap();
    let a = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("a")).unwrap();
    s.sketch_rect(a, &n(0.), &n(0.), &n(10.), &n(20.), false).unwrap();
    s.extrude(&extrude(a, 3., Op::NewBody, Direction::Normal, "first")).unwrap();
    let b = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("b")).unwrap();
    s.sketch_rect(b, &n(50.), &n(0.), &n(10.), &n(10.), false).unwrap();
    let mut ex = extrude(b, 5., Op::NewBody, Direction::Normal, "second");
    ex.height = Num::Expr("H".into());
    s.extrude(&ex).unwrap();
    let saved = scratch("all-bodies.qcad");
    s.save(Some(&saved)).unwrap();
    assert_close(gui_edit_param(&saved, "H", "6"), 600., 1e-6, "saved app path second body");
    let edited = scratch("app-edited-all-bodies.qcad");
    app_edit_save(&saved, &edited, "H", "6");
    let (s, _) = Session::open(&edited).unwrap();
    for (format, ext) in [(ExportFormat::Step, "step"), (ExportFormat::ThreeMf, "3mf"), (ExportFormat::Glb, "glb")] {
        let path = scratch(&format!("all-bodies.{ext}"));
        let report = s.export(format, &path, Quality::Standard, None).unwrap();
        assert_eq!(report.bodies.len(), 2);
        let mut vols = match format {
            ExportFormat::Step => {
                let _g = qymcad_kernel::kernel_gate();
                qymcad_kernel::read_exact(qymcad_kernel::ExactFormat::Step, path.to_str().unwrap(), 0.5)
                    .unwrap()
                    .1
                    .iter()
                    .map(|s| s.volume())
                    .collect::<Vec<_>>()
            }
            ExportFormat::ThreeMf => qymcad_io::import_3mf(path.to_str().unwrap()).unwrap().iter().map(|m| m.mesh.volume()).collect(),
            ExportFormat::Glb => {
                let b = std::fs::read(&path).unwrap();
                let jl = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
                let doc: Value = serde_json::from_slice(&b[20..20 + jl]).unwrap();
                let bin = &b[28 + jl..];
                let read = |i: usize| {
                    let acc = &doc["accessors"][i];
                    let view = &doc["bufferViews"][acc["bufferView"].as_u64().unwrap() as usize];
                    let at = view["byteOffset"].as_u64().unwrap_or(0) + acc["byteOffset"].as_u64().unwrap_or(0);
                    (&bin[at as usize..], acc["count"].as_u64().unwrap() as usize)
                };
                doc["meshes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|mesh| {
                        let p = &mesh["primitives"][0];
                        let (positions, n) = read(p["attributes"]["POSITION"].as_u64().unwrap() as usize);
                        let vertices: Vec<[f64; 3]> = (0..n)
                            .map(|i| {
                                std::array::from_fn(|k| {
                                    let off = i * 12 + k * 4;
                                    f32::from_le_bytes(positions[off..off + 4].try_into().unwrap()) as f64 * 1000.
                                })
                            })
                            .collect();
                        let (indices, n) = read(p["indices"].as_u64().unwrap() as usize);
                        let tris = (0..n / 3)
                            .map(|i| {
                                std::array::from_fn(|k| {
                                    let off = (i * 3 + k) * 4;
                                    vertices[u32::from_le_bytes(indices[off..off + 4].try_into().unwrap()) as usize]
                                })
                            })
                            .collect::<Vec<_>>();
                        tri_volume(&tris)
                    })
                    .collect()
            }
            _ => unreachable!(),
        };
        vols.sort_by(f64::total_cmp);
        assert_eq!(vols.len(), 2);
        for v in &vols {
            assert_close(*v, 600., 0.001, "every read back body");
        }
    }
}

/// Apply the app's parameter edit (see `common::gui_edit_param`) to the saved file `source` and save the result,
/// with its faces and B-reps, to `destination`.
fn app_edit_save(source: &std::path::Path, destination: &std::path::Path, name: &str, expr: &str) {
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(source.to_str().unwrap()).unwrap();
    project.ensure_document();
    for b in &project.bodies {
        if !b.faces.is_empty() {
            project.regen_faces.insert(b.id, b.faces.clone());
        }
    }
    let shapes = restore_shapes(breps);
    let seen = project.param_map();
    project.parameters.iter_mut().find(|p| p.name == name).unwrap().expr = expr.into();
    project.eval_parameters();
    for si in 0..project.sketches.len() {
        if project.sketch_reads_names(si) {
            project.solve_sketch(si);
        }
    }
    let vars = project.param_map();
    let mut changed: Vec<String> =
        vars.iter().filter(|(k, v)| seen.get(*k).is_none_or(|old| (old - **v).abs() > 1e-12)).map(|(k, _)| k.clone()).collect();
    changed.extend(seen.keys().filter(|k| !vars.contains_key(*k)).cloned());
    for n in changed {
        project.mark_param_dependents_dirty_for(&n);
    }
    let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut project, shapes);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    for b in &mut project.bodies {
        if let Some(f) = project.regen_faces.get(&b.id) {
            b.faces = f.clone();
        }
    }
    let breps = {
        let _g = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(id, s)| (*id, s.to_brep_bytes().unwrap())).collect::<Vec<_>>()
    };
    qymcad_io::save_project_with_brep(&project, destination.to_str().unwrap(), &breps).unwrap();
}
