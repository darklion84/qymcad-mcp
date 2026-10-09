//! Catalogue-key presentation without mutating stored document names.
use crate::*;
use qymcad_core::errors::CoreError;

#[test]
fn names_errors_warnings_and_undo_use_english_catalogue_words() {
    let mut session = Session::new_part();
    let sketch = session.sketch_create(&PlaneRef::Base(BaseName::XY), Some("name-part-n#3")).unwrap();
    session.p.timeline.iter_mut().find(|n| n.id == sketch).unwrap().name = "feat-name-combine-cut".into();
    session.p.regen_errors.insert(sketch, CoreError::CutRemovedNothing);
    session.p.regen_warnings.insert(sketch, CoreError::EdgesDropped { asked: 2, dropped: 1 });
    let info = session.info();
    // Pinned English catalogue: main.ftl name-part-n substitutes v; feat-name-combine-cut names the feature.
    assert_eq!(info.sketches[0].name, "Part 3");
    assert_eq!(info.timeline.iter().find(|n| n.id == sketch).unwrap().name, "Cut with a sketch");
    assert_eq!(info.errors[0].name, "Cut with a sketch");
    assert!(info.errors[0].message.contains("The cut removed nothing"), "{:?}", info.errors);
    assert!(info.errors[0].message.contains("check direction (reverse) or height"), "{:?}", info.errors);
    assert!(info.warnings.iter().any(|w| w.contains("Cut with a sketch") && !w.contains("feat-name-")), "{:?}", info.warnings);
    let refusal = session.render(View::Iso, 100, 100, None).err().unwrap().to_string();
    assert!(refusal.contains(&info.errors[0].message) && !refusal.contains("error-cut-removed-nothing"), "{refusal}");
    let snapshot = session.begin_tool_edit().unwrap();
    session.p.regen_errors.clear();
    session.p.regen_warnings.clear();
    session.finish_tool_edit(snapshot, true);
    let undone = session.undo().unwrap();
    assert_eq!(undone.rebuild.errors, info.errors);
    assert!(undone.rebuild.warnings.iter().all(|w| w.name == "Cut with a sketch"));
    assert_eq!(session.project().timeline.iter().find(|n| n.id == sketch).unwrap().name, "feat-name-combine-cut");
    assert_eq!(session.project().sketches[0].name, "name-part-n#3");
}
