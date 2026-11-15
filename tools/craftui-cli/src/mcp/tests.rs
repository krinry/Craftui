use super::prompts::{get_prompt, list_prompts};
use super::resources::{list_resources, read_resource};
use super::tools::{call_tool, list_tools};
use serde_json::json;

#[test]
fn test_mcp_list_tools_contains_all_core_capabilities() {
    let tools = list_tools();
    assert!(tools.len() >= 6);

    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"craftui_validate"));
    assert!(names.contains(&"craftui_widget_catalog"));
    assert!(names.contains(&"craftui_scaffold"));
    assert!(names.contains(&"craftui_render_preview"));
    assert!(names.contains(&"craftui_doctor"));
    assert!(names.contains(&"craftui_explain"));
}

#[test]
fn test_mcp_validate_tool_valid_code() {
    let code = r#"
export component SimpleCounter {
    in-out property <int> count: 0;

    Container {
        padding: 16px;
        Column {
            gap: 8px;
            Text {
                text: "Count: " + count;
                color: #38bdf8;
            }
            Button {
                text: "+ Increment";
                on_tap => {
                    count += 1;
                }
            }
        }
    }
}
"#;
    let result = call_tool("craftui_validate", json!({ "code": code }));
    assert!(!result.is_error);
    assert!(result.content[0].text.contains("Validation succeeded"));
}

#[test]
fn test_mcp_validate_tool_catches_invalid_syntax_and_unknown_widget() {
    let bad_code = r#"
export component BadView {
    UnknownWidget {
        non_existent_prop: 123;
    }
}
"#;
    let result = call_tool("craftui_validate", json!({ "code": bad_code }));
    assert!(result.is_error);
    assert!(
        result.content[0]
            .text
            .contains("unknown component or widget 'UnknownWidget'")
    );
}

#[test]
fn test_mcp_widget_catalog_tool() {
    // Unfiltered catalog
    let all = call_tool("craftui_widget_catalog", json!({}));
    assert!(!all.is_error);
    assert!(all.content[0].text.contains("VirtualList"));
    assert!(all.content[0].text.contains("TextInput"));
    assert!(all.content[0].text.contains("Button"));

    // Filtered by Button
    let filtered = call_tool("craftui_widget_catalog", json!({ "widget": "Button" }));
    assert!(!filtered.is_error);
    assert!(filtered.content[0].text.contains("Button"));
}

#[test]
fn test_mcp_scaffold_tool_generates_valid_cui() {
    let templates = ["counter", "login_form", "settings_card"];
    for t in templates {
        let res = call_tool(
            "craftui_scaffold",
            json!({ "template": t, "name": "TestView" }),
        );
        assert!(!res.is_error, "Failed scaffolding template {t}");
        let text = &res.content[0].text;
        assert!(text.contains("export component TestView"));

        // Extract CUI portion and validate it
        let cui_start = text.find("export component TestView").unwrap();
        let cui_end = text.find("// === Rust Controller ===").unwrap();
        let cui_code = &text[cui_start..cui_end];

        let validation = call_tool("craftui_validate", json!({ "code": cui_code }));
        assert!(
            !validation.is_error,
            "Scaffolded template {t} failed validation: {}",
            validation.content[0].text
        );
    }
}

#[test]
fn test_mcp_resources_and_prompts() {
    let resources = list_resources();
    assert!(!resources.is_empty());

    let cheatsheet = read_resource("craftui://docs/cheatsheet");
    assert!(cheatsheet.is_some());
    assert!(cheatsheet.unwrap().text.contains("Cheatsheet"));

    let prompts = list_prompts();
    assert!(!prompts.is_empty());

    let prompt = get_prompt(
        "craftui_new_screen",
        &json!({ "screen_name": "HomeScreen", "description": "Dashboard" }),
    );
    assert!(prompt.is_some());
    let msgs = prompt.unwrap();
    assert_eq!(msgs.len(), 2);
    assert!(msgs[1].content.text.contains("HomeScreen"));
}
