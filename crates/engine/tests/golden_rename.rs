mod common;
use common::*;
use qymcad_core::model::Project;
use qymcad_engine::*;

const FEATURE: &str = "Плита α # / :";

fn extrusion(sketch: Id) -> Extrude {
    Extrude {
        sketch,
        profiles: None,
        profiles_at: None,
        height: "h".into(),
        down: None,
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: Some("plate".into()),
    }
}

fn stock() -> (Session, Id, Id) {
    let mut s = Session::new_part();
    assert!(s.param_set("h", &5.0.into()).is_ok());
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("profile")).unwrap();
    assert!(s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &6.0.into(), false).is_ok());
    let (body, report) = s.extrude(&extrusion(sk)).unwrap();
    volume(&report, 10.0 * 6.0 * 5.0); // Rectangle area * extrusion height = 300 mm³.
    let named = s.dimension_set(&DimensionTarget::Feature { feature: body, key: "height".into() }, None, Some("named_height")).unwrap();
    assert_eq!(named.dimension.name, "named_height");
    assert_eq!(s.project().feat_dims[&body]["height"], "h");
    (s, sk, body)
}

fn volume(r: &Rebuild, expected: f64) {
    assert!(r.errors.is_empty(), "{r:?}");
    assert_eq!(r.bodies.len(), 1);
    assert_close(r.bodies[0].volume, expected, 1e-6, "area * height");
}

fn labels(p: &Project, sk: Id, body: Id) -> (String, String, String) {
    (
        p.timeline.iter().find(|n| n.id == body).unwrap().name.clone(),
        p.sketches[p.sketch_index(sk).unwrap()].name.clone(),
        p.bodies[p.mesh_index(body).unwrap()].name.clone(),
    )
}

fn breps(s: &Session) -> Vec<(Id, Vec<u8>)> {
    let _gate = qymcad_kernel::kernel_gate();
    s.project().bodies.iter().filter_map(|b| s.shape(b.id).map(|sh| (b.id, sh.to_brep_bytes().unwrap()))).collect()
}

// Full recipe + exact live B-reps + full reporting and topology, captured BEFORE refusal.
fn snapshot(s: &mut Session, body: Id) -> (String, Vec<(Id, Vec<u8>)>, DocInfo, Topology) {
    let topology = s.topology(Some(body), true).unwrap();
    (ron::to_string(s.project()).unwrap(), breps(s), s.info(), topology)
}

fn renamed(s: &mut Session, sk: Id, body: Id) {
    let result = s.rename(RenameTarget::Feature(body), FEATURE).unwrap();
    assert_eq!(result.name, FEATURE);
    assert!(result.changed);
    assert_eq!(s.rename(RenameTarget::Sketch(sk), "drawing").unwrap().name, "drawing");
    assert_eq!(s.rename(RenameTarget::Body(body), "Housing").unwrap().name, "Housing");
}

#[test]
fn three_stores_change_exactly_one_native_field_and_no_geometry() {
    let (mut s, sk, body) = stock();
    for (target, name, kind) in [
        (RenameTarget::Feature(body), FEATURE, "feature"),
        (RenameTarget::Sketch(sk), "drawing", "sketch"),
        (RenameTarget::Body(body), "Housing", "body"),
    ] {
        let before = s.project().clone();
        let before_breps = breps(&s);
        let mut expected = before.clone();
        let (id, previous) = match target {
            RenameTarget::Feature(id) => {
                let n = expected.timeline.iter_mut().find(|n| n.id == id).unwrap();
                let old = n.name.clone();
                n.name = name.into();
                (id, old)
            }
            RenameTarget::Sketch(id) => {
                let i = expected.sketch_index(id).unwrap();
                let old = expected.sketches[i].name.clone();
                expected.sketches[i].name = name.into();
                (id, old)
            }
            RenameTarget::Body(id) => {
                let i = expected.mesh_index(id).unwrap();
                let old = expected.bodies[i].name.clone();
                expected.bodies[i].name = name.into();
                (id, old)
            }
        };
        let result = s.rename(target, name).unwrap();
        assert_eq!(
            result,
            RenameResult {
                target: RenamedTarget { kind, id },
                previous_name: previous,
                name: name.into(),
                display_name: name.into(),
                changed: true,
                duplicates: vec![]
            }
        );
        assert_eq!(labels(s.project(), sk, body), labels(&expected, sk, body), "three independent native label stores");
        assert_eq!(
            ron::to_string(s.project()).unwrap(),
            ron::to_string(&expected).unwrap(),
            "only selected field may change; includes full recipe, feat_dims, named_dims and persistent names"
        );
        assert_eq!(s.project().feat_dims, before.feat_dims);
        assert_eq!(ron::to_string(&s.project().named_dims).unwrap(), ron::to_string(&before.named_dims).unwrap());
        assert_eq!(breps(&s), before_breps);
        assert!(s.project().timeline.iter().all(|n| !n.dirty));
        volume(&s.current_rebuild(), 10.0 * 6.0 * 5.0); // 300 mm³, no geometry edit.
    }
    assert_eq!(labels(s.project(), sk, body), (FEATURE.into(), "drawing".into(), "Housing".into()));
    let info = serde_json::to_value(s.info()).unwrap();
    assert_eq!(info["bodies"][0]["name"], FEATURE);
    assert_eq!(info["bodies"][0]["body_name"], "Housing");
    // A sketch's feature node is a separate valid feature target, too.
    let before = s.project().clone();
    assert_eq!(s.rename(RenameTarget::Feature(sk), "sketch timeline").unwrap().previous_name, "profile");
    assert_eq!(s.project().sketches[s.project().sketch_index(sk).unwrap()].name, "drawing");
    assert_eq!(s.project().feat_dims, before.feat_dims);
}

