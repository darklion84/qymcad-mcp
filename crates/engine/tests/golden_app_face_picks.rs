//! Face references the app reopens in edit mode (qymcad-part reopen.rs, `live_picks` in params.rs:67) read only
//! `query.picked_descs()`: the ids picked by hand. A face given as a description must therefore be stored as the
//! app stores it (`Ref::picks` / `Ref::one`), or «Править» opens Shell, PushFace and Draft with an empty
//! selection and OK destroys the feature. Each case creates (or re-picks) from a description, saves, reloads
//! with the native io, checks the stored picks, then edits `h` on the app path and checks `live_picks` again.
mod common;
use common::*;
use qymcad_core::feature::FeatureKind;
use qymcad_core::model::Project;
use qymcad_core::refs::Ref;
use qymcad_engine::*;

fn box_stock() -> (Session, Id) {
    let mut s = Session::new_part();
    s.param_set("h", &10.0.into()).unwrap();
    s.param_set("t", &1.0.into()).unwrap();
    s.param_set("d", &5.0.into()).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
    s.sketch_rect(sketch, &0.0.into(), &0.0.into(), &20.0.into(), &20.0.into(), false).unwrap();
    let (body, report) = s
        .extrude(&Extrude {
            sketch,
            profiles: None,
            profiles_at: None,
            height: "h".into(),
            down: None,
            op: Op::Add,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: Some("box".into()),
        })
        .unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert_close(report.bodies[0].volume, 4000.0, 1e-6, "20*20*10");
    (s, body)
}

fn facing(dir: [f64; 3]) -> Sel {
    Sel::Facing { dir, tol_deg: 1.0 }
}

fn walls() -> Sel {
    Sel::Union(vec![facing([1.0, 0.0, 0.0]), facing([-1.0, 0.0, 0.0]), facing([0.0, 1.0, 0.0]), facing([0.0, -1.0, 0.0])])
}

fn ids(s: &mut Session, sel: &Sel) -> Vec<u32> {
    let mut v = s.select(None, Element::Faces, sel).unwrap().1;
    v.sort_unstable();
    assert!(!v.is_empty());
    v
}

fn square_frustum(h: f64, angle_deg: f64) -> f64 {
    let b = 20.0 + 2.0 * h * angle_deg.to_radians().tan();
    h / 3.0 * (400.0 + b * b + 20.0 * b)
}

/// The refs reopen.rs hands to `live_picks` for `node`, with the source body.
fn refs(p: &Project, node: Id) -> (Id, Vec<Ref>) {
    match &p.timeline.iter().find(|n| n.id == node).unwrap().kind {
        FeatureKind::Shell { src, faces, .. } => (*src, vec![faces.clone()]),
        FeatureKind::PushFace { src, face, .. } => (*src, vec![face.clone()]),
        FeatureKind::Draft { src, faces, neutral, .. } => (*src, vec![faces.clone(), neutral.clone()]),
        other => panic!("unexpected kind {other:?}"),
    }
}

fn sorted(mut v: Vec<u32>) -> Vec<u32> {
    v.sort_unstable();
    v
}

/// `qymcad_part::params::live_picks` for faces: picked ids that are live on `body` (all of them while nothing is live).
fn live_picks(p: &Project, body: Id, r: &Ref) -> Vec<u32> {
    let live: Vec<u32> = p.regen_faces.get(&body).map(|fs| fs.iter().map(|f| f.id).collect()).unwrap_or_default();
    sorted(r.query.picked_descs().into_iter().filter(|d| live.is_empty() || live.contains(d)).collect())
}

/// Save `s`, reload natively and check every face ref of `node` is the hand pick `expected[i]`; then edit `h` on
/// the app path, rebuild, and check `live_picks` still returns them and the volume is `volume_h12`.
fn check_app_reopen(s: &mut Session, node: Id, expected: &[Vec<u32>], file: &str, volume_h12: f64) {
    let path = scratch(file);
    s.save(Some(&path)).unwrap();
    let loaded = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap().project;
    let (_, stored) = refs(&loaded, node);
    assert_eq!(stored.len(), expected.len());
    for (r, want) in stored.iter().zip(expected) {
        let picked = sorted(r.query.picked_descs());
        assert!(!picked.is_empty(), "{file}: stored ref has no hand picks, the app reopens it empty: {r:?}");
        assert_eq!(&picked, want, "{file}: stored picks differ from the selected faces");
    }
    let (volume, _, _, project) = gui_edit_param_model(&path, "h", "12");
    assert_close(volume, volume_h12, 1e-3, "app rebuild after h edit");
    let (src, stored) = refs(&project, node);
    assert!(project.regen_faces.get(&src).is_some_and(|f| !f.is_empty()), "{file}: no live faces after app regen");
    for (r, want) in stored.iter().zip(expected) {
        assert_eq!(&live_picks(&project, src, r), want, "{file}: app edit mode would not select the faces");
    }
}

