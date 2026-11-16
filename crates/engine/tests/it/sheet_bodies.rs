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

/// body_boolean: a sheet cut by a solid that pierces it keeps the sheet's part outside the solid (hand: 720 − 2·12·6 = 576).
/// Every other combination is refused by the kernel; the document stays as it was.
#[test]
fn probe_body_boolean_on_a_sheet_cuts_the_sheet_and_refuses_the_rest() {
    let make = || {
        let mut s = Session::new_part();
        let sheet = loft_sheet(&mut s, 10.);
        let solid = box_xy(&mut s, 30., 12., 6.);
        (s, sheet, solid)
    };
    let (mut s, sheet, solid) = make();
    let r = boolean(&mut s, sheet, solid, BooleanOp::Cut).unwrap();
    assert!(r.rebuild.errors.is_empty());
    let bodies = s.result_bodies();
    assert_eq!(bodies.len(), 1, "the tool is consumed: {bodies:?}");
    assert!(bodies[0].sheet);
    close(bodies[0].area, 576., 1e-6, "sheet minus the block's footprint on two walls");
    for (a_sheet, op) in [
        (true, BooleanOp::Union),
        (false, BooleanOp::Union),
        (false, BooleanOp::Cut),
        (true, BooleanOp::Intersect),
        (false, BooleanOp::Intersect),
    ] {
        let (mut s, sheet, solid) = make();
        let before = serde_json::to_string(&s.result_bodies()).unwrap();
        let (a, b) = if a_sheet { (sheet, solid) } else { (solid, sheet) };
        assert!(boolean(&mut s, a, b, op).is_err(), "{op:?} sheet-first={a_sheet}");
        assert_eq!(serde_json::to_string(&s.result_bodies()).unwrap(), before, "{op:?} sheet-first={a_sheet}: rolled back");
    }
}

/// move, mirror, linear and circular arrays act on a sheet like on any body; it stays a sheet and its area is the sum of the copies.
#[test]
fn probe_move_mirror_and_arrays_keep_a_sheet_a_sheet() {
    let (mut s, sheet) = fresh();
    let (m, r) = s.move_body(&Move { body: Some(sheet), translation: Some([v(5.), v(0.), v(3.)]), rotation: None, name: None }).unwrap();
    assert!(r.errors.is_empty());
    let b = info(&s, m.body);
    assert!(b.sheet);
    close(b.area, 720., 1e-6, "moved area");
    close(b.bbox[0], -5., 1e-3, "x min after +5");
    close(b.bbox[2], 3., 1e-3, "z min after +3");

    for (keep, area, x_min) in [(true, 1440., -10.), (false, 720., 50.)] {
        let (mut s, sheet) = fresh();
        let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::YZ), &v(30.), None).unwrap();
        let (id, r) = s.mirror(Some(sheet), &PlaneRef::Plane(plane), keep, None).unwrap();
        assert!(r.errors.is_empty());
        let b = info(&s, id);
        assert!(b.sheet);
        close(b.area, area, 1e-6, "mirror area");
        close(b.bbox[0], x_min, 1e-3, "mirror x min");
        close(b.bbox[3], 70., 1e-3, "mirror x max (image of x -10)");
    }
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

/// hole: the cylinder cuts the surface where it meets it; the hole is πr² of the wall (a mesh area: within 0.1).
/// Through all (default depth) drills both walls it crosses.
#[test]
fn probe_hole_on_a_sheet_cuts_a_round_opening_in_each_wall_it_crosses() {
    let hole = |depth: Option<Num>| {
        let (mut s, sheet) = fresh();
        let side = s.topology(Some(sheet), false).unwrap().faces.iter().find(|f| f.normal.is_some_and(|n| n[0] > 0.9)).unwrap().id;
        let (id, r) = s
            .hole(&Hole {
                body: Some(sheet),
                face: Sel::Ids(vec![side]),
                at: None,
                offset: None,
                diameter: v(4.),
                depth,
                kind: HoleKind::Plain,
                dia2: None,
                depth2: None,
                name: None,
            })
            .unwrap();
        assert!(r.errors.is_empty());
        let b = info(&s, id);
        assert!(b.sheet);
        b.area
    };
    close(hole(Some(v(6.))), 720. - std::f64::consts::PI * 4., 0.1, "one wall, blind 6");
    close(hole(None), 720. - 2. * std::f64::consts::PI * 4., 0.2, "both walls, through all");
}

