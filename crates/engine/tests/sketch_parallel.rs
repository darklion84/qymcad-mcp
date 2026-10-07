//! A required Parallel must be satisfied independently of coordinate scale.
mod common;
use common::*;
use qymcad_engine::*;

#[test]
fn implied_parallelism_uses_angle_tolerance_at_any_line_length() {
    use qymcad_core::{
        feature::Purpose,
        model::{Constraint, EntityKind, Project},
    };
    for length in [1.0, 1e6] {
        let mut p = Project::default();
        p.new_document();
        let sk = p.add_sketch("S", vec![], None);
        p.add_sketch_node(sk, "S");
        let si = p.sketch_index(sk).unwrap();
        // tan(theta) = 5e-10: this falls inside the 1e-9 angular tolerance at either scale.
        let line = p.add_line_entity(si, 0.0, 5.0, length, 5.0 + length * 5e-10, Purpose::Construction);
        let (a, b) = match p.sketches[si].entities.iter().find(|e| e.id == line).unwrap().kind {
            EntityKind::Line { a, b } => (a, b),
            _ => unreachable!(),
        };
        p.sketches[si].constraints.extend([Constraint::Fixed { p: a }, Constraint::Fixed { p: b }]);
        let path = scratch(&format!("parallel_scale_{length}.qcad"));
        qymcad_io::save_project_guarded_with_brep(&p, path.to_str().unwrap(), &[]).unwrap();
        let (mut s, _) = Session::open(&path).unwrap();
        let c = ConstrainSpec {
            kind: ConstraintKind::Distance,
            refs: vec![SketchRef::Id(line), SketchRef::Frame(FrameRef::XAxis)],
            value: None,
            axis: DistAxis::Aligned,
            reference: true,
        };
        let result = s.sketch_constrain(sk, &c);
        assert!(result.is_ok(), "a 5e-10-radian parallel deviation was refused at length {length}: {result:?}");
    }
}
