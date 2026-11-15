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
    let templates = [
        "counter",
        "login_form",
        "settings_card",
        "master_detail",
        "search_filter",
        "todo_list",
        "custom_card",
    ];
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

    // Unsupported template returns error
    let bad_res = call_tool(
        "craftui_scaffold",
        json!({ "template": "nonexistent_template", "name": "BadView" }),
    );
    assert!(bad_res.is_error);
    assert!(bad_res.content[0].text.contains("Unsupported template"));
}

#[test]
fn test_mcp_explain_topics_and_rejections() {
    let topics = [
        "reactivity",
        "layout",
        "two_way_binding",
        "animations",
        "virtual_list",
        "theming",
    ];
    for topic in topics {
        let res = call_tool("craftui_explain", json!({ "topic": topic }));
        assert!(!res.is_error, "Failed explain topic {topic}");
        assert!(!res.content[0].text.is_empty());
    }

    let bad_topic = call_tool("craftui_explain", json!({ "topic": "unknown_topic" }));
    assert!(bad_topic.is_error);
    assert!(bad_topic.content[0].text.contains("not found"));
}

#[test]
fn test_mcp_render_preview_validation() {
    let valid_cui = "export component SimpleCard { Text { text: \"Hello\"; } }";

    // Invalid dimensions are rejected
    let bad_width = call_tool(
        "craftui_render_preview",
        json!({ "code": valid_cui, "width": 99999 }),
    );
    assert!(bad_width.is_error);

    let zero_height = call_tool(
        "craftui_render_preview",
        json!({ "code": valid_cui, "height": 0 }),
    );
    assert!(zero_height.is_error);

    // Non-existent component requested is rejected
    let missing_comp = call_tool(
        "craftui_render_preview",
        json!({ "code": valid_cui, "component": "NonExistent" }),
    );
    assert!(missing_comp.is_error);
    assert!(
        missing_comp.content[0]
            .text
            .contains("Requested component 'NonExistent' not found")
    );
}

#[test]
fn test_preview_defaults_to_entry_not_imported_component() {
    let code = r#"import { Card } from "std/material.cui"
export component App { Text { text: "Entry component"; } }"#;
    let default = call_tool(
        "craftui_render_preview",
        json!({"code": code, "width": 80, "height": 80}),
    );
    let explicit = call_tool(
        "craftui_render_preview",
        json!({"code": code, "component": "App", "width": 80, "height": 80}),
    );
    assert!(!default.is_error, "{}", default.content[0].text);
    assert!(!explicit.is_error, "{}", explicit.content[0].text);
    assert_eq!(default.content[0].text, explicit.content[0].text);
}

#[test]
fn test_explain_uses_supported_theme_and_list_properties() {
    let theme = call_tool("craftui_explain", json!({"topic": "theming"}));
    assert!(theme.content[0].text.contains("Theme.text_primary"));
    assert!(!theme.content[0].text.contains("`theme."));
    let list = call_tool("craftui_explain", json!({"topic": "virtual_list"}));
    assert!(list.content[0].text.contains("item_extent: 56px"));
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
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].role, "user");
    assert!(msgs[0].content.text.contains("HomeScreen"));
}
