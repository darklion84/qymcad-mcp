//! F1 (ADR 0048): a document saved with an assembly active still builds into its part.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const GUI: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/f1-gui-assembly-active.qcad");

fn cut(sketch: Id, target: Option<Id>, height: f64) -> Extrude {
    let mut e: Extrude = serde_json::from_value(json!({"sketch":sketch,"height":height,"op":"cut"})).unwrap();
    e.target = target;
    e
}
fn root(s: &Session) -> Id {
    s.project().components.iter().find(|c| c.parent.is_none()).unwrap().id
}
/// Rewrite the saved navigation context to `active`, keeping every other archive entry, as QymCAD.app
/// does when the user saves while standing in the assembly.
fn save_with_active(s: &mut Session, active: Id, name: &str) -> PathBuf {
    let plain = scratch(&format!("{name}-plain.qcad"));
    s.save(Some(&plain)).unwrap();
    let mut zin = zip::ZipArchive::new(std::fs::File::open(&plain).unwrap()).unwrap();
    let out = scratch(&format!("{name}.qcad"));
    let mut zout = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).unwrap();
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).unwrap();
        if f.name() == "document.ron" {
            let text = String::from_utf8(bytes).unwrap();
            let line = text.lines().find(|l| l.trim_start().starts_with("active_component:")).unwrap().to_owned();
            let indent = &line[..line.len() - line.trim_start().len()];
            bytes = text.replacen(&line, &format!("{indent}active_component: Some({active}),"), 1).into_bytes();
        }
        zout.start_file(f.name(), zip::write::SimpleFileOptions::default()).unwrap();
        zout.write_all(&bytes).unwrap();
    }
    zout.finish().unwrap();
    out
}
fn open(path: &Path) -> Session {
    let (s, r) = Session::open(path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    s
}
/// 10 x 20 x 5 stock centred on the origin in the document's part, saved with the root assembly active.
fn stock_with_assembly_active(name: &str) -> (Session, Id, Id) {
    let mut s = Session::new_part();
    let part = s.project().active_component.unwrap();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("stock")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &20.0.into(), false).unwrap();
    let (_, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5})).unwrap()).unwrap();
    assert!(r.errors.is_empty());
    let assembly = root(&s);
    let path = save_with_active(&mut s, assembly, name);
    let s = open(&path);
    assert_eq!(s.project().active_component, Some(assembly));
    (s, part, assembly)
}

#[test]
fn f1_gui_file_existing_part_sketch_implicit_cut_builds_in_its_part() {
    let mut s = open(Path::new(GUI));
    assert_eq!(s.project().active_component, Some(1), "fixture: the assembly is active");
    let sketch = s.project().sketches.iter().find(|sk| !sk.contour_ids.is_empty()).unwrap().id;
    let body = s.result_bodies().last().unwrap().id;
    let (id, r) = s.extrude(&cut(sketch, None, 1.0)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(s.project().body_owner(id), Some(2), "feature built in the sketch's part");
    assert_eq!(s.project().active_component, Some(1), "navigation context unchanged");
    // Control: the explicit target always worked; implicit must give the same solid.
    let mut control = open(Path::new(GUI));
    let (cid, cr) = control.extrude(&cut(sketch, Some(body), 1.0)).unwrap();
    assert!(cr.errors.is_empty());
    let volume = |s: &Session, id: Id| s.result_bodies().into_iter().find(|b| b.id == id).unwrap().volume;
    assert_close(volume(&s, id), volume(&control, cid), 1e-6, "implicit = explicit-target cut");
}

#[test]
fn f1_gui_file_new_sketch_goes_to_the_sole_part_and_cuts() {
    let mut s = open(Path::new(GUI));
    let before = s.result_bodies().last().unwrap().volume;
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("F1 new")).unwrap();
    assert_eq!(s.project().sketch_owner(sk), Some(2), "sole part, not the assembly");
    assert_eq!(s.project().active_component, Some(1));
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    let (id, r) = s.extrude(&cut(sk, None, 1.0)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(s.project().body_owner(id), Some(2));
    let after = s.result_bodies().into_iter().find(|b| b.id == id).unwrap().volume;
    assert!(after <= before, "a cut never adds material: {after} > {before}");
}

#[test]
fn f1_minimal_assembly_active_new_sketch_cut_is_exact() {
    let (mut s, part, assembly) = stock_with_assembly_active("f1-minimal");
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("pocket")).unwrap();
    assert_eq!(s.project().sketch_owner(sk), Some(part));
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &2.0.into(), &2.0.into(), false).unwrap();
    let (id, r) = s.extrude(&cut(sk, None, 5.0)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(s.project().body_owner(id), Some(part));
    assert_eq!(s.project().active_component, Some(assembly));
    let b = s.result_bodies().into_iter().find(|b| b.id == id).unwrap();
    assert_close(b.volume, 10.0 * 20.0 * 5.0 - 2.0 * 2.0 * 5.0, 1e-6, "stock minus 2x2 through pocket = 980");
    for (a, e) in s.shape(id).unwrap().bbox().unwrap().into_iter().zip([-5.0, -10.0, 0.0, 5.0, 10.0, 5.0]) {
        assert_close(a, e, 1e-6, "stock bbox unchanged by an inner pocket");
    }
    // Saved with the assembly still active, the app-path load rebuilds it cleanly.
    let path = scratch("f1-minimal-cut.qcad");
    s.save(Some(&path)).unwrap();
    let reopened = open(&path);
    assert_eq!(reopened.project().active_component, Some(assembly));
    assert_close(reopened.result_bodies().last().unwrap().volume, 980.0, 1e-6, "reopened volume");
}

