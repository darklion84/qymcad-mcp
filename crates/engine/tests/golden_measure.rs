//! E2 native ruler semantics: expected values are elementary support-geometry formulas.
mod common;
use common::*;
use qymcad_engine::*;

#[test]
fn native_points_are_full_precision_and_directional() {
    let mut s = Session::new_part();
    let r = s.measure(MeasureOperand::Point([0.0; 3]), Some(MeasureOperand::Point([3.0, 4.0, 0.0]))).unwrap();
    // Pythagoras: sqrt(3²+4²)=5; native delta is b-a.
    assert_eq!(r.distance_mm, Some(5.0));
    assert_eq!(r.distance_kind, Some("point_point"));
    assert_eq!(r.delta_mm, Some([3.0, 4.0, 0.0]));
    assert_eq!(r.coordinate_frame, "world");
    let r = s.measure(MeasureOperand::Point([3.0, 4.0, 0.0]), Some(MeasureOperand::Point([0.0; 3]))).unwrap();
    assert_eq!(r.distance_mm, Some(5.0));
    assert_eq!(r.delta_mm, Some([-3.0, -4.0, 0.0]));
}

use serde_json::json;
use std::{collections::BTreeMap, f64::consts::PI, io::Read, path::Path};

fn extrusion(sketch: Id, height: Num) -> Extrude {
    Extrude {
        sketch,
        height,
        down: None,
        profiles: None,
        profiles_at: None,
        op: Op::NewBody,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    }
}
fn add_block(s: &mut Session, center: [f64; 2], size: [f64; 2], height: Num) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(sk, &center[0].into(), &center[1].into(), &size[0].into(), &size[1].into(), false).unwrap();
    let (body, report) = s.extrude(&extrusion(sk, height)).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    body
}
fn stock() -> (Session, Id) {
    let mut s = Session::new_part();
    assert!(s.param_set("h", &5.0.into()).is_ok());
    let body = add_block(&mut s, [0., 0.], [20., 10.], "h".into());
    (s, body)
}
fn add_cylinder(s: &mut Session, center: [f64; 2], radius: f64, height: f64) -> Id {
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &center[0].into(), &center[1].into(), &(2. * radius).into(), false).unwrap();
    let (body, report) = s.extrude(&extrusion(sk, height.into())).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    body
}
fn face(body: Id, dir: [f64; 3]) -> MeasureOperand {
    MeasureOperand::Face { body, selection: facing(dir) }
}
fn facing(dir: [f64; 3]) -> Sel {
    Sel::Facing { dir, tol_deg: 1.0 }
}
fn edge(body: Id, id: u32, at: Option<MeasureAt>) -> MeasureOperand {
    MeasureOperand::Edge { body, selection: Sel::Ids(vec![id]), at }
}
fn pair(s: &mut Session, a: MeasureOperand, b: MeasureOperand, distance: f64, kind: &str) -> MeasureResult {
    let r = s.measure(a, Some(b)).unwrap();
    assert_close(r.distance_mm.unwrap(), distance, 1e-6, kind);
    assert_eq!(r.distance_kind, Some(kind));
    assert_eq!(r.coordinate_frame, "world");
    r
}
fn block_values(s: &mut Session, body: Id, h: f64) {
    // Parallel cap planes z=0,z=h: |h-0|=h, anti-parallel normals fold to 0°.
    let r = pair(s, face(body, [0., 0., 1.]), face(body, [0., 0., -1.]), h, "support_plane_gap");
    assert_eq!(r.angle_deg, Some(0.));
    assert_eq!(r.angle_kind, Some("acute_unsigned"));
    let r = s.measure(face(body, [0., 0., 1.]), Some(face(body, [1., 0., 0.]))).unwrap();
    // Orthogonal support planes intersect: only their 90° angle is meaningful.
    assert_eq!(r.distance_mm, None);
    assert_eq!(r.distance_kind, None);
    assert_close(r.angle_deg.unwrap(), 90., 1e-9, "orthogonal planes");
    assert_eq!(r.angle_kind, Some("acute_unsigned"));
    // Point at z12 and cap plane z=h have perpendicular gap |12-h|.
    pair(s, MeasureOperand::Point([0., 0., 12.]), face(body, [0., 0., 1.]), 12. - h, "point_support_plane");
    let t = s.topology(Some(body), false).unwrap();
    let e = t.edges.iter().find(|e| e.kind == EdgeKind::Line && (e.length - 20.).abs() < 1e-6 && (e.mid[2] - h).abs() < 1e-6).unwrap();
    let r = s.measure(edge(body, e.id, None), None).unwrap();
    // The top x-directed edge spans x=-10..10, so its length is 20 mm.
    assert_close(r.length_mm.unwrap(), 20., 1e-6, "20 mm straight top edge");
    assert_eq!(r.resolved.len(), 1);
    assert_eq!(r.coordinate_frame, "world");
}

