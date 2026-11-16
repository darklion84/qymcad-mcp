//! Sheet bodies (T0): reports, the false void warning, default bodies, and what each tool does on a loft sheet.
use crate::common;
use qymcad_engine::*;

fn v(x: f64) -> Num {
    Num::Value(x)
}

fn extrude(sketch: Id, height: f64, op: Op) -> Extrude {
    serde_json::from_value(serde_json::json!({"sketch": sketch, "height": height, "op": op})).unwrap()
}

/// A ruled loft sheet between a 20×16 rectangle on XY and the same one `h` above: a tube of four planar faces,
/// area 2·(20+16)·h, open at both ends.
fn loft_sheet(s: &mut Session, h: f64) -> Id {
    let low = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(low, &v(0.), &v(0.), &v(20.), &v(16.), false).unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &v(h), None).unwrap();
    let high = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    s.sketch_rect(high, &v(0.), &v(0.), &v(20.), &v(16.), false).unwrap();
    let sections = vec![LoftSection { sketch: low, profile: None }, LoftSection { sketch: high, profile: None }];
    let (_, r) = s.loft(&Loft { sections, ruled: true, op: Op::NewBody, target: None, surface: true, name: None }).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r.bodies.last().unwrap().id
}

fn info(s: &Session, body: Id) -> BodyInfo {
    s.result_bodies().into_iter().find(|b| b.id == body).unwrap_or_else(|| panic!("body {body} is not current"))
}

/// A solid box `w`×`d`×`h` from the XY plane up, centred on the origin, as a body of its own.
fn box_xy(s: &mut Session, w: f64, d: f64, h: f64) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(0.), &v(0.), &v(w), &v(d), false).unwrap();
    let (_, r) = s.extrude(&extrude(sk, h, Op::NewBody)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r.bodies.last().unwrap().id
}

fn close(actual: f64, expected: f64, tol: f64, what: &str) {
    assert!((actual - expected).abs() <= tol, "{what}: got {actual}, expected {expected} ± {tol}");
}

// ---- (a) reports ----

/// The tube: 2·(20+16)·10 = 720 mm² of surface, no volume, flagged as a sheet; a solid beside it is not.
#[test]
fn a_loft_sheet_reports_sheet_area_and_no_volume_and_a_solid_does_not() {
    let mut s = Session::new_part();
    let sheet = loft_sheet(&mut s, 10.);
    let solid = box_xy(&mut s, 12., 12., 6.);
    for b in s.result_bodies() {
        if b.id == sheet {
            assert!(b.sheet);
            close(b.area, 720., 1e-6, "tube area");
            assert_eq!(b.volume, 0.);
        } else {
            assert_eq!(b.id, solid);
            assert!(!b.sheet);
            close(b.area, 2. * 144. + 4. * 72., 1e-6, "block area");
            close(b.volume, 864., 1e-6, "block volume");
        }
    }
    let json = serde_json::to_value(info(&s, sheet)).unwrap();
    assert_eq!(json["sheet"], true);
    close(json["area_mm2"].as_f64().unwrap(), 720., 1e-6, "area_mm2");
    assert_eq!(json["volume_mm3"], 0.0);
    let json = serde_json::to_value(info(&s, solid)).unwrap();
    assert_eq!(json["sheet"], false);
    close(json["area_mm2"].as_f64().unwrap(), 576., 1e-6, "block area_mm2");
    // The rebuild entry a tool returns is the same record.
    let rebuilt = s.current_rebuild();
    assert!(rebuilt.bodies.iter().any(|b| b.id == sheet && b.sheet && (b.area - 720.).abs() < 1e-6));
    // The document stores the flag and the faces: a file opened without a rebuild reports the same.
    let path = common::scratch("t0-sheet-report.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, _) = Session::open(&path).unwrap();
    let b = info(&opened, sheet);
    assert!(b.sheet);
    close(b.area, 720., 1e-6, "area after open");
}

/// Export: a sheet goes out like any body, says it is an open surface and carries no tetrahedron-sum volume; a solid keeps it.
#[test]
fn export_marks_a_sheet_and_leaves_out_its_meaningless_mesh_volume() {
    let mut s = Session::new_part();
    let sheet = loft_sheet(&mut s, 10.);
    let solid = box_xy(&mut s, 12., 12., 6.);
    for (format, name) in [(ExportFormat::Stl, "t0-sheet.stl"), (ExportFormat::Step, "t0-sheet.step")] {
        let r = s.export(format, &common::scratch(name), Quality::Standard, None).unwrap();
        let one = r.bodies.iter().find(|b| b.id == sheet).unwrap();
        assert!(one.sheet);
        close(one.area_mm2, 720., 1e-6, "exported sheet area");
        assert_eq!(one.mesh_volume_mm3, None, "a sheet has no mesh volume");
        assert!(one.note.is_some_and(|n| n.contains("open surface")), "{one:?}");
        let other = r.bodies.iter().find(|b| b.id == solid).unwrap();
        assert!(!other.sheet && other.note.is_none());
        close(other.area_mm2, 576., 1e-6, "exported block area");
        assert_eq!(other.mesh_volume_mm3.is_some(), format.is_mesh());
    }
}

// ---- (b) the false warning ----

/// A sheet has a shell and no solid; the rebuild and the open both used to warn of a sealed void.
#[test]
fn a_sheet_is_not_a_sealed_void() {
    let mut s = Session::new_part();
    let low = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(low, &v(0.), &v(0.), &v(20.), &v(16.), false).unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &v(10.), None).unwrap();
    let high = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    s.sketch_rect(high, &v(0.), &v(0.), &v(20.), &v(16.), false).unwrap();
    let sections = vec![LoftSection { sketch: low, profile: None }, LoftSection { sketch: high, profile: None }];
    let (_, r) = s.loft(&Loft { sections, ruled: true, op: Op::NewBody, target: None, surface: true, name: None }).unwrap();
    let sealed = |warnings: Vec<String>| warnings.into_iter().filter(|w| w.contains("sealed")).collect::<Vec<_>>();
    assert!(sealed(r.warnings.iter().map(|w| w.to_string()).collect()).is_empty(), "{:?}", r.warnings);
    assert!(sealed(s.info().warnings).is_empty(), "{:?}", s.info().warnings);
    let path = common::scratch("t0-sheet-warning.qcad");
    s.save(Some(&path)).unwrap();
    let (opened, r) = Session::open(&path).unwrap();
    assert!(sealed(r.warnings.iter().map(|w| w.to_string()).collect()).is_empty(), "{:?}", r.warnings);
    assert!(sealed(opened.info().warnings).is_empty(), "{:?}", opened.info().warnings);
}

