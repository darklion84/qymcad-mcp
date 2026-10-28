use qymcad_mcp::tools::Registry;
use serde_json::{json, Value};

fn call(registry: &mut Registry, name: &str, args: Value) -> Value {
    let content = registry.call(name, args).unwrap().unwrap_or_else(|error| panic!("{name}: {error}"));
    serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn body_boolean_tool_batches_with_named_piece_and_undo() {
    let mut registry = Registry::new();
    call(&mut registry, "doc_new", json!({}));
    call(&mut registry, "sketch_create", json!({"plane":"XY","name":"base sketch"}));
    call(&mut registry, "sketch_add", json!({"sketch":"base sketch","entities":[{"type":"rect","w":30,"h":10}]}));
    call(&mut registry, "extrude", json!({"sketch":"base sketch","height":20,"op":"new_body","name":"base"}));
    call(&mut registry, "sketch_create", json!({"plane":"XY","name":"tool sketch"}));
    call(&mut registry, "sketch_add", json!({"sketch":"tool sketch","entities":[{"type":"rect","cx":0,"cy":0,"w":2,"h":12}]}));
    call(&mut registry, "extrude", json!({"sketch":"tool sketch","height":20,"op":"new_body","name":"tool"}));
    let result = call(
        &mut registry,
        "batch",
        json!({"steps":[
            {"tool":"body_boolean","args":{"a":"base","b":"tool","op":"cut","name":"split cut","piece_names":["left","right"]}},
            {"tool":"chamfer","args":{"body":"right","edges":{"extreme":"+z"},"dist":0.5,"name":"piece chamfer"}}
        ]}),
    );
    assert_eq!(result["steps"][0]["created"]["name"], "split cut");
    assert_eq!(result["bodies"].as_array().unwrap().len(), 2);
    assert_eq!(call(&mut registry, "undo", json!({}))["undone"]["tool"], "batch");
    assert_eq!(call(&mut registry, "doc_info", json!({}))["bodies"].as_array().unwrap().len(), 2);
}
