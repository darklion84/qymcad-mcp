//! Export of result bodies to exchange files: STEP (exact B-rep) and meshes (STL, 3MF, GLB, OBJ).
//!
//! Mirrors QymCAD.app's File > Export (`crates/qymcad/src/gui/io_jobs.rs` `write_exact_to` / `write_mesh_to`):
//! every body goes out with its world transform (`body_world_transform`); meshes are re-tessellated from the live
//! B-rep at the chosen deflection. Unlike the app we always write flat (one object per body, no component tree,
//! no colours): see FINDINGS F-020.

use crate::error::{Error, Result};
use crate::session::Session;
use qymcad_core::geom::{Mesh, Point3};
use qymcad_core::model::Id;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// An export file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// STEP AP214, exact B-rep, millimetres (`.step`, `.stp`).
    Step,
    /// Binary STL, millimetres (`.stl`).
    Stl,
    /// 3MF package, millimetres, one object per body (`.3mf`).
    #[serde(rename = "3mf")]
    ThreeMf,
    /// Binary glTF, metres, +Y up (`.glb`).
    Glb,
    /// Wavefront OBJ, millimetres (`.obj`).
    Obj,
}

impl ExportFormat {
    /// File extensions a path for this format may have (lowercase, without the dot).
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            ExportFormat::Step => &["step", "stp"],
            ExportFormat::Stl => &["stl"],
            ExportFormat::ThreeMf => &["3mf"],
            ExportFormat::Glb => &["glb"],
            ExportFormat::Obj => &["obj"],
        }
    }

    pub fn is_mesh(self) -> bool {
        self != ExportFormat::Step
    }
}

/// Mesh detail: the chordal deflection used to tessellate curved faces. The presets of QymCAD.app's mesh export
/// dialog (`crates/qymcad/src/gui/panels_windows.rs` `mesh_quality_dialog`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    /// 0.2 mm
    Draft,
    /// 0.05 mm (default)
    #[default]
    Standard,
    /// 0.02 mm
    High,
    /// 0.005 mm
    Max,
}

impl Quality {
    /// The deflection in mm.
    pub fn deflection(self) -> f64 {
        match self {
            Quality::Draft => 0.2,
            Quality::Standard => 0.05,
            Quality::High => 0.02,
            Quality::Max => 0.005,
        }
    }
}

/// What an export wrote.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ExportReport {
    pub format: ExportFormat,
    pub path: String,
    /// The bodies written, in file order.
    pub bodies: Vec<ExportedBody>,
    /// Mesh formats: the deflection used, mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deflection_mm: Option<f64>,
    /// Mesh formats: triangles written in total.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triangles: Option<usize>,
    /// Size of the written file.
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ExportedBody {
    pub id: Id,
    pub name: String,
    /// Mesh formats: volume of the written mesh (sum of tetrahedra, absolute), mm³. Compare with the B-rep volume to see
    /// the tessellation error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_volume_mm3: Option<f64>,
}

impl Session {
    /// The bodies an export or render works on: `bodies` (each must be a current result body), or all result
    /// bodies. Errors if there is nothing to write.
    pub(crate) fn output_bodies(&self, bodies: Option<&[Id]>) -> Result<Vec<Id>> {
        // A failed feature passes its source body through unchanged (FINDINGS F-008): writing or showing the
        // result would silently drop that feature, so refuse until the document rebuilds cleanly.
        if !self.p.regen_errors.is_empty() {
            let mut lines: Vec<String> = self
                .p
                .regen_errors
                .iter()
                .map(|(id, e)| format!("{} ({id}): {}", self.node_name(*id), crate::localization::error(e)))
                .collect();
            lines.sort();
            return Err(Error::Invalid(format!(
                "the document has features that did not build, so the bodies do not show the full recipe: {}. Fix or \
                 remove them first (doc_info lists them)",
                lines.join("; ")
            )));
        }
        let results: Vec<Id> = self.result_bodies().iter().map(|b| b.id).collect();
        let Some(asked) = bodies else {
            if results.is_empty() {
                return Err(Error::Invalid("the document has no bodies yet; build something first".into()));
            }
            return Ok(results);
        };
        if asked.is_empty() {
            return Err(Error::Invalid("`bodies` is empty; omit it to use every result body".into()));
        }
        let mut out = Vec::new();
        for &id in asked {
            if !results.contains(&id) {
                return Err(Error::Invalid(format!(
                    "{id} (`{}`) is not a current result body; result bodies are {results:?} (a feature's id is its body's id, \
                     and a body consumed by a later feature is not a result)",
                    self.node_name(id)
                )));
            }
            if !out.contains(&id) {
                out.push(id);
            }
        }
        Ok(out)
    }