// ---- (c) defaults skip sheets ----

/// A solid block `w`×`w`×`h` from the XY plane up, centred on the origin.
fn block(s: &mut Session, w: f64, h: f64) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(0.), &v(0.), &v(w), &v(w), false).unwrap();
    let (_, r) = s.extrude(&extrude(sk, h, Op::Add)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r.bodies.last().unwrap().id
}

/// A sheet after a solid: `extrude op:add` without a target lands on the solid (it failed with «Boolean» on the sheet).
#[test]
fn extrude_add_without_a_target_lands_on_the_solid_not_the_sheet() {
    let mut s = Session::new_part();
    let solid = block(&mut s, 40., 10.);
    let sheet = loft_sheet(&mut s, 10.);
    assert_eq!(s.result_bodies().iter().map(|b| b.id).collect::<Vec<_>>(), vec![solid, sheet]);
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &v(0.), &v(0.), &v(10.), &v(10.), false).unwrap();
    let (_, r) = s.extrude(&extrude(sk, 20., Op::Add)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let bodies = s.result_bodies();
    assert_eq!(bodies.len(), 2, "the sheet stays beside the solid: {bodies:?}");
    let grown = bodies.iter().find(|b| !b.sheet).unwrap();
    close(grown.volume, 40. * 40. * 10. + 10. * 10. * 10., 1e-6, "the boss grew from the solid (z 10..20 over the block)");
    assert!(bodies.iter().any(|b| b.id == sheet && b.sheet), "the sheet is untouched");
}

