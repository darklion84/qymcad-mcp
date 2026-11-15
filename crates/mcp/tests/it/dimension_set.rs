//! Driver selectors are exclusive, strict, atomic, batchable and undoable.
use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};
fn call(r: &mut Registry, tool: &str, args: Value) -> Value {
    let content = r.call(tool, args).unwrap().unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}
#[test]
fn dimension_set_is_strict_batchable_and_undoable() {
    let mut r = Registry::new();
    call(&mut r, "doc_new", json!({}));
    call(&mut r, "sketch_create", json!({"name":"s","plane":"XY"}));
    call(&mut r, "sketch_add", json!({"sketch":"s","entities":[{"type":"rect","cx":0,"cy":0,"w":10,"h":6}]}));
    call(&mut r, "extrude", json!({"sketch":"s","height":2,"name":"stock"}));
    let result = call(&mut r, "dimension_set", json!({"target":{"feature":"stock","key":"height"},"name":"Depth","value":3}));
    assert_eq!(result["dimension"]["name"], "Depth");
    let named = call(&mut r, "doc_info", json!({}))["named_dimensions"].clone();
    assert_eq!(named[0]["target"]["key"], "height");
    for target in [
        json!({"feature":"stock","key":"height","extra":true}),
        json!({"feature":"stock","key":"height","sketch":"s","constraint":0}),
        json!({"sketch":"s","constraint":0,"key":"height"}),
    ] {
        assert!(r.call("dimension_set", json!({"target":target,"value":4})).unwrap().is_err());
        assert_eq!(call(&mut r, "doc_info", json!({}))["named_dimensions"], named);
    }
    let result =
        call(&mut r, "batch", json!({"steps":[{"tool":"dimension_set","args":{"target":{"feature":"stock","key":"height"},"value":5}}]}));
    assert_eq!(result["ok"], true);
    assert_eq!(call(&mut r, "doc_info", json!({}))["named_dimensions"][0]["value"], 5.0);
    call(&mut r, "undo", json!({}));
    assert_eq!(call(&mut r, "doc_info", json!({}))["named_dimensions"], named);
    let failed = r
        .call(
            "batch",
            json!({"steps":[
                {"tool":"dimension_set","args":{"target":{"feature":"stock","key":"height"},"value":8}},
                {"tool":"dimension_set","args":{"target":{"feature":"stock","key":"unknown"},"value":4}}
            ]}),
        )
        .unwrap();
    assert!(failed.is_err());
    assert_eq!(call(&mut r, "doc_info", json!({}))["named_dimensions"], named, "failed batch restores dimensions");
}
