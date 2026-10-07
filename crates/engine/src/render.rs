//! A small CPU renderer: an orthographic, shaded view of result bodies with dark feature edges, as a PNG, so an
//! agent (a vision model) can look at what it built.
//!
//! Adapted from QymCAD's thumbnail rasterizer `render_component_thumbnail`
//! (`crates/qymcad/src/gui/render_scene.rs` in QymCAD v0.1.0-dev.20261001, AGPL-3.0-or-later like this crate; it is
//! `pub(crate)` in the app crate, so it is copied rather than called). Kept from it: the body display meshes placed
//! by their transform, the isometric camera basis of `Cam3::default()`, back-face culling, the edge-function
//! triangle fill with a z-buffer. Changed: any view and size, a fit to the projected extent, a light fixed to the
//! camera, a light background, 2×2 supersampling, and edges drawn where the B-rep face under a pixel changes (an
//! id buffer), which gives face boundaries and silhouettes with hidden lines removed.

use crate::error::{Error, Result};
use crate::session::Session;
use qymcad_core::model::Id;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A view direction. Orthographic; +Z is up in every side view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum View {
    /// Isometric from front-right-top (camera at +X −Y +Z), QymCAD's default camera.
    #[default]
    Iso,
    /// From +Z looking down: +X right, +Y up.
    Top,
    /// From −Z looking up: +X right, −Y up.
    Bottom,
    /// From −Y: +X right, +Z up.
    Front,
    /// From +Y: −X right, +Z up.
    Back,
    /// From −X: −Y right, +Z up.
    Left,
    /// From +X: +Y right, +Z up.
    Right,
}