/// The default body of every tool that takes none is the solid, also when a sheet was made last.
#[test]
fn the_default_body_of_a_tool_is_the_solid_when_a_sheet_came_last() {
    let mut s = Session::new_part();
    let solid = block(&mut s, 40., 10.);
    let sheet = loft_sheet(&mut s, 10.);
    assert_eq!(s.topology(None, false).unwrap().body, solid);
    assert_eq!(s.topology(Some(sheet), false).unwrap().body, sheet, "an explicit sheet is still served");
    // Default fillet on all top edges: the solid is rounded, no «solid tool on a sheet».
    let (_, r) = s.fillet(None, &Sel::EdgesOf(Box::new(Sel::Facing { dir: [0., 0., 1.], tol_deg: 1. })), &v(2.), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let rounded = s.result_bodies().into_iter().find(|b| !b.sheet).unwrap();
    // Four rim edges, (1 − π/4)·r² each per mm: 4·(1 − π/4)·4·36 = 123.6 mm³ without the corners, 137.4 with the whole 40 mm.
    let removed = 16000. - rounded.volume;
    assert!((123. ..138.).contains(&removed), "the top rim of the solid is rounded: {removed}");
}

/// With nothing but a sheet in the document the sheet is still the default (the only body), so thicken and topology work on it.
#[test]
fn a_document_of_one_sheet_still_defaults_to_it() {
    let mut s = Session::new_part();
    let sheet = loft_sheet(&mut s, 10.);
    assert_eq!(s.topology(None, false).unwrap().body, sheet);
}

/// TR 5: the exact rule, as the tool texts now state it. A tool without `body` takes the part's last solid; a sheet is the default only
/// when it is the only body of the document. Two sheets (no solid anywhere), or a sheet in the active Part and a solid in another, give no
/// default: the caller must name the body. Tools that make a sheet (`face_copy`) and read topology take the sole sheet.
#[test]
fn a_sheet_is_the_default_only_as_the_only_body_of_the_document() {
    // One sheet: topology, face_copy and thicken all take it.
    let mut s = Session::new_part();
    let sheet = loft_sheet(&mut s, 10.);
    assert_eq!(s.topology(None, false).unwrap().body, sheet);
    let face = s.topology(None, false).unwrap().faces[0].id;
    let (copy, ids, r) = s.face_copy(None, &Sel::Ids(vec![face]), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(ids, vec![face]);
    assert!(info(&s, copy).sheet, "a copy of a face of the sole sheet");
    // Two sheets and no solid: nothing is the default.
    let before = serde_json::to_string(s.project()).unwrap();
    let e = refused(s.topology(None, false));
    assert!(
        e.contains("pass body") && e.contains("no solid body") && e.contains(&format!("({sheet}, sheet)")),
        "TR 9, not 'has no body': {e}"
    );
    let e = refused(s.face_copy(None, &Sel::Ids(vec![face]), None));
    assert!(e.contains("pass body"), "{e}");
    assert_eq!(serde_json::to_string(s.project()).unwrap(), before);
    // A sheet in the active Part and a solid in another Part: the active Part has no solid, and the sheet is not the only body.
    let mut s = Session::new_part();
    let sheet = loft_sheet(&mut s, 10.);
    let sheet_part = s.project().body_owner(sheet).unwrap();
    s.part_new("other").unwrap();
    let solid = block(&mut s, 20., 10.);
    assert_eq!(s.topology(None, false).unwrap().body, solid, "the other Part is active: its solid");
    s.part_select(&PartRef::Id(sheet_part)).unwrap();
    let e = refused(s.topology(None, false));
    assert!(e.contains("pass body"), "{e}");
    assert_eq!(s.topology(Some(sheet), false).unwrap().body, sheet, "named, it is served");
}

// ---- (d) the probe: what each tool does on a loft sheet ----

/// A sheet, a fresh Part holding only it.
fn fresh() -> (Session, Id) {
    let mut s = Session::new_part();
    let sheet = loft_sheet(&mut s, 10.);
    (s, sheet)
}

fn refused(r: Result<impl std::fmt::Debug>) -> String {
    r.unwrap_err().to_string()
}

/// Ids of the tube's faces: -y, +x, +y, -x (areas 200, 160, 200, 160).
fn faces(s: &mut Session, sheet: Id) -> Vec<u32> {
    s.topology(Some(sheet), false).unwrap().faces.iter().map(|f| f.id).collect()
}

// The tube is 20×16 by 10 high, 720 mm² of four planar faces, open at both ends. The rows below are the ADR 0059 table.

fn boolean(s: &mut Session, a: Id, b: Id, op: BooleanOp) -> Result<BodyBooleanResult> {
    s.body_boolean(&BodyBooleanOptions { a, b, op, name: None, piece_names: None })
}

/// body_boolean: the app cannot take a sheet as A (the status says "select body A first" and the sheet is not selectable), so a sheet
/// as A is refused by the server (F-200), whatever the operation. A sheet as B is the app's own native failure, kept: cut, intersect
/// and union all fail and roll back (the core API alone would let a sheet cut a solid: 720 − 2·12·6 = 576, which the app cannot do).
#[test]
fn probe_body_boolean_refuses_a_sheet_as_a_and_the_kernel_refuses_it_as_b() {
    let make = || {
        let mut s = Session::new_part();
        let sheet = loft_sheet(&mut s, 10.);
        let solid = box_xy(&mut s, 30., 12., 6.);
        (s, sheet, solid)
    };
    for op in [BooleanOp::Cut, BooleanOp::Union, BooleanOp::Intersect] {
        let (mut s, sheet, solid) = make();
        let before = serde_json::to_string(s.project()).unwrap();
        let e = refused(boolean(&mut s, sheet, solid, op));
        assert!(e.contains(&format!("body {sheet} is a sheet")) && e.contains("F-200") && e.contains("body A"), "{op:?}: {e}");
        assert_eq!(serde_json::to_string(s.project()).unwrap(), before, "{op:?}: sheet as A, unchanged");
        let e = refused(boolean(&mut s, solid, sheet, op));
        assert!(!e.contains("F-200"), "{op:?}: a sheet as B is the kernel's own refusal: {e}");
        assert_eq!(serde_json::to_string(s.project()).unwrap(), before, "{op:?}: sheet as B, rolled back");
    }
}

/// move, linear and circular arrays act on a sheet like on any body (the GUI takes a sheet as the object of both arrays); it stays a
/// sheet and its area is the sum of the copies. Mirror is not here: the GUI mirrors the solid, a sheet only gives the plane (F-200).
#[test]
fn probe_move_and_arrays_keep_a_sheet_a_sheet() {
    let (mut s, sheet) = fresh();
    let (m, r) = s.move_body(&Move { body: Some(sheet), translation: Some([v(5.), v(0.), v(3.)]), rotation: None, name: None }).unwrap();
    assert!(r.errors.is_empty());
    let b = info(&s, m.body);
    assert!(b.sheet);
    close(b.area, 720., 1e-6, "moved area");
    close(b.bbox[0], -5., 1e-3, "x min after +5");
    close(b.bbox[2], 3., 1e-3, "z min after +3");

    let (mut s, sheet) = fresh();
    let (id, _) = s.linear_array(Some(sheet), &ArrayDir { dx: v(30.), dy: v(0.), dz: v(0.), count: v(3.) }, None, None).unwrap();
    let b = info(&s, id);
    assert!(b.sheet);
    close(b.area, 2160., 1e-6, "three copies");
    close(b.bbox[3], 70., 1e-3, "last copy");
    let (mut s, sheet) = fresh();
    let axis = AxisRef::Through { origin: [40., 0., 0.], dir: [0., 0., 1.] };
    let (id, _) = s.circular_array(Some(sheet), &v(3.), &v(360.), Some(&axis), None).unwrap();
    let b = info(&s, id);
    assert!(b.sheet);
    close(b.area, 2160., 1e-3, "three turned copies");
}

/// Refused by QymCAD itself, the document unchanged: fillet (`solid tool on a sheet`) and push_face (`push face on a sheet`).
#[test]
fn probe_tools_the_kernel_refuses_on_a_sheet_stay_refused_and_change_nothing() {
    let (mut s, sheet) = fresh();
    let side = faces(&mut s, sheet)[1];
    let before = serde_json::to_string(&s.result_bodies()).unwrap();
    let e = refused(s.fillet(Some(sheet), &Sel::Kind(SelectionKind::Line), &v(1.), None));
    assert!(e.contains("solid tool on a sheet"), "{e}");
    let e = refused(s.push_face(Some(sheet), &Sel::Ids(vec![side]), &v(2.), None));
    assert!(e.contains("push face on a sheet"), "{e}");
    assert_eq!(serde_json::to_string(&s.result_bodies()).unwrap(), before);
}

/// shell: QymCAD returns a smaller sheet instead of a wall (F-196); the server refuses and names the sheet.
#[test]
fn shell_of_a_sheet_is_refused_with_the_document_unchanged_f196() {
    let (mut s, sheet) = fresh();
    let side = faces(&mut s, sheet)[0];
    let before = serde_json::to_string(s.project()).unwrap();
    for side_kind in [Side::Inward, Side::Outward, Side::Centred] {
        let e = refused(s.shell(Some(sheet), &Sel::Ids(vec![side]), &v(1.), side_kind, None));
        assert!(e.contains(&format!("body {sheet} is a sheet")) && e.contains("F-196") && e.contains("thicken"), "{e}");
    }
    assert_eq!(serde_json::to_string(s.project()).unwrap(), before);
    // A solid still hollows.
    let solid = box_xy(&mut s, 20., 16., 10.);
    let top = s.topology(Some(solid), false).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.9)).unwrap().id;
    let (_, r) = s.shell(Some(solid), &Sel::Ids(vec![top]), &v(1.), Side::Inward, None).unwrap();
    assert!(r.errors.is_empty());
}

