//! `tools/list` is sent in full to clients without deferred tool loading, on every session. These tests keep its
//! size from growing back (review finding T1b/T1b+).
use qymcad_mcp::tools::Registry;

/// Size of the serialized `tools/list` result when the budget was set (215853 bytes) plus about 5%.
/// Raising it is a decision: shorten a description or a doc comment first.
const BUDGET_BYTES: usize = 226_600;

#[test]
fn tools_list_stays_within_budget() {
    let list = Registry::new().list();
    let total = serde_json::to_string(&list).unwrap().len();
    if total > BUDGET_BYTES {
        let mut rows: Vec<(usize, String)> =
            list.iter().map(|t| (serde_json::to_string(t).unwrap().len(), t["name"].as_str().unwrap().to_string())).collect();
        rows.sort_by(|a, b| b.cmp(a));
        let top: Vec<String> = rows.iter().take(6).map(|(n, name)| format!("{name} {n}")).collect();
        panic!("tools/list is {total} bytes, budget {BUDGET_BYTES}; largest: {}", top.join(", "));
    }
}

#[test]
fn selection_grammar_is_sent_once_and_in_select() {
    let list = Registry::new().list();
    let all = serde_json::to_string(&list).unwrap();
    assert_eq!(all.matches("Selection grammar (faces or edges)").count(), 1, "the grammar must not be repeated per tool");
    assert_eq!(all.matches("tangent_chain").count(), 1, "grammar keywords must not leak into other schemas");
    let select = list.iter().find(|t| t["name"] == "select").unwrap()["description"].as_str().unwrap();
    assert!(select.contains("Selection grammar (faces or edges)"), "the grammar lives in `select`");
    // Every other selection argument points at it.
    assert!(all.contains("Full grammar: tool `select`."));
}
