//! Object aliases must round-trip through the resolver's trimming boundary.
use qymcad_engine::*;
use std::collections::BTreeSet;
use std::path::Path;

const INVALID_NAMES: &[&str] = &[" Imported ", " Leading", "Trailing ", "\u{2003}Leading", "Trailing\u{a0}"];

fn fixture() -> &'static Path {
    Path::new("tests/fixtures/server-box-10x10x5-inch.igs")
}

fn snapshot(s: &Session) -> (serde_json::Value, Vec<(Id, Vec<u8>)>) {
    let project = serde_json::to_value(s.project()).unwrap();
    let ids: BTreeSet<_> = s.project().timeline.iter().flat_map(|node| node.kind.bodies()).collect();
    let _gate = qymcad_kernel::kernel_gate();
    let breps = ids.into_iter().filter_map(|id| s.shape(id).map(|shape| (id, shape.to_brep_bytes().unwrap()))).collect();
    (project, breps)
}

fn name_error(error: Error) {
    let message = error.to_string();
    assert!(message.contains("name"), "{message}");
    assert!(message.contains("whitespace") || message.contains("empty") || message.contains("blank"), "{message}");
}

fn box_body(s: &mut Session, x: f64, name: &str) -> Id {
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sketch, &x.into(), &0.0.into(), &10.0.into(), &10.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch,
        profiles: None,
        height: 10.0.into(),
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: Some(name.into()),
    })
    .unwrap()
    .0
}

#[test]
fn invalid_import_names_are_refused_without_changing_project_or_original_breps() {
    let mut s = Session::new_part();
    box_body(&mut s, 0.0, "Existing body");
    let before = snapshot(&s);
    for name in INVALID_NAMES {
        name_error(s.import(fixture(), 1.0, None, Some(name)).expect_err(&format!("refuse import name {name:?}")));
        assert_eq!(snapshot(&s), before, "import name {name:?} changed recipe or original B-reps");
    }
}

#[test]
fn invalid_filename_derived_import_name_is_refused_atomically() {
    let dir = std::env::temp_dir().join(format!("qymcad-object-names-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(" Imported .igs");
    std::fs::copy(fixture(), &path).unwrap();
    let mut s = Session::new_part();
    box_body(&mut s, 0.0, "Existing body");
    let before = snapshot(&s);
    let result = s.import(&path, 1.0, None, None);
    std::fs::remove_dir_all(dir).unwrap();
    name_error(result.expect_err("filename-derived part names must obey the shared validator"));
    assert_eq!(snapshot(&s), before);
}

#[test]
fn invalid_sketch_names_are_refused_before_mutation() {
    let mut s = Session::new_part();
    let before = snapshot(&s);
    for name in INVALID_NAMES {
        name_error(s.sketch_create(&PlaneRef::Base(BaseName::XY), Some(name)).expect_err(&format!("refuse sketch name {name:?}")));
        assert_eq!(snapshot(&s), before, "sketch name {name:?} changed the project");
    }
}

#[test]
fn invalid_plane_and_feature_names_are_refused_atomically() {
    let mut s = Session::new_part();
    let body = box_body(&mut s, 0.0, "Existing body");
    let before = snapshot(&s);
    for name in [" Plane ", "\u{2003}Feature"] {
        name_error(s.plane_offset(&PlaneRef::Base(BaseName::XY), &1.0.into(), Some(name)).unwrap_err());
        assert_eq!(snapshot(&s), before);
        name_error(s.mirror(Some(body), &PlaneRef::Base(BaseName::YZ), false, Some(name)).unwrap_err());
        assert_eq!(snapshot(&s), before);
    }
}

#[test]
fn invalid_split_and_boolean_piece_names_are_refused_atomically() {
    let mut s = Session::new_part();
    let a = box_body(&mut s, 0.0, "A");
    let b = box_body(&mut s, 5.0, "B");
    let before = snapshot(&s);
    for name in [" Piece ", "Piece\u{a0}"] {
        name_error(
            s.split(&SplitOptions {
                body: Some(a),
                plane: PlaneRef::Base(BaseName::XY),
                offset: 5.0.into(),
                name: Some("Split".into()),
                piece_names: Some(vec![name.into(), "Upper".into()]),
            })
            .unwrap_err(),
        );
        assert_eq!(snapshot(&s), before);
        name_error(
            s.body_boolean(&BodyBooleanOptions {
                a,
                b,
                op: BooleanOp::Union,
                name: Some("Boolean".into()),
                piece_names: Some(vec![name.into()]),
            })
            .unwrap_err(),
        );
        assert_eq!(snapshot(&s), before);
    }
}

#[test]
fn internal_spaces_remain_valid_and_names_resolve() {
    let mut s = Session::new_part();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("Internal sketch spaces")).unwrap();
    assert_eq!(s.resolve("Internal sketch spaces").unwrap(), sketch);
    let result = s.import(fixture(), 1.0, None, Some("Imported body spaces")).unwrap();
    assert_eq!(result.bodies.len(), 1);
    assert_eq!(result.bodies[0].name, "Imported body spaces");
    assert_eq!(s.resolve(&result.bodies[0].name).unwrap(), result.bodies[0].body);
}