#[test]
fn block_support_planes_and_single_line() {
    let (mut s, body) = stock();
    block_values(&mut s, body, 5.);
}

#[test]
fn native_support_line_and_skew_lines() {
    let mut s = Session::new_part();
    // Rectangle centred at (.5,.5) has an x-directed bottom edge from (0,0,0) to (1,0,0).
    let a = add_block(&mut s, [0.5, 0.5], [1., 1.], 1.0.into());
    let top = s.topology(Some(a), false).unwrap();
    let xline = top.edges.iter().find(|e| (e.mid[0] - 0.5).abs() < 1e-6 && e.mid[1].abs() < 1e-6 && e.mid[2].abs() < 1e-6).unwrap().id;
    // Endpoints are(0,0,0) and(1,0,0), with native orientation unspecified.
    // Start→end length is |1-0|=1; reversing must negate the signed x delta.
    let ends = pair(&mut s, edge(a, xline, Some(MeasureAt::Start)), edge(a, xline, Some(MeasureAt::End)), 1. - 0., "point_point");
    let d = ends.delta_mm.unwrap();
    assert_eq!([d[0].abs(), d[1], d[2]], [1., 0., 0.]);
    let reverse = pair(&mut s, edge(a, xline, Some(MeasureAt::End)), edge(a, xline, Some(MeasureAt::Start)), 1. - 0., "point_point");
    assert_eq!(reverse.delta_mm, Some([-d[0], -d[1], -d[2]]));
    let start = s.measure(MeasureOperand::Point([0.; 3]), Some(edge(a, xline, Some(MeasureAt::Start)))).unwrap().delta_mm.unwrap();
    let end = s.measure(MeasureOperand::Point([0.; 3]), Some(edge(a, xline, Some(MeasureAt::End)))).unwrap().delta_mm.unwrap();
    assert!(
        (start == [0., 0., 0.] && end == [1., 0., 0.]) || (start == [1., 0., 0.] && end == [0., 0., 0.]),
        "native oriented endpoints must be the two hand-constructed corners"
    );
    let r = pair(&mut s, MeasureOperand::Point([100., 0., 0.]), edge(a, xline, None), 0., "point_support_line");
    assert_ne!(r.distance_mm, Some(100. - 1.), "finite-segment gap 99 must never replace native support-line distance");
    let b = add_block(&mut s, [0.5, 0.5], [1., 1.], 7.0.into());
    let t = s.topology(Some(b), false).unwrap();
    let yline = t.edges.iter().find(|e| e.mid[0].abs() < 1e-6 && (e.mid[1] - 0.5).abs() < 1e-6 && (e.mid[2] - 7.).abs() < 1e-6).unwrap().id;
    // X axis at z0 and Y axis at z7: their common normal is (0,0,7), angle 90°.
    let r = pair(&mut s, edge(a, xline, None), edge(b, yline, None), 7., "support_lines");
    assert_close(r.angle_deg.unwrap(), 90., 1e-9, "skew orthogonal lines");
    assert_eq!(r.angle_kind, Some("acute_unsigned"));
}