/// chamfer: QymCAD's GUI offers no chamfer on a sheet (checked in the app 2026-11-16: "no body in the part", sheet edges
/// cannot be picked). Through the core API it bevels shared edges only and skips boundary edges silently (F-197, pinned in
/// t0_probes). Every selection on a sheet is refused, the document unchanged; a solid still chamfers.
#[test]
fn chamfer_on_a_sheet_is_refused_as_in_the_app_f197() {
    let (mut s, sheet) = fresh();
    let t = s.topology(Some(sheet), true).unwrap();
    let shared: Vec<u32> = t.edges.iter().filter(|e| e.faces.is_some()).map(|e| e.id).collect();
    let boundary: Vec<u32> = t.edges.iter().filter(|e| e.faces.is_none()).map(|e| e.id).collect();
    assert_eq!((shared.len(), boundary.len()), (4, 8));
    let before = serde_json::to_string(s.project()).unwrap();
    for pick in [vec![shared[0]], vec![boundary[0]], [shared.clone(), boundary.clone()].concat()] {
        let e = refused(s.chamfer(Some(sheet), &Sel::Ids(pick), &v(1.), None, None));
        assert!(e.contains(&format!("body {sheet} is a sheet")) && e.contains("F-197"), "{e}");
    }
    let e = refused(s.chamfer(Some(sheet), &Sel::Kind(SelectionKind::Line), &v(1.), None, None));
    assert!(e.contains("F-197"), "{e}");
    assert_eq!(serde_json::to_string(s.project()).unwrap(), before);
    let solid = box_xy(&mut s, 20., 16., 10.);
    let (_, r) = s.chamfer(Some(solid), &Sel::Kind(SelectionKind::Line), &v(1.), None, None).unwrap();
    assert!(r.errors.is_empty());
}

