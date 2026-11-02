//! E4 exchange colour, independently parsed, with flat-writer geometry controls.
mod common;
use common::{assert_close, scratch};
use qymcad_engine::*;
use serde_json::{json, Value};
use std::{io::Read, path::Path};

const RGB: [u8; 3] = [17, 34, 51];
// A centred 10 × 20 rectangle extruded +4: V=800 and these six extrema.
const VOLUME: f64 = 10.0 * 20.0 * 4.0;
const BBOX: [f64; 6] = [-5.0, -10.0, 0.0, 5.0, 10.0, 4.0];

fn block(s: &mut Session, x: f64, name: &str) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some(&format!("{name} sketch"))).unwrap();
    s.sketch_rect(sk, &x.into(), &0.0.into(), &10.0.into(), &20.0.into(), false).unwrap();
    let (id, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":4,"op":"new_body","name":name})).unwrap()).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    assert_close(r.bodies.iter().find(|b| b.id == id).unwrap().volume, VOLUME, 1e-6, "10*20*4 prism");
    id
}
fn stock() -> (Session, Id) {
    let mut s = Session::new_part();
    let b = block(&mut s, 0.0, "manual");
    let a = s.appearance(b, None, Some(Some(RGB))).unwrap();
    assert_eq!((a.color_rgb, a.color_source), (RGB, "manual"));
    (s, b)
}
fn report(r: &ExportReport, ids: &[Id], colors: bool) {
    let v = serde_json::to_value(r).unwrap();
    assert_eq!(v["colors"], colors);
    assert_eq!(v["skipped_hidden"], json!([]));
    assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), ids);
}
fn body_colour(r: &ExportReport, id: Id, rgb: [u8; 3], source: &str) {
    let v = serde_json::to_value(r.bodies.iter().find(|b| b.id == id).unwrap()).unwrap();
    assert_eq!(v["color_rgb"], json!(rgb));
    assert_eq!(v["color_source"], source);
}
fn attr<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    let at = tag.find(&format!(" {key}=\""))? + key.len() + 3;
    Some(&tag[at..at + tag[at..].find('"')?])
}
fn tags<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    text.split(&format!("<{key} ")).skip(1).map(|s| &s[..s.find('>').unwrap()]).collect()
}
fn xml(path: &Path) -> String {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name("3D/3dmodel.model").unwrap().read_to_string(&mut text).unwrap();
    text
}
#[derive(Debug)]
struct Object {
    name: String,
    colour: Option<String>,
    triangles: Vec<[[f64; 3]; 3]>,
    triangle_colours: Vec<Option<String>>,
}
fn objects(path: &Path) -> Vec<Object> {
    let text = xml(path);
    let palette = tags(&text, "base").iter().map(|t| attr(t, "displaycolor").unwrap().to_owned()).collect::<Vec<_>>();
    text.split("<object ")
        .skip(1)
        .filter(|s| s[..s.find("</object>").unwrap()].contains("<mesh>"))
        .map(|s| {
            let s = &s[..s.find("</object>").unwrap()];
            let head = &s[..s.find('>').unwrap()];
            let colour = attr(head, "pindex").map(|i| palette[i.parse::<usize>().unwrap()].clone());
            let vertices = tags(s, "vertex")
                .iter()
                .map(|t| ["x", "y", "z"].map(|k| attr(&format!(" {t}"), k).unwrap().parse::<f64>().unwrap()))
                .collect::<Vec<_>>();
            let ts = tags(s, "triangle");
            Object {
                name: attr(head, "name").unwrap().into(),
                colour: colour.clone(),
                triangles: ts
                    .iter()
                    .map(|t| ["v1", "v2", "v3"].map(|k| vertices[attr(&format!(" {t}"), k).unwrap().parse::<usize>().unwrap()]))
                    .collect(),
                triangle_colours: ts
                    .iter()
                    .map(|t| attr(t, "p1").map(|i| palette[i.parse::<usize>().unwrap()].clone()).or_else(|| colour.clone()))
                    .collect(),
            }
        })
        .collect()
}
fn bbox(ts: &[[[f64; 3]; 3]]) -> [f64; 6] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in ts.iter().flatten() {
        for k in 0..3 {
            b[k] = b[k].min(p[k]);
            b[k + 3] = b[k + 3].max(p[k]);
        }
    }
    b
}
fn volume(ts: &[[[f64; 3]; 3]]) -> f64 {
    ts.iter()
        .map(|[a, b, c]| {
            (a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
        })
        .sum::<f64>()
        .abs()
}
fn same_bbox(got: [f64; 6], expected: [f64; 6], tolerance: f64) {
    for k in 0..6 {
        assert_close(got[k], expected[k], tolerance, &format!("bbox[{k}]"));
    }
}
fn flat_mesh(s: &Session, b: Id) -> qymcad_core::geom::Mesh {
    let mut m = qymcad_kernel::ask::export_mesh(s.shape(b).unwrap(), Quality::Standard.deflection()).unwrap().mesh;
    m.transform(&s.project().body_world_transform(b));
    m
}
fn glb(path: &Path) -> Value {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"glTF");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2);
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize, bytes.len());
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    assert_eq!(&bytes[16..20], b"JSON");
    serde_json::from_slice(&bytes[20..20 + n]).unwrap()
}
fn glb_triangles(path: &Path) -> Vec<[[f64; 3]; 3]> {
    let doc = glb(path);
    let bytes = std::fs::read(path).unwrap();
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let chunk = 20 + json_len;
    assert_eq!(&bytes[chunk + 4..chunk + 8], b"BIN\0");
    let bin = &bytes[chunk + 8..];
    let offset = |a: &Value| {
        let view = &doc["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
        (view["byteOffset"].as_u64().unwrap_or(0) + a["byteOffset"].as_u64().unwrap_or(0)) as usize
    };
    let mut triangles = Vec::new();
    for mesh in doc["meshes"].as_array().unwrap() {
        for p in mesh["primitives"].as_array().unwrap() {
            let positions = &doc["accessors"][p["attributes"]["POSITION"].as_u64().unwrap() as usize];
            let indices = &doc["accessors"][p["indices"].as_u64().unwrap() as usize];
            assert_eq!(positions["componentType"], 5126); // glTF float32 positions.
            assert_eq!(indices["componentType"], 5125); // glTF uint32 indices.
            assert_eq!(p["mode"], 4); // TRIANGLES.
            let (v, at) = (offset(positions), offset(indices));
            let count = indices["count"].as_u64().unwrap() as usize;
            assert_eq!(count % 3, 0);
            for t in 0..count / 3 {
                triangles.push(std::array::from_fn(|k| {
                    let at = at + (t * 3 + k) * 4;
                    let index = u32::from_le_bytes(bin[at..at + 4].try_into().unwrap()) as usize;
                    std::array::from_fn(|c| {
                        let at = v + index * 12 + c * 4;
                        f64::from(f32::from_le_bytes(bin[at..at + 4].try_into().unwrap())) * 1000.0
                    })
                }));
            }
        }
    }
    triangles
}
fn linear(rgb: [u8; 3]) -> [f64; 3] {
    // IEC sRGB transfer, byte/255 then its piecewise inverse transfer function.
    rgb.map(|b| {
        let c = f64::from(b) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    })
}
fn assert_material(doc: &Value, index: usize, rgb: [u8; 3]) {
    let p = &doc["materials"][index]["pbrMetallicRoughness"];
    for (k, c) in linear(rgb).iter().enumerate() {
        assert_close(p["baseColorFactor"][k].as_f64().unwrap(), *c, 1e-6, "linear sRGB");
    }
    assert_eq!(p["baseColorFactor"][3], 1.0);
    assert_eq!(p["metallicFactor"], 0.0);
}
fn primitive_colour(doc: &Value, p: &Value, rgb: [u8; 3]) {
    assert_material(doc, p["material"].as_u64().unwrap() as usize, rgb);
}
fn position_bounds(doc: &Value) -> Vec<(Value, Value)> {
    doc["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            let a = &doc["accessors"][m["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap() as usize];
            (a["min"].clone(), a["max"].clone())
        })
        .collect()
}
fn step_colours(path: &Path) -> Vec<[f64; 3]> {
    std::fs::read_to_string(path)
        .unwrap()
        .split("COLOUR_RGB(")
        .skip(1)
        .map(|s| {
            let fields = s[..s.find(')').unwrap()].split(',').skip(1).map(|s| s.trim().parse::<f64>().unwrap()).collect::<Vec<_>>();
            [fields[0], fields[1], fields[2]]
        })
        .collect()
}
fn has_step_colour(path: &Path, rgb: [u8; 3]) {
    let expected = rgb.map(|c| f64::from(c) / 255.0); // STEP normalises each exact byte by 255.
    assert!(step_colours(path).iter().any(|c| (0..3).all(|k| (c[k] - expected[k]).abs() <= 1e-6)), "missing {expected:?}");
}
fn imported_step(path: &Path) -> Session {
    let mut s = Session::new_part();
    let r = s.import(path, 1.0, None, None).unwrap();
    assert!(r.rebuild.errors.is_empty(), "{:?}", r.rebuild.errors);
    s
}

