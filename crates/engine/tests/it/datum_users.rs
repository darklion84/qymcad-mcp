//! Datum scheduling guards must remain parameter users, labelled as sketch dependencies.

use qymcad_engine::*;

#[test]
fn datum_parameter_users_identify_sketch_dependencies() {
    let mut s = Session::new_part();
    s.param_set("t", &6.0.into()).unwrap();
    let (plane, _) = s.plane_offset(&PlaneRef::Base(BaseName::XY), &Num::Expr("t".into()), Some("top")).unwrap();
    let sketch = s.sketch_create(&PlaneRef::Plane(plane), Some("pocket sketch")).unwrap();
    let label = format!("datum dependency of sketch {sketch} `pocket sketch`");
    let users = s.param_users("t");
    assert!(users.contains(&label), "expected {label:?}; got {users:?}");
    assert!(!users.contains(&format!("feature {sketch} `pocket sketch`")), "a datum guard is not a modelling feature");
    let e = s.param_delete("t").unwrap_err();
    assert!(e.to_string().contains(&label), "{e}");
}