// ---- (d2) the tools the GUI does not offer on a sheet (F-200) ----

/// A hole through the top of a solid, as the plain control for `hole`.
fn hole_args(body: Id, face: u32) -> Hole {
    Hole {
        body: Some(body),
        face: Sel::Ids(vec![face]),
        at: None,
        offset: None,
        diameter: v(4.),
        depth: Some(v(6.)),
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: None,
    }
}

fn face_with_normal(s: &mut Session, body: Id, axis: usize, sign: f64) -> u32 {
    s.topology(Some(body), false).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[axis] * sign > 0.9)).unwrap().id
}

/// The tester's GUI check (2026-11-16): on a Part holding a solid and a sheet, hole, draft, remove_face, split, split_face, mirror and a
/// boolean with the sheet as A do not take the sheet (the GUI answers "nothing there" or cannot select it). Each is refused here, naming the
/// body as a sheet, with the document unchanged; the same tool on the solid beside it works, and a sheet's FACE still serves as a plane.
#[test]
fn the_tools_the_gui_does_not_offer_on_a_sheet_are_refused_with_the_document_unchanged_f200() {
    let (mut s, sheet) = fresh();
    let solid = box_xy(&mut s, 40., 40., 20.);
    let side = face_with_normal(&mut s, sheet, 0, 1.);
    let sheet_top_edge_face = faces(&mut s, sheet)[0];
    let marks = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_point(marks, &v(0.), &v(0.), false).unwrap();
    s.rebuild();
    let before = serde_json::to_string(s.project()).unwrap();
    let check = |what: &str, e: String| {
        assert!(e.contains(&format!("body {sheet} is a sheet")) && e.contains("F-200"), "{what}: {e}");
    };
    check("hole", refused(s.hole(&hole_args(sheet, side))));
    let by_sketch = HoleSketch {
        body: Some(sheet),
        sketch: marks,
        points: None,
        flip: false,
        diameter: v(4.),
        depth: Some(v(6.)),
        kind: HoleKind::Plain,
        dia2: None,
        depth2: None,
        name: None,
    };
    check("hole by sketch", refused(s.hole_by_sketch(&by_sketch)));
    let draft = Draft {
        body: Some(sheet),
        faces: Sel::Ids(vec![side]),
        neutral: Sel::Ids(vec![sheet_top_edge_face]),
        angle: v(5.),
        flip: false,
        name: None,
    };
    check("draft", refused(s.draft(&draft)));
    check("remove_face", refused(s.remove_face(Some(sheet), &Sel::Ids(vec![side]), None)));
    for plane in [PlaneRef::Base(BaseName::XY), PlaneRef::Face { body: sheet, face: side }] {
        let split = SplitOptions { body: Some(sheet), plane: plane.clone(), offset: v(5.), name: None, piece_names: None };
        check("split", refused(s.split(&split)));
        let split_face = SplitFaceOptions { body: Some(sheet), plane, offset: v(5.), name: None };
        check("split_face", refused(s.split_face(&split_face)));
    }
    for keep in [true, false] {
        check("mirror", refused(s.mirror(Some(sheet), &PlaneRef::Base(BaseName::YZ), keep, None)));
        // A sheet as the object is refused also when the plane is a face of the solid.
        let top = face_with_normal(&mut s, solid, 2, 1.);
        check("mirror, face plane", refused(s.mirror(Some(sheet), &PlaneRef::Face { body: solid, face: top }, keep, None)));
    }
    for op in [BooleanOp::Cut, BooleanOp::Union, BooleanOp::Intersect] {
        check("body_boolean", refused(boolean(&mut s, sheet, solid, op)));
    }
    assert_eq!(serde_json::to_string(s.project()).unwrap(), before, "no refusal changed the document");

    // The solid beside the sheet still takes each of them.
    let top = face_with_normal(&mut s, solid, 2, 1.);
    let (hole, r) = s.hole(&hole_args(solid, top)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let (x, bottom) = (face_with_normal(&mut s, hole, 0, 1.), face_with_normal(&mut s, hole, 2, -1.));
    let draft =
        Draft { body: Some(hole), faces: Sel::Ids(vec![x]), neutral: Sel::Ids(vec![bottom]), angle: v(5.), flip: false, name: None };
    let (_, r) = s.draft(&draft).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
}

/// remove_face and the split/mirror family on the solid: the same tools, the solid as the object, a sheet's face as the plane.
#[test]
fn a_sheets_face_still_works_as_a_plane_for_split_split_face_and_mirror_on_a_solid_f200() {
    let plane_of_sheet = |s: &mut Session, sheet: Id| PlaneRef::Face { body: sheet, face: face_with_normal(s, sheet, 0, 1.) };
    // split: the +x wall of the tube is the plane x = 10; the block spans x -20..20.
    let (mut s, sheet) = fresh();
    let solid = box_xy(&mut s, 40., 40., 20.);
    let plane = plane_of_sheet(&mut s, sheet);
    let r = s.split(&SplitOptions { body: Some(solid), plane, offset: v(0.), name: None, piece_names: None }).unwrap();
    assert_eq!(r.pieces.len(), 2);
    // split_face
    let (mut s, sheet) = fresh();
    let solid = box_xy(&mut s, 40., 40., 20.);
    let plane = plane_of_sheet(&mut s, sheet);
    let r = s.split_face(&SplitFaceOptions { body: Some(solid), plane, offset: v(0.), name: None }).unwrap();
    assert!(r.faces.after > r.faces.before);
    // mirror: the block, about x = 10
    let (mut s, sheet) = fresh();
    let solid = box_xy(&mut s, 40., 40., 20.);
    let plane = plane_of_sheet(&mut s, sheet);
    let (id, r) = s.mirror(Some(solid), &plane, false, None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    close(info(&s, id).bbox[0], 0., 1e-3, "the block 20..-20 mirrored about x=10 spans 0..40");
    // remove_face: a solid with a blind hole heals; the same node is refused on the sheet.
    let (mut s, sheet) = fresh();
    let solid = box_xy(&mut s, 40., 40., 20.);
    let top = face_with_normal(&mut s, solid, 2, 1.);
    let (hole, _) = s.hole(&hole_args(solid, top)).unwrap();
    let t = s.topology(Some(hole), false).unwrap();
    let wall = t.faces.iter().find(|f| f.kind == FaceKind::Cylinder).unwrap().id;
    let bottom = t.faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99) && (f.centroid[2] - 14.).abs() < 1e-6).unwrap().id;
    let (_, _, r) = s.remove_face(Some(hole), &Sel::Ids(vec![wall, bottom]), None).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(s.result_bodies().iter().any(|b| b.id == sheet && b.sheet), "the sheet is untouched");
}

