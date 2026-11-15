//! Package K phase 0: probes of what the app does with Split faces (`FeatureKind::SplitFace`, P14).
//! The native probes go through the core API only (`Project` + `add_split_face` + the app's regeneration), no server guard.
//! Each probe prints what it observed (run with --nocapture, lines start with `K0`); assertions pin the observation, so a changed
//! app shows here. What is unstable is printed and not asserted.
use crate::common;
use qymcad_core::feature::{apply12, apply12_dir, mat_inv12, FaceKey, PlaneAt, PLACE_IDENTITY};
use qymcad_core::model::{Id, Project};
use qymcad_kernel::Shape;
use std::collections::{HashMap, HashSet};

type V3 = [f64; 3];

/// One face of a built body as the app lists it.
#[derive(Clone, Debug)]
struct F {
    id: u32,
    c: V3,
    n: V3,
    area: f64,
}

/// What the native regeneration made of one body.
#[derive(Debug)]
struct S {
    faces: usize,
    distinct_ids: usize,
    zero_ids: usize,
    volume: Option<f64>,
    valid: Option<bool>,
    solids: u32,
    shells: u32,
    errors: Vec<String>,
}

/// A document as the app holds it: the project and the live shapes between rebuilds.
struct Doc {
    p: Project,
    shapes: HashMap<Id, Shape>,
    errors: Vec<String>,
}

impl Doc {
    fn new() -> Doc {
        let mut p = Project::default();
        p.new_document();
        Doc { p, shapes: HashMap::new(), errors: vec![] }
    }

    /// The rebuild after a local edit: what is dirty is rebuilt over the shapes the last rebuild left (the app's own path).
    fn build(&mut self) {
        self.p.eval_parameters();
        let shapes = std::mem::take(&mut self.shapes);
        let (report, shapes) = qymcad_testkit::regenerate_dirty_with_shapes(&mut self.p, shapes);
        self.shapes = shapes;
        self.errors = report.errors.iter().map(|e| format!("{e:?}")).collect();
    }

    /// A forced rebuild of a copy of the project from an empty shape cache.
    fn full_copy(&self) -> Doc {
        let mut p = self.p.clone();
        p.eval_parameters();
        let (report, shapes) = qymcad_testkit::regenerate(&mut p);
        Doc { p, shapes, errors: report.errors.iter().map(|e| format!("{e:?}")).collect() }
    }

    fn edit(&mut self, node: Id, key: &str, text: &str) {
        self.p.set_feat_dim(node, key, text.to_string());
        self.build();
    }

    /// The last result body, as `measure` of j0.
    fn last(&self) -> Option<Id> {
        let consumed = self.p.consumed_bodies();
        self.p.timeline.iter().flat_map(|n| n.kind.bodies()).rfind(|b| !consumed.contains(b))
    }

    /// A split of faces by a base plane (0 XY, 1 XZ, 2 YZ) shifted by `off`; the number is typed as the server writes it.
    fn split(&mut self, src: Id, plane: u8, off: f64) -> Id {
        let id = self.p.add_split_face(src, plane, 0, off);
        self.p.set_feat_dim(id, "offset", format!("{off}"));
        id
    }

    /// A split by the plane of a face of `body` (the click of the app), shifted by `off`.
    fn split_face_plane(&mut self, src: Id, body: Id, f: &F, off: f64) -> Id {
        let id = self.p.add_split_face(src, 0, 0, off);
        self.p.set_op_face(id, body, FaceKey { index: 0, centroid: f.c, normal: f.n, id: f.id });
        self.p.set_feat_dim(id, "offset", format!("{off}"));
        id
    }

    fn faces(&self, b: Id) -> Vec<F> {
        let mut v: Vec<F> = self
            .p
            .regen_faces
            .get(&b)
            .map(|fs| fs.iter().map(|f| F { id: f.id, c: [f.centroid.x, f.centroid.y, f.centroid.z], n: f.normal, area: f.area }).collect())
            .unwrap_or_default();
        v.sort_by_key(|f| f.id);
        v
    }

    fn sum(&self, b: Option<Id>) -> S {
        let faces = b.map(|b| self.faces(b)).unwrap_or_default();
        let ids: HashSet<u32> = faces.iter().map(|f| f.id).collect();
        let errors = self.errors.clone();
        let Some(shape) = b.and_then(|b| self.shapes.get(&b)) else {
            return S { faces: 0, distinct_ids: 0, zero_ids: 0, volume: None, valid: None, solids: 0, shells: 0, errors };
        };
        let _g = qymcad_kernel::kernel_gate();
        S {
            faces: faces.len(),
            distinct_ids: ids.len(),
            zero_ids: faces.iter().filter(|f| f.id == 0).count(),
            volume: Some(shape.volume()),
            valid: Some(shape.is_valid()),
            solids: shape.solid_count(),
            shells: shape.shell_count(),
            errors,
        }
    }

    /// The z levels of the horizontal edges of `b`: where the cuts stand.
    fn levels(&self, b: Id) -> Vec<f64> {
        let mut z: Vec<f64> = self
            .p
            .regen_edges
            .get(&b)
            .map(|es| es.iter().filter(|e| e.dir[2].abs() < 1e-6).map(|e| (e.mid[2] * 1e4).round() / 1e4).collect())
            .unwrap_or_default();
        z.sort_by(|a, b| a.partial_cmp(b).unwrap());
        z.dedup();
        z
    }

    /// Would the app's pre-check (`ask::splits_faces`) let this plane through on the built body `b`.
    fn asks(&self, b: Id, origin: V3, normal: V3) -> bool {
        qymcad_kernel::ask::splits_faces(self.shapes.get(&b).expect("shape"), PlaneAt { origin, normal })
    }
}

/// A body built by a chain of nodes: `solid` is the body the split reads, `sp` the split node (its id is its body), `a` the first box.
struct M {
    d: Doc,
    a: Id,
    solid: Id,
    sp: Id,
}

fn shift(p: &mut Project, b: Id, x: f64, y: f64, z: f64) -> Id {
    let mut m = PLACE_IDENTITY;
    (m[3], m[7], m[11]) = (x, y, z);
    p.add_move(b, m)
}

/// The block 40x30x10 (x -20..20, y -15..15, z 0..h), split by XY at `off`.
fn block(h: f64, off: f64) -> M {
    let mut d = Doc::new();
    let a = d.p.add_box(40.0, 30.0, h);
    let sp = d.split(a, 0, off);
    d.build();
    M { d, a, solid: a, sp }
}

/// A U body: the box 40x30x20 less a slot 20 wide, 10 deep from the top (y walls are U-shaped faces, three pieces above the base).
fn u_body(off: f64) -> M {
    let mut d = Doc::new();
    let a = d.p.add_box(40.0, 30.0, 20.0);
    let b = d.p.add_box(20.0, 34.0, 12.0);
    let mv = shift(&mut d.p, b, 0.0, 0.0, 10.0);
    let u = d.p.add_body_boolean(a, mv, 0);
    let sp = d.split(u, 0, off);
    d.build();
    M { d, a, solid: u, sp }
}

/// An L body: the box 40x30x20 less the upper half of its x > 0 side (a step at z = 10; y walls are L-shaped faces).
fn l_body(off: f64) -> M {
    let mut d = Doc::new();
    let a = d.p.add_box(40.0, 30.0, 20.0);
    let b = d.p.add_box(30.0, 34.0, 12.0);
    let mv = shift(&mut d.p, b, 15.0, 0.0, 10.0);
    let l = d.p.add_body_boolean(a, mv, 0);
    let sp = d.split(l, 0, off);
    d.build();
    M { d, a, solid: l, sp }
}

fn show(label: &str, s: &S) {
    eprintln!("K0 {label}: {s:?}");
}

fn dump(label: &str, faces: &[F]) {
    eprintln!("K0 {label}: {} faces", faces.len());
    for f in faces {
        eprintln!(
            "K0   id {:>6} c ({:7.3} {:7.3} {:7.3}) n ({:5.2} {:5.2} {:5.2}) area {:8.3}",
            f.id, f.c[0], f.c[1], f.c[2], f.n[0], f.n[1], f.n[2], f.area
        );
    }
}

