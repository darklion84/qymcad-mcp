//! Stage-1 sketch surfaces, strict input schemas, batch and undo.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};
fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let c = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn sketch_project_resolves_edge_selection_and_is_batchable() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"profile","entities":[{"type":"rect","w":40,"h":30}]}));
    call(&mut r, "extrude", json!({"sketch":"profile","height":10,"name":"body"}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"projection"}));
    let selected = call(&mut r, "select", json!({"body":"body","edges":{"edges_of":{"facing":"+z"}}}));
    let result = call(&mut r, "sketch_project", json!({"sketch":"projection","body":"body","edges":{"edges_of":{"facing":"+z"}}}));
    assert_eq!(result["projections"].as_array().unwrap().len(), 4);
    assert!(result["entities"].as_array().unwrap().iter().all(|e| e["driven"] == true));
    let again = call(&mut r, "sketch_project", json!({"sketch":"projection","body":"body","edges":{"edges_of":{"facing":"+z"}}}));
    assert_eq!(again["projections"], result["projections"]);
    assert_eq!(selected["count"], 4);
    let before = call(&mut r, "sketch_info", json!({"sketch":"projection"}));
    for args in [
        json!({"sketch":"projection","body":"body","edges":1,"face":1}),
        json!({"sketch":"projection","body":"body","edges":[2147483647]}),
        json!({"sketch":"missing","body":"body","edges":1}),
    ] {
        assert!(r.call("sketch_project", args).unwrap().is_err());
        assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"projection"})), before);
    }
    let batch = call(
        &mut r,
        "batch",
        json!({"steps":[{"tool":"sketch_project","args":{"sketch":"projection","body":"body","edges":{"edges_of":{"facing":"+z"}}}}],"verbose":true}),
    );
    assert_eq!(batch["steps"][0]["result"]["projections"].as_array().unwrap().len(), 4);
    call(&mut r, "undo", json!({}));
    assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"projection"}))["entities"], before["entities"]);
}
#[test]
fn sketch_project_refusals_and_batch_leave_geometry_and_history_unchanged() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"profile","entities":[{"type":"rect","w":40,"h":30}]}));
    call(&mut r, "extrude", json!({"sketch":"profile","height":10,"name":"body"}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"projection"}));
    call(&mut r, "param_set", json!({"name":"history_marker","value":123}));
    let before = call(&mut r, "sketch_info", json!({"sketch":"projection"}));
    let doc = call(&mut r, "doc_info", json!({}));
    let top = call(&mut r, "select", json!({"body":"body","faces":{"facing":"+z"}}))["faces"][0]["id"].clone();
    for args in [
        json!({"sketch":"projection"}),
        json!({"sketch":"projection","edges":[]}),
        json!({"sketch":"projection","edges":{"kind":"circle"}}),
        json!({"sketch":"projection","edges":{"edges_of":{"facing":"+z"}},"face":top}),
        json!({"sketch":"projection","edges":[2147483647]}),
        json!({"sketch":"missing","edges":{"edges_of":{"facing":"+z"}}}),
        json!({"sketch":"projection","body":"missing","edges":1}),
        json!({"sketch":"projection","face":{"kind":"plane"}}),
        json!({"sketch":"projection","face":top}),
        json!({"sketch":"projection","edges":1,"typo":true}),
    ] {
        assert!(r.call("sketch_project", args.clone()).unwrap().is_err(), "accepted {args}");
        assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"projection"})), before);
        assert_eq!(call(&mut r, "doc_info", json!({})), doc);
    }
    let failed = r
        .call(
            "batch",
            json!({"steps":[
                {"tool":"sketch_project","args":{"sketch":"projection","edges":{"edges_of":{"facing":"+z"}}}},
                {"tool":"sketch_project","args":{"sketch":"projection","edges":[2147483647]}}
            ]}),
        )
        .unwrap()
        .unwrap_err();
    assert!(failed.contains("step 2"), "{failed}");
    assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"projection"})), before);
    assert_eq!(call(&mut r, "doc_info", json!({})), doc);
    let undone = call(&mut r, "undo", json!({}));
    assert_eq!(undone["undone"]["tool"], "param_set", "refusals added undo entries");
    assert_eq!(undone["undone"]["arguments"]["name"], "history_marker");
    let projected = call(&mut r, "sketch_project", json!({"sketch":"projection","edges":{"edges_of":{"facing":"+z"}}}));
    assert_eq!(projected["entities"].as_array().unwrap().len(), 4);
    let undo = call(&mut r, "undo", json!({}));
    assert_eq!(undo["undone"]["tool"], "sketch_project");
    assert_eq!(undo["undone"]["label"], "Project sketch geometry");
    assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"projection"})), before);
}