#[test]
fn manual_colour_3mf_geometry_matches_flat() {
    let (s, b) = stock();
    let path = scratch("e4-manual.3mf");
    let r = s.export(ExportFormat::ThreeMf, &path, Quality::Standard, None).unwrap();
    let o = objects(&path);
    assert_eq!(o.len(), 1);
    assert!(!o[0].name.is_empty());
    assert_eq!(o[0].colour.as_deref(), Some("#112233"));
    let flat = scratch("e4-flat.3mf");
    qymcad_io::export_3mf(&[flat_mesh(&s, b)], flat.to_str().unwrap()).unwrap();
    assert_eq!(o[0].triangles.len(), objects(&flat)[0].triangles.len());
    assert_eq!(o[0].triangles, objects(&flat)[0].triangles);
    assert_eq!(o[0].triangles.len(), 6 * 2); // Six planar quads, two triangles per quad.
    assert_close(volume(&o[0].triangles), VOLUME, VOLUME * 0.001, "3MF prism volume");
    same_bbox(bbox(&o[0].triangles), BBOX, 1e-6);
    report(&r, &[b], true);
    body_colour(&r, b, RGB, "manual");
    assert_eq!(r.triangles, Some(6 * 2));
    assert_close(r.bodies[0].mesh_volume_mm3.unwrap(), VOLUME, 1e-6, "reported volume");
}
#[test]
fn manual_colour_glb_geometry_matches_flat() {
    let (s, b) = stock();
    let path = scratch("e4-manual.glb");
    let r = s.export(ExportFormat::Glb, &path, Quality::Standard, None).unwrap();
    let d = glb(&path);
    primitive_colour(&d, &d["meshes"][0]["primitives"][0], RGB);
    let flat = scratch("e4-flat.glb");
    qymcad_io::export_glb(&[flat_mesh(&s, b)], flat.to_str().unwrap()).unwrap();
    assert_eq!(position_bounds(&d), position_bounds(&glb(&flat)));
    let triangles = glb_triangles(&path);
    assert_eq!(triangles, glb_triangles(&flat));
    assert_eq!(triangles.len(), 6 * 2); // Six planar quads, two triangles per quad.
    assert_close(volume(&triangles), VOLUME, VOLUME * 0.001, "independent GLB volume in mm3");
    let (lo, hi) = &position_bounds(&d)[0];
    // (x,y,z) mm -> (x,z,-y)/1000 metres, +Y up.
    for (k, v) in [-5.0 / 1000.0, 0.0, -10.0 / 1000.0].iter().enumerate() {
        assert_close(lo[k].as_f64().unwrap(), *v, 1e-9, "GLB min metres");
    }
    for (k, v) in [5.0 / 1000.0, 4.0 / 1000.0, 10.0 / 1000.0].iter().enumerate() {
        assert_close(hi[k].as_f64().unwrap(), *v, 1e-9, "GLB max metres");
    }
    report(&r, &[b], true);
    body_colour(&r, b, RGB, "manual");
    assert_eq!(r.triangles, Some(6 * 2));
    assert_close(r.bodies[0].mesh_volume_mm3.unwrap(), VOLUME, 1e-6, "reported mm3 in GLB");
}
#[test]
fn manual_colour_step_geometry_matches_flat() {
    let (s, b) = stock();
    let path = scratch("e4-manual.step");
    let r = s.export(ExportFormat::Step, &path, Quality::Standard, None).unwrap();
    has_step_colour(&path, RGB);
    let flat = scratch("e4-flat.step");
    {
        let _g = qymcad_kernel::kernel_gate();
        qymcad_kernel::write_step(&[(s.shape(b).unwrap(), s.project().body_world_transform(b))], flat.to_str().unwrap()).unwrap();
    }
    let a = imported_step(&path);
    let f = imported_step(&flat);
    assert_eq!(a.info().bodies.len(), 1);
    assert_close(a.info().bodies[0].volume, VOLUME, 1e-6, "STEP prism volume");
    same_bbox(a.info().bodies[0].bbox, BBOX, 1e-6);
    assert_eq!(a.info().bodies[0].bbox, f.info().bodies[0].bbox);
    assert_close(a.info().bodies[0].volume, f.info().bodies[0].volume, 1e-9, "tree/flat exact geometry");
    report(&r, &[b], true);
    body_colour(&r, b, RGB, "manual");
    assert_eq!(r.triangles, None);
}