    /// Write `bodies` (default: every result body) to `path` in `format`. The caller has validated the path; the
    /// extension is checked again here. `quality` applies to mesh formats only.
    /// Precondition: the current thread must not hold `qymcad_kernel::kernel_gate()` (F-069).
    pub fn export(&self, format: ExportFormat, path: &Path, quality: Quality, bodies: Option<&[Id]>) -> Result<ExportReport> {
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
        if !format.extensions().contains(&ext.as_str()) {
            return Err(Error::Invalid(format!("a {format:?} file must end in .{}", format.extensions().join(" or ."))));
        }
        let final_path = path.to_str().ok_or_else(|| Error::Invalid(format!("path is not UTF-8: {}", path.display())))?;
        let ids = self.output_bodies(bodies)?;
        // Write inside a private directory created fresh next to the target (mode 0700; `create_dir` never follows
        // or reuses an existing entry, a planted symlink included: that name is skipped), then rename the file over the target. QymCAD's writers open
        // their path by name, so the file must live where no other user can swap it; writing the target directly
        // would truncate whatever inode it is (a hard link, a late symlink). A rename only replaces the directory
        // entry (docs/SECURITY.md).
        let stage = private_dir_next_to(path)
            .map_err(|e| Error::Io(format!("cannot create a private staging directory next to {final_path}: {e}")))?;
        let tmp = stage.join(format!("export.{ext}"));
        let result = self.export_to(format, &tmp, quality, &ids, final_path).and_then(|mut report| {
            std::fs::rename(&tmp, path).map_err(|e| Error::Io(format!("cannot move the export into place at {final_path}: {e}")))?;
            report.bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            Ok(report)
        });
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_dir(&stage);
        result
    }

    /// The actual write, into `tmp` (a fresh file); the report names `final_path`.
    fn export_to(&self, format: ExportFormat, tmp: &Path, quality: Quality, ids: &[Id], final_path: &str) -> Result<ExportReport> {
        let s = tmp.to_str().ok_or_else(|| Error::Invalid(format!("path is not UTF-8: {}", tmp.display())))?;
        let mut report =
            ExportReport { format, path: final_path.to_string(), bodies: Vec::new(), deflection_mm: None, triangles: None, bytes: 0 };
        if format.is_mesh() {
            let deflection = quality.deflection();
            let mut meshes: Vec<Mesh> = Vec::with_capacity(ids.len());
            for &id in ids {
                let shape = self.shapes.get(&id).ok_or_else(|| Error::NotFound(format!("body {id} has no geometry")))?;
                let qymcad_core::geom::Built { mut mesh, .. } = qymcad_kernel::ask::export_mesh(shape, deflection)
                    .ok_or_else(|| Error::Invalid(format!("body {id} (`{}`) did not tessellate", self.node_name(id))))?;
                weld(&mut mesh);
                mesh.transform(&self.p.body_world_transform(id));
                report.bodies.push(ExportedBody { id, name: self.node_name(id), mesh_volume_mm3: Some(mesh.volume()) });
                meshes.push(mesh);
            }
            let written = match format {
                ExportFormat::Stl => qymcad_io::export_stl(&meshes, s),
                ExportFormat::ThreeMf => qymcad_io::export_3mf(&meshes, s),
                ExportFormat::Glb => qymcad_io::export_glb(&meshes, s),
                ExportFormat::Obj => qymcad_io::export_obj(&meshes, s),
                ExportFormat::Step => unreachable!("STEP is not a mesh format"),
            };
            written.map_err(|e| Error::Io(format!("cannot write {final_path}: {e}")))?;
            report.deflection_mm = Some(deflection);
            report.triangles = Some(meshes.iter().map(|m| m.tris.len()).sum());
        } else {
            let mut pairs = Vec::with_capacity(ids.len());
            for &id in ids {
                let shape = self.shapes.get(&id).ok_or_else(|| Error::NotFound(format!("body {id} has no geometry")))?;
                pairs.push((shape, self.p.body_world_transform(id)));
                report.bodies.push(ExportedBody { id, name: self.node_name(id), mesh_volume_mm3: None });
            }
            let _gate = qymcad_kernel::kernel_gate();
            qymcad_kernel::write_step(&pairs, s).map_err(|e| Error::Io(format!("cannot write {final_path}: {e}")))?;
        }
        Ok(report)
    }
}

