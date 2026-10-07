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
    assert_close(r.bodies[0].mesh_volume.unwrap(), v, 0.05, "reported mesh volume");
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

#[test]
fn glb_is_in_metres_with_y_up() {
    let (s, _, _) = build(false);
    let path = scratch("export.glb");
    s.export(ExportFormat::Glb, &path, Quality::Standard, None).unwrap();
    let b = std::fs::read(&path).unwrap();
    assert_eq!(&b[0..4], b"glTF");
    let json_len = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
    assert_eq!(&b[16..20], b"JSON");
    let doc: Value = serde_json::from_slice(&b[20..20 + json_len]).unwrap();
    let pos = doc["meshes"][0]["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap() as usize;
    let acc = &doc["accessors"][pos];
    let v = |k: &str| -> Vec<f64> { acc[k].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect() };
    let (mn, mx) = (v("min"), v("max"));
    // ours (x, y, z) mm, Z up  ->  glTF (x, z, −y) m, Y up
    let want_min = [-0.030, 0.0, -0.020];
    let want_max = [0.030, 0.006, 0.020];
    for k in 0..3 {
        assert_close(mn[k], want_min[k], 1e-6, &format!("GLB min[{k}]"));
        assert_close(mx[k], want_max[k], 1e-6, &format!("GLB max[{k}]"));
    }
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