#[test]
fn described_shell_reopens_with_its_faces_in_the_app() {
    let (mut s, _) = box_stock();
    let top = ids(&mut s, &facing([0.0, 0.0, 1.0]));
    let (node, report) = s.shell(None, &facing([0.0, 0.0, 1.0]), &"t".into(), Side::Inward, None).unwrap();
    assert_close(report.bodies[0].volume, 4000.0 - 18.0 * 18.0 * 9.0, 1e-6, "shell");
    check_app_reopen(&mut s, node, &[top], "g-picks-shell.qcad", 4800.0 - 18.0 * 18.0 * 11.0);
}

#[test]
fn described_shell_repick_reopens_with_its_faces_in_the_app() {
    let (mut s, _) = box_stock();
    let bottom = ids(&mut s, &facing([0.0, 0.0, -1.0]));
    let (node, _) = s.shell(None, &facing([0.0, 0.0, 1.0]), &"t".into(), Side::Inward, None).unwrap();
    let (_, selected, report) = s.repick_shell(node, &facing([0.0, 0.0, -1.0])).unwrap();
    assert_eq!(selected, bottom);
    assert_close(report.bodies[0].volume, 4000.0 - 18.0 * 18.0 * 9.0, 1e-6, "shell re-pick");
    check_app_reopen(&mut s, node, &[bottom], "g-picks-shell-repick.qcad", 4800.0 - 18.0 * 18.0 * 11.0);
}

#[test]
fn described_push_face_reopens_with_its_face_in_the_app() {
    let (mut s, _) = box_stock();
    let top = ids(&mut s, &facing([0.0, 0.0, 1.0]));
    let (node, report) = s.push_face(None, &facing([0.0, 0.0, 1.0]), &"d".into(), None).unwrap();
    assert_close(report.bodies[0].volume, 20.0 * 20.0 * 15.0, 1e-6, "push");
    check_app_reopen(&mut s, node, &[top], "g-picks-push.qcad", 20.0 * 20.0 * 17.0);
}

#[test]
fn described_push_face_repick_reopens_with_its_face_in_the_app() {
    let (mut s, _) = box_stock();
    let xface = ids(&mut s, &facing([1.0, 0.0, 0.0]));
    let (node, _) = s.push_face(None, &facing([0.0, 0.0, 1.0]), &"d".into(), None).unwrap();
    let (_, selected, report) = s.repick_push_face(node, &facing([1.0, 0.0, 0.0])).unwrap();
    assert_eq!(selected, xface);
    assert_close(report.bodies[0].volume, 25.0 * 20.0 * 10.0, 1e-6, "push re-pick");
    let hint = match &s.project().timeline.iter().find(|n| n.id == node).unwrap().kind {
        FeatureKind::PushFace { face, .. } => face.hint,
        _ => unreachable!(),
    };
    assert_eq!(hint.normal, [1.0, 0.0, 0.0], "the centroid/normal hint is kept for descriptions");
    check_app_reopen(&mut s, node, &[xface], "g-picks-push-repick.qcad", 25.0 * 20.0 * 12.0);
}

#[test]
fn described_draft_reopens_with_its_faces_and_neutral_in_the_app() {
    let (mut s, _) = box_stock();
    let wall_ids = ids(&mut s, &walls());
    let bottom = ids(&mut s, &facing([0.0, 0.0, -1.0]));
    let (node, report) = s
        .draft(&Draft { body: None, faces: walls(), neutral: facing([0.0, 0.0, -1.0]), angle: 5.0.into(), flip: false, name: None })
        .unwrap();
    assert_close(report.bodies[0].volume, square_frustum(10.0, 5.0), 1e-3, "draft");
    check_app_reopen(&mut s, node, &[wall_ids, bottom], "g-picks-draft.qcad", square_frustum(12.0, 5.0));
}

#[test]
fn described_draft_repick_reopens_with_its_faces_and_neutral_in_the_app() {
    let (mut s, _) = box_stock();
    let wall_ids = ids(&mut s, &walls());
    let top = ids(&mut s, &facing([0.0, 0.0, 1.0]));
    let (node, _) = s
        .draft(&Draft {
            body: None,
            faces: facing([1.0, 0.0, 0.0]),
            neutral: facing([0.0, 0.0, -1.0]),
            angle: 5.0.into(),
            flip: false,
            name: None,
        })
        .unwrap();
    let (_, selected, neutral, report) = s.repick_draft(node, &walls(), Some(&facing([0.0, 0.0, 1.0]))).unwrap();
    assert_eq!(selected, wall_ids);
    assert_eq!(vec![neutral], top);
    assert_close(report.bodies[0].volume, square_frustum(10.0, 5.0), 1e-3, "draft re-pick");
    check_app_reopen(&mut s, node, &[wall_ids, top], "g-picks-draft-repick.qcad", square_frustum(12.0, 5.0));
}