#[test]
fn labels_survive_open_app_edit_and_engine_param_set() {
    let (mut s, sk, body) = stock();
    renamed(&mut s, sk, body);
    let path = scratch("e1-persistence.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, report) = Session::open(&path).unwrap();
    volume(&report, 10.0 * 6.0 * 5.0); // 300.
    assert_eq!(labels(opened.project(), sk, body), (FEATURE.into(), "drawing".into(), "Housing".into()));
    opened.save(Some(&path)).unwrap();
    assert_close(gui_edit_param(&path, "h", "8"), 10.0 * 6.0 * 8.0, 1e-6, "app h5→8: 480");
    let (app_volume, _, _, app) = gui_edit_param_model(&path, "h", "8");
    assert_close(app_volume, 10.0 * 6.0 * 8.0, 1e-6, "app volume: 480");
    assert_eq!(labels(&app, sk, body), (FEATURE.into(), "drawing".into(), "Housing".into()));
    assert_eq!(app.feat_dims, opened.project().feat_dims);
    assert_eq!(ron::to_string(&app.named_dims).unwrap(), ron::to_string(&opened.project().named_dims).unwrap());
    volume(&opened.param_set("h", &8.0.into()).unwrap(), 10.0 * 6.0 * 8.0); // 480.
    assert_eq!(labels(opened.project(), sk, body), labels(&app, sk, body));
    let dim = opened.named_dimensions().into_iter().find(|d| d.name == "named_height").unwrap();
    assert_eq!(dim.value, Some(8.0));
    assert_eq!(dim.target, Some(DimensionTarget::Feature { feature: body, key: "height".into() }));
    assert_eq!(dim.expression, Some("h".into()));
}

#[test]
fn resolver_uses_sketch_and_body_aliases_immediately_and_after_open() {
    let (mut s, sk, body) = stock();
    renamed(&mut s, sk, body);
    // Old timeline aliases remain valid until that independent store is renamed.
    assert_eq!(s.resolve("profile").unwrap(), sk);
    assert!(s.rename(RenameTarget::Feature(sk), "timeline drawing").unwrap().changed);
    let path = scratch("e1-resolver.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, report) = Session::open(&path).unwrap();
    volume(&report, 10.0 * 6.0 * 5.0); // 300.
    for mut doc in [s, opened] {
        assert_eq!(doc.resolve("drawing").unwrap(), sk);
        assert_eq!(doc.resolve("Housing").unwrap(), body);
        assert_eq!(doc.resolve(FEATURE).unwrap(), body);
        let before = snapshot(&mut doc, body);
        assert!(doc.resolve("profile").unwrap_err().to_string().contains("no object named"));
        assert_eq!(snapshot(&mut doc, body), before);
        let resolved = doc.resolve("drawing").unwrap();
        // Construction geometry leaves the rectangle's area exactly 10*6.
        assert!(doc
            .sketch_line(
                resolved,
                &LineSpec { x1: 20.0.into(), y1: 0.0.into(), x2: 21.0.into(), y2: 0.0.into(), construction: true, dimensioned: true }
            )
            .is_ok());
        let (new_body, report) = doc.extrude(&extrusion(doc.resolve("drawing").unwrap())).unwrap();
        assert_ne!(new_body, body);
        assert!(report.errors.is_empty(), "{report:?}");
        assert_eq!(report.bodies.len(), 2);
        for b in report.bodies {
            assert_close(b.volume, 10.0 * 6.0 * 5.0, 1e-6, "each independent extrude: 300");
        }
    }
}

#[test]
fn policy_refusals_restore_exact_state_and_trim_stores_base() {
    let (mut s, sk, body) = stock();
    assert_eq!(s.rename(RenameTarget::Feature(body), "  Base  ").unwrap().name, "Base");
    for bad in ["", "   ", "\u{2003}", "a\nb", "a\tb", "\u{0085}", "123", "+1", "007"] {
        for target in [RenameTarget::Feature(body), RenameTarget::Sketch(sk), RenameTarget::Body(body)] {
            let before = snapshot(&mut s, body);
            assert!(s.rename(target, bad).is_err(), "must refuse {bad:?}");
            assert_eq!(snapshot(&mut s, body), before, "refusal {bad:?}");
        }
    }
    for target in [RenameTarget::Feature(u64::MAX), RenameTarget::Sketch(body), RenameTarget::Body(sk)] {
        let before = snapshot(&mut s, body);
        assert!(s.rename(target, "valid").is_err());
        assert_eq!(snapshot(&mut s, body), before);
    }
}

#[test]
fn duplicates_report_other_id_and_text_refuses_ambiguity() {
    let (mut s, sk, body) = stock();
    let first = s.rename(RenameTarget::Feature(sk), "same").unwrap();
    assert_eq!(first.duplicates, Vec::<Id>::new());
    let second = s.rename(RenameTarget::Feature(body), "same").unwrap();
    assert_eq!(second.duplicates, vec![sk]);
    let before = snapshot(&mut s, body);
    assert!(s.resolve("same").unwrap_err().to_string().contains("several objects"));
    assert_eq!(snapshot(&mut s, body), before);
    assert_eq!(s.resolve(&body.to_string()).unwrap(), body);
    assert_eq!(s.rename(RenameTarget::Feature(body), "by id").unwrap().previous_name, "same");
}

#[test]
fn localisation_keeps_raw_name_and_deduplicates_aliases() {
    let (mut s, sk, body) = stock();
    for target in [RenameTarget::Feature(body), RenameTarget::Sketch(sk), RenameTarget::Body(body)] {
        // Avoid deliberate alias collisions across the two distinct ids.
        let mut doc = stock().0;
        let result = doc.rename(target, "feat-name-shell").unwrap();
        assert_eq!(result.name, "feat-name-shell");
        assert_eq!(result.display_name, "Shell");
        assert_eq!(doc.resolve("Shell").unwrap(), result.target.id);
        assert_eq!(doc.resolve("feat-name-shell").unwrap(), result.target.id);
    }
    assert_eq!(s.rename(RenameTarget::Feature(body), "shared").unwrap().duplicates, Vec::<Id>::new());
    assert_eq!(s.rename(RenameTarget::Body(body), "shared").unwrap().duplicates, Vec::<Id>::new());
    assert_eq!(s.resolve("shared").unwrap(), body, "multiple aliases for one id are not ambiguous");
    // A raw localisation key collides with another object's literal display text.
    let (mut doc, sk, body) = stock();
    assert_eq!(doc.rename(RenameTarget::Feature(body), "Shell").unwrap().duplicates, Vec::<Id>::new());
    assert_eq!(doc.rename(RenameTarget::Sketch(sk), "feat-name-shell").unwrap().duplicates, vec![body]);
}

#[test]
fn same_name_adds_no_history_and_undo_restores_exact_label() {
    let (mut s, _, body) = stock();
    let before = snapshot(&mut s, body);
    let changed = s.rename(RenameTarget::Feature(body), FEATURE).unwrap();
    assert_eq!(changed.previous_name, "plate");
    assert!(changed.changed);
    let same_before = snapshot(&mut s, body);
    let same = s.rename(RenameTarget::Feature(body), FEATURE).unwrap();
    assert_eq!(same.previous_name, FEATURE);
    assert_eq!(same.name, FEATURE);
    assert!(!same.changed);
    assert_eq!(snapshot(&mut s, body), same_before);
    s.undo().unwrap();
    assert_eq!(snapshot(&mut s, body), before, "one undo skips same-name call and restores plate exactly");
    let before_refusal = snapshot(&mut s, body);
    assert!(s.undo().is_err(), "direct setup operations create no tool history");
    assert_eq!(snapshot(&mut s, body), before_refusal);
}
