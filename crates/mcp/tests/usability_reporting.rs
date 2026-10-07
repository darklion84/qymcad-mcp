//! Reporting descriptions expose the coordinate and file-system conventions agents need.

use qymcad_mcp::tools::Registry;

#[test]
fn reporting_descriptions_explain_frames_save_directory_and_bbox_padding() {
    let tools = Registry::new().list();
    for (name, required) in [
        ("sketch_create", "world origin projected onto the face plane"),
        ("sketch_create", "x/y directions"),
        ("sketch_info", "world_frame"),
        ("doc_save", "directory must exist"),
        ("doc_info", "OCCT tolerance padding"),
        ("doc_info", "doc_open"),
    ] {
        let description = tools.iter().find(|t| t["name"] == name).unwrap()["description"].as_str().unwrap();
        assert!(description.contains(required), "{name} must explain {required}: {description}");
    }
}