/// Join the copies of one point into one vertex. The kernel meshes every face on its own and `tessellate_merged` only
/// concatenates the faces, so two faces meet at copies of their common edge's nodes and a slicer reads every edge
/// between faces as open (FINDINGS F-093). The copies have been bit-identical on every part checked, so they are
/// matched exactly, never within a tolerance that could join two distinct points; a copy off in the last bit would
/// stay an open edge. A triangle left with two corners at one point is dropped: the kernel lays such zero-area
/// triangles along a degenerate edge, the pole of a rounded corner, whose nodes all stand at one point.
///
/// Solids of one body that only touch (pattern copies meeting at an edge or a corner, a ring closing on itself along
/// an edge) must not share the point there: an edge of four triangles or a corner of two fans is non-manifold. So each
/// point is given once per fan of triangles around it, where a fan hangs together across the edges two triangles are
/// paired on: the only two of an edge, or around an edge of more, the two holding a wedge of solid (`wedges`).
/// The per-vertex normals are dropped, as no mesh writer reads them.
fn weld(mesh: &mut Mesh) {
    use std::collections::HashMap;
    let mut at = HashMap::with_capacity(mesh.verts.len());
    let mut points = Vec::with_capacity(mesh.verts.len());
    let index: Vec<u32> = mesh
        .verts
        .iter()
        .map(|p| {
            // `+ 0.0` makes -0.0 the same key as 0.0
            let key = [p.x, p.y, p.z].map(|c| (c + 0.0).to_bits());
            *at.entry(key).or_insert_with(|| {
                points.push(*p);
                (points.len() - 1) as u32
            })
        })
        .collect();
    let tris: Vec<[u32; 3]> =
        mesh.tris.iter().map(|t| t.map(|i| index[i as usize])).filter(|&[a, b, c]| a != b && b != c && c != a).collect();

    // every edge of every triangle, sorted so that the triangles of one edge stand together
    let mut edges: Vec<(u32, u32, u32)> = Vec::with_capacity(tris.len() * 3);
    for (k, &[a, b, c]) in tris.iter().enumerate() {
        for (u, v) in [(a, b), (b, c), (c, a)] {
            edges.push((u.min(v), u.max(v), k as u32));
        }
    }
    edges.sort_unstable();
    // a corner of a triangle is `3 * triangle + k`; the corners at one point joined across paired edges are one fan
    let mut fan: Vec<u32> = (0..tris.len() as u32 * 3).collect();
    fn root(fan: &mut [u32], mut k: u32) -> u32 {
        while fan[k as usize] != k {
            fan[k as usize] = fan[fan[k as usize] as usize];
            k = fan[k as usize];
        }
        k
    }
    let corner = |t: u32, point: u32| 3 * t + tris[t as usize].iter().position(|&i| i == point).unwrap() as u32;
    for run in edges.chunk_by(|x, y| (x.0, x.1) == (y.0, y.1)) {
        let pairs = match run.len() {
            2 => vec![(run[0].2, run[1].2)],
            n if n > 2 => wedges(&points, &tris, run),
            _ => Vec::new(),
        };
        for (a, b) in pairs {
            for end in [run[0].0, run[0].1] {
                let (a, b) = (root(&mut fan, corner(a, end)), root(&mut fan, corner(b, end)));
                fan[a as usize] = b;
            }
        }
    }
    drop(edges);

    let mut own = vec![u32::MAX; fan.len()];
    let mut verts = Vec::with_capacity(points.len());
    mesh.tris = (0..tris.len())
        .map(|t| {
            std::array::from_fn(|k| {
                let r = root(&mut fan, 3 * t as u32 + k as u32) as usize;
                if own[r] == u32::MAX {
                    own[r] = verts.len() as u32;
                    verts.push(points[tris[t][k] as usize]);
                }
                own[r]
            })
        })
        .collect();
    mesh.verts = verts;
    mesh.norms.clear();
}

