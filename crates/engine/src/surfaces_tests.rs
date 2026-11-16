//! TR: review round of package T (F-198 threshold, area source). In-crate because the tests set `GeomQuality`, which the public
//! API does not expose.
use super::*;
use crate::surfaces::collapsed_solid;
use qymcad_core::model::GeomQuality;

fn v(x: f64) -> Num {
    Num::Value(x)
}

/// A thin lens: a 40 mm chord closed by an arc of radius 200 (sagitta 1.0025 mm), 100 mm long. A coarse display mesh loses a large
/// share of its 2.7e3 mm3 on the arc face; the exact (0.005 mm) mesh does not.
fn lens() -> (Session, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let half = 20.0_f64;
    let centre = -(200.0_f64.powi(2) - half * half).sqrt();
    s.sketch_line(sk, &LineSpec { x1: v(-half), y1: v(0.), x2: v(half), y2: v(0.), construction: false, dimensioned: true }).unwrap();
    s.sketch_arc(
        sk,
        &ArcSpec {
            cx: v(0.),
            cy: v(centre),
            r: None,
            start_angle: None,
            end_angle: None,
            start: Some(Xy(v(half), v(0.))),
            end: Some(Xy(v(-half), v(0.))),
            ccw: true,
            construction: false,
            dimensioned: true,
        },
    )
    .unwrap();
    let ex: Extrude = serde_json::from_value(serde_json::json!({"sketch": sk, "height": "100", "op": "add"})).unwrap();
    let (body, r) = s.extrude(&ex).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    (s, body)
}

/// Every face of `body` copied and the copies stitched: a real, thin, curved solid.
fn stitched_copy(s: &mut Session, body: Id) -> Result<(Id, Rebuild)> {
    let ids: Vec<u32> = s.topology(Some(body), false).unwrap().faces.iter().map(|f| f.id).collect();
    assert_eq!(ids.len(), 4, "two caps, the flat and the arc");
    let parts: Vec<Id> = ids.iter().map(|f| s.face_copy(Some(body), &Sel::Ids(vec![*f]), None).unwrap().0).collect();
    s.stitch(&parts, &Num::Value(0.01), None)
}

/// F-198 must not refuse a thin curved real solid: the old check compared the kernel volume with the DISPLAY mesh by a flat 5 % and
/// the display mesh of the arc face (a few chords) lost far more than that at Draft and at Normal.
#[test]
fn a_thin_curved_stitched_solid_is_not_refused_at_any_quality() {
    for quality in [GeomQuality::Draft, GeomQuality::Normal, GeomQuality::Fine] {
        let (mut s, body) = lens();
        s.p.geom_quality = quality;
        s.p.mark_all_dirty();
        let r = s.rebuild();
        assert!(r.errors.is_empty(), "{quality:?}: {:?}", r.errors);
        let (solid, r) = stitched_copy(&mut s, body).unwrap_or_else(|e| panic!("{quality:?}: a real solid refused: {e}"));
        assert!(r.errors.is_empty(), "{quality:?}: {:?}", r.errors);
        let b = s.result_bodies().into_iter().find(|b| b.id == solid).unwrap();
        assert!(!b.sheet, "{quality:?}: a closed shell is a solid");
        let original = s.result_bodies().into_iter().find(|x| x.id == body).unwrap().volume;
        assert!((b.volume - original).abs() < 1.0, "{quality:?}: {} against {original}", b.volume);
    }
}

/// The gap and the tolerance of F-198 scaled down: the collapsed shell is refused whatever the plate size.
#[test]
fn a_collapsed_stitch_is_refused_for_small_gaps_too_f198() {
    for (gap, tol) in [(0.5, 1.0), (1.0, 2.0), (0.1, 0.2)] {
        let mut s = Session::new_part();
        let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
        s.sketch_rect(sk, &v(0.), &v(0.), &v(60.), &v(40.), false).unwrap();
        let ex: Extrude = serde_json::from_value(serde_json::json!({"sketch": sk, "height": "12", "op": "add"})).unwrap();
        let (plate, _) = s.extrude(&ex).unwrap();
        let up = Sel::Facing { dir: [0., 0., 1.], tol_deg: 1.0 };
        let (a, _, _) = s.face_copy(Some(plate), &up, None).unwrap();
        let (b, _, _) = s.offset_surface(Some(plate), &up, &v(gap), None).unwrap();
        let e = s.stitch(&[a, b], &v(tol), None).map(|_| ()).expect_err("must be refused").to_string();
        assert!(e.contains("not one") && e.contains("smaller `tolerance`"), "gap {gap} tol {tol}: {e}");
    }
}

/// The allowance of the check: the mesh's own error (`area * deflection`) and a relative 0.1 %, signs ignored.
#[test]
fn the_volume_allowance_follows_area_and_deflection() {
    // 4000 against 0 over 4800 mm2 at 0.005: the collapse of F-198.
    assert!(collapsed_solid(4000., 0., 4800., 0.005));
    // A thin lens: 2674.7 against 2674.0 is the mesh's rounding, 2674.7 against 2377.0 is not at a fine deflection but is at 0.16.
    assert!(!collapsed_solid(2674.7, 2674.0, 8060., 0.005));
    assert!(collapsed_solid(2674.7, 2377.0, 8060., 0.005));
    assert!(!collapsed_solid(2674.7, 2377.0, 8060., 0.16), "a coarse mesh may be that far off");
    // The relative floor and the sign.
    assert!(!collapsed_solid(1000., 999.5, 0., 0.005), "0.05 % is rounding");
    assert!(collapsed_solid(1000., 997., 0., 0.005), "0.3 % with no mesh error to explain it");
    assert!(!collapsed_solid(-2674.7, 2674.0, 8060., 0.005), "a solid turned inside out is not a collapse");
    assert!(collapsed_solid(-4000., 0., 4800., 0.005));
}
