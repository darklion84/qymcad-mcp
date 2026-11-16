//! T0 native probes: what QymCAD itself does with Shell and Chamfer on a loft sheet (F-196, F-197).
//! They go through the core API only (a loaded document, the native adders and the app's regeneration), with none of the
//! server's guards, and pin the observation: when the app starts refusing or doing the right thing, they go red and the
//! server refusals of `shell` and `chamfer` on a sheet can be dropped.
use crate::common;
use qymcad_core::feature::{ChamferMode, ShellSide};
use qymcad_core::model::{ChamferShape, Id};
use qymcad_engine::*;

fn v(x: f64) -> Num {
    Num::Value(x)
}

/// The 20×16 by 10 tube, saved and loaded the way the app loads a file; returns the model and the sheet's id.
fn loaded_tube(name: &str) -> (common::GuiReorderModel, Id) {
    let mut s = Session::new_part();
    let low = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_rect(low, &v(0.), &v(0.), &v(20.), &v(16.), false).unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &v(10.), None).unwrap();
    let high = s.sketch_create(&PlaneRef::Plane(plane), None).unwrap();
    s.sketch_rect(high, &v(0.), &v(0.), &v(20.), &v(16.), false).unwrap();
    let sections = vec![LoftSection { sketch: low, profile: None }, LoftSection { sketch: high, profile: None }];
    let (sheet, _) = s.loft(&Loft { sections, ruled: true, op: Op::NewBody, target: None, surface: true, name: None }).unwrap();
    let path = common::scratch(name);
    s.save(Some(&path)).unwrap();
    let mut model = common::gui_reorder_load(&path);
    // The app restores faces on load but not edges (F-023): name them the way the server's own open does.
    let _gate = qymcad_kernel::kernel_gate();
    let kernel = qymcad_kernel::OcctKernel { shapes: std::cell::RefCell::new(std::mem::take(&mut model.shapes)), ..Default::default() };
    for body in kernel.shapes.borrow().keys().copied().collect::<Vec<_>>() {
        use qymcad_core::feature::Kernel;
        model.project.regen_edges.insert(body, kernel.edges(body));
    }
    model.shapes = kernel.shapes.into_inner();
    (model, sheet)
}

/// Area of the body the last node made, from its faces, and whether it is still a sheet.
fn result(model: &mut common::GuiReorderModel) -> (f64, bool, f64) {
    common::gui_reorder_rebuild(model, false);
    let last = model.project.timeline.last().unwrap().kind.body().unwrap();
    let area: f64 = model.project.regen_faces[&last].iter().map(|f| f.area).sum();
    let _gate = qymcad_kernel::kernel_gate();
    (area, model.shapes[&last].is_sheet(), model.shapes[&last].volume())
}

/// F-196: the app builds a green Shell on a sheet and returns a smaller sheet, not a wall.
#[test]
fn the_app_shells_a_sheet_into_a_smaller_sheet_without_an_error_f196() {
    let ids =
        |model: &common::GuiReorderModel, sheet: Id| model.project.regen_faces[&sheet].iter().map(|f| (f.id, f.normal)).collect::<Vec<_>>();
    // open face -> (area, bbox side that was cut back): the walls beside the open one lose 2 mm of their length.
    for (axis, sign, inward_area) in [(0, 1., 520.), (0, -1., 520.), (1, 1., 480.), (1, -1., 480.)] {
        let (mut model, sheet) = loaded_tube("t0-shell-probe.qcad");
        let open = ids(&model, sheet).into_iter().find(|(_, n)| n[axis] * sign > 0.9).unwrap().0;
        model.project.add_shell_mode(sheet, 1.0, vec![open], ShellSide::Inward);
        let (area, still_sheet, volume) = result(&mut model);
        assert!(still_sheet && volume == 0.0, "no wall was built: sheet={still_sheet} volume={volume}");
        assert!((area - inward_area).abs() < 1e-6, "axis {axis} sign {sign}: area {area}");
    }
    let (mut model, sheet) = loaded_tube("t0-shell-probe-centred.qcad");
    let open = ids(&model, sheet).into_iter().find(|(_, n)| n[0] > 0.9).unwrap().0;
    model.project.add_shell_mode(sheet, 1.0, vec![open], ShellSide::Centred);
    let (area, still_sheet, _) = result(&mut model);
    assert!(still_sheet && (area - 540.).abs() < 1e-6, "centred: area {area}");
}

/// F-197: a symmetric chamfer on all 12 edges bevels the four shared ones and leaves the eight boundary edges alone.
#[test]
fn the_app_chamfers_only_the_shared_edges_of_a_sheet_and_says_nothing_f197() {
    let (mut model, sheet) = loaded_tube("t0-chamfer-probe.qcad");
    let edges: Vec<u32> = {
        let _gate = qymcad_kernel::kernel_gate();
        model.shapes[&sheet].edges_info().iter().map(|e| e.id).collect()
    };
    assert_eq!(edges.len(), 12);
    model.project.add_chamfer_ex(sheet, 1.0, ChamferShape { mode: ChamferMode::Symmetric, d2: 0.0, flip: false, ref_face: 0 }, edges);
    let (area, still_sheet, _) = result(&mut model);
    assert!(still_sheet);
    let four_corners = 720. - 4. * 10. * (2. - 2f64.sqrt());
    assert!((area - four_corners).abs() < 1e-3, "four shared edges bevelled, the boundary untouched: {area} vs {four_corners}");
}
