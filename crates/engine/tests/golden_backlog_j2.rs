//! J2 ambiguity and seam policies; geometry checks use analytic cylinder/chamfer formulas.
use qymcad_engine::*;
use std::path::Path;
mod common;

fn cone() -> (Session, Rebuild) {
    Session::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cone_negative_chamfer.qcad")).unwrap()
}

#[test]
fn ambiguous_face_warnings_reach_open_info_and_feature_results() {
    let (mut s, opened) = cone();
    let body = opened.bodies[0].id;
    assert!(
        opened.warnings.iter().any(|w| w.node == body && w.message.contains("ambiguous native face ids")),
        "open omitted ambiguous-id warning: {:?}",
        opened.warnings
    );
    assert!(s.info().warnings.iter().any(|w| w.contains("ambiguous native face ids")), "doc_info omitted ambiguous-id warning");
    let (_, report) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &1.0.into(), None).unwrap();
    assert!(
        report.warnings.iter().any(|w| w.node == body && w.message.contains("ambiguous native face ids")),
        "feature result omitted existing-body ambiguous-id warning"
    );
}

#[test]
fn nested_descriptive_ambiguous_faces_are_refused() {
    let (mut s, _) = cone();
    let topo = s.topology(None, false).unwrap();
    let duplicate = topo.faces.iter().find(|f| f.ambiguous_id).unwrap().id;
    let dir = s.project().regen_faces[&topo.body].iter().find(|f| f.id == duplicate).unwrap().normal;
    let face = Sel::OfFeature { feature: s.result_bodies()[0].id, role: None };
    for sel in [
        Sel::EdgesOf(Box::new(face.clone())),
        Sel::EdgesOf(Box::new(Sel::Facing { dir, tol_deg: 1.0 })),
        Sel::Between(Box::new(face.clone()), Box::new(Sel::Facing { dir: [0.0, 0.0, 1.0], tol_deg: 5.0 })),
        Sel::Union(vec![Sel::EdgesOf(Box::new(face)), Sel::Ids(vec![])]),
    ] {
        let result = s.select(None, Element::Edges, &sel);
        assert!(result.is_err(), "nested descriptive ambiguous face must be refused: {result:?}");
        assert!(result.unwrap_err().to_string().contains("ambiguous face id"));
    }
}

fn cylinder() -> Session {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &0.0.into(), &0.0.into(), &10.0.into(), false).unwrap();
    s.extrude(&Extrude {
        sketch: sk,
        profiles: None,
        height: 10.0.into(),
        op: Op::Add,
        direction: Direction::Normal,
        through: false,
        target: None,
        name: None,
    })
    .unwrap();
    s
}

#[test]
fn blend_selections_drop_seams_and_report_note() {
    for fillet in [false, true] {
        let mut s = cylinder();
        let sel = Sel::EdgesOf(Box::new(Sel::Kind(SelectionKind::Cylinder)));
        let preview = s.select(None, Element::Edges, &sel).unwrap().1;
        let topo = s.topology(None, false).unwrap();
        assert_eq!(preview.len(), 3, "two cylinder rims and one closing seam");
        assert_eq!(topo.edges.iter().filter(|e| e.seam).count(), 1);
        let (_, report) =
            if fillet { s.fillet(None, &sel, &0.5.into(), None) } else { s.chamfer(None, &sel, &0.5.into(), None, None) }.unwrap();
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["notes"], serde_json::json!(["dropped 1 seam edges: not blendable"]), "successful blend omitted seam note");
        if !fillet {
            // At each rim, removed cross section integrates pi*(R²-(R-c+z)²),
            // z=0..c: pi*c²*(R-c/3). There are two rims, R=5,c=.5,h=10.
            let expected = std::f64::consts::PI * 25.0 * 10.0 - 2.0 * std::f64::consts::PI * 0.25 * (5.0 - 0.5 / 3.0);
            assert!((report.bodies[0].volume - expected).abs() < 1e-6);
        }
    }
}

#[test]
fn seam_only_blend_selection_is_refused_clearly() {
    for fillet in [false, true] {
        let mut s = cylinder();
        let seam = s.topology(None, false).unwrap().edges.into_iter().find(|e| e.seam).unwrap().id;
        let before = s.info();
        let result = if fillet {
            s.fillet(None, &Sel::Ids(vec![seam]), &0.5.into(), None)
        } else {
            s.chamfer(None, &Sel::Ids(vec![seam]), &0.5.into(), None, None)
        };
        assert!(result.is_err(), "seam-only blend must be refused");
        assert!(result.unwrap_err().to_string().contains("only seam edges"), "seam-only refusal must explain that seams are not blendable");
        assert_eq!(s.info(), before);
    }
}

#[test]
fn seam_repaired_chamfers_keep_unique_edge_names_in_investigated_cases() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cone_negative_chamfer.qcad");
    let (mut original, _) = cone();
    let original_topo = original.topology(None, true).unwrap();
    let unique_edges = |topo: &Topology| {
        let mut ids: Vec<_> = topo.edges.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), topo.edges.len(), "seam repair duplicated edge ids");
    };
    unique_edges(&original_topo);
    assert!(original_topo.faces.iter().any(|f| f.ambiguous_id), "original fixture exercised native seam rescue");
    let mut repaired_cases = 0;
    for distance in [0.25_f64, 0.5, 0.75] {
        let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(fixture.to_str().unwrap()).unwrap();
        let body = project.timeline.last().unwrap().id;
        project.set_feat_dim(body, "dist", distance.to_string());
        project.timeline.last_mut().unwrap().dirty = true;
        let path = common::scratch(&format!("j2-repaired-chamfer-{distance}.qcad"));
        qymcad_io::save_project_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
        let (mut s, report) = Session::open(&path).unwrap();
        assert!(report.errors.is_empty(), "{distance}: {:?}", report.errors);
        let topo = s.topology(None, true).unwrap();
        unique_edges(&topo);
        let duplicate_faces = topo.faces.iter().filter(|f| f.ambiguous_id).count();
        repaired_cases += usize::from(duplicate_faces > 0);
        eprintln!(
            "seam investigation dist={distance}: {} edges, {} unique edge ids, {duplicate_faces} ambiguous face rows",
            topo.edges.len(),
            topo.edges.len()
        );
        // Cone r(z)=3.25-.125z through 10-mm stock; equal setback c has axial
        // reach a=c/sqrt(1+.125²). Integrated rim cuts total pi*c*a*(R+r+2c/3).
        let a = distance / (1.0_f64 + 0.125_f64.powi(2)).sqrt();
        let expected = 15.0 * 15.0 * 10.0
            - std::f64::consts::PI
                * (10.0 * (3.25_f64.powi(2) + 3.25 * 2.0 + 2.0_f64.powi(2)) / 3.0 + distance * a * (3.25 + 2.0 + 2.0 * distance / 3.0));
        assert!((report.bodies[0].volume - expected).abs() < 2e-5);
    }
    assert!(repaired_cases >= 2, "investigation must include several positively identified seam-rescue results");
}