impl View {
    /// `(right, up, fwd)`: screen right, screen up and the viewing direction, in world coordinates.
    fn basis(self) -> ([f64; 3], [f64; 3], [f64; 3]) {
        let (right, up) = match self {
            View::Iso => {
                // Cam3::default(): yaw −0.7, pitch 0.6; `Cam3::basis` in qymcad-ui-state
                let (yaw, pitch) = (-0.7f64, 0.6f64);
                let fwd = [-pitch.cos() * yaw.cos(), -pitch.cos() * yaw.sin(), -pitch.sin()];
                let right = norm(cross(fwd, [0.0, 0.0, 1.0]));
                (right, norm(cross(right, fwd)))
            }
            View::Top => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            View::Bottom => ([1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
            View::Front => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            View::Back => ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            View::Left => ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
            View::Right => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        };
        (right, up, cross(up, right))
    }

    /// How the world axes appear on screen, for the caption.
    pub fn describe(self) -> &'static str {
        match self {
            View::Iso => "isometric from front-right-top: +X to the lower right, +Y to the upper right, +Z up",
            View::Top => "from above: +X right, +Y up",
            View::Bottom => "from below: +X right, -Y up",
            View::Front => "from the front (-Y): +X right, +Z up",
            View::Back => "from the back (+Y): -X right, +Z up",
            View::Left => "from the left (-X): -Y right, +Z up",
            View::Right => "from the right (+X): +Y right, +Z up",
        }
    }
}

/// A rendered image.
#[derive(Clone, Debug)]
pub struct Rendered {
    /// PNG file bytes (8-bit RGB).
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub view: View,
    /// The bodies drawn.
    pub bodies: Vec<Id>,
    /// World-space union of the drawn bodies' B-rep bounding boxes, `[xmin, ymin, zmin, xmax, ymax, zmax]` mm.
    pub bbox: [f64; 6],
}

/// Pixel limits for one side of the image.
pub const RENDER_MIN_SIDE: u32 = 64;
pub const RENDER_MAX_SIDE: u32 = 2048;
/// The background colour (sRGB). Anything else in an image is the model or its edges.
pub const RENDER_BACKGROUND: [u8; 3] = [246, 247, 249];

const BODY: [f64; 3] = [168.0, 186.0, 210.0];
const EDGE: [u8; 3] = [28, 32, 40];
const SS: usize = 2; // supersampling factor per axis
const MARGIN: f64 = 0.07; // free border, fraction of each side

/// One body to draw: world-space vertices, triangles, and the B-rep face index of every triangle.
pub(crate) struct Item {
    pub verts: Vec<[f64; 3]>,
    pub tris: Vec<[u32; 3]>,
    pub tri_face: Vec<u32>,
}

impl Session {
    /// Render `bodies` (default: every result body) from `view` at `width` × `height` pixels.
    pub fn render(&self, view: View, width: u32, height: u32, bodies: Option<&[Id]>) -> Result<Rendered> {
        for (what, v) in [("width", width), ("height", height)] {
            if !(RENDER_MIN_SIDE..=RENDER_MAX_SIDE).contains(&v) {
                return Err(Error::Invalid(format!("{what} must be {RENDER_MIN_SIDE}..={RENDER_MAX_SIDE} pixels, got {v}")));
            }
        }
        let ids = self.output_bodies(bodies)?;
        let mut items = Vec::new();
        for &id in &ids {
            let Some(b) = self.p.mesh_index(id).map(|i| &self.p.bodies[i]) else { continue };
            let m = self.p.body_world_transform(id);
            let verts = b
                .mesh
                .verts
                .iter()
                .map(|v| {
                    let (x, y, z) = (v.x, v.y, v.z);
                    [m[0] * x + m[1] * y + m[2] * z + m[3], m[4] * x + m[5] * y + m[6] * z + m[7], m[8] * x + m[9] * y + m[10] * z + m[11]]
                })
                .collect();
            let mut tri_face = vec![u32::MAX; b.mesh.tris.len()];
            for (fi, f) in b.faces.iter().enumerate() {
                for &t in &f.triangles {
                    if let Some(slot) = tri_face.get_mut(t as usize) {
                        *slot = fi as u32;
                    }
                }
            }
            items.push(Item { verts, tris: b.mesh.tris.clone(), tri_face });
        }
        if items.iter().all(|it| it.tris.is_empty()) {
            return Err(Error::Invalid("the selected bodies have no display mesh".into()));
        }
        let rgb = rasterize(&items, view, width as usize, height as usize);
        let mut bbox = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        // Shape bounds are in the body's own frame; the picture is in world space, so transform the corners.
        for b in self.result_bodies().iter().filter(|b| ids.contains(&b.id)) {
            let m = self.p.body_world_transform(b.id);
            for i in 0..8 {
                let c = [
                    b.bbox[if i & 1 == 0 { 0 } else { 3 }],
                    b.bbox[if i & 2 == 0 { 1 } else { 4 }],
                    b.bbox[if i & 4 == 0 { 2 } else { 5 }],
                ];
                for k in 0..3 {
                    let w = m[4 * k] * c[0] + m[4 * k + 1] * c[1] + m[4 * k + 2] * c[2] + m[4 * k + 3];
                    bbox[k] = bbox[k].min(w);
                    bbox[k + 3] = bbox[k + 3].max(w);
                }
            }
        }
        Ok(Rendered { png: crate::pngfile::encode_rgb(width, height, &rgb), width, height, view, bodies: ids, bbox })
    }
}

/// Draw the items into an RGB buffer of `w` × `h`.
pub(crate) fn rasterize(items: &[Item], view: View, w: usize, h: usize) -> Vec<u8> {
    let (right, up, fwd) = view.basis();
    let (sw, sh) = (w * SS, h * SS);
    // fit the projected extent into the frame
    let (mut umin, mut umax, mut vmin, mut vmax) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for p in items.iter().flat_map(|it| &it.verts) {
        let (u, v) = (dot(*p, right), dot(*p, up));
        umin = umin.min(u);
        umax = umax.max(u);
        vmin = vmin.min(v);
        vmax = vmax.max(v);
    }
    let usable = 1.0 - 2.0 * MARGIN;
    let scale = (sw as f64 * usable / (umax - umin).max(1e-9)).min(sh as f64 * usable / (vmax - vmin).max(1e-9));
    let (uc, vc) = ((umin + umax) / 2.0, (vmin + vmax) / 2.0);
    let proj = |p: [f64; 3]| (sw as f64 / 2.0 + (dot(p, right) - uc) * scale, sh as f64 / 2.0 - (dot(p, up) - vc) * scale, dot(p, fwd));
    // a light fixed to the camera, from the upper left and in front, plus some ambient
    let light = norm([
        -0.45 * right[0] + 0.55 * up[0] - 0.70 * fwd[0],
        -0.45 * right[1] + 0.55 * up[1] - 0.70 * fwd[1],
        -0.45 * right[2] + 0.55 * up[2] - 0.70 * fwd[2],
    ]);
    let ef = |ux: f64, uy: f64, vx: f64, vy: f64, px: f64, py: f64| (vx - ux) * (py - uy) - (vy - uy) * (px - ux);
    let mut color = vec![RENDER_BACKGROUND; sw * sh];
    let mut zbuf = vec![f64::INFINITY; sw * sh];
    let mut ids = vec![0u32; sw * sh]; // 0 = background; otherwise a unique (body, face) number
    let mut next_id = 1u32;
    for it in items {
        let faces = it.tri_face.iter().filter(|f| **f != u32::MAX).max().map_or(0, |m| m + 1);
        let base_id = next_id;
        next_id += faces + 1;
        for (ti, tri) in it.tris.iter().enumerate() {
            let (a, b, c) = (it.verts[tri[0] as usize], it.verts[tri[1] as usize], it.verts[tri[2] as usize]);
            let n = norm(cross(sub(b, a), sub(c, a)));
            if dot(n, fwd) >= 0.0 {
                continue; // bodies are oriented outwards: this one faces away
            }
            let k = 0.32 + 0.53 * dot(n, light).max(0.0) + 0.15 * (-dot(n, fwd)).max(0.0);
            let col = [(BODY[0] * k).min(255.0) as u8, (BODY[1] * k).min(255.0) as u8, (BODY[2] * k).min(255.0) as u8];
            let id = match it.tri_face[ti] {
                u32::MAX => base_id + faces, // a triangle of no known face
                f => base_id + f,
            };
            let ((ax, ay, az), (bx, by, bz), (cx, cy, cz)) = (proj(a), proj(b), proj(c));
            let area = ef(ax, ay, bx, by, cx, cy);
            if area.abs() < 1e-12 {
                continue;
            }
            let minx = ax.min(bx).min(cx).floor().max(0.0) as usize;
            let maxx = (ax.max(bx).max(cx).ceil().max(0.0) as usize).min(sw);
            let miny = ay.min(by).min(cy).floor().max(0.0) as usize;
            let maxy = (ay.max(by).max(cy).ceil().max(0.0) as usize).min(sh);
            for py in miny..maxy {
                for px in minx..maxx {
                    let (fx, fy) = (px as f64 + 0.5, py as f64 + 0.5);
                    let (w0, w1, w2) =
                        (ef(bx, by, cx, cy, fx, fy) / area, ef(cx, cy, ax, ay, fx, fy) / area, ef(ax, ay, bx, by, fx, fy) / area);
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let depth = w0 * az + w1 * bz + w2 * cz;
                    let i = py * sw + px;
                    if depth < zbuf[i] {
                        zbuf[i] = depth;
                        color[i] = col;
                        ids[i] = id;
                    }
                }
            }
        }
    }
    // edges: a pixel whose right or lower neighbour shows another face (or the background), drawn on both sides
    let mut edge = vec![false; sw * sh];
    for y in 0..sh {
        for x in 0..sw {
            let i = y * sw + x;
            for j in [(x + 1 < sw).then(|| i + 1), (y + 1 < sh).then(|| i + sw)].into_iter().flatten() {
                if ids[i] != ids[j] {
                    edge[i] = true;
                    edge[j] = true;
                }
            }
        }
    }
    for (c, e) in color.iter_mut().zip(&edge) {
        if *e {
            *c = EDGE;
        }
    }
    // downsample SS×SS → 1
    let mut out = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 3];
            for dy in 0..SS {
                for dx in 0..SS {
                    let c = color[(y * SS + dy) * sw + x * SS + dx];
                    for k in 0..3 {
                        acc[k] += c[k] as u32;
                    }
                }
            }
            let n = (SS * SS) as u32;
            out.extend(acc.iter().map(|a| ((a + n / 2) / n) as u8));
        }
    }
    out
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt();
    if l > 0.0 {
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every view's basis is right-handed and orthonormal, and `fwd` points away from the camera.
    #[test]
    fn view_bases_are_orthonormal() {
        for v in [View::Iso, View::Top, View::Bottom, View::Front, View::Back, View::Left, View::Right] {
            let (r, u, f) = v.basis();
            for (a, b) in [(r, u), (u, f), (f, r)] {
                assert!(dot(a, b).abs() < 1e-12, "{v:?} not orthogonal");
            }
            for a in [r, u, f] {
                assert!((dot(a, a) - 1.0).abs() < 1e-12, "{v:?} not unit");
            }
            // right × up points towards the viewer = −fwd
            let t = cross(r, u);
            assert!((dot(t, f) + 1.0).abs() < 1e-12, "{v:?} not right-handed");
        }
        assert_eq!(View::Top.basis().2, [0.0, 0.0, -1.0]);
        assert_eq!(View::Front.basis().2, [0.0, 1.0, 0.0]);
        assert_eq!(View::Right.basis().2, [-1.0, 0.0, 0.0]);
        let iso = View::Iso.basis().2;
        assert!(iso[0] < 0.0 && iso[1] > 0.0 && iso[2] < 0.0, "iso camera sits at +X -Y +Z: {iso:?}");
    }

    /// One square facing the camera fills the frame minus the margin and shows a dark outline.
    #[test]
    fn a_square_fills_the_frame() {
        let item = Item {
            verts: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]],
            tris: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
        };
        let (w, h) = (100, 100);
        let img = rasterize(&[item], View::Top, w, h);
        let px = |x: usize, y: usize| [img[(y * w + x) * 3], img[(y * w + x) * 3 + 1], img[(y * w + x) * 3 + 2]];
        assert_eq!(px(2, 2), RENDER_BACKGROUND);
        assert_eq!(px(50, 50), px(30, 70), "one face, one flat colour");
        assert_ne!(px(50, 50), RENDER_BACKGROUND);
        // the edge on the margin line (7 px) is dark
        let e = px(7, 50);
        assert!(e.iter().all(|c| *c < 140), "outline at x=7: {e:?}");
        // it spans the frame minus the margins (7..=92) on both axes, and nothing outside
        for (x, y) in [(9, 50), (90, 50), (50, 9), (50, 90)] {
            assert_ne!(px(x, y), RENDER_BACKGROUND, "inside at ({x}, {y})");
        }
        for (x, y) in [(4, 50), (95, 50), (50, 4), (50, 95)] {
            assert_eq!(px(x, y), RENDER_BACKGROUND, "margin at ({x}, {y})");
        }
        // a back-facing square is culled
        let back = Item {
            verts: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]],
            tris: vec![[0, 2, 1], [0, 3, 2]],
            tri_face: vec![0, 0],
        };
        let img = rasterize(&[back], View::Top, w, h);
        assert!(img.chunks(3).all(|c| c == RENDER_BACKGROUND), "back faces are not drawn");
    }
}