/// The pairs of triangles around an edge of more than two (`run`: the edge's ends and one triangle each) that hold a
/// wedge of solid between them. Each triangle has the solid against its outward normal, so turning about the edge it
/// faces the solid either ahead or behind; a triangle facing ahead and the next one round facing back enclose solid.
/// Closed solids touching along the edge alternate so and every triangle is paired; any other triangle is left
/// unpaired, as open as it was.
fn wedges(points: &[Point3], tris: &[[u32; 3]], run: &[(u32, u32, u32)]) -> Vec<(u32, u32)> {
    let p = |i: u32| {
        let q = points[i as usize];
        [q.x, q.y, q.z]
    };
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let (u, v) = (run[0].0, run[0].1);
    let (origin, d) = (p(u), sub(p(v), p(u)));
    // two directions square to the edge, `e2 = d × e1`, measure the turn about it
    let k = (0..3).min_by(|&i, &j| d[i].abs().total_cmp(&d[j].abs())).unwrap();
    let unit = |a: [f64; 3]| a.map(|c| c / dot(a, a).sqrt());
    let d = unit(d);
    let e1 = unit(cross(d, std::array::from_fn(|i| if i == k { 1.0 } else { 0.0 })));
    let e2 = cross(d, e1);
    let mut round: Vec<(f64, bool, u32)> = run
        .iter()
        .map(|&(_, _, t)| {
            let tri = tris[t as usize];
            let w = *tri.iter().find(|&&i| i != u && i != v).unwrap();
            let r = sub(p(w), origin);
            let normal = cross(sub(p(tri[1]), p(tri[0])), sub(p(tri[2]), p(tri[0])));
            // `d × r` points to a larger turn; the solid is against the normal
            (dot(r, e2).atan2(dot(r, e1)), dot(normal, cross(d, r)) < 0.0, t)
        })
        .collect();
    round.sort_by(|a, b| a.0.total_cmp(&b.0));
    let n = round.len();
    // Triangles at one turn (faces lying on each other, within rounding) hold no solid between them: the one with
    // its solid behind goes first, so no pair spans them. The list starts after its widest gap, so such a group never
    // straddles the ±π seam.
    const SAME: f64 = 1e-9;
    let gap = |i: usize| if i + 1 < n { round[i + 1].0 - round[i].0 } else { round[0].0 + std::f64::consts::TAU - round[i].0 };
    let widest = (0..n).max_by(|&i, &j| gap(i).total_cmp(&gap(j))).unwrap();
    round.rotate_left((widest + 1) % n);
    round.chunk_by_mut(|a, b| (b.0 - a.0).rem_euclid(std::f64::consts::TAU) <= SAME).for_each(|g| g.sort_by_key(|t| t.1));
    // a pair needs a turn between them: when every triangle lies at one turn the wrap pair would span the tie
    let apart = |i: usize, j: usize| (round[j].0 - round[i].0).rem_euclid(std::f64::consts::TAU) > SAME;
    (0..n)
        .map(|i| (i, (i + 1) % n))
        .filter(|&(i, j)| round[i].1 && !round[j].1 && apart(i, j))
        .map(|(i, j)| (round[i].2, round[j].2))
        .collect()
}

static STAGE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Create a fresh directory `.{stem}.qymcad-mcp-{pid}-{n}.d` (mode 0700) next to `path`, like `mkdtemp`: a name that
/// already exists, whatever it is, is skipped rather than used.
fn private_dir_next_to(path: &Path) -> std::io::Result<std::path::PathBuf> {
    private_dir_next_to_with_seq(path, &STAGE_SEQ)
}

