//! J2 result formatting through actual tool handlers.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, name: &str, args: Value) -> Value {
    let content = r.call(name, args).unwrap().unwrap();
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn blend_reports_seam_note_and_keeps_preview_seam() {
    for tool in ["chamfer", "fillet"] {
        let mut r = Registry::new();
        call(&mut r, "doc_new", json!({}));
        call(&mut r, "sketch_create", json!({"plane":"XY", "name":"disc"}));
        call(&mut r, "sketch_add", json!({"sketch":"disc", "entities":[{"type":"circle", "d":10}]}));
        call(&mut r, "extrude", json!({"sketch":"disc", "height":10}));
        let sel = json!({"edges_of":{"kind":"cylinder"}});
        assert_eq!(call(&mut r, "select", json!({"edges":sel}))["count"], 3, "two rims plus seam remain in preview");
        let topo = call(&mut r, "topology", json!({}));
        assert_eq!(topo["edges"].as_array().unwrap().iter().filter(|e| e["seam"] == true).count(), 1);
        let dimension = if tool == "fillet" { "radius" } else { "dist" };
        let mut args = json!({"edges":sel});
        args[dimension] = json!(0.5);
        let result = call(&mut r, tool, args);
        assert_eq!(result["rebuild"]["notes"], json!(["dropped 1 seam edge: not blendable"]));
        assert!(
            !call(&mut r, "doc_info", json!({}))["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("dropped")),
            "selection note is transient"
        );
    }
}