#[test]
fn two_body_colours_and_world_placement_once() {
    let (mut s, a) = stock();
    let b = block(&mut s, 60.0, "palette");
    let (m, r) = s
        .move_body(&Move {
            body: Some(a),
            translation: Some([30.0.into(), 0.0.into(), 0.0.into()]),
            rotation: None,
            name: Some("moved".into()),
        })
        .unwrap();
    assert!(r.errors.is_empty());
    let (m, r) = s
        .move_body(&Move {
            body: Some(m.body),
            translation: None,
            rotation: Some(Rotation { angle: 90.0, axis: Axis::Z, pivot: Some([0.0; 3]) }),
            name: Some("moved".into()),
        })
        .unwrap();
    assert!(r.errors.is_empty());
    let a = m.body;
    // Translate +30X then rotate about world origin: (x,y,z)->(-y,x+30,z).
    let moved = [-10.0, 25.0, 0.0, 10.0, 35.0, 4.0];
    // Native palette index 1 (second independent root) is brass, per model/curves.rs's literal palette.
    let palette = [200, 160, 110];
    assert_eq!(s.project().mesh_color(s.project().mesh_index(b).unwrap()), palette);
    for (format, ext) in [(ExportFormat::ThreeMf, "3mf"), (ExportFormat::Glb, "glb"), (ExportFormat::Step, "step")] {
        let path = scratch(&format!("e4-two.{ext}"));
        let r = s.export(format, &path, Quality::Standard, Some(&[a, b])).unwrap();
        report(&r, &[a, b], true);
        body_colour(&r, a, RGB, "manual");
        body_colour(&r, b, palette, "palette");
        match format {
            ExportFormat::ThreeMf => {
                let o = objects(&path);
                assert_eq!(o.len(), 2);
                assert_eq!(o[0].colour.as_deref(), Some("#112233"));
                assert_eq!(o[1].colour, Some(format!("#{:02X}{:02X}{:02X}", palette[0], palette[1], palette[2])));
                same_bbox(bbox(&o[0].triangles), moved, 1e-6);
                assert_close(volume(&o[0].triangles), VOLUME, 1e-6, "moved prism");
            }
            ExportFormat::Glb => {
                let d = glb(&path);
                assert_eq!(d["meshes"].as_array().unwrap().len(), 2);
                primitive_colour(&d, &d["meshes"][0]["primitives"][0], RGB);
                primitive_colour(&d, &d["meshes"][1]["primitives"][0], palette);
            }
            ExportFormat::Step => {
                has_step_colour(&path, RGB);
                has_step_colour(&path, palette);
                let imported = imported_step(&path);
                let bodies = imported.info().bodies;
                assert_eq!(bodies.len(), 2);
                let own = bodies.iter().find(|b| b.color_rgb == RGB).unwrap();
                same_bbox(own.bbox, moved, 1e-6);
                assert_close(own.volume, VOLUME, 1e-6, "moved STEP prism");
                assert_eq!(bodies.iter().filter(|b| b.color_rgb == palette).count(), 1);
            }
            _ => unreachable!(),
        }
    }
    // A parent placement is separate from Move's baked local geometry: catches applying world placement twice.
    let archive = scratch("e4-world.qcad");
    s.save(Some(&archive)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(archive.to_str().unwrap()).unwrap();
    let part = project.components.iter_mut().find(|c| c.parent.is_some()).unwrap();
    part.transform = [1.0, 0.0, 0.0, 100.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    qymcad_io::save_project_guarded_with_brep(&project, archive.to_str().unwrap(), &breps).unwrap();
    let (placed, r) = Session::open(&archive).unwrap();
    assert!(r.errors.is_empty());
    let expected = [moved[0] + 100.0, moved[1], moved[2], moved[3] + 100.0, moved[4], moved[5]];
    let p = scratch("e4-world.3mf");
    placed.export(ExportFormat::ThreeMf, &p, Quality::Standard, Some(&[a])).unwrap();
    same_bbox(bbox(&objects(&p)[0].triangles), expected, 1e-6);
    let p = scratch("e4-world.step");
    placed.export(ExportFormat::Step, &p, Quality::Standard, Some(&[a])).unwrap();
    let imported = imported_step(&p);
    // Body info bounds are local; this checks imported STEP placement in document world coordinates.
    same_bbox(imported.render(View::Top, 64, 64, None).unwrap().bbox, expected, 1e-6);
    let flat = scratch("e4-world-flat.step");
    {
        let _g = qymcad_kernel::kernel_gate();
        qymcad_kernel::write_step(&[(placed.shape(a).unwrap(), placed.project().body_world_transform(a))], flat.to_str().unwrap()).unwrap();
    }
    same_bbox(imported_step(&flat).render(View::Top, 64, 64, None).unwrap().bbox, expected, 1e-6);
    {
        let _g = qymcad_kernel::kernel_gate();
        let (_, shapes) = qymcad_kernel::read_exact(qymcad_kernel::ExactFormat::Step, p.to_str().unwrap(), 0.05).unwrap();
        assert_eq!(shapes.len(), 1);
        assert_close(shapes[0].volume(), VOLUME, 1e-6, "world STEP exact volume");
        same_bbox(shapes[0].bbox().unwrap(), expected, 1e-6);
    }
}

#[test]
fn imported_triangle_and_face_colours_remain_aligned() {
    // Same coloured PLY fixture as E3: outward tetrahedron V=2*3*4/6=4 mm3, two triangles per colour.
    let p = scratch("e4-coloured.ply");
    std::fs::write(&p,"ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 4\nproperty list uchar int vertex_indices\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nend_header\n0 0 0\n2 0 0\n0 3 0\n0 0 4\n3 0 2 1 10 20 30\n3 0 1 3 10 20 30\n3 0 3 2 40 50 60\n3 1 2 3 40 50 60\n").unwrap();
    let mut s = Session::new_part();
    let r = s.import_mesh(&p, None, None, 1.0, Some("coloured")).unwrap();
    assert!(r.rebuild.errors.is_empty());
    assert_eq!(r.pieces.len(), 1);
    let b = r.pieces[0].body;
    s.appearance(b, None, Some(Some(RGB))).unwrap();
    let p = scratch("e4-imported.3mf");
    let r = s.export(ExportFormat::ThreeMf, &p, Quality::Standard, None).unwrap();
    let o = objects(&p);
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].triangle_colours, vec![Some("#0A141E".into()), Some("#0A141E".into()), Some("#28323C".into()), Some("#28323C".into())]);
    assert_eq!(r.triangles, Some(4));
    assert_close(volume(&o[0].triangles), 2.0 * 3.0 * 4.0 / 6.0, 1e-6, "tetrahedron");
    let p = scratch("e4-imported.glb");
    s.export(ExportFormat::Glb, &p, Quality::Standard, None).unwrap();
    assert_glb_triangle_counts(&glb(&p), &[([10, 20, 30], 2), ([40, 50, 60], 2)]);
    // E3's native tree STEP fixture: one of the six quad faces has RGB 71,82,93 (two triangles).
    let (source, body) = stock();
    let mut tree = source.project().export_tree(source.project().root, |id| id == body);
    tree.iter_mut().find(|n| n.body == Some(body)).unwrap().face_colors =
        vec![(source.project().bodies[source.project().mesh_index(body).unwrap()].faces[0].id, [71, 82, 93])];
    let p = scratch("e4-face-source.step");
    {
        let _g = qymcad_kernel::kernel_gate();
        qymcad_kernel::write_step_tree(&tree, &[(body, source.shape(body).unwrap())], p.to_str().unwrap()).unwrap();
    }
    let mut imported = imported_step(&p);
    let b = imported.info().bodies[0].id;
    imported.appearance(b, None, Some(Some(RGB))).unwrap();
    let p = scratch("e4-face.3mf");
    let r = imported.export(ExportFormat::ThreeMf, &p, Quality::Standard, None).unwrap();
    let o = objects(&p);
    assert_eq!(o[0].triangle_colours.iter().filter(|c| c.as_deref() == Some("#47525D")).count(), 2);
    assert_eq!(o[0].triangle_colours.iter().filter(|c| c.as_deref() == Some("#112233")).count(), 5 * 2);
    assert_eq!(r.triangles, Some(6 * 2));
    assert_close(volume(&o[0].triangles), VOLUME, 1e-6, "face-coloured prism");
    let p = scratch("e4-face.glb");
    imported.export(ExportFormat::Glb, &p, Quality::Standard, None).unwrap();
    assert_glb_triangle_counts(&glb(&p), &[(RGB, 5 * 2), ([71, 82, 93], 2)]);
    // Re-export the face override under a real parent placement; copying a STEP shape must keep its face names.
    let archive = scratch("e4-face-world.qcad");
    imported.save(Some(&archive)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(archive.to_str().unwrap()).unwrap();
    let owner = project.body_owner(b).unwrap();
    project.components.iter_mut().find(|c| c.id == owner).unwrap().transform =
        [0.0, -1.0, 0.0, 30.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    qymcad_io::save_project_guarded_with_brep(&project, archive.to_str().unwrap(), &breps).unwrap();
    let (placed, r) = Session::open(&archive).unwrap();
    assert!(r.errors.is_empty());
    let p = scratch("e4-face-world.step");
    placed.export(ExportFormat::Step, &p, Quality::Standard, None).unwrap();
    has_step_colour(&p, RGB);
    has_step_colour(&p, [71, 82, 93]);
    let mut back = imported_step(&p);
    // (x,y,z)->(30-y,x,z) maps [-5,-10,0;5,10,4] to [20,-5,0;40,5,4].
    same_bbox(back.render(View::Top, 64, 64, None).unwrap().bbox, [20.0, -5.0, 0.0, 40.0, 5.0, 4.0], 1e-6);
    assert_close(back.info().bodies[0].volume, VOLUME, 1e-6, "placed face-coloured STEP volume");
    let p = scratch("e4-face-world-back.3mf");
    back.export(ExportFormat::ThreeMf, &p, Quality::Standard, None).unwrap();
    let o = objects(&p);
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].triangle_colours.iter().filter(|c| c.as_deref() == Some("#47525D")).count(), 2);
    assert_eq!(o[0].triangle_colours.iter().filter(|c| c.as_deref() == Some("#112233")).count(), 5 * 2);
    // A colour-free reset is unnecessary: round-tripped face colour is verified independently of the body map.
    assert_eq!(back.topology(None, true).unwrap().faces.len(), 6); // Six planar faces of the rectangular prism.
}
fn assert_glb_triangle_counts(d: &Value, expected: &[([u8; 3], usize)]) {
    let ps = d["meshes"][0]["primitives"].as_array().unwrap();
    assert_eq!(ps.len(), expected.len());
    for (rgb, n) in expected {
        let p = ps
            .iter()
            .find(|p| {
                let c = &d["materials"][p["material"].as_u64().unwrap() as usize]["pbrMetallicRoughness"]["baseColorFactor"];
                (0..3).all(|k| (c[k].as_f64().unwrap() - linear(*rgb)[k]).abs() < 1e-6)
            })
            .unwrap();
        primitive_colour(d, p, *rgb);
        assert_eq!(d["accessors"][p["indices"].as_u64().unwrap() as usize]["count"].as_u64().unwrap() as usize, n * 3);
    }
}