/// A node a GUI file holds on a sheet (the app builds Shell/Draft/RemoveFace there when the file is made through the core API) is not
/// judged on open, but a re-pick (`feature`) of it applies the same refusal as creation, the document unchanged.
#[test]
fn repicking_a_shell_draft_or_remove_face_node_on_a_sheet_is_refused_f200() {
    use qymcad_core::feature::ShellSide;
    let (mut s, sheet) = fresh();
    let path = common::scratch("t0-f200-repick-src.qcad");
    s.save(Some(&path)).unwrap();
    let mut model = common::gui_reorder_load(&path);
    let t = model.project.regen_faces[&sheet].iter().map(|f| (f.id, f.normal)).collect::<Vec<_>>();
    let pick = |axis: usize| t.iter().find(|(_, n)| n[axis] > 0.9).unwrap().0;
    let (x, y) = (pick(0), pick(1));
    let shell = model.project.add_shell_mode(sheet, 1., vec![x], ShellSide::Inward);
    let draft = model.project.add_draft(sheet, vec![x], y, 5., false);
    let face = model.project.regen_faces[&sheet].iter().find(|f| f.id == x).unwrap();
    let key = qymcad_core::feature::FaceKey {
        index: 0,
        centroid: [face.centroid.x, face.centroid.y, face.centroid.z],
        normal: face.normal,
        id: x,
    };
    let remove = model.project.add_remove_face(sheet, vec![key]);
    let breps: Vec<(Id, Vec<u8>)> = {
        let _gate = qymcad_kernel::kernel_gate();
        model.shapes.iter().filter_map(|(id, sh)| sh.to_brep_bytes().map(|b| (*id, b))).collect()
    };
    let with_nodes = common::scratch("t0-f200-repick.qcad");
    qymcad_io::save_project_with_brep(&model.project, with_nodes.to_str().unwrap(), &breps).unwrap();
    let (mut s, _) = Session::open(&with_nodes).unwrap();
    let before = serde_json::to_string(s.project()).unwrap();
    for (what, e) in [
        ("shell", refused(s.repick_shell(shell, &Sel::Ids(vec![y])))),
        ("draft", refused(s.repick_draft(draft, &Sel::Ids(vec![x]), None))),
        ("remove_face", refused(s.repick_remove_face(remove, &Sel::Ids(vec![x])))),
    ] {
        assert!(e.contains(&format!("body {sheet} is a sheet")) && e.contains("F-200"), "{what}: {e}");
    }
    assert_eq!(serde_json::to_string(s.project()).unwrap(), before);
}

// ---- (e) thicken of a sheet ----