/// draft: the +x wall tilted 5° about the +y face's plane; the neighbours follow (hand: 160/cos5° + 160 + 20·10 + (20 + 16·tan5°)·10).
#[test]
fn probe_draft_on_a_sheet_tilts_the_wall_and_its_neighbours_follow() {
    let (mut s, sheet) = fresh();
    let t = s.topology(Some(sheet), false).unwrap();
    let pick = |axis: usize| t.faces.iter().find(|f| f.normal.is_some_and(|n| n[axis] > 0.9)).unwrap().id;
    let (x, y) = (pick(0), pick(1));
    let (id, r) = s
        .draft(&Draft { body: Some(sheet), faces: Sel::Ids(vec![x]), neutral: Sel::Ids(vec![y]), angle: v(5.), flip: false, name: None })
        .unwrap();
    assert!(r.errors.is_empty());
    let b = info(&s, id);
    assert!(b.sheet);
    let tan = 5f64.to_radians().tan();
    let hand = 160. / 5f64.to_radians().cos() + 160. + 200. + (20. + 16. * tan) * 10.;
    close(b.area, hand, 1e-2, "drafted tube area");
    close(b.bbox[3], 10. + 16. * tan, 1e-2, "the far end of the wall leans out");
}

/// Refused by QymCAD, the document unchanged: remove_face, fillet (`solid tool on a sheet`), push_face (`push face on a sheet`),
/// split (the kernel counts no two pieces).
#[test]
fn probe_tools_the_kernel_refuses_on_a_sheet_stay_refused_and_change_nothing() {
    let (mut s, sheet) = fresh();
    let side = faces(&mut s, sheet)[1];
    let before = serde_json::to_string(&s.result_bodies()).unwrap();
    let e = refused(s.remove_face(Some(sheet), &Sel::Ids(vec![side]), None));
    assert!(e.contains("cannot be removed"), "{e}");
    let e = refused(s.fillet(Some(sheet), &Sel::Kind(SelectionKind::Line), &v(1.), None));
    assert!(e.contains("solid tool on a sheet"), "{e}");
    let e = refused(s.push_face(Some(sheet), &Sel::Ids(vec![side]), &v(2.), None));
    assert!(e.contains("push face on a sheet"), "{e}");
    let e = refused(s.split(&SplitOptions {
        body: Some(sheet),
        plane: PlaneRef::Base(BaseName::XY),
        offset: v(5.),
        name: None,
        piece_names: None,
    }));
    assert!(e.contains("split plane cuts nothing"), "{e}");
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

/// chamfer: an edge shared by two faces chamfers right (hand: 720 − 10·(2 − √2)); boundary edges picked with them are silently
/// left alone by QymCAD (F-197), so a selection that holds one is refused.
#[test]
fn chamfer_on_a_sheet_takes_shared_edges_and_refuses_boundary_edges_f197() {
    let (mut s, sheet) = fresh();
    let t = s.topology(Some(sheet), true).unwrap();
    let shared: Vec<u32> = t.edges.iter().filter(|e| e.faces.is_some()).map(|e| e.id).collect();
    let boundary: Vec<u32> = t.edges.iter().filter(|e| e.faces.is_none()).map(|e| e.id).collect();
    assert_eq!((shared.len(), boundary.len()), (4, 8));
    let before = serde_json::to_string(s.project()).unwrap();
    for pick in [vec![boundary[0]], boundary.clone(), [shared.clone(), boundary.clone()].concat()] {
        let e = refused(s.chamfer(Some(sheet), &Sel::Ids(pick), &v(1.), None, None));
        assert!(e.contains(&format!("body {sheet} is a sheet")) && e.contains("F-197"), "{e}");
    }
    let e = refused(s.chamfer(Some(sheet), &Sel::Kind(SelectionKind::Line), &v(1.), None, None));
    assert!(e.contains("F-197"), "{e}");
    assert_eq!(serde_json::to_string(s.project()).unwrap(), before);
    let (id, r) = s.chamfer(Some(sheet), &Sel::Ids(vec![shared[0]]), &v(1.), None, None).unwrap();
    assert!(r.errors.is_empty());
    let b = info(&s, id);
    assert!(b.sheet);
    close(b.area, 720. - 10. * (2. - 2f64.sqrt()), 1e-3, "one corner chamfered");
}

// ---- (e) thicken of a sheet ----

/// thicken of a sheet, both signs (the app's command takes either): +1 grows outward, -1 inward; the sheet becomes a solid.
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
