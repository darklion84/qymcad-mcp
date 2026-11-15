use qymcad_engine::Error;

#[test]
fn exact_duplicates_collapse_but_distinct_diagnostics_keep_their_order() {
    let first = "divide (11): split failed";
    let other_node = "divide (12): split failed";
    let other_reason = "divide (11): source body not built";
    let text = Error::Rebuild(vec![first.into(), other_node.into(), first.into(), other_reason.into(), other_node.into()]).to_string();
    assert_eq!(text, format!("the model did not rebuild (operation rolled back): {first}; {other_node}; {other_reason}"));
}