fn private_dir_next_to_with_seq(path: &Path, seq: &std::sync::atomic::AtomicU64) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("export");
    let mut last = None;
    for _ in 0..64 {
        let dir = path.with_file_name(format!(
            ".{stem}.qymcad-mcp-{}-{}.d",
            std::process::id(),
            seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free name")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    /// Every edge has exactly two triangles, running it in opposite directions, and the triangles around every point
    /// form one fan: a closed manifold.
    fn assert_manifold(m: &Mesh) {
        let mut directed = std::collections::HashMap::new();
        for &[a, b, c] in &m.tris {
            for e in [(a, b), (b, c), (c, a)] {
                *directed.entry(e).or_insert(0) += 1;
            }
        }
        for (&(a, b), &n) in &directed {
            assert!(n == 1 && directed.get(&(b, a)) == Some(&1), "edge {a}-{b} is open or doubled");
        }
        for v in 0..m.verts.len() as u32 {
            let around: Vec<[u32; 3]> = m.tris.iter().copied().filter(|t| t.contains(&v)).collect();
            // walk the fan: from a triangle to the one across its edge leaving v
            let (mut seen, mut t) = (1, around[0]);
            loop {
                let k = t.iter().position(|&i| i == v).unwrap();
                let out = t[(k + 2) % 3]; // the edge (out, v) is entered by the next triangle as (v, out)
                t = *around.iter().find(|u| (0..3).any(|j| u[j] == v && u[(j + 1) % 3] == out)).unwrap();
                if t == around[0] {
                    break;
                }
                seen += 1;
            }
            assert_eq!(seen, around.len(), "point {v} is shared by {} fans", around.len());
        }
    }

    /// A closed tetrahedron, written as the kernel writes faces: every face with its own copies of its corners. `f`
    /// places the corners of the unit one; it must keep the handedness, so the winding stays outward.
    fn tetra(f: impl Fn([f64; 3]) -> [f64; 3], mesh: &mut Mesh) {
        let p = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(f);
        for face in [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]] {
            let at = mesh.verts.len() as u32;
            mesh.verts.extend(face.map(|i| qymcad_core::geom::Point3::new(p[i][0], p[i][1], p[i][2])));
            mesh.tris.push([at, at + 1, at + 2]);
        }
    }

    #[test]
    fn welding_closes_each_solid_and_keeps_touching_solids_apart() {
        type Place = fn([f64; 3]) -> [f64; 3];
        let cases: [(&str, Option<Place>); 4] = [
            ("one solid", None),
            ("apart", Some(|[x, y, z]| [x + 5.0, y, z])),
            ("touching along an edge", Some(|[x, y, z]| [-x, -y, z])), // turned half about Z: shares the Z edge
            ("touching at a corner", Some(|[x, y, z]| [x + 1.0, y, z])), // shares (1, 0, 0)
        ];
        for (what, second) in cases {
            let mut m = Mesh::default();
            tetra(|p| p, &mut m);
            if let Some(f) = second {
                tetra(f, &mut m);
            }
            let volume = m.volume();
            assert!(volume > 0.0, "{what}: outward winding");
            weld(&mut m);
            let solids = if second.is_some() { 2 } else { 1 };
            assert_eq!(m.verts.len(), 4 * solids, "{what}: points");
            assert_eq!(m.tris.len(), 4 * solids, "{what}: triangles");
            assert!((m.volume() - volume).abs() < 1e-12, "{what}: volume");
            assert_manifold(&m);
        }
    }

    /// Four solids around one corner, each touching the others only along edges (review round 2, Codex): every edge of
    /// the first one's base carries four triangles. Pairing only two-triangle edges cut that base off as a piece of
    /// its own and left six open edges.
    #[test]
    fn welding_pairs_the_triangles_around_an_edge_of_four() {
        let mut m = Mesh::default();
        tetra(|p| p, &mut m);
        tetra(|[x, y, z]| [x, -y, -z], &mut m);
        tetra(|[x, y, z]| [-x, y, -z], &mut m);
        tetra(|[x, y, z]| [1.0 - y, 1.0 - x, -z], &mut m);
        let volume = m.volume();
        weld(&mut m);
        assert_eq!((m.verts.len(), m.tris.len()), (16, 16), "four solids of their own points");
        assert!((m.volume() - volume).abs() < 1e-12);
        assert_manifold(&m);
    }

    /// Two solids lying on each other face to face (review round 3, Codex): the coincident faces hold no solid between
    /// them and must not tie the solids together.
    #[test]
    fn welding_keeps_solids_apart_across_a_shared_face() {
        let mut m = Mesh::default();
        tetra(|p| p, &mut m);
        tetra(|[x, y, z]| [y, x, -z], &mut m);
        weld(&mut m);
        assert_eq!(m.verts.len(), 8);
        assert_manifold(&m);
    }

    /// The outer faces of unit cubes at `cells`, every face with its own corner copies, wound outward.
    fn cubes(cells: &[[i32; 3]], mesh: &mut Mesh) {
        for c in cells {
            for (a, s) in (0..3).flat_map(|a| [(a, 1), (a, -1)]) {
                let mut n = *c;
                n[a] += s;
                if cells.contains(&n) {
                    continue;
                }
                let (b, d) = ((a + 1) % 3, (a + 2) % 3); // e_b × e_d = e_a
                let mut quad = [[0, 0], [1, 0], [1, 1], [0, 1]].map(|[i, j]| {
                    let mut p = c.map(f64::from);
                    p[a] += if s > 0 { 1.0 } else { 0.0 };
                    p[b] += f64::from(i);
                    p[d] += f64::from(j);
                    qymcad_core::geom::Point3::new(p[0], p[1], p[2])
                });
                if s < 0 {
                    quad.reverse();
                }
                let at = mesh.verts.len() as u32;
                mesh.verts.extend(quad);
                mesh.tris.extend([[at, at + 1, at + 2], [at, at + 2, at + 3]]);
            }
        }
    }

    /// One connected surface that closes on itself along an edge (review round 3, Codex): a ring of seven cubes whose
    /// ends touch along x = y = 1. The two wedges there must keep their own points although the surface is one piece.
    #[test]
    fn welding_splits_a_connected_surface_touching_itself_along_an_edge() {
        let mut m = Mesh::default();
        cubes(&[[0, 0, 0], [-1, 0, 0], [-1, 1, 0], [-1, 2, 0], [0, 2, 0], [1, 2, 0], [1, 1, 0]], &mut m);
        assert!((m.volume() - 7.0).abs() < 1e-12, "outward winding");
        weld(&mut m);
        assert!((m.volume() - 7.0).abs() < 1e-12);
        assert_manifold(&m);
    }

    /// A stray sheet triangle on a solid's edge makes an edge of three (review round 3, MiMo): the solid's two
    /// triangles there are paired, the sheet's is left on its own points.
    #[test]
    fn welding_leaves_a_stray_sheet_on_an_edge_of_three_unpaired() {
        for (sheet_first, flipped) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut m = Mesh::default();
            let mut sheet = Mesh::default();
            tetra(|p| p, &mut m);
            sheet
                .verts
                .extend([[0.0, 0.0, 0.0], [0.5, -1.0, -1.0], [1.0, 0.0, 0.0]].map(|p| qymcad_core::geom::Point3::new(p[0], p[1], p[2])));
            sheet.tris.push(if flipped { [0, 2, 1] } else { [0, 1, 2] });
            if sheet_first {
                std::mem::swap(&mut m, &mut sheet);
            }
            let at = m.verts.len() as u32;
            m.verts.extend(sheet.verts);
            m.tris.extend(sheet.tris.iter().map(|t| t.map(|i| i + at)));
            weld(&mut m);
            let k = if sheet_first { 0 } else { 4 };
            let solid: Vec<[u32; 3]> = m.tris.iter().enumerate().filter(|&(i, _)| i != k).map(|(_, t)| *t).collect();
            assert_eq!(m.verts.len(), 7, "the sheet keeps three points of its own");
            assert!(m.tris[k].iter().all(|i| !solid.iter().flatten().any(|j| j == i)));
            let mut closed = Mesh { verts: m.verts.clone(), tris: solid, norms: Vec::new() };
            let used: Vec<u32> = (0..7).filter(|i| closed.tris.iter().flatten().any(|j| j == i)).collect();
            closed.tris.iter_mut().for_each(|t| *t = t.map(|i| used.iter().position(|&u| u == i).unwrap() as u32));
            closed.verts = used.iter().map(|&i| m.verts[i as usize]).collect();
            assert_manifold(&closed);
        }
    }

    /// Coincident sheets alone on an edge (review round 4, MiMo): every triangle lies at one turn, so none are paired.
    #[test]
    fn wedges_pair_nothing_when_every_triangle_lies_at_one_turn() {
        let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]].map(|p| qymcad_core::geom::Point3::new(p[0], p[1], p[2]));
        let tris = [[0, 1, 2], [1, 0, 2], [0, 1, 2], [1, 0, 2]];
        let run: Vec<(u32, u32, u32)> = (0..4).map(|t| (0, 1, t)).collect();
        assert!(wedges(&points, &tris, &run).is_empty());
    }

    /// KNOWN LIMIT (review round 4, Codex; F-093): two cubes standing diagonally between two plates touch along an
    /// edge whose ends are each one fan, so splitting points per fan cannot separate the two wedges there and the edge
    /// keeps four triangles. Such a solid has zero thickness along that line and is itself non-manifold; resolving it
    /// needs the triangles re-paired across the gap with both ends duplicated. Kept ignored as the case to start from.
    #[test]
    #[ignore = "known limit: a solid pinched along an edge stays non-manifold there (F-093)"]
    fn welding_a_solid_pinched_along_an_edge() {
        let mut m = Mesh::default();
        let mut cells = vec![[0, 0, 0], [1, 1, 0]];
        for z in [-1, 1] {
            for x in 0..2 {
                for y in 0..2 {
                    cells.push([x, y, z]);
                }
            }
        }
        cubes(&cells, &mut m);
        weld(&mut m);
        assert_manifold(&m);
    }

    #[test]
    fn welding_drops_the_slivers_at_a_pole() {
        let mut m = Mesh::default();
        tetra(|p| p, &mut m);
        // a zero-area triangle along a degenerate edge: two corners are copies of one point
        let at = m.verts.len() as u32;
        m.verts.extend([[0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]].map(|p| qymcad_core::geom::Point3::new(p[0], p[1], p[2])));
        m.tris.push([at, at + 1, at + 2]);
        weld(&mut m);
        assert_eq!(m.tris.len(), 4);
        assert_manifold(&m);
    }

    /// Entries planted at the next staging names (symlinks to another directory) are skipped, never followed or
    /// reused; the directory made is private (review round 2, Codex).
    #[test]
    fn staging_skips_planted_entries() {
        let base = std::env::temp_dir().join(format!("qymcad-mcp-stage-{}", std::process::id()));
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let target = base.join("part.stl");
        let n = 0;
        let seq = AtomicU64::new(n);
        let name = |k: u64| base.join(format!(".part.qymcad-mcp-{}-{k}.d", std::process::id()));
        for k in n..n + 3 {
            std::os::unix::fs::symlink(&elsewhere, name(k)).unwrap();
        }
        // Another staging user may allocate after the test has planted its names. Advance past all three names
        // deterministically, as four intervening exports would, without depending on the test scheduler.
        for _ in 0..4 {
            let other = private_dir_next_to(&base.join("other.stl")).unwrap();
            std::fs::remove_dir(other).unwrap();
        }
        let dir = private_dir_next_to_with_seq(&target, &seq).unwrap();
        // Starting at n and refusing three occupied names must allocate n + 3 regardless of unrelated users.
        assert_eq!(dir, name(n + 3), "the three planted names are skipped");
        let meta = std::fs::symlink_metadata(&dir).unwrap();
        assert!(meta.is_dir() && !meta.file_type().is_symlink());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(meta.permissions().mode() & 0o777, 0o700, "private");
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0, "nothing created through a link");
        std::fs::remove_dir_all(&base).unwrap();
    }
}