/// thicken of a sheet, both signs (the app's command takes either): on this fixture (normals outward) +1 grows outward, -1 inward; the
/// sheet becomes a solid. The side is the sheet's own orientation (see `a_sheets_thicken_follows_its_own_face_normal_...`).
/// Hand: outward (22×18 − 20×16)·10 less the four rounded corners (4 − π)·10 = 751.416; inward (20×16 − 18×14)·10 = 680.
#[test]
fn thicken_of_a_sheet_goes_either_way_and_makes_a_solid() {
    let thicken = |t: f64| {
        let (mut s, sheet) = fresh();
        let side = faces(&mut s, sheet)[1];
        let (id, r) = s.thicken(Some(sheet), &Sel::Ids(vec![side]), &v(t), None).unwrap();
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(s.result_bodies().len(), 1, "the sheet is replaced by the solid");
        (s, id)
    };
    let (s, id) = thicken(1.);
    let b = info(&s, id);
    assert!(!b.sheet);
    close(b.volume, 760. - 10. * (4. - std::f64::consts::PI), 1e-3, "outward plate");
    close(b.bbox[3], 11., 1e-3, "grows past the sheet");
    let (s, id) = thicken(-1.);
    let b = info(&s, id);
    assert!(!b.sheet);
    close(b.volume, 680., 1e-3, "inward plate");
    close(b.bbox[3], 10., 1e-3, "stays inside the sheet");
    // Zero and values past the app's range are refused, the document unchanged.
    let (mut s, sheet) = fresh();
    let side = faces(&mut s, sheet)[1];
    let before = serde_json::to_string(s.project()).unwrap();
    for t in [0., 1e-12, 100_001., -100_001.] {
        let e = refused(s.thicken(Some(sheet), &Sel::Ids(vec![side]), &v(t), None));
        assert!(e.contains("sheet") && e.contains("either sign"), "{t}: {e}");
    }
    assert_eq!(serde_json::to_string(s.project()).unwrap(), before);
}

/// The sign of a sheet's thicken is the face's own normal, as in the app (no flip anywhere: `apply_thicken_cmd` passes the number to
/// `add_thicken`, the kernel offsets along the sheet's orientation). Which way that is on a loft depends on the order of its sections:
/// the tester's GUI tube had its -x wall (x = -10) with the normal [1, 0, 0], so +2 kept the outer box 20 x 16 x 10, the same as the
/// server gives for the same input on a loft made from the top section down. Whichever face of the sheet is picked, the WHOLE sheet is
/// offset (all four walls give the same solid).
#[test]
fn a_sheets_thicken_follows_its_own_face_normal_and_takes_the_whole_sheet() {
    let tube = |top_first: bool| {
        let mut s = Session::new_part();
        let low = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(low, &v(0.), &v(0.), &v(20.), &v(16.), false).unwrap();
        let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &v(10.), None).unwrap();
        let high = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
        s.sketch_rect(high, &v(0.), &v(0.), &v(20.), &v(16.), false).unwrap();
        let order = if top_first { [high, low] } else { [low, high] };
        let sections = order.iter().map(|&sketch| LoftSection { sketch, profile: None }).collect();
        let (_, r) = s.loft(&Loft { sections, ruled: true, op: Op::NewBody, target: None, surface: true, name: None }).unwrap();
        let sheet = r.bodies.last().unwrap().id;
        (s, sheet)
    };
    for (top_first, normal_x, bbox_x) in [(false, -1., 12.), (true, 1., 10.)] {
        let mut volumes = Vec::new();
        for face in 0..4 {
            let (mut s, sheet) = tube(top_first);
            let t = s.topology(Some(sheet), false).unwrap();
            let wall = t.faces.iter().find(|f| f.centroid[0] < -9.9 && f.normal.is_some_and(|n| n[0].abs() > 0.9)).unwrap();
            close(wall.normal.unwrap()[0], normal_x, 1e-9, "normal of the -x wall");
            let (id, r) = s.thicken(Some(sheet), &Sel::Ids(vec![t.faces[face].id]), &v(2.), None).unwrap();
            assert!(r.errors.is_empty(), "{:?}", r.errors);
            let b = info(&s, id);
            assert!(!b.sheet);
            close(b.bbox[3], bbox_x, 1e-3, &format!("outer x max, sections top_first={top_first}, face {face}"));
            volumes.push(b.volume);
        }
        assert!(volumes.iter().all(|x| (x - volumes[0]).abs() < 1e-6), "the whole sheet is offset whichever face is picked: {volumes:?}");
    }
}

