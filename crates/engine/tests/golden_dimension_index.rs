//! F2 (ADR 0049): an indexed dimension_set edits exactly the selected row, even when the x and y pins
//! of one point share their native references.
mod common;
use common::*;
use qymcad_core::model::Project;
use qymcad_engine::*;
use serde_json::json;
use std::collections::HashSet;

/// Indices of driving dimension rows whose native reference set is shared with another row.
fn ambiguous_rows(s: &Session, sk: Id) -> Vec<usize> {
    let si = s.project().sketch_index(sk).unwrap();
    let cs = &s.project().sketches[si].constraints;
    let key = |c: &qymcad_core::model::Constraint| Project::dim_refs(c).map(|r| r.into_iter().collect::<std::collections::BTreeSet<_>>());
    (0..cs.len()).filter(|&i| key(&cs[i]).is_some_and(|k| cs.iter().filter(|c| key(c) == Some(k.clone())).count() > 1)).collect()
}
fn points(s: &Session, sk: Id) -> Vec<(f64, f64)> {
    let si = s.project().sketch_index(sk).unwrap();
    s.project().sketches[si].points.iter().map(|p| (p.x, p.y)).collect()
}
fn circle() -> (Session, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    s.sketch_circle(sk, &3.0.into(), &4.0.into(), &2.0.into(), false).unwrap();
    (s, sk)
}
/// Closed triangle (3,4),(13,4),(13,14) extruded 2: area 1/2*10*10 = 50, V = 100.
fn triangle() -> (Session, Id, Id) {
    let mut s = Session::new_part();
    let sk = s.sketch_create(&PlaneRef::Base(BaseName::XY), None).unwrap();
    let spec: PolylineSpec = serde_json::from_value(json!({"points":[[3,4],[13,4],[13,14]],"closed":true})).unwrap();
    s.sketch_polyline(sk, &spec).unwrap();
    let (body, r) = s.extrude(&serde_json::from_value(json!({"sketch":sk,"height":2})).unwrap()).unwrap();
    assert!(r.errors.is_empty());
    assert_close(s.result_bodies()[0].volume, 100.0, 1e-6, "triangle 50 x 2");
    (s, sk, body)
}

#[test]
fn f2_circle_centre_x_and_y_rows_each_edit_only_their_own_axis() {
    let (s0, sk) = circle();
    let rows = ambiguous_rows(&s0, sk);
    assert_eq!(rows.len(), 2, "centre x/y pins share references: {rows:?}");
    let centre = |s: &Session| {
        let si = s.project().sketch_index(sk).unwrap();
        let c = s.project().sketches[si].entities.iter().find_map(|e| e.kind.as_circle().map(|c| c.center)).unwrap();
        let p = s.project().sketches[si].point(c).unwrap();
        (p.x, p.y)
    };
    let mut results = Vec::new();
    for &ci in &rows {
        let (mut s, _) = circle();
        let edit = s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: ci }, Some(&7.0.into()), None).unwrap();
        assert_eq!(edit.dimension.target, Some(DimensionTarget::Sketch { sketch: sk, constraint: ci }), "reports the selected row");
        assert_eq!(edit.dimension.value, Some(7.0));
        let (x, y) = centre(&s);
        // Exactly one coordinate becomes 7, the other keeps its original value (3 or 4).
        assert!(((x - 7.0).abs() < 1e-9 && (y - 4.0).abs() < 1e-9) || ((x - 3.0).abs() < 1e-9 && (y - 7.0).abs() < 1e-9), "({x},{y})");
        results.push(((x * 1e6).round() as i64, (y * 1e6).round() as i64));
    }
    assert_ne!(results[0], results[1], "the two rows drive different axes");
}

#[test]
fn f2_triangle_second_vertex_y_edit_changes_volume_to_70() {
    let (s0, sk, _) = triangle();
    let rows = ambiguous_rows(&s0, sk);
    assert!(rows.len() >= 6, "three vertices with x/y pins: {rows:?}");
    // Moving (13,4) to (13,7): triangle (3,4),(13,7),(13,14), area 1/2*|10*10 - 3*10| = 35, V = 70.
    let target: HashSet<(i64, i64)> = [(3, 4), (13, 7), (13, 14)].into_iter().map(|(x, y)| (x * 1_000_000, y * 1_000_000)).collect();
    let mut hits = 0;
    for &ci in &rows {
        let (mut s, _, _) = triangle();
        let edit = s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: ci }, Some(&7.0.into()), None).unwrap();
        assert!(edit.rebuild.errors.is_empty());
        let got: HashSet<(i64, i64)> =
            points(&s, sk).into_iter().map(|(x, y)| ((x * 1e6).round() as i64, (y * 1e6).round() as i64)).collect();
        if target.is_subset(&got) {
            hits += 1;
            assert_close(s.result_bodies()[0].volume, 70.0, 1e-6, "triangle 35 x 2");
            // App path: saved and reopened, the edited row and solid stay.
            let path = scratch("f2-triangle.qcad");
            s.save(Some(&path)).unwrap();
            let (opened, r) = Session::open(&path).unwrap();
            assert!(r.errors.is_empty());
            assert_close(opened.result_bodies()[0].volume, 70.0, 1e-6, "reopened");
        }
    }
    assert_eq!(hits, 1, "exactly one row is the second vertex's y");
}

