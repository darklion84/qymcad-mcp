//! Accepted live-review regressions, exercised through the real tool handlers.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(r: &mut Registry, name: &str, args: Value) -> Value {
    let content = r.call(name, args).unwrap().unwrap();
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

fn block() -> Registry {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY", "name":"profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"profile", "entities":[{"type":"rect", "w":40, "h":40}]}));
    let built = call(&mut r, "extrude", json!({"sketch":"profile", "height":10, "name":"stock"}));
    // Rectangular prism: V=w*l*h.
    assert!((built["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - 40.0 * 40.0 * 10.0).abs() < 1e-6);
    r
}

fn empty_hint(r: &mut Registry, selection: Value, expected: &str) {
    let preview = call(r, "select", json!({"edges":selection}));
    assert_eq!(preview["count"], 0);
    let hint = preview["hint"].as_str().unwrap();
    assert!(hint.starts_with(expected), "body-level hint: {hint}");
    let before = call(r, "doc_info", json!({}));
    for (tool, dimension) in [("fillet", json!({"radius":1})), ("chamfer", json!({"dist":0.5}))] {
        let mut args = dimension;
        args["edges"] = selection.clone();
        let error = r.call(tool, args).unwrap().unwrap_err();
        assert!(error.contains(hint), "{tool} repeats selection hint: {error}");
        assert_eq!(call(r, "doc_info", json!({})), before, "empty {tool} leaves geometry unchanged");
    }
}

#[test]
fn composed_convex_hint_reports_all_box_corners() {
    let mut r = block();
    // A prism has four edges in each of three directions: 12 convex edges, all lines.
    assert_eq!(call(&mut r, "select", json!({"edges":{"convex":true}}))["count"], 3 * 4);
    empty_hint(
        &mut r,
        json!({"and":[{"convex":true},{"kind":"circle"}]}),
        "this body has 12 convex (outward) edges, but none matches the other conditions of the selection; it has 0 concave edges",
    );
    empty_hint(&mut r, json!({"concave":true}), "0 concave (inward) edges; this body has 12 convex edges");
}

#[test]
fn composed_concave_hint_reports_cone_plane_rim() {
    let mut r = block();
    call(&mut r, "sketch_create", json!({"plane":"XZ", "name":"cone"}));
    call(&mut r, "sketch_add", json!({"sketch":"cone", "entities":[{"type":"polyline", "points":[[0,10],[4,10],[0,14]], "closed":true}]}));
    let built = call(&mut r, "revolve", json!({"sketch":"cone", "axis":"sketch_y", "op":"add"}));
    // Cone V=πR²h/3 from integrating πR²(1-z/h)² dz; joins the box at z=10.
    let expected = 40.0 * 40.0 * 10.0 + std::f64::consts::PI * 4.0_f64.powi(2) * 4.0 / 3.0;
    assert!((built["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6);
    // The cone apex adds no edge; only its circular concave base supplements the box's 12 convex edges.
    assert_eq!(call(&mut r, "select", json!({"edges":{"concave":true}}))["count"], 1);
    assert_eq!(call(&mut r, "select", json!({"edges":{"convex":true}}))["count"], 3 * 4);
    empty_hint(
        &mut r,
        json!({"and":[{"concave":true},{"kind":"line"}]}),
        "this body has 1 concave (inward) edge, but none matches the other conditions of the selection; it has 12 convex edges",
    );
}

fn shallow_hole() -> Registry {
    let mut r = block();
    // R-r=6-2=4; depth=(R-r)*tan(0.9°), a deliberately uncertain circular rim.
    call(
        &mut r,
        "hole",
        json!({"face":{"facing":"+z"}, "diameter":4, "depth":6,
        "kind":"countersink", "dia2":12, "depth2":4.0*0.9_f64.to_radians().tan()}),
    );
    r
}

#[test]
fn uncertain_note_respects_other_and_conditions() {
    let mut r = shallow_hole();
    // The one uncertain rim is circular; no uncertain edge is a line (seams are excluded).
    for kind in ["concave", "convex"] {
        let line = call(&mut r, "select", json!({"edges":{"and":[{kind:true},{"kind":"line"}]}}));
        assert!(line.get("note").is_none(), "line filter excludes the circular uncertain rim: {line}");
        let circle = call(&mut r, "select", json!({"edges":{"and":[{kind:true},{"kind":"circle"}]}}));
        assert_eq!(circle["note"], "omitted 1 uncertain edges");
    }
}

#[test]
fn uncertain_note_in_nonmonotone_compositions_is_truthful() {
    let mut r = shallow_hole();
    let minus = call(&mut r, "select", json!({"edges":{"minus":[{"convex":true},{"kind":"circle"}]}}));
    assert_eq!(minus["note"], "this body has 1 edge whose corner side is uncertain; it is not in this result");
    let union = call(&mut r, "select", json!({"edges":{"union":[{"convex":true},{"kind":"circle"}]}}));
    assert!(union.get("note").is_none(), "union includes the uncertain circle, so it is not omitted: {union}");
}

fn rounded_drilled_block() -> Registry {
    let mut r = block();
    let edges = json!({"and":[{"along":"x"},{"edges_of":{"facing":"+z"}}]});
    assert_eq!(call(&mut r, "select", json!({"edges":edges}))["count"], 2);
    let filleted = call(&mut r, "fillet", json!({"edges":edges,"radius":1}));
    call(&mut r, "hole", json!({"face":{"facing":"+z"},"diameter":8,"at":[0,0,10],"through":true}));
    // Two quarter-circle spandrels of area R²(1-π/4), each extruded by width40.
    let expected = 40.0 * 40.0 * 10.0 - 2.0 * (1.0 - std::f64::consts::PI / 4.0) * 40.0;
    assert!((filleted["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6);
    r
}

#[test]
fn chamfer_chain_tangent_junctions_are_not_uncertain_corners() {
    let mut r = rounded_drilled_block();
    let cylinder_edges = json!({"edges_of":{"kind":"cylinder"}});
    // Each of two fillet cylinders has four boundaries; hole has two rims plus seam.
    assert_eq!(call(&mut r, "select", json!({"edges":cylinder_edges}))["count"], 2 * 4 + 3);
    call(&mut r, "chamfer", json!({"edges":cylinder_edges,"dist":0.5}));
    let topo = call(&mut r, "topology", json!({"adjacency":true}));
    let short: Vec<_> = topo["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["kind"] == "line"
                && e["seam"] != true
                && e["mid"][2].as_f64().unwrap() >= 9.0
                && (e["length"].as_f64().unwrap() - 0.5 * 2.0_f64.sqrt()).abs() < 0.00005
        })
        .collect();
    // Each of four end-arc/vertical transitions has two plane/cone tangent boundary lines.
    assert_eq!(short.len(), 4 * 2);
    for edge in &short {
        let mut kinds: Vec<_> = edge["faces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| topo["faces"].as_array().unwrap().iter().find(|f| f["id"] == *id).unwrap()["kind"].as_str().unwrap())
            .collect();
        kinds.sort_unstable();
        assert_eq!(kinds, ["cone", "plane"], "each transition is a cone/plane boundary");
    }
    for corner in ["concave", "convex"] {
        let selected = call(&mut r, "select", json!({"edges":{corner:true}}));
        assert!(selected.get("note").is_none(), "all eight plane/cone G1 junctions must be excluded: {selected}");
        assert!(short.iter().all(|e| !selected["edges"].as_array().unwrap().iter().any(|picked| picked["id"] == e["id"])));
        let circle = call(&mut r, "select", json!({"edges":{"and":[{corner:true},{"kind":"circle"}]}}));
        assert!(circle.get("note").is_none(), "short lines cannot be omitted circle candidates: {circle}");
    }
}

#[test]
fn mesh_export_volume_names_its_cubic_millimetre_unit() {
    let mut r = block();
    let dir = std::env::temp_dir().join(format!("qymcad-backlog3-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (format, extension) in [("stl", "stl"), ("3mf", "3mf"), ("glb", "glb"), ("obj", "obj"), ("step", "step")] {
        let exported = call(&mut r, "export", json!({"format":format,"path":dir.join(format!("box.{extension}"))}));
        let body = &exported["bodies"][0];
        assert!(body.get("mesh_volume").is_none(), "no compatibility alias: {body}");
        if format == "step" {
            assert!(body.get("mesh_volume_mm3").is_none(), "exact export has no mesh volume");
        } else {
            // A box mesh is exact in every quality/format: V=w*l*h mm³, including GLB's metre coordinates.
            let volume = body["mesh_volume_mm3"].as_f64().expect("mesh_volume_mm3 must include its unit");
            assert!((volume - 40.0 * 40.0 * 10.0).abs() < 1e-6);
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn save_reports_fresh_and_replaced_targets() {
    let mut r = block();
    let path = std::env::temp_dir().join(format!("qymcad-backlog3-save-{}.qcad", std::process::id()));
    assert!(!path.exists(), "test starts at a fresh path");
    let first = call(&mut r, "doc_save", json!({"path":path}));
    assert_ne!(first["replaced"], true, "fresh save is not a replacement");
    let second = call(&mut r, "doc_save", json!({"path":path}));
    assert_eq!(second["replaced"], true, "second explicit save reports replacement");
    assert_eq!(call(&mut r, "doc_save", json!({}))["replaced"], true, "associated save also reports replacement");
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"different"}));
    call(&mut r, "sketch_add", json!({"sketch":"different","entities":[{"type":"rect","w":10,"h":12}]}));
    call(&mut r, "extrude", json!({"sketch":"different","height":2}));
    assert_eq!(call(&mut r, "doc_save", json!({"path":path}))["replaced"], true, "explicit path allows different-model replacement");
    let opened = call(&mut r, "doc_open", json!({"path":path}));
    // The replacement prism volume proves the second model, rather than the old 40×40×10 block, was saved.
    assert!((opened["doc"]["bodies"][0]["volume_mm3"].as_f64().unwrap() - 10.0 * 12.0 * 2.0).abs() < 1e-6);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn direction_errors_list_unsigned_and_signed_axes() {
    let mut r = block();
    let error = r.call("select", json!({"edges":{"along":"bad"}})).unwrap().unwrap_err();
    for axis in ["x", "y", "z", "+x", "-x", "+y", "-y", "+z", "-z"] {
        assert!(error.contains(&format!("\"{axis}\"")), "accepted direction {axis} missing: {error}");
        // Four prism edges per direction; signs do not change an along selection.
        assert_eq!(call(&mut r, "select", json!({"edges":{"along":axis}}))["count"], 4);
    }
}

#[test]
fn incompatible_kind_errors_use_public_lowercase_names() {
    let mut r = block();
    for (elements, kinds) in [("edges", vec!["plane", "cylinder", "cone", "sphere"]), ("faces", vec!["line", "circle", "arc", "curve"])] {
        for kind in kinds {
            let error = r.call("select", json!({elements:{"kind":kind}})).unwrap().unwrap_err();
            assert!(error.contains(&format!("kind {kind} cannot select {elements}")), "public kind name: {error}");
        }
    }
}

#[test]
fn datum_sketch_plane_label_includes_name_and_native_id() {
    let mut r = block();
    call(&mut r, "plane_offset", json!({"base":"XY","dist":3,"name":"p3"}));
    call(&mut r, "sketch_create", json!({"plane":{"plane":"p3"},"name":"on datum"}));
    let info = call(&mut r, "sketch_info", json!({"sketch":"on datum"}));
    let doc = call(&mut r, "doc_info", json!({}));
    let datum = doc["timeline"].as_array().unwrap().iter().find(|n| n["name"] == "p3").unwrap();
    assert_eq!(info["plane"], format!("p3 (plane {})", datum["id"]), "named datum uses timeline name and retains id");
    assert_eq!(doc["sketches"].as_array().unwrap().iter().find(|s| s["name"] == "on datum").unwrap()["plane"], info["plane"]);
    assert_eq!(doc["sketches"][0]["plane"], "XY", "base plane label remains conventional");
}

#[test]
fn blend_descriptions_explain_kernel_tangent_propagation() {
    let list = Registry::new().list();
    for name in ["fillet", "chamfer"] {
        let description = list.iter().find(|t| t["name"] == name).unwrap()["description"].as_str().unwrap();
        assert!(description.contains("the kernel extends a blend along edges tangent-continuous with a selected edge; preview with select shows only the selected edges"),
            "{name} explains tangent propagation: {description}");
    }
}

#[test]
fn kernel_chamfer_extends_beyond_cylinder_selection() {
    let mut r = rounded_drilled_block();
    let source = call(&mut r, "topology", json!({"adjacency":true}));
    let selection = json!({"edges_of":{"kind":"cylinder"}});
    let preview = call(&mut r, "select", json!({"edges":selection}));
    // Two 4-boundary fillet cylinders plus a 2-rim/1-seam bore: preview is 11, not the full kernel contours.
    assert_eq!(preview["count"], 2 * 4 + 3);
    let extra: Vec<_> = source["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            if e["kind"] != "line" || e["seam"] == true {
                return false;
            }
            let a = e["a"].as_array().unwrap();
            let b = e["b"].as_array().unwrap();
            // Four vertical stock corners have span h-r=9; two top end edges lie at x=±w/2,z=h.
            (b[2].as_f64().unwrap() - a[2].as_f64().unwrap()).abs() > 9.0 - 1e-6
                || ((a[2].as_f64().unwrap() - 10.0).abs() < 1e-6
                    && (b[2].as_f64().unwrap() - 10.0).abs() < 1e-6
                    && (e["mid"][0].as_f64().unwrap().abs() - 40.0 / 2.0).abs() < 1e-6)
        })
        .collect();
    assert_eq!(extra.len(), 4 + 2);
    assert!(
        extra.iter().all(|e| !preview["edges"].as_array().unwrap().iter().any(|picked| picked["id"] == e["id"])),
        "six propagated edges are absent from preview"
    );
    // Equal setbacks c bisect orthogonal supports: four (±x,±y) planes and two (±x,+z) planes.
    let diagonal = 1.0 / 2.0_f64.sqrt();
    let mut normals = Vec::new();
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            normals.push([sx * diagonal, sy * diagonal, 0.0]);
        }
        normals.push([sx * diagonal, 0.0, diagonal]);
    }
    let has_normal = |topo: &Value, n: &[f64; 3]| {
        topo["faces"].as_array().unwrap().iter().any(|f| {
            f["normal"].as_array().is_some_and(|normal| normal.iter().zip(n).all(|(a, b)| (a.as_f64().unwrap() - b).abs() < 0.00005))
        })
    };
    assert!(normals.iter().all(|n| !has_normal(&source, n)), "propagated bevel planes do not exist before chamfer");
    let before = call(&mut r, "doc_info", json!({}))["bodies"][0]["volume_mm3"].as_f64().unwrap();
    let built = call(&mut r, "chamfer", json!({"edges":selection,"dist":0.5}));
    assert_eq!(call(&mut r, "doc_info", json!({}))["warnings"], json!([]), "kernel propagates without a warning");
    let after = call(&mut r, "topology", json!({}));
    for n in &normals {
        assert!(has_normal(&after, n), "kernel adds unselected-edge bevel plane with normal {n:?}");
    }
    let (h, l, fillet_r, bore_r, c) = (10.0, 40.0, 1.0, 4.0, 0.5_f64);
    // Propagated straight bevels: triangular area c²/2 times 4(h-r)+2(l-2r).
    // Four selected quarter-cylinder ends sum to π∫₀ᶜ[r²-(r-x)²]dx = π(rc²-c³/3).
    // Two selected bore rims sum to 2π∫₀ᶜ[(R+x)²-R²]dx = 2π(Rc²+c³/3).
    let removed = (4.0 * (h - fillet_r) + 2.0 * (l - 2.0 * fillet_r)) * c.powi(2) / 2.0
        + std::f64::consts::PI * (fillet_r * c.powi(2) - c.powi(3) / 3.0)
        + 2.0 * std::f64::consts::PI * (bore_r * c.powi(2) + c.powi(3) / 3.0);
    let actual = before - built["rebuild"]["bodies"][0]["volume_mm3"].as_f64().unwrap();
    assert!((actual - removed).abs() < 1e-6, "propagated prism/cone volume: got {actual}, expected {removed}");
}

#[test]
fn uncertain_union_note_distinguishes_body_total_from_absent_subset() {
    let mut r = block();
    // Two disjoint R=6 shallow countersinks at x=±8: separation16 > 2R, one uncertain rim per cone.
    for x in [-8, 8] {
        call(
            &mut r,
            "hole",
            json!({"face":{"and":[{"facing":"+z"},{"extreme":"+z"}]},"at":[x,0,10],"diameter":4,"depth":6,
            "kind":"countersink","dia2":12,"depth2":4.0*0.9_f64.to_radians().tan()}),
        );
    }
    let topo = call(&mut r, "topology", json!({}));
    let rim = topo["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| {
            e["kind"] == "circle"
                && (e["radius"].as_f64().unwrap() - 6.0).abs() < 1e-6
                && (e["center"][0].as_f64().unwrap() + 8.0).abs() < 1e-6
        })
        .unwrap()["id"]
        .clone();
    let selected = call(&mut r, "select", json!({"edges":{"union":[{"convex":true},[rim]]}}));
    assert_eq!(
        selected["note"], "this body has 2 edges whose corner side is uncertain; 1 of them is not in this result",
        "two uncertain rims, one included by the pick branch, one absent"
    );
}
