//! G1: native Draft (P9). Hand volumes: a 20×20×10 block drafted on its four walls about the bottom face is a
//! square frustum V = h/3·(a² + b² + ab), b = 20 ± 2·h·tan|angle|; a drafted cylinder side is a cone frustum.
mod common;
use common::*;
use qymcad_core::feature::FeatureKind;
use qymcad_engine::*;

const BOTTOM: [f64; 3] = [0.0, 0.0, -1.0];
const TOP: [f64; 3] = [0.0, 0.0, 1.0];

fn block() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("h", &10.0.into()).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let (body, report) = s.extrude(&extrude(sketch, "h")).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_close(report.bodies[0].volume, 4000.0, 1e-6, "20*20*10");
    (s, body)
}

fn cylinder() -> (Session, Id) {
    let mut s = Session::new_part();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sketch, &0.0.into(), &0.0.into(), &20.0.into(), false).unwrap();
    let (body, report) = s.extrude(&extrude(sketch, "20")).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    (s, body)
}

fn extrude(sketch: Id, height: &str) -> Extrude {
    Extrude {
        sketch,
        profiles: None,
        profiles_at: None,
        height: Num::from(height),
        down: None,
        op: Op::Add,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    }
}

fn facing(dir: [f64; 3]) -> Sel {
    Sel::Facing { dir, tol_deg: 1.0 }
}

fn walls() -> Sel {
    Sel::Union(vec![facing([1.0, 0.0, 0.0]), facing([-1.0, 0.0, 0.0]), facing([0.0, 1.0, 0.0]), facing([0.0, -1.0, 0.0])])
}

fn draft(faces: Sel, neutral: Sel, angle: impl Into<Num>, flip: bool) -> Draft {
    Draft { body: None, faces, neutral, angle: angle.into(), flip, name: None }
}

fn square_frustum(h: f64, angle_deg: f64) -> f64 {
    let a = 20.0;
    let b = a + 2.0 * h * angle_deg.to_radians().tan();
    h / 3.0 * (a * a + b * b + a * b)
}

fn volume(report: &Rebuild, expected: f64) {
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.bodies.len(), 1, "{report:?}");
    assert_close(report.bodies[0].volume, expected, 1e-3, "drafted volume");
}

type Snapshot = (String, Vec<(Id, Vec<u8>)>, DocInfo);

fn snapshot(s: &Session) -> Snapshot {
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        s.project().bodies.iter().filter_map(|b| s.shape(b.id).map(|shape| (b.id, shape.to_brep_bytes().unwrap()))).collect()
    };
    (ron::ser::to_string(s.project()).unwrap(), breps, s.info())
}

fn unchanged(s: &Session, before: &Snapshot) {
    assert_eq!(ron::ser::to_string(s.project()).unwrap(), before.0, "recipe changed");
    assert_eq!(snapshot(s).1, before.1, "live shapes changed");
    assert_eq!(s.info(), before.2, "doc info changed");
}

#[test]
fn bottom_neutral_minus_five_narrows_to_the_hand_frustum() {
    let (mut s, _) = block();
    let (_, report) = s.draft(&draft(walls(), facing(BOTTOM), -5.0, false)).unwrap();
    volume(&report, square_frustum(10.0, -5.0));
    assert_close(square_frustum(10.0, -5.0), 3660.2510, 1e-3, "research table");
}

#[test]
fn bottom_neutral_plus_five_widens_and_flip_equals_negation() {
    let (mut s, _) = block();
    let (_, report) = s.draft(&draft(walls(), facing(BOTTOM), 5.0, false)).unwrap();
    volume(&report, square_frustum(10.0, 5.0));
    assert_close(square_frustum(10.0, 5.0), 4360.1603, 1e-3, "research table");
    let (mut flipped, _) = block();
    let (_, report) = flipped.draft(&draft(walls(), facing(BOTTOM), 5.0, true)).unwrap();
    volume(&report, square_frustum(10.0, -5.0));
}

#[test]
fn drafted_wall_normals_tilt_by_the_angle_from_the_pull_direction() {
    for angle in [-5.0, 5.0, 12.5] {
        let (mut s, _) = block();
        let (body, report) = s.draft(&draft(walls(), facing(BOTTOM), angle, false)).unwrap();
        assert!(report.errors.is_empty(), "{report:?}");
        let topo = s.topology(Some(body), false).unwrap();
        assert_eq!(topo.faces.len(), 6, "a draft keeps every face");
        let tilted: Vec<_> = topo.faces.iter().filter_map(|f| f.normal).filter(|n| n[2].abs() < 0.9).collect();
        assert_eq!(tilted.len(), 4, "four drafted walls");
        for n in tilted {
            // pull = outward normal of the bottom neutral = -Z.
            let tilt = (-n[2]).clamp(-1.0, 1.0).asin().to_degrees();
            assert_close(tilt, angle, 1e-6, "asin(n · pull)");
        }
    }
}

