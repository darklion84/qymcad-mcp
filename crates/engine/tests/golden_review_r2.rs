//! Stored cross-part projections retain native local coordinates, with explicit diagnostics.
mod common;
use common::*;
use qymcad_core::model::ProjSource;
use qymcad_engine::*;

fn fixture(label: &str) -> (std::path::PathBuf, Id, Id, Vec<Id>) {
    let mut s = Session::new_part();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/server-box-10x10x5-inch.igs");
    let body = s.import(&source, 1.0, None, Some("Placed source")).unwrap().bodies[0].body;
    let mut p = s.project().clone();
    p.move_component(p.body_owner(body).unwrap(), [100.0, 0.0, 0.0]);
    let path = scratch(&format!("r2-{label}.qcad"));
    let breps = {
        let _gate = qymcad_kernel::kernel_gate();
        vec![(body, s.shape(body).unwrap().to_brep_bytes().unwrap())]
    };
    qymcad_io::save_project_guarded_with_brep(&p, path.to_str().unwrap(), &breps).unwrap();
    let (mut s, _) = Session::open(&path).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Base(BaseName::XY), Some("Cross sketch")).unwrap();
    let edges: Vec<_> = s
        .topology(Some(body), false)
        .unwrap()
        .edges
        .iter()
        .filter(|e| (e.a[2] - 5.0).abs() < 1e-6 && (e.b[2] - 5.0).abs() < 1e-6)
        .map(|e| ProjSource::Edge(e.id))
        .collect();
    assert_eq!(edges.len(), 4);
    assert!(s.sketch_project(sketch, body, &edges).unwrap_err().to_string().contains("own part"));
    let mut p = s.project().clone();
    // Same public native insertion as the reviewer's r2_probe.rs; no forged projection/entity ids.
    let kernel = qymcad_kernel::OcctKernel {
        shapes: std::cell::RefCell::new(
            qymcad_kernel::imports::restore(
                breps.iter().map(|(body, bytes)| qymcad_kernel::imports::SavedBody { body: *body, bytes: bytes.clone() }).collect(),
            )
            .into_iter()
            .map(|b| (b.body, b.shape))
            .collect(),
        ),
        ..Default::default()
    };
    let projections = {
        let _gate = qymcad_kernel::kernel_gate();
        let si = p.sketch_index(sketch).unwrap();
        edges
            .into_iter()
            .map(|edge| {
                let id = p.add_sketch_projection(si, body, edge, &kernel);
                assert_ne!(id, 0);
                id
            })
            .collect()
    };
    qymcad_io::save_project_guarded_with_brep(&p, path.to_str().unwrap(), &breps).unwrap();
    println!("cross-part fixture: {}", path.display());
    (path, sketch, body, projections)
}

#[test]
fn saved_cross_part_projection_warns_for_every_record_and_keeps_local_geometry() {
    let (path, sketch, body, projections) = fixture("cross-part-warnings");
    let (mut s, report) = Session::open(&path).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let sketch_owner = s.project().sketch_owner(sketch).unwrap();
    let body_owner = s.project().body_owner(body).unwrap();
    assert_ne!(sketch_owner, body_owner);
    let warnings: Vec<_> = report
        .warnings
        .iter()
        .filter(|w| w.message.contains("projected in the source part's local coordinates; its placement is ignored, as in the QymCAD app"))
        .collect();
    assert_eq!(warnings.len(), projections.len(), "one warning per cross-part projection: {:?}", report.warnings);
    for id in projections {
        let warning = warnings.iter().find(|w| w.message.contains(&format!("projection {id}:"))).unwrap();
        assert_eq!(warning.node, sketch);
        assert_eq!(warning.name, "Cross sketch");
        assert!(warning.message.contains(&format!("Cross sketch ({sketch})")), "{warning:?}");
        assert!(warning.message.contains(&format!("Placed source ({body})")), "{warning:?}");
        for owner in [sketch_owner, body_owner] {
            let part = s.project().components.iter().find(|c| c.id == owner).unwrap();
            qymcad_i18n::set_language("en");
            assert!(warning.message.contains(&format!("{} ({owner})", qymcad_i18n::name(&part.name))), "{warning:?}");
        }
    }
    let detail = s.sketch_detail(sketch).unwrap();
    assert_close(detail.points.iter().map(|p| p.x).fold(f64::INFINITY, f64::min), -5.0, 1e-6, "local projected min X");
    assert_close(detail.points.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max), 5.0, 1e-6, "local projected max X");
    let (result, rebuild) = s
        .extrude(&Extrude {
            sketch,
            profiles: None,
            height: 2.0.into(),
            op: Op::NewBody,
            direction: Direction::Normal,
            through: false,
            target: None,
            name: None,
        })
        .unwrap();
    assert!(rebuild.errors.is_empty());
    let output = rebuild.bodies.iter().find(|b| b.id == result).unwrap();
    assert_close(output.volume, 200.0, 1e-5, "10 x 10 x 2 local extrusion");
    assert_close(output.bbox[0], -5.0, 1e-4, "local extrusion min X");
    assert_close(output.bbox[3], 5.0, 1e-4, "local extrusion max X");
    assert_eq!(s.current_rebuild().warnings.len(), 4, "warnings survive subsequent rebuilds");
}

#[test]
fn saved_cross_part_projection_is_marked_in_sketch_info() {
    let (path, sketch, _, _) = fixture("cross-part-inspection");
    let (s, _) = Session::open(&path).unwrap();
    // Check the serialized inspection surface, so this regression runs before the new field exists.
    let info = serde_json::to_value(s.sketch_detail(sketch).unwrap()).unwrap();
    let projected: Vec<_> = info["entities"].as_array().unwrap().iter().filter(|e| e.get("projection").is_some()).collect();
    assert_eq!(projected.len(), 4);
    for entity in projected {
        assert_eq!(entity["cross_part"], true, "cross-part projection must be marked: {entity}");
        assert_eq!(entity["projection_state"], "attached");
    }
}

#[test]
fn same_part_projection_is_unmarked_and_has_no_cross_part_warning() {
    let mut s = Session::new_part();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/server-box-10x10x5-inch.igs");
    let body = s.import(&source, 1.0, None, Some("Own source")).unwrap().bodies[0].body;
    let topo = s.topology(Some(body), false).unwrap();
    let face = topo.faces.iter().find(|f| f.normal.is_some_and(|n| n[2] > 0.99)).unwrap().id;
    let sketch = s.sketch_create(&PlaneRef::Face { body, face }, None).unwrap();
    s.sketch_project(sketch, body, &[ProjSource::Edge(topo.edges[0].id)]).unwrap();
    let info = serde_json::to_value(s.sketch_detail(sketch).unwrap()).unwrap();
    assert!(info["entities"].as_array().unwrap().iter().all(|e| e.get("cross_part").is_none_or(|flag| flag == false)));
    assert!(s.current_rebuild().warnings.is_empty());
}