#[test]
fn descriptive_measurements_survive_open_native_app_edit_and_engine_edit() {
    let (mut s, body) = stock();
    let path = scratch("e2-native-edit.qcad");
    s.save(Some(&path)).unwrap();
    let (mut opened, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    block_values(&mut opened, body, 5.);
    // Independent app emulation executes h5→8; V=20*10*8=1600.
    assert_close(gui_edit_param(&path, "h", "8"), 20. * 10. * 8., 1e-6, "native app volume");
    let (v, _, app_faces, mut app_project) = gui_edit_param_model(&path, "h", "8");
    assert_close(v, 20. * 10. * 8., 1e-6, "native app project volume");
    // Inspect the helper's actual app-path output BEFORE saving or any further regeneration.
    use qymcad_core::measure::{measure_pair, MeasureItem as M};
    let top = app_faces.iter().find(|f| f.normal[2] > 0.99).unwrap();
    let bottom = app_faces.iter().find(|f| f.normal[2] < -0.99).unwrap();
    let plane = |f: &qymcad_core::geom::MeshFace| M::Plane { origin: [f.centroid.x, f.centroid.y, f.centroid.z], normal: f.normal };
    let gap = measure_pair(&plane(top), &plane(bottom));
    assert_close(gap.distance.unwrap(), 8. - 0., 1e-6, "unrepaired app cap gap");
    assert_eq!(gap.angle_deg, Some(0.), "anti-parallel app cap normals fold to zero");
    let gap = measure_pair(&M::Point([0., 0., 12.]), &plane(top));
    assert_close(gap.distance.unwrap(), 12. - 8., 1e-6, "unrepaired app point-to-top");
    // Persist that app recipe using native regeneration, with no engine parameter repair.
    let (report, shapes) = qymcad_testkit::regenerate(&mut app_project);
    assert!(report.errors.is_empty(), "native app save: {:?}", report.errors);
    for (id, faces) in report.built {
        app_project.set_body_faces(id, faces);
    }
    let breps: Vec<_> = {
        let _gate = qymcad_kernel::kernel_gate();
        shapes.iter().map(|(&id, shape)| (id, shape.to_brep_bytes().unwrap())).collect()
    };
    let app_path = scratch("e2-native-edited.qcad");
    qymcad_io::save_project_with_brep(&app_project, app_path.to_str().unwrap(), &breps).unwrap();
    let (mut app_open, r) = Session::open(&app_path).unwrap();
    assert!(r.errors.is_empty());
    // The same descriptive cap selections give gap8 and point-to-top 12-8=4.
    block_values(&mut app_open, body, 8.);
    let r = opened.param_set("h", &8.0.into()).unwrap();
    assert!(r.errors.is_empty());
    assert_close(r.bodies[0].volume, 20. * 10. * 8., 1e-6, "engine edit volume");
    block_values(&mut opened, body, 8.);
}

#[test]
fn cylinder_radii_circle_length_and_native_pair_meanings() {
    let mut s = Session::new_part();
    let body = add_cylinder(&mut s, [0., 0.], 4., 10.);
    let t = s.topology(Some(body), false).unwrap();
    let cylinder = t.faces.iter().find(|f| f.kind == FaceKind::Cylinder).unwrap().id;
    let rim = t.edges.iter().find(|e| e.kind == EdgeKind::Circle).unwrap().id;
    let cylindrical = || MeasureOperand::Face { body, selection: Sel::Ids(vec![cylinder]) };
    let r = s.measure(cylindrical(), None).unwrap();
    // Ø8 means radius 8/2=4 and diameter 2*4=8.
    assert_close(r.radius_mm.unwrap(), 4., 1e-9, "cylinder radius");
    assert_close(r.diameter_mm.unwrap(), 2. * 4., 1e-9, "cylinder diameter");
    let r = s.measure(edge(body, rim, None), None).unwrap();
    assert_close(r.radius_mm.unwrap(), 4., 1e-9, "rim radius");
    assert_close(r.diameter_mm.unwrap(), 2. * 4., 1e-9, "rim diameter");
    assert_close(r.length_mm.unwrap(), 2. * PI * 4., 1e-6, "full circle circumference");
    // Infinite cylinder ignores axial bounds: radial10 - radius4 = 6 even at z100.
    pair(&mut s, MeasureOperand::Point([10., 0., 100.]), cylindrical(), 10. - 4., "point_cylinder_surface");
    let a = add_cylinder(&mut s, [40., 0.], 3., 10.);
    let b = add_cylinder(&mut s, [60., 0.], 5., 10.);
    let cyl = |body| MeasureOperand::Face { body, selection: Sel::Kind(SelectionKind::Cylinder) };
    let r = pair(&mut s, cyl(a), cyl(b), 20. - 3. - 5., "signed_axis_clearance");
    assert_eq!(r.angle_deg, Some(0.));
    // Native signed clearance can be negative: coincident axes 0 - 4 - 4 = -8.
    pair(&mut s, cyl(body), cyl(body), 0. - 4. - 4., "signed_axis_clearance");
    let a = add_cylinder(&mut s, [80., 0.], 2., 10.);
    let b = add_cylinder(&mut s, [90., 0.], 3., 10.);
    let rim_at = |s: &mut Session, body| {
        let t = s.topology(Some(body), false).unwrap();
        t.edges.iter().find(|e| e.kind == EdgeKind::Circle && e.mid[2].abs() < 1e-6).unwrap().id
    };
    let ea = rim_at(&mut s, a);
    let eb = rim_at(&mut s, b);
    // Centres differ by (10,0,0): centre distance10, independent of 2+3 radii.
    let r = pair(&mut s, edge(a, ea, None), edge(b, eb, None), 10., "circle_centre_distance");
    assert_ne!(r.distance_mm, Some(10. - 2. - 3.));
    // Point axial3,radial6 relative to the radius2 circle: sqrt(3²+(6-2)²)=5.
    pair(&mut s, MeasureOperand::Point([86., 0., 3.]), edge(a, ea, None), 5., "point_full_circle");
}

fn archive(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..z.len())
        .map(|i| {
            let mut entry = z.by_index(i).unwrap();
            let name = entry.name().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}
fn snapshot(s: &mut Session) -> (String, DocInfo, Vec<Topology>) {
    let bodies: Vec<_> = s.result_bodies().iter().map(|b| b.id).collect();
    let topologies = bodies.into_iter().map(|b| s.topology(Some(b), true).unwrap()).collect();
    (ron::to_string(s.project()).unwrap(), s.info(), topologies)
}
fn refuse(s: &mut Session, a: MeasureOperand, b: Option<MeasureOperand>) {
    // Install a real modelling undo entry BEFORE capturing the state/refusal.
    let mut history = s.begin_tool_edit().unwrap();
    history.record_call("param_set", json!({"name":"refusal_marker","value":123}));
    assert!(s.param_set("refusal_marker", &123.0.into()).is_ok());
    s.finish_tool_edit(history, true);
    let before = snapshot(s);
    assert!(s.measure(a, b).is_err(), "unsupported/invalid operand must be refused");
    assert_eq!(snapshot(s), before, "full Project, doc_info and topology unchanged by refusal");
    let undo = s.undo().unwrap();
    assert_eq!(undo.call.unwrap().tool, "param_set", "refusal must not add an undo entry");
    assert!(!s.params().iter().any(|p| p.name == "refusal_marker"));
    assert!(s.undo().is_err(), "setup direct engine calls have no undo entries");
}

#[test]
fn refusal_selection_arity_wrong_kind_wrong_body_unknown_id_are_read_only() {
    let (mut s, body) = stock();
    let t = s.topology(Some(body), false).unwrap();
    let face_id = t.faces[0].id;
    let edge_id = t.edges[0].id;
    let p = || Some(MeasureOperand::Point([0.; 3]));
    refuse(&mut s, MeasureOperand::Face { body, selection: Sel::Facing { dir: [1., 1., 1.], tol_deg: 1. } }, p());
    // +Z and -Z select exactly the two cap faces, not one arbitrary first hit.
    refuse(&mut s, MeasureOperand::Face { body, selection: Sel::Union(vec![facing([0., 0., 1.]), facing([0., 0., -1.])]) }, p());
    refuse(&mut s, MeasureOperand::Face { body, selection: Sel::Ids(vec![edge_id]) }, p());
    refuse(&mut s, edge(body, face_id, None), p());
    refuse(&mut s, edge(body, u32::MAX, None), p());
    let other = add_block(&mut s, [50., 0.], [2., 2.], 3.0.into());
    let other_face = s.topology(Some(other), false).unwrap().faces[0].id;
    assert!(!s.topology(Some(body), false).unwrap().faces.iter().any(|f| f.id == other_face));
    refuse(&mut s, MeasureOperand::Face { body, selection: Sel::Ids(vec![other_face]) }, p());
}

#[test]
fn refusal_unsupported_single_nonfinite_and_invalid_at_are_read_only() {
    let (mut s, body) = stock();
    let e = s.topology(Some(body), false).unwrap().edges[0].id;
    refuse(&mut s, MeasureOperand::Point([0.; 3]), None);
    refuse(&mut s, face(body, [0., 0., 1.]), None);
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        refuse(&mut s, MeasureOperand::Point([bad, 0., 0.]), Some(MeasureOperand::Point([0.; 3])));
    }
    refuse(&mut s, edge(body, e, Some(MeasureAt::Center)), Some(MeasureOperand::Point([0.; 3])));
}

#[test]
fn refusal_consumed_body_and_circle_plane_fallback_are_read_only() {
    let (mut s, body) = stock();
    let (_, r) = s
        .move_body(&Move { body: Some(body), translation: Some([30.0.into(), 0.0.into(), 0.0.into()]), rotation: None, name: None })
        .unwrap();
    assert!(r.errors.is_empty());
    refuse(&mut s, face(body, [0., 0., 1.]), Some(MeasureOperand::Point([0.; 3])));
    let cylinder = add_cylinder(&mut s, [60., 0.], 4., 10.);
    let t = s.topology(Some(cylinder), false).unwrap();
    let rim = t.edges.iter().find(|e| e.kind == EdgeKind::Circle).unwrap().id;
    refuse(&mut s, edge(cylinder, rim, None), Some(face(cylinder, [0., 0., 1.])));
}

#[test]
fn refusal_spline_pair_and_cone_pair_are_read_only() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_spline(
        sk,
        &SplineSpec {
            points: vec![Xy(0.0.into(), 0.0.into()), Xy(10.0.into(), 10.0.into()), Xy(20.0.into(), 0.0.into())],
            closed: false,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    s.sketch_line(
        sk,
        &LineSpec { x1: 0.0.into(), y1: 0.0.into(), x2: 20.0.into(), y2: 0.0.into(), construction: false, dimensioned: true },
    )
    .unwrap();
    let (body, r) = s.extrude(&extrusion(sk, 3.0.into())).unwrap();
    assert!(r.errors.is_empty());
    let t = s.topology(Some(body), false).unwrap();
    let spline = t.edges.iter().find(|e| e.kind == EdgeKind::Other).unwrap().id;
    refuse(&mut s, edge(body, spline, None), Some(MeasureOperand::Point([0.; 3])));
    // Revolve a trapezoid (r bottom4, r top2, height10) around its vertical edge: true cone/frustum face.
    let mut c = Session::new_part();
    let sk = c.sketch_create(&PlaneRef::Base(BaseName::XZ), None).unwrap();
    c.sketch_polyline(
        sk,
        &PolylineSpec {
            points: vec![Xy(0.0.into(), 0.0.into()), Xy(4.0.into(), 0.0.into()), Xy(2.0.into(), 10.0.into()), Xy(0.0.into(), 10.0.into())],
            closed: true,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    let axis = c
        .sketch_line(
            sk,
            &LineSpec { x1: 0.0.into(), y1: (-1.0).into(), x2: 0.0.into(), y2: 11.0.into(), construction: true, dimensioned: true },
        )
        .unwrap();
    let (body, r) = c
        .revolve(&Revolve {
            sketch: sk,
            profiles: None,
            axis: AxisRef::Line(axis.entities[0]),
            angle: 360.0.into(),
            direction: Direction::Normal,
            op: Op::NewBody,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(r.errors.is_empty());
    let cone = c.topology(Some(body), false).unwrap().faces.iter().find(|f| f.kind == FaceKind::Cone).unwrap().id;
    refuse(&mut c, MeasureOperand::Face { body, selection: Sel::Ids(vec![cone]) }, Some(MeasureOperand::Point([0.; 3])));
}

#[test]
fn successful_measure_preserves_archive_and_previous_modelling_undo() {
    let (mut s, body) = stock();
    let mut history = s.begin_tool_edit().unwrap();
    history.record_call("param_set", json!({"name":"h","value":8}));
    let r = s.param_set("h", &8.0.into()).unwrap();
    assert!(r.errors.is_empty());
    s.finish_tool_edit(history, true);
    let before_path = scratch("e2-readonly-before.qcad");
    s.save(Some(&before_path)).unwrap();
    let before = snapshot(&mut s);
    let entries = archive(&before_path);
    block_values(&mut s, body, 8.);
    assert_eq!(snapshot(&mut s), before);
    let after_path = scratch("e2-readonly-after.qcad");
    s.save(Some(&after_path)).unwrap();
    assert_eq!(archive(&after_path), entries, "every archive entry is byte identical");
    let undone = s.undo().unwrap();
    assert_eq!(undone.call.unwrap().tool, "param_set");
    // Undo restores preceding modelling edit h8→h5, not a measurement boundary.
    block_values(&mut s, body, 5.);
    assert!(s.undo().is_err());
}

#[test]
fn move_translation_and_quarter_turn_measure_in_world_coordinates() {
    let (mut s, src) = stock();
    // Native Move applies translation then rotation: (x,y,z)→(-y,x+30,z).
    let (translated, r) = s
        .move_body(&Move { body: Some(src), translation: Some([30.0.into(), 0.0.into(), 0.0.into()]), rotation: None, name: None })
        .unwrap();
    assert!(r.errors.is_empty());
    let (m, r) = s
        .move_body(&Move {
            body: Some(translated.body),
            translation: None,
            rotation: Some(Rotation { axis: Axis::Z, angle: 90., pivot: Some([0.; 3]) }),
            name: None,
        })
        .unwrap();
    assert!(r.errors.is_empty());
    let t = s.topology(Some(m.body), false).unwrap();
    let line = t.edges.iter().find(|e| (e.mid[0] - 5.).abs() < 1e-6 && (e.mid[1] - 30.).abs() < 1e-6 && e.mid[2].abs() < 1e-6).unwrap().id;
    // Both oriented endpoints lie on x5,z0; midpoint is (5,30,0).
    let r = pair(&mut s, edge(m.body, line, Some(MeasureAt::Mid)), MeasureOperand::Point([8., 34., 0.]), 5., "point_point");
    assert_eq!(r.delta_mm, Some([3., 4., 0.]));
    // Moved cap planes stay z0,z5, so separation remains5.
    pair(&mut s, face(m.body, [0., 0., 1.]), face(m.body, [0., 0., -1.]), 5., "support_plane_gap");
}

#[test]
fn component_placement_transforms_points_directions_and_planes_to_world() {
    let (mut s, body) = stock();
    let t = s.topology(Some(body), false).unwrap();
    let line = t.edges.iter().find(|e| (e.mid[0]).abs() < 1e-6 && (e.mid[1] + 5.).abs() < 1e-6 && e.mid[2].abs() < 1e-6).unwrap().id;
    let side = t.faces.iter().find(|f| f.normal == Some([1., 0., 0.])).unwrap().id;
    let opposite = t.faces.iter().find(|f| f.normal == Some([-1., 0., 0.])).unwrap().id;
    let path = scratch("e2-placed.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let parts: Vec<_> = project.components.iter().enumerate().filter(|(_, c)| c.parent.is_some()).map(|(i, _)| i).collect();
    assert_eq!(parts.len(), 1);
    // True body placement, not baked shape geometry: x'=30-y, y'=x, z'=z.
    project.components[parts[0]].transform = [0., -1., 0., 30., 1., 0., 0., 0., 0., 0., 1., 0.];
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (mut placed, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    // Local edge midpoint(0,-5,0) becomes(35,0,0); delta to(38,4,0)=(3,4,0), |delta|=5.
    let r = pair(&mut placed, edge(body, line, Some(MeasureAt::Mid)), MeasureOperand::Point([38., 4., 0.]), 5., "point_point");
    assert_eq!(r.delta_mm, Some([3., 4., 0.]));
    let f = |id| MeasureOperand::Face { body, selection: Sel::Ids(vec![id]) };
    // Local x=10 plane becomes world y=10; point y17 has gap17-10=7.
    pair(&mut placed, MeasureOperand::Point([30., 17., 0.]), f(side), 17. - 10., "point_support_plane");
    // Opposite side x=-10 becomes y=-10: plane gap10-(-10)=20.
    pair(&mut placed, f(side), f(opposite), 10. - (-10.), "support_plane_gap");
}

#[test]
fn arc_single_reports_real_span_but_pair_uses_full_underlying_circle() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let arc: ArcSpec = serde_json::from_value(json!({"r":4,"start_angle":0,"end_angle":90})).unwrap();
    s.sketch_arc(sk, &arc).unwrap();
    s.sketch_polyline(
        sk,
        &PolylineSpec {
            points: vec![Xy(0.0.into(), 4.0.into()), Xy(0.0.into(), 0.0.into()), Xy(4.0.into(), 0.0.into())],
            closed: false,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    let (body, r) = s.extrude(&extrusion(sk, 3.0.into())).unwrap();
    assert!(r.errors.is_empty());
    let t = s.topology(Some(body), false).unwrap();
    let arc = t.edges.iter().find(|e| e.kind == EdgeKind::Arc && e.mid[2].abs() < 1e-6).unwrap().id;
    let r = s.measure(edge(body, arc, None), None).unwrap();
    // Quarter of radius4 circle: actual span length=(π/2)*4=2π, not full circumference8π.
    assert_close(r.radius_mm.unwrap(), 4., 1e-6, "arc underlying radius");
    assert_close(r.diameter_mm.unwrap(), 2. * 4., 1e-6, "arc underlying diameter");
    assert_close(r.length_mm.unwrap(), PI * 4. / 2., 1e-6, "actual quarter arc span");
    // (-4,0) lies on the full circle but outside the first-quadrant trimmed arc: native full-circle gap0.
    pair(&mut s, MeasureOperand::Point([-4., 0., 0.]), edge(body, arc, None), 0., "point_full_circle");
    // Its analytic centre is(0,0,0); centre→(3,4,0) uses sqrt(3²+4²)=5.
    let r = pair(&mut s, edge(body, arc, Some(MeasureAt::Center)), MeasureOperand::Point([3., 4., 0.]), 5., "point_point");
    assert_eq!(r.delta_mm, Some([3., 4., 0.]));
}

#[test]
fn spline_single_length_is_explicitly_polyline_approximation() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_spline(
        sk,
        &SplineSpec {
            points: vec![Xy(0.0.into(), 0.0.into()), Xy(10.0.into(), 10.0.into()), Xy(20.0.into(), 0.0.into())],
            closed: false,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    s.sketch_line(
        sk,
        &LineSpec { x1: 0.0.into(), y1: 0.0.into(), x2: 20.0.into(), y2: 0.0.into(), construction: false, dimensioned: true },
    )
    .unwrap();
    let (body, r) = s.extrude(&extrusion(sk, 3.0.into())).unwrap();
    assert!(r.errors.is_empty());
    let spline = s.topology(Some(body), false).unwrap().edges.into_iter().find(|e| e.kind == EdgeKind::Other).unwrap().id;
    let r = s.measure(edge(body, spline, None), None).unwrap();
    assert_eq!(r.length_kind, Some("polyline_approximation"));
    // Natural cubic through(0,0),(10,10),(20,0): each symmetric half has
    // x=10u, y=5(3u-u³). Independent length integral is 2∫₀¹ sqrt(10²+[15(1-u²)]²)du.
    // Composite Simpson with 10000 panels gives its analytic curve length; the
    // reported polyline deliberately permits 0.02mm chord approximation error.
    let n = 10000;
    let f = |u: f64| (100. + 225. * (1. - u * u).powi(2)).sqrt();
    let mut sum = f(0.) + f(1.);
    for i in 1..n {
        sum += if i % 2 == 0 { 2. } else { 4. } * f(i as f64 / n as f64);
    }
    let expected = 2. * sum / (3. * n as f64);
    assert_close(r.length_mm.unwrap(), expected, 0.02, "independent cubic arc-length integral");
    assert_eq!(r.radius_mm, None);
    assert_eq!(r.diameter_mm, None);
}

#[test]
fn closed_spline_single_has_perimeter_without_a_fake_chord_direction() {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_spline(
        sk,
        &SplineSpec {
            points: vec![
                Xy(0.0.into(), 0.0.into()),
                Xy(20.0.into(), 0.0.into()),
                Xy(20.0.into(), 10.0.into()),
                Xy(0.0.into(), 10.0.into()),
            ],
            closed: true,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    let (body, report) = s.extrude(&extrusion(sk, 3.0.into())).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    let t = s.topology(Some(body), false).unwrap();
    let spline = t.edges.iter().find(|e| e.kind == EdgeKind::Other).unwrap();
    // A whole periodic curve has coincident endpoints: zero chord, positive perimeter.
    for i in 0..3 {
        assert_close(spline.a[i] - spline.b[i], 0., 1e-9, "periodic endpoint closure");
    }
    let id = spline.id;
    let r = s.measure(edge(body, id, None), None).unwrap();
    assert_eq!(r.length_kind, Some("polyline_approximation"));
    // Independent periodic cubic through a 20×10 rectangle. Solving its symmetric
    // C2 interpolation equations gives node derivatives (+a,-b),(+a,+b),(-a,+b),(-a,-b),
    // a=3H/(L+3H), b=3L/(3L+H), scaled by each span's chord.
    // Native occt_io.cpp:903–912 samples at n=max(24,8*(NbKnots-1)) equal GLOBAL
    // parameter steps. geom/bspline.rs:93–148 uses chord-proportional spans and one
    // knot per C2 join: five unique knots, n=32, knots[0,1/3,1/2,5/6,1].
    // Expected length is therefore the hand-defined finite sum Σ|P((k+1)/32)-P(k/32)|,
    // evaluating Hermite cubics below, never reading the measured shape's samples.
    let (l, h) = (20., 10.);
    let a = 3. * h / (l + 3. * h);
    let b = 3. * l / (3. * l + h);
    let points = [[0., 0.], [l, 0.], [l, h], [0., h]];
    let derivatives = [[a, -b], [a, b], [-a, b], [-a, -b]];
    let knots = [0., l, l + h, 2. * l + h, 2. * l + 2. * h];
    let point = |u: f64| -> [f64; 2] {
        let along = u * (2. * l + 2. * h);
        let k = (0..4).find(|&k| along <= knots[k + 1]).unwrap();
        let j = (k + 1) % 4;
        let chord = knots[k + 1] - knots[k];
        let t = (along - knots[k]) / chord;
        std::array::from_fn(|d| {
            (2. * t * t * t - 3. * t * t + 1.) * points[k][d]
                + (t * t * t - 2. * t * t + t) * derivatives[k][d] * chord
                + (-2. * t * t * t + 3. * t * t) * points[j][d]
                + (t * t * t - t * t) * derivatives[j][d] * chord
        })
    };
    let n = 8 * (5 - 1);
    let expected: f64 = (0..n)
        .map(|k| {
            let p = point(k as f64 / n as f64);
            let q = point((k + 1) as f64 / n as f64);
            (q[0] - p[0]).hypot(q[1] - p[1])
        })
        .sum();
    // Each native sample is f32. Cubic convex hull coordinates fit ±25mm; two
    // endpoint-rounding errors plus subtraction over 32 segments are bounded by
    // f32::EPSILON*25*32*3 < 0.0003mm. No curve-versus-polyline allowance is needed.
    let float_bound = f32::EPSILON as f64 * 25. * n as f64 * 3.;
    assert_close(r.length_mm.unwrap(), expected, float_bound, "independent sampled periodic cubic perimeter");
    assert_eq!(r.radius_mm, None);
    assert_eq!(r.diameter_mm, None);
    refuse(&mut s, edge(body, id, None), Some(MeasureOperand::Point([0.; 3])));
}