#[test]
fn one_wall_and_cylinder_side_match_hand_values() {
    let (mut s, _) = block();
    let (_, report) = s.draft(&draft(facing([1.0, 0.0, 0.0]), facing(BOTTOM), 5.0, false)).unwrap();
    // One wall leans out by 10·tan5 at the top: a triangular prism 20 mm long.
    volume(&report, 4000.0 + 0.5 * 10.0 * 10.0 * 5f64.to_radians().tan() * 20.0);

    let (mut c, body) = cylinder();
    let side = c.topology(Some(body), false).unwrap().faces.into_iter().find(|f| f.kind == FaceKind::Cylinder).unwrap().id;
    let (_, report) = c.draft(&draft(Sel::Ids(vec![side]), facing(BOTTOM), 5.0, false)).unwrap();
    let r2 = 10.0 + 20.0 * 5f64.to_radians().tan();
    let cone = std::f64::consts::PI * 20.0 / 3.0 * (100.0 + r2 * r2 + 10.0 * r2);
    volume(&report, cone);
    assert_close(cone, 7446.7245, 1e-3, "research table");
}

#[test]
fn formula_angle_follows_param_set_and_dimension_set_and_guards_the_range() {
    let (mut s, _) = block();
    s.param_set("a", &(-5.0).into()).unwrap();
    let (feature, report) = s.draft(&Draft { name: Some("taper".into()), ..draft(walls(), facing(BOTTOM), "a", false) }).unwrap();
    volume(&report, square_frustum(10.0, -5.0));
    assert_eq!(s.node_name(feature), "taper");
    assert_eq!(s.project().feat_dims[&feature]["angle"], "a");
    volume(&s.param_set("a", &(-3.0).into()).unwrap(), square_frustum(10.0, -3.0));
    volume(&s.param_set("h", &12.0.into()).unwrap(), square_frustum(12.0, -3.0));

    // The app's command limits: 0 < |angle| <= 60 (start.rs), enforced on evaluated edits too.
    let before = snapshot(&s);
    for bad in [70.0, -61.0, 0.0] {
        let error = s.param_set("a", &bad.into()).unwrap_err().to_string();
        assert!(error.contains("draft angle"), "{bad}: {error}");
        unchanged(&s, &before);
    }
    let target = DimensionTarget::Feature { feature, key: "angle".into() };
    let error = s.dimension_set(&target, Some(&61.0.into()), None).unwrap_err().to_string();
    assert!(error.contains("draft angle"), "{error}");
    unchanged(&s, &before);
    s.dimension_set(&target, Some(&"a*2".into()), None).unwrap();
    volume(&s.rebuild(), square_frustum(12.0, -6.0));
    assert_eq!(s.project().feat_dims[&feature]["angle"], "a*2");
    let error = s.param_set("a", &31.0.into()).unwrap_err().to_string();
    assert!(error.contains("draft angle"), "a*2 = 62: {error}");
}

#[test]
fn refusals_leave_the_document_unchanged() {
    let (mut s, body) = block();
    let before = snapshot(&s);
    let cases: Vec<(Draft, &str)> = vec![
        (draft(walls(), facing(BOTTOM), 0.0, false), "draft angle"),
        (draft(walls(), facing(BOTTOM), 61.0, false), "draft angle"),
        (draft(walls(), facing(BOTTOM), -60.5, false), "draft angle"),
        (draft(facing(BOTTOM), facing(BOTTOM), 5.0, false), "only the neutral"),
        (draft(facing(TOP), facing(BOTTOM), 5.0, false), "parallel"),
        (draft(walls(), walls(), 5.0, false), "exactly one face"),
        (draft(facing([1.0, 1.0, 0.0]), facing(BOTTOM), 5.0, false), "matched no face"),
        // -45° on a 20 mm block 10 mm tall closes the top face into an apex: native builds a pyramid silently.
        (draft(walls(), facing(BOTTOM), -45.0, false), "lost"),
    ];
    for (args, expected) in cases {
        let error = s.draft(&args).unwrap_err().to_string();
        assert!(error.contains(expected), "{args:?}: expected `{expected}` in {error}");
        unchanged(&s, &before);
    }
    let _ = body;

    let (mut c, cyl) = cylinder();
    let side = c.topology(Some(cyl), false).unwrap().faces.into_iter().find(|f| f.kind == FaceKind::Cylinder).unwrap().id;
    let before = snapshot(&c);
    let error = c.draft(&draft(facing(TOP), Sel::Ids(vec![side]), 5.0, false)).unwrap_err().to_string();
    assert!(error.contains("not planar"), "curved neutral: {error}");
    unchanged(&c, &before);
}