#[test]
fn sketch_project_face_selector_requires_one_hosted_face() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"profile"}));
    call(&mut r, "sketch_add", json!({"sketch":"profile","entities":[{"type":"rect","w":40,"h":30}]}));
    let body = call(&mut r, "extrude", json!({"sketch":"profile","height":10}))["body"].clone();
    let top = call(&mut r, "select", json!({"body":body,"faces":{"facing":"+z"}}))["faces"][0]["id"].clone();
    call(&mut r, "sketch_create", json!({"plane":{"body":body,"face":top},"name":"hosted"}));
    let before = call(&mut r, "sketch_info", json!({"sketch":"hosted"}));
    let error = r.call("sketch_project", json!({"sketch":"hosted","face":{"kind":"plane"}})).unwrap().unwrap_err();
    assert!(error.contains("exactly one face"), "{error}");
    assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"hosted"})), before);
    let projected = call(&mut r, "sketch_project", json!({"sketch":"hosted","face":{"facing":"+z"}}));
    assert_eq!(projected["projections"].as_array().unwrap().len(), 1);
    assert_eq!(projected["entities"].as_array().unwrap().len(), 4);
    assert_eq!(projected["points"].as_array().unwrap().len(), 4);
    assert!(projected["entities"].as_array().unwrap().iter().all(|e| e["type"] == "line" && e["driven"] == true));
}

#[test]
fn spline_mcp_is_visible_and_batch_undo_restores_it() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"curves"}));
    let result = call(
        &mut r,
        "batch",
        json!({"steps":[{"tool":"sketch_add","args":{"sketch":"curves","entities":[{"type":"spline","points":[[2,3],[7,8],[12,3]]}]}}],"verbose":true}),
    );
    let spline = &result["steps"][0]["result"]["sketch"]["entities"][0];
    assert_eq!(spline["type"], "spline");
    assert_eq!(spline["closed"], false);
    assert_eq!(spline["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(result["steps"][0]["result"]["sketch"]["dof"], json!([0, 0]));
    call(&mut r, "undo", json!({}));
    assert!(call(&mut r, "sketch_info", json!({"sketch":"curves"}))["entities"].as_array().unwrap().is_empty());
    let before = call(&mut r, "sketch_info", json!({"sketch":"curves"}));
    for bad in [
        json!({"type":"spline","points":[[2,3],[7,8]],"closd":true}),
        json!({"type":"spline","points":[[2,3],[2,3]]}),
        json!({"type":"spline","points":[[2,3],[7,8]],"closed":true}),
    ] {
        assert!(r.call("sketch_add", json!({"sketch":"curves","entities":[bad]})).unwrap().is_err());
        assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"curves"})), before);
    }
}