/// A solid keeps F-150: positive only. And a sheet's thicken edits to a negative value through `dimension_set` and a parameter.
#[test]
fn only_a_sheets_thicken_may_be_edited_negative_and_a_solids_stays_positive() {
    let (mut s, sheet) = fresh();
    let side = faces(&mut s, sheet)[1];
    s.param_set("t", &v(1.)).unwrap();
    let (id, _) = s.thicken(Some(sheet), &Sel::Ids(vec![side]), &Num::Expr("t".into()), None).unwrap();
    let r = s.param_set("t", &v(-2.)).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    close(info(&s, id).volume, (320. - 16. * 12.) * 10., 1e-3, "inward 2 mm");
    s.dimension_set(&DimensionTarget::Feature { feature: id, key: "thickness".into() }, Some(&v(-1.)), None).unwrap();
    assert!(s.dimension_set(&DimensionTarget::Feature { feature: id, key: "thickness".into() }, Some(&v(0.)), None).is_err());
    // The solid case.
    let mut s = Session::new_part();
    let solid = block(&mut s, 20., 10.);
    let top = s.topology(Some(solid), false).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.9)).unwrap().id;
    let e = refused(s.thicken(Some(solid), &Sel::Ids(vec![top]), &v(-1.), None));
    assert!(e.contains("F-150"), "{e}");
}

// ---- (g) a document saved without faces ----

/// A file the app saved without stored faces has none cached until its topology is read; the area still comes from the kernel
/// (it was 0 there, and `doc_info` changed when a tool read the topology).
#[test]
fn area_does_not_depend_on_the_face_cache() {
    use std::io::{Read, Write};
    let mut s = Session::new_part();
    let solid = block(&mut s, 12., 5.);
    let original = common::scratch("t0-area-with-faces.qcad");
    s.save(Some(&original)).unwrap();
    let legacy = common::scratch("t0-area-no-faces.qcad");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&legacy).unwrap());
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&original).unwrap()).unwrap();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        if entry.name().starts_with("faces/") {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        zip.start_file(entry.name().to_string(), zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap();
    let (mut opened, _) = Session::open(&legacy).unwrap();
    let before = info(&opened, solid).area;
    close(before, 2. * 144. + 4. * 60., 1e-3, "area before any topology call");
    let t = opened.topology(Some(solid), false).unwrap();
    close(t.faces.iter().map(|f| f.area).sum::<f64>(), before, 1e-3, "the same once the faces are read");
    assert_eq!(info(&opened, solid).area, before);
}

// ---- (h) a mesh body's area in export ----

/// The faces of an imported mesh body written to a `.qcad`, left out again: what a file saved without `faces/` looks like.
fn tetra_without_faces() -> (Session, Id, f64) {
    use std::io::{Read, Write};
    let dir = std::env::temp_dir().join(format!("qymcad-tr6-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stl = dir.join("tetra.stl");
    // Three right-angled faces of 50 and one equilateral face of side 10 sqrt 2: 150 + 86.6025 mm2.
    std::fs::write(
        &stl,
        "solid t\n facet normal 0 0 0\n  outer loop\n   vertex 0 0 0\n   vertex 10 0 0\n   vertex 0 10 0\n  endloop\n endfacet\n \
         facet normal 0 0 0\n  outer loop\n   vertex 0 0 0\n   vertex 0 10 0\n   vertex 0 0 10\n  endloop\n endfacet\n \
         facet normal 0 0 0\n  outer loop\n   vertex 0 0 0\n   vertex 0 0 10\n   vertex 10 0 0\n  endloop\n endfacet\n \
         facet normal 0 0 0\n  outer loop\n   vertex 10 0 0\n   vertex 0 0 10\n   vertex 0 10 0\n  endloop\n endfacet\nendsolid t\n",
    )
    .unwrap();
    let mut s = Session::new_part();
    let mesh = s.import_mesh(&stl, None, None, 1.0, None).unwrap().pieces[0].body;
    let expected = 150. + 3f64.sqrt() / 4. * 200.;
    close(info(&s, mesh).area, expected, 1e-3, "doc_info of the import");
    let original = common::scratch("tr6-with-faces.qcad");
    s.save(Some(&original)).unwrap();
    let legacy = common::scratch("tr6-no-faces.qcad");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&legacy).unwrap());
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&original).unwrap()).unwrap();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        if entry.name().starts_with("faces/") {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        zip.start_file(entry.name().to_string(), zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    let (opened, _) = Session::open(&legacy).unwrap();
    (opened, mesh, expected)
}

/// TR 6: `export` and `doc_info` give a mesh body the same `area_mm2`, the sum of its triangles, also when the body has no cached faces
/// (a file saved without `faces/`: `export` said 0 where `doc_info` summed the triangles) and for a freshly imported one.
#[test]
fn export_reports_the_triangle_area_of_a_mesh_body_like_doc_info() {
    let (s, mesh, expected) = tetra_without_faces();
    assert!(s.project().bodies.iter().find(|b| b.id == mesh).unwrap().faces.is_empty(), "setup: no cached faces");
    close(info(&s, mesh).area, expected, 1e-3, "doc_info");
    for (format, name) in [(ExportFormat::Stl, "mesh.stl"), (ExportFormat::ThreeMf, "mesh.3mf")] {
        let report = s.export(format, &common::scratch(name), Quality::Standard, None).unwrap();
        let body = report.bodies.iter().find(|b| b.id == mesh).unwrap();
        close(body.area_mm2, expected, 1e-3, &format!("export {name}"));
        assert!(!body.sheet);
    }
}
