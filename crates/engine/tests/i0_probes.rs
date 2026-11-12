//! Package I phase 0: probes of what the app does around datum points, datum axes and the construction toggle.
//! Each probe prints what it observed (run with --nocapture); assertions pin the observation, so a changed app shows here.
mod common;
use common::*;
use qymcad_engine::*;
use serde_json::json;

fn rect_with_hole() -> (Session, u64, u64, u64) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &0.0.into(), &0.0.into(), &20.0.into(), &10.0.into(), false).unwrap();
    let circle = s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
    (s, sk, circle, 0)
}

/// P11: a circle inside a rectangle turned construction. The consumer extrudes the rectangle; the hole silently disappears?
#[test]
fn p11_a_hole_turned_construction_changes_the_consumer_without_an_error() {
    let (mut s, sk, circle, _) = rect_with_hole();
    let info = s.sketch_info(sk).unwrap();
    eprintln!("P11 contours before: {:?}", info.contours);
    let (_body, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
    eprintln!("P11 volume with hole: {} errors {:?}", r.bodies[0].volume, r.errors);
    let done = s.sketch_edit(sk, |s| s.sketch_construction(sk, &[circle], true));
    eprintln!("P11 toggle result: {:?}", done.as_ref().map(|d| &d.0.entities).map_err(|e| e.to_string()));
    eprintln!("P11 after: contours {:?}", s.sketch_info(sk).unwrap().contours);
    eprintln!("P11 volume after: {:?}", s.result_bodies().iter().map(|b| b.volume).collect::<Vec<_>>());
    eprintln!("P11 errors {:?}", s.current_rebuild().errors);
}

/// P11b: the only profile a consumer uses turned construction: the contour is gone.
#[test]
fn p11b_the_consumers_only_contour_turned_construction() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let circle = s.sketch_circle(sk, &0.0.into(), &0.0.into(), &4.0.into(), false).unwrap();
    s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":5,"op":"new_body"})).unwrap()).unwrap();
    let done = s.sketch_edit(sk, |s| s.sketch_construction(sk, &[circle], true));
    eprintln!("P11b toggle: {:?}", done.as_ref().map(|d| &d.0.entities).map_err(|e| e.to_string()));
    eprintln!("P11b bodies after: {:?}", s.result_bodies().iter().map(|b| b.volume).collect::<Vec<_>>());
    eprintln!("P11b errors {:?}", s.current_rebuild().errors);
}

/// P12: the app's toggle on a text id, and the entity of a text.
#[test]
fn p12_text_is_not_toggled() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let text = s
        .sketch_text(sk, &TextSpec { text: "A".into(), at: [0.0, 0.0], height: 8.0, angle: 0.0, font: None, construction: false })
        .unwrap();
    let path = scratch("p12.qcad");
    s.save(Some(&path)).unwrap();
    let mut m = native_assembly_load(&path);
    let si = 0;
    let before: Vec<bool> = m.project.sketches[si].texts.iter().map(|t| t.construction).collect();
    let now = m.project.toggle_construction(si, &[text]);
    let after: Vec<bool> = m.project.sketches[si].texts.iter().map(|t| t.construction).collect();
    eprintln!("P12 text {text}: before {before:?} toggle returned {now} after {after:?}");
    let r = s.sketch_construction(sk, &[text], true);
    eprintln!("P12 server: {:?}", r.map(|_| ()).map_err(|e| e.to_string()));
}