#[test]
fn figures_and_tangent_circle_mcp_surface_is_strict_and_atomic() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "param_set", json!({"name":"N","value":6}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"figures"}));
    let result = call(
        &mut r,
        "sketch_add",
        json!({"sketch":"figures","entities":[
            {"type":"ellipse","cx":30,"cy":30,"rx":8,"ry":4,"turn":30,"construction":true},
            {"type":"ellipse_arc","cx":60,"cy":30,"rx":8,"ry":4,"turn":30,"start_angle":30,"end_angle":120,"construction":true},
            {"type":"rect","cx":100,"cy":30,"w":20,"h":10,"turn":30,"anchor":"corner","construction":true},
            {"type":"polygon","cx":150,"cy":30,"sides":"N","r":8,"angle":30,"construction":true}
        ]}),
    );
    assert_eq!(result["sketch"]["dof"], json!([0, 0]));
    let before = call(&mut r, "sketch_info", json!({"sketch":"figures"}));
    for bad in [
        json!({"type":"ellipse","rx":8,"ry":4,"turm":30}),
        json!({"type":"rect","w":20,"h":10,"anchor":"edge"}),
        json!({"type":"polygon","sides":6.5,"r":8}),
        json!({"type":"circle_tangent","curves":[{"kind":"line","extra":true},1],"radius":2,"near":[2,2]}),
        json!({"type":"circle_tangent","curves":[{"ids":[1],"kind":"line"},1],"radius":2,"near":[2,2]}),
    ] {
        assert!(r.call("sketch_add", json!({"sketch":"figures","entities":[bad]})).unwrap().is_err());
        assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"figures"})), before);
    }
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"touch"}));
    let lines = call(
        &mut r,
        "sketch_add",
        json!({"sketch":"touch","entities":[
            {"type":"line","x1":0,"y1":0,"x2":20,"y2":0,"construction":true},
            {"type":"line","x1":0,"y1":0,"x2":0,"y2":20,"construction":true}
        ]}),
    );
    let one = lines["created"][0]["entities"][0].clone();
    let two = lines["created"][1]["entities"][0].clone();
    let result = call(
        &mut r,
        "sketch_add",
        json!({"sketch":"touch","entities":[
            {"type":"circle_tangent","curves":[one,two],"radius":2,"near":[2,2]}
        ]}),
    );
    assert_eq!(result["sketch"]["dof"], json!([0, 0]));
    assert_eq!(result["created"][0]["type"], "circle_tangent");
    call(&mut r, "undo", json!({}));
    assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"touch"}))["entities"].as_array().unwrap().len(), 2);
}

#[test]
fn generic_curve_constraints_work_through_batch_and_undo() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"plane":"XY","name":"relations"}));
    let result = call(
        &mut r,
        "sketch_add",
        json!({"sketch":"relations","entities":[
            {"type":"ellipse","cx":10,"cy":10,"rx":8,"ry":4,"construction":true},
        {"type":"line","x1":14,"y1":14,"x2":24,"y2":14,"dimensioned":false}
        ]}),
    );
    let line = result["created"][1]["entities"][0].clone();
    let start = result["created"][1]["points"][0].clone();
    call(
        &mut r,
        "batch",
        json!({"steps":[
            {"tool":"sketch_constrain","args":{"sketch":"relations","kind":"horizontal","refs":[line]}},
            {"tool":"sketch_constrain","args":{"sketch":"relations","kind":"distance","refs":[line],"value":10}},
        {"tool":"sketch_constrain","args":{"sketch":"relations","kind":"distance","refs":["origin",start],"axis":"x","value":14}}
        ]}),
    );
    let before = call(&mut r, "sketch_info", json!({"sketch":"relations"}));
    let constrained = call(
        &mut r,
        "sketch_constrain",
        json!({
            "sketch":"relations","kind":"point_on_curve","refs":[start,{"kind":"ellipse"}]
        }),
    );
    assert_eq!(constrained["dof"], json!([0, 0]));
    assert_eq!(constrained["indices"].as_array().unwrap().len(), 1);
    assert!(r
        .call(
            "sketch_constrain",
            json!({
                "sketch":"relations","kind":"point_on_curve","refs":[start,{"kind":"ellipse","unknown":true}]
            })
        )
        .unwrap()
        .is_err());
    call(&mut r, "undo", json!({}));
    assert_eq!(call(&mut r, "sketch_info", json!({"sketch":"relations"})), before);
}
