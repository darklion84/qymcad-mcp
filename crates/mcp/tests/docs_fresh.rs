//! docs/TOOLS.md is generated from the registry; fail when it is stale.
#[test]
fn tools_md_is_up_to_date() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/TOOLS.md");
    let on_disk = std::fs::read_to_string(path).unwrap_or_default();
    let fresh = qymcad_mcp::tools::Registry::new().markdown();
    assert!(on_disk == fresh, "docs/TOOLS.md is stale: run `cargo run -p qymcad-mcp -- --dump-tools > docs/TOOLS.md`");
}