fn near(a: V3, b: V3, tol: f64) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= tol)
}

fn wall(f: &F) -> bool {
    f.n[2].abs() < 0.5
}

/// Ids present before and after an edit whose face moved to the other side of the cutting plane (the plane stood at `off_b` then at
/// `off_a` along z): an id that was the lower piece and is now the upper one, or the other way round. Walls only.
fn swapped(label: &str, before: &[F], off_b: f64, after: &[F], off_a: f64) -> Vec<u32> {
    let mut out = Vec::new();
    let b_ids: HashSet<u32> = before.iter().map(|f| f.id).collect();
    let a_ids: HashSet<u32> = after.iter().map(|f| f.id).collect();
    let gone: Vec<u32> = before.iter().filter(|f| !a_ids.contains(&f.id)).map(|f| f.id).collect();
    let new: Vec<u32> = after.iter().filter(|f| !b_ids.contains(&f.id)).map(|f| f.id).collect();
    eprintln!("K0 {label}: ids gone {gone:?} new {new:?}");
    for fb in before.iter().filter(|f| wall(f)) {
        let Some(fa) = after.iter().find(|f| f.id == fb.id) else { continue };
        let (up_b, up_a) = (fb.c[2] > off_b, fa.c[2] > off_a);
        eprintln!(
            "K0   id {:>6} n ({:5.2} {:5.2}) c ({:6.2} {:6.2} {:6.2}) -> ({:6.2} {:6.2} {:6.2}) {}",
            fb.id,
            fb.n[0],
            fb.n[1],
            fb.c[0],
            fb.c[1],
            fb.c[2],
            fa.c[0],
            fa.c[1],
            fa.c[2],
            if up_b != up_a { "SWAPPED SIDE" } else { "same side" }
        );
        if up_b != up_a {
            out.push(fb.id);
        }
    }
    out
}

/// Faces of `a` whose id differs from the face of `b` with the same normal and centre (centre tolerance `tol`).
fn id_mismatch(a: &[F], b: &[F], tol: f64) -> usize {
    a.iter().filter(|fa| b.iter().any(|fb| near(fa.c, fb.c, tol) && near(fa.n, fb.n, 1e-6) && fb.id != fa.id)).count()
}

fn close(got: Option<f64>, want: f64, rel: f64, what: &str) {
    let got = got.unwrap_or_else(|| panic!("{what}: no body"));
    assert!((got - want).abs() <= want.abs() * rel, "{what}: got {got}, expected {want} (rel {rel})");
}

fn red(errors: &[String], code: &str) -> bool {
    errors.iter().any(|e| e.contains(code))
}

// ---------------------------------------------------------------------------------------------------------------------------------
// P1: which piece of a cut face keeps the id, and does that change under an edit.
// ---------------------------------------------------------------------------------------------------------------------------------

/// P1a: the block cut at z = 5, offset edited 5 -> 3 -> 7 on the app's dirty-rebuild path.
#[test]
fn p1a_block_offset_edit_keeps_the_piece_ids() {
    let mut m = block(10.0, 5.0);
    let s = m.d.sum(Some(m.sp));
    show("P1a block z=5", &s);
    assert!(s.errors.is_empty(), "{s:?}");
    assert_eq!((s.faces, s.distinct_ids), (10, 10), "6 -> 10 faces, ids unique");
    close(s.volume, 12000.0, 1e-9, "volume");
    let f0 = m.d.faces(m.sp);
    dump("P1a z=5", &f0);
    let mut prev = (f0.clone(), 5.0);
    for (text, off) in [("3", 3.0), ("7", 7.0)] {
        m.d.edit(m.sp, "offset", text);
        let s = m.d.sum(Some(m.sp));
        show(&format!("P1a offset {text}"), &s);
        let f1 = m.d.faces(m.sp);
        dump(&format!("P1a z={text}"), &f1);
        let sw = swapped(&format!("P1a {} -> {text}", prev.1), &prev.0, prev.1, &f1, off);
        eprintln!("K0 P1a swapped ids: {sw:?}");
        assert!(sw.is_empty(), "no piece changes sides: {sw:?}");
        assert!(s.errors.is_empty() && s.faces == 10 && s.distinct_ids == 10, "{s:?}");
        let fresh = block(10.0, off);
        let ff = fresh.d.faces(fresh.sp);
        let mismatch = id_mismatch(&f1, &ff, 1e-6);
        eprintln!("K0 P1a ids that differ from a build from scratch at the same offset: {mismatch}");
        assert_eq!(mismatch, 0, "an edited block has the ids of a block built at that offset");
        prev = (f1, off);
    }
}

/// P1b: the same on a forced rebuild of the whole document (empty shape cache): is the id of the lower piece the same in both.
#[test]
fn p1b_block_forced_rebuild_matches_the_dirty_one() {
    let mut m = block(10.0, 5.0);
    m.d.edit(m.sp, "offset", "3");
    let dirty = m.d.faces(m.sp);
    let full = m.d.full_copy();
    let ff = full.faces(m.sp);
    dump("P1b dirty z=3", &dirty);
    dump("P1b forced z=3", &ff);
    let mismatch = id_mismatch(&dirty, &ff, 1e-6);
    eprintln!("K0 P1b dirty vs forced: faces {} vs {}, ids differing at the same place {mismatch}", dirty.len(), ff.len());
    assert_eq!((dirty.len(), ff.len(), mismatch), (10, 10, 0));
}

/// P1c: the block's height edited 10 -> 14 (the cut stays at z = 5): the upper piece grows, the lower one does not move.
#[test]
fn p1c_block_height_edit() {
    let mut m = block(10.0, 5.0);
    let before = m.d.faces(m.sp);
    m.d.edit(m.a, "dz", "14");
    let s = m.d.sum(Some(m.sp));
    show("P1c height 14", &s);
    let after = m.d.faces(m.sp);
    dump("P1c height 14", &after);
    let sw = swapped("P1c height 10 -> 14", &before, 5.0, &after, 5.0);
    eprintln!("K0 P1c swapped: {sw:?}");
    assert!(s.errors.is_empty() && s.faces == 10, "{s:?}");
    close(s.volume, 40.0 * 30.0 * 14.0, 1e-9, "volume");
    assert!(sw.is_empty(), "the pieces keep their sides: {sw:?}");
}

/// P1d: the U body (a wall face cut into three pieces above the base): which of the three keeps the id, and under an edit.
#[test]
fn p1d_u_body_three_pieces() {
    let mut m = u_body(15.0);
    let s = m.d.sum(Some(m.sp));
    show("P1d U z=15", &s);
    let f0 = m.d.faces(m.sp);
    dump("P1d U z=15", &f0);
    let solid = m.d.faces(m.solid);
    dump("P1d U before the split", &solid);
    let mut prev = (f0, 15.0);
    for (text, off) in [("17", 17.0), ("12", 12.0), ("15", 15.0)] {
        m.d.edit(m.sp, "offset", text);
        let s = m.d.sum(Some(m.sp));
        show(&format!("P1d offset {text}"), &s);
        let f1 = m.d.faces(m.sp);
        let sw = swapped(&format!("P1d {} -> {text}", prev.1), &prev.0, prev.1, &f1, off);
        eprintln!("K0 P1d swapped: {sw:?}");
        assert!(sw.is_empty(), "{sw:?}");
        let fresh = u_body(off);
        let ff = fresh.d.faces(fresh.sp);
        let mismatch = id_mismatch(&f1, &ff, 1e-6);
        eprintln!("K0 P1d vs scratch: faces {} vs {}, ids differing at the same place {mismatch}", f1.len(), ff.len());
        assert_eq!((f1.len(), mismatch), (18, 0));
        prev = (f1, off);
    }
    // The body edited upstream: the block widens in y (the slot tool covers it).
    m.d.edit(m.a, "dy", "34");
    let s = m.d.sum(Some(m.sp));
    show("P1d U after dy 34", &s);
    let f2 = m.d.faces(m.sp);
    let sw = swapped("P1d dy 30 -> 34", &prev.0, 15.0, &f2, 15.0);
    eprintln!("K0 P1d swapped: {sw:?}");
    assert!(s.errors.is_empty() && sw.is_empty(), "{s:?} {sw:?}");
}