#[test]
fn stl_obj_bytes_ignore_body_colour() {
    let (mut s, b) = stock();
    let archive = scratch("e4-colour-copy.qcad");
    s.save(Some(&archive)).unwrap();
    let (mut copy, r) = Session::open(&archive).unwrap();
    assert!(r.errors.is_empty());
    let a = copy.appearance(b, None, Some(None)).unwrap();
    assert_eq!(a.color_source, "palette");
    for (format, ext) in [(ExportFormat::Stl, "stl"), (ExportFormat::Obj, "obj")] {
        let coloured = scratch(&format!("e4-coloured.{ext}"));
        let plain = scratch(&format!("e4-plain.{ext}"));
        let r = s.export(format, &coloured, Quality::Standard, None).unwrap();
        let p = copy.export(format, &plain, Quality::Standard, None).unwrap();
        report(&r, &[b], false);
        report(&p, &[b], false);
        assert_eq!(std::fs::read(coloured).unwrap(), std::fs::read(plain).unwrap());
        assert_eq!(r.triangles, Some(6 * 2));
        assert_close(r.bodies[0].mesh_volume_mm3.unwrap(), VOLUME, 1e-6, "colourless prism");
    }
}

fn state(s: &mut Session, ids: &[Id]) -> (DocInfo, Vec<Topology>) {
    let topology = ids.iter().map(|&b| s.topology(Some(b), true).unwrap()).collect();
    (s.info(), topology)
}
fn directory_entries(path: &Path) -> Vec<std::ffi::OsString> {
    let mut names = std::fs::read_dir(path).unwrap().map(|e| e.unwrap().file_name()).collect::<Vec<_>>();
    names.sort();
    names
}
#[test]
fn visible_only_export_and_render_refusals_are_atomic() {
    let (mut s, a) = stock();
    let b = block(&mut s, 60.0, "hidden block");
    let hidden = s.appearance(b, Some(false), None).unwrap();
    assert_eq!((hidden.body.id, hidden.visible), (b, false));
    let dir = scratch("e4-visibility");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("default.3mf");
    let r = s.export(ExportFormat::ThreeMf, &path, Quality::Standard, None).unwrap();
    report(&r, &[a, b], true);
    assert_eq!(objects(&path).len(), 2);
    let path = dir.join("visible.3mf");
    let r = s.export_visible(ExportFormat::ThreeMf, &path, Quality::Standard, None, true).unwrap();
    assert_eq!(r.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), [a]);
    assert_eq!(serde_json::to_value(&r).unwrap()["skipped_hidden"], json!([b]));
    assert_eq!(objects(&path).len(), 1);
    same_bbox(bbox(&objects(&path)[0].triangles), BBOX, 1e-6);
    assert_eq!(r.triangles, Some(6 * 2));
    let default = s.render(View::Top, 64, 64, None).unwrap();
    assert_eq!(default.bodies, [a, b]);
    assert_eq!(default.skipped_hidden, Vec::<Id>::new());
    let visible = s.render_visible(View::Top, 64, 64, None, true).unwrap();
    assert_eq!(visible.bodies, [a]);
    assert_eq!(visible.skipped_hidden, [b]);
    assert_eq!((visible.width, visible.height), (64, 64));
    same_bbox(visible.bbox, BBOX, 1e-6);
    let mut decoder = png::Decoder::new(std::io::Cursor::new(&visible.png)).read_info().unwrap();
    let mut buffer = vec![0; decoder.output_buffer_size().unwrap()];
    let image = decoder.next_frame(&mut buffer).unwrap();
    assert_eq!((image.width, image.height), (64, 64));
    // Mixed explicit selection must refuse the whole call too, including when a visible body comes first.
    for (i, ids) in [vec![b], vec![a, b]].iter().enumerate() {
        let path = dir.join(format!("refused-{i}.3mf"));
        let before = state(&mut s, &[a, b]);
        let entries = directory_entries(&dir);
        let e = s.export_visible(ExportFormat::ThreeMf, &path, Quality::Standard, Some(ids), true).unwrap_err();
        assert!(e.to_string().contains(&b.to_string()), "{e}");
        assert!(e.to_string().contains("hidden block"), "{e}");
        assert!(!path.exists());
        assert_eq!(directory_entries(&dir), entries);
        assert_eq!(state(&mut s, &[a, b]), before);
        let before = state(&mut s, &[a, b]);
        let entries = directory_entries(&dir);
        let e = s.render_visible(View::Top, 64, 64, Some(ids), true).unwrap_err();
        assert!(e.to_string().contains(&b.to_string()), "{e}");
        assert!(e.to_string().contains("hidden block"), "{e}");
        assert_eq!(directory_entries(&dir), entries);
        assert_eq!(state(&mut s, &[a, b]), before);
    }
    assert!(!s.appearance(a, Some(false), None).unwrap().visible);
    let path = dir.join("all-hidden.3mf");
    let before = state(&mut s, &[a, b]);
    let entries = directory_entries(&dir);
    assert!(s.export_visible(ExportFormat::ThreeMf, &path, Quality::Standard, None, true).is_err());
    assert!(!path.exists());
    assert_eq!(directory_entries(&dir), entries);
    assert_eq!(state(&mut s, &[a, b]), before);
    let before = state(&mut s, &[a, b]);
    let entries = directory_entries(&dir);
    assert!(s.render_visible(View::Top, 64, 64, None, true).is_err());
    assert_eq!(directory_entries(&dir), entries);
    assert_eq!(state(&mut s, &[a, b]), before);
    // Explicit opt-out also retains both hidden bodies and no skipped IDs.
    let path = dir.join("optout.3mf");
    let r = s.export_visible(ExportFormat::ThreeMf, &path, Quality::Standard, None, false).unwrap();
    report(&r, &[a, b], true);
    assert_eq!(objects(&path).len(), 2);
    let r = s.render_visible(View::Top, 64, 64, None, false).unwrap();
    assert_eq!(r.bodies, [a, b]);
    assert_eq!(r.skipped_hidden, Vec::<Id>::new());
}
