use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    process::Command,
};

const COVERAGE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/COVERAGE.md");

fn variants(source: &str, name: &str) -> BTreeSet<String> {
    let marker = format!("pub enum {name} {{");
    let block = source.split_once(&marker).unwrap_or_else(|| panic!("missing declaration {name}")).1;
    let body = block.split_once("\n}").unwrap_or_else(|| panic!("unterminated declaration {name}")).0;
    let values: Vec<String> = body
        .lines()
        .filter_map(|line| {
            let item = line.trim().trim_end_matches(',');
            let ident = item.split(['(', '{', ' ']).next().unwrap_or("");
            (!ident.is_empty() && ident.chars().next().is_some_and(char::is_uppercase)).then(|| ident.to_owned())
        })
        .collect();
    let set: BTreeSet<_> = values.iter().cloned().collect();
    assert_eq!(values.len(), set.len(), "duplicate variant in {name} declaration");
    set
}

fn package_crates(metadata: &serde_json::Value) -> PathBuf {
    metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "qymcad-core" && p["source"].as_str().is_some_and(|s| s.starts_with("git+")))
        .and_then(|p| p["manifest_path"].as_str())
        .map(|p| PathBuf::from(p).parent().and_then(std::path::Path::parent).unwrap().to_path_buf())
        .expect("offline cargo metadata must locate pinned qymcad-core")
}

fn assert_all(source: &str, enum_name: &str, parsed: &BTreeSet<String>) {
    // Each enum keeps its own `ALL` in its own `impl` block; several enums share a file.
    let imp = source.split_once(&format!("impl {enum_name} {{")).unwrap_or_else(|| panic!("missing impl {enum_name}")).1;
    let rest = imp.split_once("pub const ALL:").unwrap_or_else(|| panic!("missing {enum_name}::ALL")).1;
    let rest = rest.split_once("= [").unwrap_or_else(|| panic!("missing {enum_name}::ALL array")).1;
    let body = rest.split_once(']').unwrap_or_else(|| panic!("unterminated {enum_name}::ALL")).0;
    let entries: BTreeSet<String> = body
        .split(',')
        .filter_map(|s| {
            let s = s.trim();
            (!s.is_empty()).then(|| s.rsplit("::").next().unwrap().to_owned())
        })
        .collect();
    assert_eq!(entries, *parsed, "{enum_name}::ALL must match parsed variants");
}

#[test]
fn every_pinned_enum_variant_has_exactly_one_coverage_row() {
    let metadata =
        Command::new("cargo").args(["metadata", "--offline", "--format-version", "1"]).output().expect("run cargo metadata --offline");
    assert!(metadata.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&metadata.stderr));
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let crates = package_crates(&metadata);
    let core = std::fs::read_to_string(crates.join("qymcad-core/src/feature/kind.rs")).unwrap();
    let ui_state = crates.join("qymcad-ui-state/src/records");
    let tools = std::fs::read_to_string(ui_state.join("tools.rs")).unwrap();
    let part = std::fs::read_to_string(ui_state.join("part_tool.rs")).unwrap();

    let mut expected: BTreeSet<String> = qymcad_testkit::feature_kinds().into_iter().map(|v| format!("FeatureKind::{v}")).collect();
    let parsed_features = variants(&core, "FeatureKind");
    assert_eq!(expected, parsed_features.iter().map(|v| format!("FeatureKind::{v}")).collect());
    for (name, source) in [("PartTool", &part), ("SketchShape", &tools), ("ConstraintKind", &tools)] {
        expected.extend(variants(source, name).into_iter().map(|v| format!("{name}::{v}")));
    }
    let part_variants = variants(&part, "PartTool");
    let sketch_variants = variants(&tools, "SketchShape");
    let constraint_variants = variants(&tools, "ConstraintKind");
    assert_all(&part, "PartTool", &part_variants);
    assert_all(&tools, "SketchShape", &sketch_variants);
    assert_all(&tools, "ConstraintKind", &constraint_variants);

    let doc = std::fs::read_to_string(COVERAGE).unwrap();
    let mut mapped: BTreeMap<String, usize> = BTreeMap::new();
    for line in doc.lines().filter(|line| line.starts_with('|')) {
        let cells: Vec<_> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() != 6 || cells[0] == "ID" || cells[0].starts_with('-') {
            continue;
        }
        for key in cells[5].split(',').map(|k| k.trim().trim_matches('`')).filter(|s| *s != "—" && !s.is_empty()) {
            *mapped.entry(key.to_owned()).or_default() += 1;
        }
    }
    let mut problems = Vec::new();
    for (key, count) in &mapped {
        if !expected.contains(key) {
            problems.push(format!("unknown coverage key {key}"));
        }
        if *count != 1 {
            problems.push(format!("key {key} is mapped {count} times"));
        }
    }
    for key in expected.difference(&mapped.keys().cloned().collect()) {
        problems.push(format!("missing coverage for {key}"));
    }
    assert!(problems.is_empty(), "docs/COVERAGE.md must classify every pinned variant exactly once:\n{}", problems.join("\n"));
}