#[test]
fn collapse_through_a_later_edit_is_refused() {
    let (mut s, _) = block();
    s.param_set("a", &(-30.0).into()).unwrap();
    let (_, report) = s.draft(&draft(walls(), facing(BOTTOM), "a", false)).unwrap();
    volume(&report, square_frustum(10.0, -30.0));
    let before = snapshot(&s);
    let error = s.param_set("a", &(-45.0).into()).unwrap_err().to_string();
    assert!(error.contains("lost"), "{error}");
    unchanged(&s, &before);
}

#[test]
fn repick_replaces_faces_and_neutral_keeping_the_node() {
    let (mut s, _) = block();
    let (feature, report) = s.draft(&draft(facing([1.0, 0.0, 0.0]), facing(BOTTOM), 5.0, false)).unwrap();
    assert!(report.errors.is_empty());
    let (_, selected, neutral, report) = s.repick_draft(feature, &walls(), None).unwrap();
    assert_eq!(selected.len(), 4);
    volume(&report, square_frustum(10.0, 5.0));
    let (_, _, new_neutral, report) = s.repick_draft(feature, &walls(), Some(&facing(TOP))).unwrap();
    assert_ne!(neutral, new_neutral);
    // Neutral on top: the bottom widens by the same frustum.
    volume(&report, square_frustum(10.0, 5.0));
    assert!(s.project().timeline.iter().any(|n| n.id == feature && matches!(n.kind, FeatureKind::Draft { .. })));
    let before = snapshot(&s);
    let error = s.repick_draft(feature, &facing(TOP), None).unwrap_err().to_string();
    assert!(error.contains("only the neutral"), "{error}");
    unchanged(&s, &before);
}

/// The app's picking removes the neutral face from the drafted set (qymcad-part aims.rs:284); the tool does the
/// same, stores the remaining faces as picks like the app's edit, and says so in `notes`.
#[test]
fn a_neutral_among_the_faces_is_dropped_like_the_app() {
    let (mut s, _) = block();
    let (_, report) = s.draft(&draft(Sel::Union(vec![walls(), facing(BOTTOM)]), facing(BOTTOM), 5.0, false)).unwrap();
    volume(&report, square_frustum(10.0, 5.0));
    assert!(report.notes.iter().any(|n| n.contains("neutral") && n.contains("removed")), "{:?}", report.notes);
    let node = s.project().timeline.iter().rev().find(|n| matches!(n.kind, FeatureKind::Draft { .. })).unwrap().id;
    // Re-pick keeping the stored neutral (bottom) while the new selection holds it again.
    let (_, selected, neutral, report) = s.repick_draft(node, &Sel::Union(vec![walls(), facing(BOTTOM)]), None).unwrap();
    assert_eq!(selected.len(), 4, "the bottom neutral left the drafted set");
    volume(&report, square_frustum(10.0, 5.0));
    assert!(!selected.contains(&neutral));
    assert!(report.errors.is_empty(), "{report:?}");
    assert!(report.notes.iter().any(|n| n.contains("neutral") && n.contains("removed")), "{:?}", report.notes);
}

#[test]
fn save_and_reopen_keep_formula_volume_and_app_edit() {
    let (mut s, _) = block();
    s.param_set("a", &4.0.into()).unwrap();
    let (feature, report) = s.draft(&draft(walls(), facing(BOTTOM), "a", true)).unwrap();
    volume(&report, square_frustum(10.0, -4.0));
    let path = scratch("g1-draft.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    volume(&report, square_frustum(10.0, -4.0));
    assert_eq!(opened.project().feat_dims[&feature]["angle"], "a");
    assert!(matches!(opened.project().timeline.iter().find(|n| n.id == feature).unwrap().kind, FeatureKind::Draft { flip: true, .. }));
    volume(&opened.param_set("a", &6.0.into()).unwrap(), square_frustum(10.0, -6.0));
    // The app's own parameter edit path rebuilds the saved file to the same hand value.
    assert_close(gui_edit_param(&path, "a", "6"), square_frustum(10.0, -6.0), 1e-3, "app angle edit");
}