#[test]
fn f1_assembly_active_with_two_parts_refuses_unanchored_sketch_before_change() {
    let (maker, _) = {
        let mut m = Session::new_part();
        let sk = m.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        m.sketch_rect(sk, &0.0.into(), &0.0.into(), &4.0.into(), &4.0.into(), false).unwrap();
        let (id, _) = m.extrude(&serde_json::from_value(json!({"sketch":sk,"height":4})).unwrap()).unwrap();
        (m, id)
    };
    let step = scratch("f1-second-part.step");
    maker.export(ExportFormat::Step, &step, Quality::Standard, None).unwrap();
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("stock")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &10.0.into(), &20.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5})).unwrap()).unwrap();
    s.import(&step, 1.0, None, Some("second")).unwrap();
    let assembly = root(&s);
    let path = save_with_active(&mut s, assembly, "f1-two-parts");
    let mut s = open(&path);
    let parts: Vec<_> = s.project().components.iter().filter(|c| s.project().component_is_part(c.id)).map(|c| c.id).collect();
    assert_eq!(parts.len(), 2, "fixture: two parts");
    let before = scratch("f1-two-parts-before.qcad");
    s.save(Some(&before)).unwrap();
    let e = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("ambiguous")).unwrap_err().to_string();
    assert!(e.contains(&format!("active component {assembly}")) && e.contains("cannot choose a modelling part from 2 parts"), "{e}");
    for id in &parts {
        assert!(e.contains(&format!("{id} '")), "lists part {id}: {e}");
    }
    let after = scratch("f1-two-parts-after.qcad");
    s.save(Some(&after)).unwrap();
    assert_eq!(std::fs::read(&before).unwrap(), std::fs::read(&after).unwrap(), "refusal leaves the document byte-identical");
    // An existing part-owned sketch stays usable in the same document.
    let (id, r) = s.extrude(&cut(sk, None, 1.0)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(s.project().sketch_owner(sk), s.project().body_owner(id));
}

#[test]
fn f1_explicit_target_in_another_part_is_refused_before_change() {
    let (mut s, part, _) = stock_with_assembly_active("f1-cross");
    let body = s.result_bodies().last().unwrap().id;
    assert_eq!(s.project().body_owner(body), Some(part));
    // A sketch on the face of an imported body belongs to the import's part; targeting the stock crosses parts.
    let (maker, _) = {
        let mut m = Session::new_part();
        let sk = m.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        m.sketch_rect(sk, &30.0.into(), &0.0.into(), &4.0.into(), &4.0.into(), false).unwrap();
        let (id, _) = m.extrude(&serde_json::from_value(json!({"sketch":sk,"height":4})).unwrap()).unwrap();
        (m, id)
    };
    let step = scratch("f1-cross.step");
    maker.export(ExportFormat::Step, &step, Quality::Standard, None).unwrap();
    let imported = s.import(&step, 1.0, None, Some("other")).unwrap().bodies[0].body;
    let topo = s.topology(Some(imported), false).unwrap();
    let top = topo.faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    let sk = s.sketch_create(&PlaneRef::Face { body: imported, face: top }, Some("on import")).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &1.0.into(), &1.0.into(), false).unwrap();
    let before = scratch("f1-cross-before.qcad");
    s.save(Some(&before)).unwrap();
    let e = s.extrude(&cut(sk, Some(body), 1.0)).unwrap_err().to_string();
    assert!(
        e.contains(&format!("target body {body} belongs to part {part}")) && e.contains("cross-component inputs are not allowed"),
        "{e}"
    );
    let after = scratch("f1-cross-after.qcad");
    s.save(Some(&after)).unwrap();
    // The engine settles pending edits before any call (F-034): only the sketch's dirty flag may differ.
    assert_eq!(entries_without_dirty(&before), entries_without_dirty(&after), "refusal changes no recipe or B-rep byte");
}
fn entries_without_dirty(path: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).unwrap();
            if f.name() == "document.ron" {
                let text = String::from_utf8(bytes).unwrap();
                bytes = text.lines().filter(|l| !l.trim_start().starts_with("dirty:")).collect::<Vec<_>>().join("\n").into_bytes();
            }
            (f.name().to_owned(), bytes)
        })
        .collect()
}
