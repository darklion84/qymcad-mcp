//! Round I live regressions, using unchanged native recipes as fixtures.
use qymcad_engine::*;
use std::path::Path;

fn cone(control: bool) -> Session {
    let name = if control { "cone_negative_axis_reversed" } else { "cone_negative_chamfer" };
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}.qcad"));
    let (s, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // Stock volume minus the conical frustum and two equal setbacks c=.5.
    // r(z)=3.25-.125z; axial bevel reach a=c/sqrt(1+k²).
    // Integrating the two radius-square differences cancels their ±k terms.
    let k = 0.125_f64;
    let c = 0.5;
    let a = c / (1.0 + k * k).sqrt();
    let expected = 15.0 * 15.0 * 10.0
        - std::f64::consts::PI * (10.0 * (3.25_f64.powi(2) + 3.25 * 2.0 + 2.0_f64.powi(2)) / 3.0 + c * a * (3.25 + 2.0 + 2.0 * c / 3.0));
    assert!((s.result_bodies()[0].volume - expected).abs() < 2e-5, "formula chamfer volume");
    s
}

#[test]
fn split_cone_bevels_are_not_spheres() {
    for control in [false, true] {
        let mut s = cone(control);
        let t = s.topology(None, true).unwrap();
        assert!(t.faces.iter().all(|f| f.kind != FaceKind::Sphere), "a straight conical bevel cannot be spherical: {:?}", t.faces);
        assert_eq!(t.faces.iter().filter(|f| f.kind == FaceKind::Plane).count(), 6);
        s.render(View::Iso, 128, 128, None).unwrap();
    }
}

#[test]
fn duplicate_face_selection_is_refused_with_workaround() {
    let mut s = cone(false);
    let t = s.topology(None, false).unwrap();
    let duplicate = t.faces.iter().find(|f| t.faces.iter().filter(|g| g.id == f.id).count() > 1).unwrap().id;
    assert!(!t.warnings.is_empty());
    assert!(t.faces.iter().filter(|f| f.id == duplicate).all(|f| f.ambiguous_id));
    let result = s.select(None, Element::Faces, &Sel::Ids(vec![duplicate]));
    assert!(result.is_err(), "ambiguous face selection must be refused, got {result:?}");
    let message = result.unwrap_err().to_string();
    assert!(message.contains("ambiguous") && message.contains("reverse") && message.contains("axis"), "{message}");
    let mut control = cone(true);
    let t = control.topology(None, false).unwrap();
    assert_eq!(t.faces.len(), 6 + 1 + 2, "six stock planes, original cone, two bevel cones");
    assert!(t.warnings.is_empty());
    for f in t.faces {
        assert!(!f.ambiguous_id);
        assert_eq!(control.select(None, Element::Faces, &Sel::Ids(vec![f.id])).unwrap().1, vec![f.id]);
    }
}

#[test]
fn bbox_serialization_rounds_half_away_normalizes_zero_and_keeps_large_finite_values() {
    // Odd numerators over 32 are exactly representable binary values. Scaling by
    // 10^4 gives n*625/2, an exact half-integer: 10312.5 and 10937.5. Half-away
    // rounds these to 10313 and 10938 units, hence 1.0313 and 1.0938 mm.
    for (numerator, expected) in [(33.0, 1.0313), (35.0, 1.0938)] {
        let tie = numerator / 32.0;
        for signed in [tie, -tie] {
            assert_eq!((signed * 10_000.0_f64).abs().fract(), 0.5, "scaled value must be a true binary tie");
        }
        let body = BodyInfo {
            id: 1,
            name: String::new(),
            volume: 1.23456789,
            // ±.00001 mm are closer to zero; 10^305 cannot be scaled by 10^4 in f64.
            bbox: [-tie, -0.00001, -0.0, tie, 0.00001, 1e305],
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["bbox"], serde_json::json!([-expected, 0.0, 0.0, expected, 0.0, 1e305]));
        assert_eq!(json["volume_mm3"], serde_json::json!(1.23456789));
        for i in [1, 2, 4] {
            assert_eq!(json["bbox"][i].as_f64().unwrap().to_bits(), 0.0_f64.to_bits());
        }
    }
}

#[test]
fn nested_explicit_ambiguous_face_id_is_refused() {
    let mut s = cone(false);
    let t = s.topology(None, false).unwrap();
    let duplicate = t.faces.iter().find(|f| f.ambiguous_id).unwrap().id;
    let result = s.select(None, Element::Edges, &Sel::EdgesOf(Box::new(Sel::Ids(vec![duplicate]))));
    assert!(result.is_err(), "nested ambiguous face reference must be refused: {result:?}");
    assert!(result.unwrap_err().to_string().contains("ambiguous"));
}