/// P1e: the L body, cut at z = 5 and at z = 15 (the y walls are L-shaped faces, cut into two connected pieces).
#[test]
fn p1e_l_body_offsets() {
    for off in [5.0, 15.0] {
        let mut m = l_body(off);
        let s = m.d.sum(Some(m.sp));
        show(&format!("P1e L z={off}"), &s);
        let f0 = m.d.faces(m.sp);
        dump(&format!("P1e L z={off}"), &f0);
        assert!(s.errors.is_empty(), "{s:?}");
        close(s.volume, 18000.0, 1e-9, "volume");
        let to = off - 1.0;
        m.d.edit(m.sp, "offset", &format!("{to}"));
        let f1 = m.d.faces(m.sp);
        let sw = swapped(&format!("P1e L {off} -> {to}"), &f0, off, &f1, to);
        eprintln!("K0 P1e swapped: {sw:?}; faces {} -> {}", f0.len(), f1.len());
        assert!(sw.is_empty() && f1.len() == 12, "{sw:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// P2: an edit that changes the number of faces cut.
// ---------------------------------------------------------------------------------------------------------------------------------

/// P2: the U body cut at z = 5 (below the slot floor: the four outer walls) then at z = 15 (the slot as well): the number of faces
/// changes without an error; what happens to the ids of the neighbours; and a fillet resting on a piece's edge.
#[test]
fn p2_the_number_of_cut_faces_changes_on_an_edit() {
    let mut m = u_body(5.0);
    let s5 = m.d.sum(Some(m.sp));
    show("P2 U z=5", &s5);
    let f5 = m.d.faces(m.sp);
    dump("P2 U z=5", &f5);
    m.d.edit(m.sp, "offset", "15");
    let s15 = m.d.sum(Some(m.sp));
    show("P2 U z=5 -> 15", &s15);
    let f15 = m.d.faces(m.sp);
    dump("P2 U z=15 (edited)", &f15);
    let before_ids: HashSet<u32> = f5.iter().map(|f| f.id).collect();
    let kept = f15.iter().filter(|f| before_ids.contains(&f.id)).count();
    eprintln!("K0 P2 faces {} -> {}, ids kept {}, new {}", f5.len(), f15.len(), kept, f15.len() - kept);
    let sw = swapped("P2 5 -> 15", &f5, 5.0, &f15, 15.0);
    eprintln!("K0 P2 swapped (walls moving over the plane are expected here): {sw:?}");
    let scratch = u_body(15.0);
    let fs = scratch.d.faces(scratch.sp);
    let mismatch = id_mismatch(&f15, &fs, 1e-6);
    eprintln!("K0 P2 ids differing from a build from scratch at z=15: {mismatch}");
    let full = m.d.full_copy();
    eprintln!("K0 P2 ids differing between the edited document and a forced rebuild of it: {}", id_mismatch(&f15, &full.faces(m.sp), 1e-6));
    assert_eq!(kept, 14, "all fourteen old ids stay on the body");
    assert!(s5.errors.is_empty() && s15.errors.is_empty(), "no error on either: {s5:?} {s15:?}");
    assert!(f15.len() > f5.len(), "the edit changed the number of faces: {} -> {}", f5.len(), f15.len());
    close(s15.volume, 18000.0, 1e-9, "volume");
}

/// A push of one face of `src` by 1 mm (the app's push/pull on a face): the id of the face is the whole reference.
fn push(d: &mut Doc, src: Id, f: &F) -> Id {
    let key = FaceKey { index: 0, centroid: f.c, normal: f.n, id: f.id };
    let id = d.p.add_push_face(src, key, 1.0);
    d.build();
    id
}

/// P2b: a push of one piece (the upper U-shaped piece of the wall y = -15 above a cut at z = 5, 400 mm2) rests on the split; the
/// offset is edited 5 -> 15, where that id passes to a tower piece (50 mm2). The volume of the push says which face it moved.
#[test]
fn p2b_a_consumer_of_a_piece_follows_the_id_not_the_piece() {
    let mut m = u_body(5.0);
    let piece = m.d.faces(m.sp).into_iter().find(|f| f.n[1] < -0.9 && f.c[2] > 5.0).expect("the upper piece of the wall y=-15");
    eprintln!("K0 P2b the face pushed: {piece:?}");
    let pu = push(&mut m.d, m.sp, &piece);
    let s = m.d.sum(Some(pu));
    show("P2b push of the upper piece, cut z=5", &s);
    let dv = s.volume.unwrap() - 18000.0;
    eprintln!("K0 P2b volume added by the push: {dv:.4}");
    assert!(s.errors.is_empty(), "{s:?}");
    m.d.edit(m.sp, "offset", "15");
    let s = m.d.sum(Some(pu));
    show("P2b the same push after the offset 5 -> 15", &s);
    let dv2 = s.volume.map(|v| v - 18000.0);
    eprintln!("K0 P2b volume added by the push after the edit: {dv2:?} (the id now stands on a piece of area 50, the old piece had 400)");
    let moved: Vec<F> = m.d.faces(pu).into_iter().filter(|f| f.n[1] < -0.9 && f.c[1] < -15.5).collect();
    eprintln!("K0 P2b faces pushed out of the wall y=-15 after the edit: {moved:?}");
    assert!(s.errors.is_empty(), "no error: {s:?}");
    assert!((dv - 400.0).abs() < 1e-6 && dv2.is_some_and(|v| (v - 50.0).abs() < 1e-6), "400 then 50: {dv} {dv2:?}");
}

/// P2c: the L body, cut at z = 5 and edited to z = 15 (the step): the wall x = 20 stops being cut and the step wall x = 0 starts to be.
/// A push on the upper piece of the wall x = 20 (z 5..10) rests on that id.
#[test]
fn p2c_the_cut_wall_changes_over_the_step() {
    let mut m = l_body(5.0);
    let f5 = m.d.faces(m.sp);
    let piece = f5.iter().find(|f| f.n[0] > 0.9 && f.c[2] > 5.0 && f.c[0] > 19.0).expect("the upper piece of x=20").clone();
    eprintln!("K0 P2c the face pushed: {piece:?}");
    let pu = push(&mut m.d, m.sp, &piece);
    let s = m.d.sum(Some(pu));
    show("P2c push on the upper piece of x=20, cut z=5", &s);
    let before = s.volume.unwrap() - 18000.0;
    eprintln!("K0 P2c volume added: {before:.4}");
    m.d.edit(m.sp, "offset", "15");
    let s = m.d.sum(Some(pu));
    show("P2c after the offset 5 -> 15", &s);
    let f15 = m.d.faces(m.sp);
    eprintln!(
        "K0 P2c faces split {} -> {}; the id {} on the cut body: {:?}",
        f5.len(),
        f15.len(),
        piece.id,
        f15.iter().find(|f| f.id == piece.id)
    );
    let after = s.volume.map(|v| v - 18000.0);
    eprintln!("K0 P2c volume added after the edit: {after:?}");
    assert!(s.errors.is_empty() && f15.iter().all(|f| f.id != piece.id), "{s:?}");
    assert!(
        (before - 150.0).abs() < 1e-6 && after.is_some_and(|v| (v - 300.0).abs() < 1e-6),
        "150 then the whole wall, 300: {before} {after:?}"
    );
}

// ---------------------------------------------------------------------------------------------------------------------------------
// P3: touching planes.
// ---------------------------------------------------------------------------------------------------------------------------------

/// How a split of `src` came out: refused (no body, or the red node), no division (no face added), split, or a broken body.
fn verdict(before: usize, s: &S) -> &'static str {
    if s.volume.is_none() || !s.errors.is_empty() {
        "REFUSED (visible)"
    } else if s.valid == Some(false) {
        "BROKEN body"
    } else if s.faces == before {
        "SUCCESS WITHOUT DIVISION"
    } else {
        "split"
    }
}

/// Split `a` (one primitive node) by `plane` at `off`; print the verdict, and what the app's pre-check says of the unsplit body.
fn touch(label: &str, mut d: Doc, a: Id, plane: u8, off: f64) -> (String, S) {
    d.build();
    let before = d.faces(a).len();
    let normal = match plane {
        1 => [0.0, 1.0, 0.0],
        2 => [1.0, 0.0, 0.0],
        _ => [0.0, 0.0, 1.0],
    };
    let ask = d.asks(a, [normal[0] * off, normal[1] * off, normal[2] * off], normal);
    let sp = d.split(a, plane, off);
    d.build();
    let b = d.last().filter(|b| *b == sp).or(Some(sp));
    let s = d.sum(b);
    let v = verdict(before, &s);
    eprintln!(
        "K0 P3 {label}: GUI pre-check says {}; node: {v}; faces {before} -> {}; ids distinct {}; volume {:?}; valid {:?}; errors {:?}",
        if ask { "divides" } else { "nothing (the GUI refuses)" },
        s.faces,
        s.distinct_ids,
        s.volume,
        s.valid,
        s.errors
    );
    (v.to_string(), s)
}

fn box_doc() -> (Doc, Id) {
    let mut d = Doc::new();
    let a = d.p.add_box(40.0, 30.0, 10.0);
    (d, a)
}

/// P3a: the plane on the block's own faces, and a hair away from them.
#[test]
fn p3a_block_top_and_bottom_face_planes() {
    let mut verdicts = Vec::new();
    for off in [10.0, 10.0 - 1e-6, 10.0 - 1e-7, 10.0 - 1e-9, 10.0 + 1e-9, 10.0 + 1e-7, 0.0, 1e-9, 1e-7, 1e-6] {
        let (d, a) = box_doc();
        let (v, s) = touch(&format!("block z={off:e}"), d, a, 0, off);
        verdicts.push((off, v, s.faces));
    }
    eprintln!("K0 P3a summary {verdicts:?}");
    // The plane that is the top face: refused (as the acceptance matrix has it).
    assert!(verdicts[0].1.starts_with("REFUSED"), "z = top face: {:?}", verdicts[0]);
    for (i, want) in
        ["REFUSED", "SUCCESS", "REFUSED", "REFUSED", "REFUSED", "REFUSED", "REFUSED", "REFUSED", "REFUSED", "SUCCESS"].iter().enumerate()
    {
        assert!(verdicts[i].1.starts_with(want), "offset {:e}: {:?}", verdicts[i].0, verdicts[i]);
    }
}

/// P3b: the plane through an edge of a block only (a cube turned 45 degrees about z, the plane x = 10 sqrt 2 touches a vertical edge).
#[test]
fn p3b_plane_through_an_edge_of_a_block() {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let edge = 10.0 * std::f64::consts::SQRT_2;
    for d_off in [0.0, -1e-9, -1e-7, -1e-6, 1e-9] {
        let mut d = Doc::new();
        let a = d.p.add_box(20.0, 20.0, 10.0);
        let mut m = PLACE_IDENTITY;
        (m[0], m[1], m[4], m[5]) = (h, -h, h, h);
        let turned = d.p.add_move(a, m);
        let (v, _) = touch(&format!("cube turned 45deg, x = 10*sqrt2 {d_off:+e}"), d, turned, 2, edge + d_off);
        eprintln!("K0 P3b {d_off:+e}: {v}");
        let want = if d_off == -1e-6 { "SUCCESS" } else { "REFUSED" };
        assert!(v.starts_with(want), "{d_off:+e}: {v}");
    }
}

/// P3c: the plane tangent to a cylinder (the line of the tangent, x = r), to its cap, and to a sphere.
#[test]
fn p3c_tangent_planes_to_curved_bodies() {
    for d_off in [0.0, -1e-6, -1e-7, -1e-9, 1e-9] {
        let mut d = Doc::new();
        let a = d.p.add_cylinder(10.0, 20.0);
        let (v, s) = touch(&format!("cylinder r=10, plane x = 10 {d_off:+e} (a line of tangency)"), d, a, 2, 10.0 + d_off);
        if d_off == -1e-6 {
            assert!(v == "split" && s.faces == 7, "{v} {s:?}");
        } else {
            assert!(v.starts_with("REFUSED"), "{d_off:+e}: {v}");
        }
    }
    for d_off in [0.0, -1e-7] {
        let mut d = Doc::new();
        let a = d.p.add_cylinder(10.0, 20.0);
        touch(&format!("cylinder h=20, plane z = 20 {d_off:+e} (the cap)"), d, a, 0, 20.0 + d_off);
    }
    for d_off in [0.0, -1e-6, -1e-9] {
        let mut d = Doc::new();
        let a = d.p.add_sphere(10.0);
        let (v, s) = touch(&format!("sphere r=10, plane x = 10 {d_off:+e} (a point of tangency)"), d, a, 2, 10.0 + d_off);
        if d_off == 0.0 {
            assert!(v.starts_with("REFUSED"), "{v}");
        } else {
            assert!(v == "split" && s.faces == 3, "{v} {s:?}");
        }
    }
    let mut d = Doc::new();
    let a = d.p.add_sphere(10.0);
    touch("sphere r=10, plane z = 0 (the equator)", d, a, 0, 0.0);
}

/// P3d: the plane 1e-6 under the top face: the app's pre-check says it divides, the node is green, the face count did not grow and the
/// volume dropped. What the kernel body really holds.
#[test]
fn p3d_the_plane_a_micron_under_a_face() {
    for off in
        [10.0 - 1e-6, 10.0 - 5e-7, 10.0 - 2e-7, 10.0 - 1.5e-7, 1e-6, 5e-7, 10.0 - 2e-6, 10.0 - 5e-6, 10.0 - 1e-5, 10.0 - 1e-4, 10.0 - 1e-3]
    {
        let (mut d, a) = box_doc();
        d.build();
        let sp = d.split(a, 0, off);
        d.build();
        let s = d.sum(Some(sp));
        let (kinds, sliver, bbox) = {
            let sh = d.shapes.get(&sp).expect("shape");
            let _g = qymcad_kernel::kernel_gate();
            (sh.face_kinds().map(|k| k.iter().sum::<u32>()), sh.sliver_faces(), sh.bbox())
        };
        eprintln!(
            "K0 P3d z={off}: listed faces {} ; kernel faces {kinds:?} ; sliver faces {sliver} ; volume {:?} (12000 exact, lost {:?}) ; valid {:?} ; errors {:?} ; bbox {bbox:?}",
            s.faces,
            s.volume,
            s.volume.map(|v| 12000.0 - v),
            s.valid,
            s.errors
        );
        if (1e-6 - 1e-12..=1e-5 + 1e-12).contains(&(10.0 - off)) {
            assert!(s.errors.is_empty() && s.faces == 6, "success without a division: {s:?}");
            assert!(s.volume.unwrap() < 12000.0 - 1e-4, "and the volume shrank: {s:?}");
        }
        if (10.0 - off - 1e-4).abs() < 1e-9 || (10.0 - off - 1e-3).abs() < 1e-9 {
            assert!(s.errors.is_empty() && s.faces == 10, "a division: {s:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// P5: curved faces.
// ---------------------------------------------------------------------------------------------------------------------------------

/// P5: a plane over curved faces: the number of faces, the ids, the volume. The counts are printed; the volume and the validity
/// are asserted wherever the node built.
#[test]
fn p5_curved_faces() {
    type Maker = fn(&mut Project) -> Id;
    let cases: Vec<(&str, Maker, u8, f64, f64)> = vec![
        ("cylinder r10 h20 across z=10", |p| p.add_cylinder(10.0, 20.0), 0, 10.0, PI_R2_H),
        ("cylinder through the axis y=0", |p| p.add_cylinder(10.0, 20.0), 1, 0.0, PI_R2_H),
        ("cylinder through the axis x=0", |p| p.add_cylinder(10.0, 20.0), 2, 0.0, PI_R2_H),
        ("cylinder off the axis x=3 (a chord)", |p| p.add_cylinder(10.0, 20.0), 2, 3.0, PI_R2_H),
        ("sphere r10 equator z=0", |p| p.add_sphere(10.0), 0, 0.0, 4.0 / 3.0 * std::f64::consts::PI * 1000.0),
        ("sphere r10 cap z=5", |p| p.add_sphere(10.0), 0, 5.0, 4.0 / 3.0 * std::f64::consts::PI * 1000.0),
        ("sphere r10 meridian x=0", |p| p.add_sphere(10.0), 2, 0.0, 4.0 / 3.0 * std::f64::consts::PI * 1000.0),
        ("cone 10/4 h20 across z=10", |p| p.add_cone(10.0, 4.0, 20.0), 0, 10.0, CONE),
        ("cone through the axis y=0", |p| p.add_cone(10.0, 4.0, 20.0), 1, 0.0, CONE),
        ("torus 12/4 equator z=0", |p| p.add_torus(12.0, 4.0), 0, 0.0, TORUS),
        ("torus 12/4 z=2", |p| p.add_torus(12.0, 4.0), 0, 2.0, TORUS),
        ("torus 12/4 meridian y=0", |p| p.add_torus(12.0, 4.0), 1, 0.0, TORUS),
        ("torus 12/4 plane x=12 (through the tube centre)", |p| p.add_torus(12.0, 4.0), 2, 12.0, TORUS),
    ];
    for (label, make, plane, off, volume) in cases {
        let mut d = Doc::new();
        let a = make(&mut d.p);
        d.build();
        let before = d.sum(Some(a));
        let sp = d.split(a, plane, off);
        d.build();
        let s = d.sum(Some(sp));
        let after = d.faces(sp);
        eprintln!(
            "K0 P5 {label}: faces {} -> {} ({}), ids distinct {} zero {}, volume {:?} (want {volume:.4}), valid {:?}, solids {}/{} shells, errors {:?}",
            before.faces,
            s.faces,
            verdict(before.faces, &s),
            s.distinct_ids,
            s.zero_ids,
            s.volume,
            s.valid,
            s.solids,
            s.shells,
            s.errors
        );
        if s.faces != s.distinct_ids {
            dump(&format!("P5 {label} (duplicate ids)"), &after);
        }
        if label.starts_with("torus 12/4 meridian") {
            assert!(s.errors.is_empty() && s.faces == 1, "{label}: success without a division: {s:?}");
        }
        if label.starts_with("sphere r10 meridian") {
            assert!(red(&s.errors, "SplitFaces"), "{label}: {s:?}");
        }
        if s.volume.is_some() && s.errors.is_empty() {
            close(s.volume, volume, 1e-6, label);
            assert_eq!(s.valid, Some(true), "{label}");
        }
    }
}

const PI_R2_H: f64 = std::f64::consts::PI * 100.0 * 20.0;
/// The frustum r 10 -> 4, h 20.
const CONE: f64 = std::f64::consts::PI * 20.0 / 3.0 * (100.0 + 40.0 + 16.0);
/// The torus 12/4.
const TORUS: f64 = 2.0 * std::f64::consts::PI * std::f64::consts::PI * 12.0 * 16.0;

// ---------------------------------------------------------------------------------------------------------------------------------
// P6: the plane of a curved face.
// ---------------------------------------------------------------------------------------------------------------------------------

/// P6: a split whose plane is a face that is not a plane: what the app builds. The key is made as the GUI makes it (the centre and
/// the normal the face is listed with).
#[test]
fn p6_the_plane_of_a_face_that_is_not_flat() {
    type Maker = fn(&mut Project) -> Id;
    let cases: Vec<(&str, Maker)> = vec![
        ("cylinder side", |p| p.add_cylinder(10.0, 20.0)),
        ("sphere", |p| p.add_sphere(10.0)),
        ("cone side", |p| p.add_cone(10.0, 4.0, 20.0)),
        ("torus", |p| p.add_torus(12.0, 4.0)),
    ];
    for (label, make) in cases {
        let mut d = Doc::new();
        let a = make(&mut d.p);
        d.build();
        let faces = d.faces(a);
        let curved = faces.iter().max_by(|x, y| x.area.partial_cmp(&y.area).unwrap()).unwrap().clone();
        eprintln!("K0 P6 {label}: faces {} ; the curved face as listed: {curved:?}", faces.len());
        for off in [0.0, 2.0] {
            let mut d = Doc::new();
            let a = make(&mut d.p);
            d.build();
            let sp = d.split_face_plane(a, a, &curved, off);
            d.build();
            let b = Some(sp);
            let s = d.sum(b);
            let lv = d.levels(sp);
            eprintln!(
                "K0 P6 {label} offset {off}: {} ; faces {} -> {}, volume {:?}, valid {:?}, errors {:?}, horizontal-edge levels {lv:?}",
                verdict(faces.len(), &s),
                faces.len(),
                s.faces,
                s.volume,
                s.valid,
                s.errors
            );
            assert!(s.errors.is_empty(), "{label}: the app builds something: {s:?}");
            match label {
                "cylinder side" | "cone side" => assert_eq!(s.faces, 7, "{label}"),
                "torus" => assert_eq!(s.faces, 1, "{label}: no division"),
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// P8: the face that gave the plane vanishes.
// ---------------------------------------------------------------------------------------------------------------------------------

/// P8: a plane by the top face (z = 10) of the lower of two boxes of one body, offset -5; then the upper box is moved down onto the
/// lower one, so the union swallows that face. Red node, or green with the plane where the face was (the creation fingerprint).
#[test]
fn p8_the_face_that_gave_the_plane_is_gone() {
    let mut d = Doc::new();
    let a = d.p.add_box(40.0, 30.0, 10.0);
    let b = d.p.add_box(40.0, 30.0, 10.0);
    let mv = shift(&mut d.p, b, 0.0, 0.0, 12.0);
    let u = d.p.add_body_boolean(a, mv, 1);
    d.build();
    let top = d.faces(u).into_iter().find(|f| f.n[2] > 0.9 && (f.c[2] - 10.0).abs() < 1e-6).expect("the lower box's top");
    eprintln!("K0 P8 the face giving the plane: {top:?}");
    let sp = d.split_face_plane(u, u, &top, -5.0);
    d.build();
    let s0 = d.sum(Some(sp));
    show("P8 before", &s0);
    eprintln!("K0 P8 before: cut levels {:?}", d.levels(sp));
    assert!(s0.errors.is_empty(), "{s0:?}");
    // The upper box moves down by 4: the union is one solid and the old top face is inside it.
    d.edit(mv, "dz", "6");
    let faces_u = d.faces(u);
    eprintln!("K0 P8 after: is the face id still on the union body? {}", faces_u.iter().any(|f| f.id == top.id));
    let s1 = d.sum(Some(sp));
    show("P8 after the face was swallowed", &s1);
    eprintln!("K0 P8 after: cut levels {:?}; errors on the node {:?}", d.levels(sp), d.errors);
    dump("P8 after", &d.faces(sp));
    assert!(s1.errors.is_empty(), "the node stays green: {s1:?}");
    assert_eq!(d.levels(sp), vec![0.0, 5.0, 16.0], "and the plane stands where it was made (z = 5)");
    assert!(faces_u.iter().all(|f| f.id != top.id), "the face is gone");
    // Control: the face stays, it only moves (the box height 10 -> 14): the plane follows it.
    let mut c = block(10.0, 5.0);
    c.d.edit(c.a, "dz", "14");
    let mut d2 = Doc::new();
    let a2 = d2.p.add_box(40.0, 30.0, 10.0);
    d2.build();
    let top2 = d2.faces(a2).into_iter().find(|f| f.n[2] > 0.9).unwrap();
    let sp2 = d2.split_face_plane(a2, a2, &top2, -5.0);
    d2.build();
    eprintln!("K0 P8 control before: levels {:?}", d2.levels(sp2));
    d2.edit(a2, "dz", "14");
    eprintln!(
        "K0 P8 control, the height 10 -> 14: levels {:?}, errors {:?} (the plane follows the face when its id lives)",
        d2.levels(sp2),
        d2.errors
    );
    let _ = c;
}

// ---------------------------------------------------------------------------------------------------------------------------------
// P10: a Part with a placement.
// ---------------------------------------------------------------------------------------------------------------------------------

/// P10: the Part placed at z = +100 (and turned): the base plane XY with offset 5 cuts in the body's local frame; the GUI's pre-check
/// converts the plane from the world, which can disagree with that when an assembly is the active context.
#[test]
fn p10_part_with_a_placement() {
    for (label, t) in [
        ("shift z +100", {
            let mut m = PLACE_IDENTITY;
            m[11] = 100.0;
            m
        }),
        ("turn 90 deg about x, shift z +100", {
            let mut m = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 100.0];
            m[3] = 0.0;
            m
        }),
    ] {
        let mut d = Doc::new();
        let part = d.p.components.iter().map(|c| c.id).next_back().expect("the part");
        let a = d.p.add_box(40.0, 30.0, 10.0);
        d.p.set_component_transform(part, t);
        let sp = d.split(a, 0, 5.0);
        d.build();
        let s = d.sum(Some(sp));
        eprintln!(
            "K0 P10 {label}: node {} ; faces {} ; errors {:?} ; local horizontal-edge levels {:?}",
            verdict(6, &s),
            s.faces,
            s.errors,
            d.levels(sp)
        );
        let local_cut = d.levels(sp).contains(&5.0);
        eprintln!("K0 P10 {label}: the cut stands at local z = 5: {local_cut}");
        // What the GUI's pre-check (split_plane_local) sees with the assembly active: the world plane z = 5 taken into the part's frame.
        let inv = mat_inv12(&t);
        for world_off in [5.0, 105.0] {
            let o = apply12(&inv, [0.0, 0.0, world_off]);
            let n = apply12_dir(&inv, [0.0, 0.0, 1.0]);
            eprintln!(
                "K0 P10 {label}: GUI pre-check for the world plane z={world_off}: local origin {o:?} normal {n:?} -> {}",
                if d.asks(a, o, n) { "divides" } else { "refuses" }
            );
        }
        assert!(s.errors.is_empty() && local_cut, "{s:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// P11: suppress / unsuppress the split under a consumer.
// ---------------------------------------------------------------------------------------------------------------------------------

/// P11: a push of one piece of the wall x = 20 (the upper piece, an id born in the split; the lower piece keeps the wall's old id),
/// then the split suppressed: the push is red, found by place, or silently on another face. Then the split switched back on.
#[test]
fn p11_suppress_the_split_under_a_consumer() {
    for (label, upper) in [("upper piece (fresh id)", true), ("lower piece (the wall's own id)", false)] {
        let mut m = block(10.0, 4.0);
        let piece =
            m.d.faces(m.sp).into_iter().find(|f| f.n[0] > 0.9 && if upper { f.c[2] > 4.0 } else { f.c[2] < 4.0 }).expect("piece of x=20");
        let pu = push(&mut m.d, m.sp, &piece);
        let s = m.d.sum(Some(pu));
        let added = s.volume.unwrap() - 12000.0;
        eprintln!("K0 P11 {label}: piece {piece:?}; push adds {added:.3} (the piece has {:.0} mm2); errors {:?}", piece.area, s.errors);
        assert!(s.errors.is_empty(), "{s:?}");
        let ti = m.d.p.timeline.iter().position(|n| n.id == m.sp).expect("the split node");
        assert!(m.d.p.set_feature_suppressed(ti, true));
        m.d.build();
        let last = m.d.last();
        let s = m.d.sum(last);
        eprintln!(
            "K0 P11 {label}, split suppressed: last body is the push {}; faces {}; volume {:?} (adds {:?}; the whole wall x=20 is 300 mm2); errors {:?}",
            last == Some(pu),
            s.faces,
            s.volume,
            s.volume.map(|v| v - 12000.0),
            s.errors
        );
        assert!(s.errors.is_empty() && s.volume.is_some_and(|v| (v - 12300.0).abs() < 1e-6), "found by place, the whole wall: {s:?}");
        assert!(!m.d.p.set_feature_suppressed(ti, false));
        m.d.build();
        let s = m.d.sum(Some(pu));
        eprintln!(
            "K0 P11 {label}, split switched back on: faces {}; adds {:?}; errors {:?}",
            s.faces,
            s.volume.map(|v| v - 12000.0),
            s.errors
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// P14: two splits one after another.
// ---------------------------------------------------------------------------------------------------------------------------------

/// P14: z = 3 then z = 6 on the already cut walls, then z = 3 again on the first seam: ids unique, counts, errors.
#[test]
fn p14_two_splits_in_a_row() {
    let mut d = Doc::new();
    let a = d.p.add_box(40.0, 30.0, 10.0);
    let s1 = d.split(a, 0, 3.0);
    d.build();
    let r1 = d.sum(Some(s1));
    show("P14 z=3", &r1);
    let s2 = d.split(s1, 0, 6.0);
    d.build();
    let r2 = d.sum(Some(s2));
    show("P14 z=3 then z=6", &r2);
    let f2 = d.faces(s2);
    dump("P14 z=3 then z=6", &f2);
    assert!(r1.errors.is_empty() && r2.errors.is_empty(), "{r1:?} {r2:?}");
    assert_eq!((r1.faces, r2.faces), (10, 14));
    assert_eq!((r1.distinct_ids, r2.distinct_ids), (10, 14), "ids unique after each split");
    close(r2.volume, 12000.0, 1e-9, "volume");
    // The plane by the seam of the first: z = 3 once more.
    let s3 = d.split(s2, 0, 3.0);
    d.build();
    let r3 = d.sum(Some(s3));
    show("P14 then z=3 again (on the first seam)", &r3);
    eprintln!("K0 P14 third split: {}", verdict(r2.faces, &r3));
    // The plane by a face of the first piece: the lower wall's face taken as the plane (a wall, x = 20) is not the seam; the top face
    // at offset -7 is z = 3 again.
    let top = f2.iter().find(|f| f.n[2] > 0.9).unwrap().clone();
    let s4 = d.split_face_plane(s3, s3, &top, -7.0);
    d.build();
    let r4 = d.sum(Some(s4));
    show("P14 top face at -7 (z=3 through the seam)", &r4);
    // Editing the first offset under the second.
    let mut e = Doc::new();
    let a = e.p.add_box(40.0, 30.0, 10.0);
    let s1 = e.split(a, 0, 3.0);
    let s2 = e.split(s1, 0, 6.0);
    e.build();
    let before = e.faces(s2);
    e.edit(s1, "offset", "4");
    let r = e.sum(Some(s2));
    show("P14 first offset 3 -> 4 under the second", &r);
    let after = e.faces(s2);
    let sw = swapped("P14 first 3 -> 4", &before, 3.0, &after, 4.0);
    eprintln!("K0 P14 swapped (against the first plane): {sw:?}");
    // Edit it past the second (4 -> 8): the order of the planes flips.
    e.edit(s1, "offset", "8");
    let r = e.sum(Some(s2));
    show("P14 first offset -> 8 (past the second, z=6), dirty rebuild", &r);
    let dup = e.faces(s2);
    dump("P14 first offset 8, second 6", &dup);
    let forced = e.full_copy();
    let rf = forced.sum(Some(s2));
    show("P14 the same document rebuilt in full", &rf);
    // The same planes built from scratch.
    let mut f = Doc::new();
    let a = f.p.add_box(40.0, 30.0, 10.0);
    let t1 = f.split(a, 0, 8.0);
    let t2 = f.split(t1, 0, 6.0);
    f.build();
    let rs = f.sum(Some(t2));
    show("P14 z=8 then z=6 built from scratch", &rs);
    assert!(r.errors.is_empty(), "{r:?}");
}

/// P1f: which piece of a cut wall keeps the old id, by the side of the plane: the base planes of all three axes, and the plane of the
/// bottom face (its normal points down, so a positive offset goes the other way).
#[test]
fn p1f_which_piece_keeps_the_old_id() {
    let cases: Vec<(&str, u8, bool, f64, usize)> = vec![
        ("XY z=5", 0, false, 5.0, 2),
        ("XZ y=5", 1, false, 5.0, 1),
        ("YZ x=5", 2, false, 5.0, 0),
        ("bottom face (normal -z), offset -5 (z=5)", 0, true, -5.0, 2),
    ];
    for (label, plane, from_face, off, axis) in cases {
        let (mut d, a) = box_doc();
        d.build();
        let before = d.faces(a);
        let sp = if from_face {
            let bottom = before.iter().find(|f| f.n[2] < -0.9).unwrap().clone();
            d.split_face_plane(a, a, &bottom, off)
        } else {
            d.split(a, plane, off)
        };
        d.build();
        let after = d.faces(sp);
        let level = if from_face { 5.0 } else { off };
        let mut kept = Vec::new();
        for fb in &before {
            if let Some(fa) = after.iter().find(|f| f.id == fb.id).filter(|f| f.area < fb.area - 1e-6) {
                kept.push(format!(
                    "id {} ({:.0} -> {:.0} mm2) {}",
                    fb.id,
                    fb.area,
                    fa.area,
                    if fa.c[axis] < level { "below" } else { "above" }
                ));
            }
        }
        eprintln!("K0 P1f {label}: the pieces that kept an old id sit: {kept:?}");
        assert!(!kept.is_empty(), "{label}: a piece keeps the old id");
    }
}

/// P1g: the side that keeps the id, on a block, for each base plane over a range of offsets (cut walls only), and across an edit of
/// the offset on the dirty path: whether the kept side flips with the offset.
#[test]
fn p1g_does_the_keeper_side_flip_with_the_offset() {
    for (name, plane, axis, offsets) in [
        ("XY", 0u8, 2usize, [-4.0, -1.0, 0.0, 1.0, 4.0]),
        ("XZ", 1, 1, [-14.0, -5.0, 0.0, 5.0, 14.0]),
        ("YZ", 2, 0, [-19.0, -5.0, 0.0, 5.0, 19.0]),
    ] {
        // XY cuts the block 40x30x10 around its mid-height: shift the offsets to 5 + o.
        let mut sides = Vec::new();
        for o in offsets {
            let off = if plane == 0 { 5.0 + o } else { o };
            let (mut d, a) = box_doc();
            d.build();
            let before = d.faces(a);
            let sp = d.split(a, plane, off);
            d.build();
            let after = d.faces(sp);
            let mut below = 0;
            let mut above = 0;
            for fb in &before {
                if let Some(fa) = after.iter().find(|f| f.id == fb.id).filter(|f| f.area < fb.area - 1e-6) {
                    if fa.c[axis] < off {
                        below += 1;
                    } else {
                        above += 1;
                    }
                }
            }
            sides.push((off, below, above));
        }
        eprintln!("K0 P1g {name}: (offset, kept-below, kept-above) {sides:?}");
        let mut edit = {
            let (mut d, a) = box_doc();
            d.build();
            let sp = d.split(a, plane, if plane == 0 { 5.0 } else { 1.0 });
            d.build();
            (d, sp)
        };
        let before = edit.0.faces(edit.1);
        for o in offsets {
            let off = if plane == 0 { 5.0 + o } else { o };
            edit.0.edit(edit.1, "offset", &format!("{off}"));
        }
        let after = edit.0.faces(edit.1);
        eprintln!(
            "K0 P1g {name}: after a run of edits the ids {} the same (before {} faces, after {})",
            if before.iter().all(|b| after.iter().any(|a| a.id == b.id)) { "are" } else { "are NOT all" },
            before.len(),
            after.len()
        );
    }
}

/// P14b: two splits in a row, the second plane on either side of the first.
#[test]
fn p14b_second_plane_below_or_above_the_first() {
    for (z1, z2) in [(3.0, 6.0), (6.0, 3.0), (8.0, 6.0), (5.0, 3.0), (5.0, 7.0), (2.0, 9.0), (9.0, 2.0)] {
        let mut d = Doc::new();
        let a = d.p.add_box(40.0, 30.0, 10.0);
        let s1 = d.split(a, 0, z1);
        let s2 = d.split(s1, 0, z2);
        d.build();
        let r = d.sum(Some(s2));
        eprintln!(
            "K0 P14b z={z1} then z={z2}: faces {} distinct ids {} errors {:?} volume {:?} valid {:?}",
            r.faces, r.distinct_ids, r.errors, r.volume, r.valid
        );
        assert!(r.errors.is_empty() && r.faces == 14, "{r:?}");
        let dup = z2 < z1;
        assert_eq!(r.distinct_ids, if dup { 10 } else { 14 }, "z={z1} then z={z2}: the second plane below the first repeats four ids");
    }
}

/// P14c: two splits in a row by planes of different directions (a cross cut), and by two parallel planes of the other axes: ids repeat
/// for every crossing pair and for parallel planes in the wrong order.
#[test]
fn p14c_the_second_plane_of_another_direction() {
    // (plane code 0 XY / 1 XZ / 2 YZ, offset): the block stands x -20..20, y -15..15, z 0..10.
    let cases = [
        ((0, 3.0), (1, 0.0)),
        ((0, 6.0), (2, 0.0)),
        ((1, 0.0), (0, 5.0)),
        ((2, 0.0), (1, 5.0)),
        ((1, 0.0), (1, 5.0)),
        ((1, 5.0), (1, 0.0)),
        ((2, 0.0), (2, -7.0)),
        ((2, -7.0), (2, 0.0)),
    ];
    for ((c1, o1), (c2, o2)) in cases {
        let mut d = Doc::new();
        let a = d.p.add_box(40.0, 30.0, 10.0);
        let s1 = d.split(a, c1, o1);
        let s2 = d.split(s1, c2, o2);
        d.build();
        let r = d.sum(Some(s2));
        eprintln!(
            "K0 P14c plane {c1} at {o1} then plane {c2} at {o2}: faces {} distinct ids {} errors {:?} volume {:?}",
            r.faces, r.distinct_ids, r.errors, r.volume
        );
        assert!(r.errors.is_empty() && r.valid == Some(true), "{r:?}");
        close(r.volume, 12000.0, 1e-6, "volume");
        // Parallel planes are clean only in the order in which the app keeps the first piece's id on the near side: rising along z and y,
        // falling along x (P1: YZ keeps the higher piece). Every crossing pair repeats ids.
        let clean = c1 == c2 && if c2 == 2 { o2 < o1 } else { o2 > o1 };
        assert_eq!(r.distinct_ids == r.faces, clean, "{r:?}");
    }
}

/// P14d: three and more parallel splits in a row, per axis, rising and falling: how many are clean before the first repeat.
#[test]
fn p14d_how_many_parallel_splits_stay_clean() {
    let axes: [(u8, [f64; 6], [f64; 6]); 3] = [
        (0, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0], [9.0, 8.0, 7.0, 6.0, 5.0, 4.0]),
        (1, [-12.0, -6.0, 0.0, 6.0, 12.0, 13.0], [12.0, 6.0, 0.0, -6.0, -12.0, -13.0]),
        (2, [-15.0, -8.0, 0.0, 8.0, 15.0, 17.0], [15.0, 8.0, 0.0, -8.0, -15.0, -17.0]),
    ];
    for (code, up, down) in axes {
        for (label, offsets) in [("rising", up), ("falling", down)] {
            let mut d = Doc::new();
            let mut src = d.p.add_box(40.0, 30.0, 10.0);
            let mut clean = 0;
            for off in offsets {
                src = d.split(src, code, off);
                d.build();
                let r = d.sum(Some(src));
                if r.errors.is_empty() && r.distinct_ids == r.faces {
                    clean += 1;
                } else {
                    break;
                }
            }
            eprintln!("K0 P14d axis {code} {label}: {clean} clean splits in a row");
            let want = match (code, label) {
                (0, "rising") | (1, "rising") => 2,
                (2, "falling") => 4,
                _ => 1,
            };
            assert_eq!(clean, want, "axis {code} {label}");
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// P19: save and open.
// ---------------------------------------------------------------------------------------------------------------------------------

fn p19_docs() -> Vec<(&'static str, M)> {
    let mut edited = u_body(5.0);
    edited.d.edit(edited.sp, "offset", "15");
    let mut two = {
        let mut d = Doc::new();
        let a = d.p.add_box(40.0, 30.0, 10.0);
        let s1 = d.split(a, 0, 3.0);
        let s2 = d.split(s1, 0, 6.0);
        d.build();
        M { d, a, solid: s1, sp: s2 }
    };
    two.d.edit(two.solid, "offset", "8");
    vec![
        ("block z=5", block(10.0, 5.0)),
        ("U body z=15", u_body(15.0)),
        ("U body z=5 edited to z=15", edited),
        ("two splits, the first edited past the second", two),
    ]
}

/// P19: save, open by the server's `Session::open`: the faces and ids come back, the warnings.
#[test]
fn p19_save_then_open() {
    use qymcad_engine::Session;
    for (label, m) in p19_docs() {
        let before = m.d.faces(m.sp);
        let breps: Vec<(Id, Vec<u8>)> = {
            let _g = qymcad_kernel::kernel_gate();
            m.d.shapes.iter().filter_map(|(id, sh)| sh.to_brep_bytes().map(|b| (*id, b))).collect()
        };
        let path = common::scratch(&format!("k0_p19_{}.qcad", label.replace([' ', '='], "_")));
        qymcad_io::save_project_guarded_with_brep(&m.d.p, path.to_str().unwrap(), &breps).unwrap();
        let (mut s, report) = Session::open(&path).unwrap();
        let after: Vec<F> = {
            // The file with repeated ids (case C) was open-able when the probe was written; the server's guard (F-189) now refuses the
            // full rebuild that reading its topology needs, since the file stores no faces.
            let topo = match s.topology(Some(m.sp), false) {
                Ok(topo) => topo,
                Err(e) => {
                    assert!(label.starts_with("two splits") && e.to_string().contains("F-189"), "{label}: {e}");
                    eprintln!("K0 P19 {label}: topology refused by the server's guard: {e}");
                    continue;
                }
            };
            let mut v: Vec<F> =
                topo.faces.iter().map(|f| F { id: f.id, c: f.centroid, n: f.normal.unwrap_or([0.0; 3]), area: f.area }).collect();
            v.sort_by_key(|f| f.id);
            eprintln!("K0 P19 {label}: topology warnings {:?}", topo.warnings);
            v
        };
        let same_ids = before.len() == after.len() && before.iter().zip(&after).all(|(a, b)| a.id == b.id && near(a.c, b.c, 1e-6));
        eprintln!(
            "K0 P19 {label}: faces {} -> {}; same ids at the same places: {same_ids}; open errors {:?}; warnings {:?}",
            before.len(),
            after.len(),
            report.errors,
            report.warnings.iter().map(|w| w.message.clone()).collect::<Vec<_>>()
        );
        assert!(report.errors.is_empty(), "{label}: {:?}", report.errors);
        assert!(same_ids, "{label}: the ids and the faces come back as saved");
        assert!(report.warnings.is_empty(), "{label}: {:?}", report.warnings);
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// The rest of section 8 that was quick.
// ---------------------------------------------------------------------------------------------------------------------------------

/// P4: the plane on an inner face (the step of the L body at z = 10) that also cuts other faces.
#[test]
fn p4_plane_on_an_inner_face() {
    let m = l_body(10.0);
    let before = m.d.faces(m.solid).len();
    let s = m.d.sum(Some(m.sp));
    eprintln!(
        "K0 P4 L step z=10: {} ; faces {before} -> {}; ids distinct {}; errors {:?}; volume {:?}; valid {:?}",
        verdict(before, &s),
        s.faces,
        s.distinct_ids,
        s.errors,
        s.volume,
        s.valid
    );
    dump("P4 L z=10", &m.d.faces(m.sp));
}

/// P7/P13: offsets that miss the body or are not numbers, and what the red node leaves (the pass-through copy).
#[test]
fn p7_offsets_that_miss_or_are_not_finite() {
    for off in ["50", "-50", "100000", "1000000", "1000000000000"] {
        let mut m = block(10.0, 5.0);
        m.d.edit(m.sp, "offset", off);
        let last = m.d.last();
        let s = m.d.sum(last);
        eprintln!(
            "K0 P7 offset {off}: last body is the split node {}; faces {}; volume {:?}; errors {:?}",
            last == Some(m.sp),
            s.faces,
            s.volume,
            s.errors
        );
        assert!(red(&s.errors, "SplitFaces"), "a plane that misses is a visible error: {s:?}");
    }
    for off in [f64::NAN, f64::INFINITY] {
        let mut d = Doc::new();
        let a = d.p.add_box(40.0, 30.0, 10.0);
        let sp = d.p.add_split_face(a, 0, 0, off);
        d.build();
        let s = d.sum(Some(sp));
        eprintln!("K0 P7 stored offset {off}: faces {}; volume {:?}; valid {:?}; errors {:?}", s.faces, s.volume, s.valid, s.errors);
    }
    // A formula that does not evaluate: the stored number stands, silently (F-001 class).
    let mut m = block(10.0, 5.0);
    m.d.p.set_feat_dim(m.sp, "offset", "nosuchname".into());
    m.d.build();
    let s = m.d.sum(Some(m.sp));
    eprintln!("K0 P7 offset = unknown name: faces {}; errors {:?}; cut levels {:?}", s.faces, s.errors, m.d.levels(m.sp));
}

/// P13: a document saved with a red split node (the plane missed after an edit) and opened: what the open reports, what body stands.
#[test]
fn p13_open_a_file_with_a_red_split() {
    use qymcad_engine::Session;
    let mut m = block(10.0, 5.0);
    m.d.edit(m.sp, "offset", "50");
    assert!(red(&m.d.errors, "SplitFaces"));
    let breps: Vec<(Id, Vec<u8>)> = {
        let _g = qymcad_kernel::kernel_gate();
        m.d.shapes.iter().filter_map(|(id, sh)| sh.to_brep_bytes().map(|b| (*id, b))).collect()
    };
    let path = common::scratch("k0_p13_red_split.qcad");
    let saved = qymcad_io::save_project_guarded_with_brep(&m.d.p, path.to_str().unwrap(), &breps);
    eprintln!("K0 P13 the app's save of a red document: {:?}", saved.as_ref().map(|_| "saved").map_err(|e| e.to_string()));
    if saved.is_ok() {
        let (s, report) = Session::open(&path).unwrap();
        eprintln!(
            "K0 P13 open: errors {:?}; warnings {:?}; bodies {:?}",
            report.errors,
            report.warnings.iter().map(|w| w.message.clone()).collect::<Vec<_>>(),
            report.bodies.iter().map(|b| (b.id, b.volume)).collect::<Vec<_>>()
        );
        assert!(
            report.errors.iter().any(|e| e.message.contains("SplitFaces") || e.message.contains("plane")),
            "the red node is in the open report: {:?}",
            report.errors
        );
        let _ = s;
    }
}

/// P15: a fillet on the seam edge itself (the horizontal edge at z = 5 on the x = 20 wall, between two coplanar pieces).
#[test]
fn p15_fillet_on_a_seam_edge() {
    let mut m = block(10.0, 5.0);
    let edge = m.d.p.regen_edges[&m.sp].iter().find(|e| near(e.mid, [20.0, 0.0, 5.0], 0.01)).map(|e| e.id).expect("the seam edge");
    let fil = m.d.p.add_fillet(m.sp, 1.0, vec![edge]);
    m.d.build();
    let s = m.d.sum(Some(fil));
    eprintln!("K0 P15 fillet on the seam edge: faces {}; volume {:?} (12000 before); errors {:?}", s.faces, s.volume, s.errors);
}

/// P15b: a fillet on the corner edge of the upper piece only (x = 20, y = 15, z 5..10, 5 mm long) rounds the whole corner, 10 mm: the
/// pieces of one straight edge are one chain for the fillet. It removes (1 - pi/4) r^2 per mm of edge.
#[test]
fn p15b_fillet_on_one_piece_of_an_edge_takes_the_whole_edge() {
    let k = 1.0 - std::f64::consts::FRAC_PI_4;
    let mut m = block(10.0, 5.0);
    let edge = m.d.p.regen_edges[&m.sp].iter().find(|e| near(e.mid, [20.0, 15.0, 7.5], 0.01)).map(|e| e.id).expect("the upper corner edge");
    let fil = m.d.p.add_fillet(m.sp, 1.0, vec![edge]);
    m.d.build();
    let s = m.d.sum(Some(fil));
    let length = (12000.0 - s.volume.unwrap()) / k;
    eprintln!("K0 P15b fillet on the 5 mm piece of the edge: errors {:?}; rounded length {length:.3} mm", s.errors);
    assert!(s.errors.is_empty() && (length - 10.0).abs() < 1e-6, "{s:?} {length}");
}