#[test]
fn f2_formula_on_an_ambiguous_row_follows_its_parameter() {
    let (s0, sk) = circle();
    let rows = ambiguous_rows(&s0, sk);
    for &ci in &rows {
        let (mut s, _) = circle();
        s.param_set("yy", &7.0.into()).unwrap();
        let edit = s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: ci }, Some(&"yy".into()), None).unwrap();
        assert_eq!(edit.dimension.expression.as_deref(), Some("yy"));
        assert_eq!(edit.dimension.value, Some(7.0));
        s.param_set("yy", &9.0.into()).unwrap();
        let si = s.project().sketch_index(sk).unwrap();
        let c = &s.project().sketches[si].constraints[ci];
        assert_eq!(Project::dim_value_of(c), Some(9.0), "row {ci} follows yy");
        assert_eq!(c.expr(), Some("yy"));
    }
}

#[test]
fn f2_naming_an_ambiguous_row_is_refused_even_with_empty_name() {
    let (mut s, sk) = circle();
    let ci = ambiguous_rows(&s, sk)[1];
    let before = serde_json::to_value(s.project()).unwrap();
    for name in ["CentreY", ""] {
        let e = s
            .dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: ci }, Some(&7.0.into()), Some(name))
            .unwrap_err()
            .to_string();
        assert!(e.contains(&format!("constraint {ci} in sketch {sk} has ambiguous native driver references")), "{e}");
        assert_eq!(serde_json::to_value(s.project()).unwrap(), before, "refused before any change");
    }
}

#[test]
fn f2_undo_restores_the_old_row_and_solid_in_one_step() {
    let (mut s, sk, _) = triangle();
    let before = serde_json::to_value(s.project()).unwrap();
    let ci = ambiguous_rows(&s, sk)[3];
    let snapshot = s.begin_tool_edit().unwrap();
    s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: ci }, Some(&7.0.into()), None).unwrap();
    s.finish_tool_edit(snapshot, true);
    s.undo().unwrap();
    let mut after = serde_json::to_value(s.project()).unwrap();
    // Undo keeps the monotonic allocator (F-056).
    after["next_id"] = before["next_id"].clone();
    assert_eq!(after, before);
    assert_close(s.result_bodies()[0].volume, 100.0, 1e-6, "old solid");
}

/// A native name lives on the first set-equal row; another row reading it is no cycle.
#[test]
fn f2_second_row_may_read_the_name_of_the_first_row() {
    let (mut s, sk) = circle();
    let rows = ambiguous_rows(&s, sk);
    let (first, second) = (rows[0], rows[1]);
    let path = scratch("f2-first-row-name.qcad");
    s.save(Some(&path)).unwrap();
    let qymcad_io::LoadedProject { mut project, breps } = qymcad_io::load_project_with_brep(path.to_str().unwrap()).unwrap();
    let refs = Project::dim_key_pub(&Project::dim_refs(&project.sketches[0].constraints[first]).unwrap());
    assert!(project.name_dim("first_axis".into(), qymcad_core::model::DimTarget::Sketch { sketch: sk, refs }));
    qymcad_io::save_project_guarded_with_brep(&project, path.to_str().unwrap(), &breps).unwrap();
    let (mut s, r) = Session::open(&path).unwrap();
    assert!(r.errors.is_empty());
    let first_value = Project::dim_value_of(&s.project().sketches[0].constraints[first]).unwrap();
    let edit = s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: second }, Some(&"first_axis+2".into()), None).unwrap();
    assert_eq!(edit.dimension.value, Some(first_value + 2.0));
    assert_eq!(edit.dimension.name, "");
    assert_eq!(edit.dimension.expression.as_deref(), Some("first_axis+2"));
    assert_eq!(Project::dim_value_of(&s.project().sketches[0].constraints[first]), Some(first_value));
    // The named row itself still cannot read its own name.
    let error =
        s.dimension_set(&DimensionTarget::Sketch { sketch: sk, constraint: first }, Some(&"first_axis+2".into()), None).unwrap_err();
    assert!(error.to_string().contains("LeansOnItself"), "{error}");
}
