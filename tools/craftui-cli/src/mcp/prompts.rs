use super::protocol::{ContentItem, Prompt, PromptArgument, PromptMessage};

pub fn list_prompts() -> Vec<Prompt> {
    vec![
        Prompt {
            name: "craftui_new_screen".to_string(),
            description: Some(
                "Prompt template to guide an LLM in creating a new CraftUI screen".to_string(),
            ),
            arguments: vec![
                PromptArgument {
                    name: "screen_name".to_string(),
                    description: Some(
                        "Name of the screen (e.g., SettingsScreen, CheckoutView)".to_string(),
                    ),
                    required: true,
                },
                PromptArgument {
                    name: "description".to_string(),
                    description: Some(
                        "Description of what the screen should do and contain".to_string(),
                    ),
                    required: true,
                },
            ],
        },
        Prompt {
            name: "craftui_debug_component".to_string(),
            description: Some(
                "Prompt template to guide an LLM in fixing errors in a .cui file".to_string(),
            ),
            arguments: vec![
                PromptArgument {
                    name: "error_log".to_string(),
                    description: Some("Compiler or validation error output".to_string()),
                    required: true,
                },
                PromptArgument {
                    name: "code".to_string(),
                    description: Some("The problematic .cui code".to_string()),
                    required: true,
                },
            ],
        },
    ]
}

pub fn get_prompt(name: &str, args: &serde_json::Value) -> Option<Vec<PromptMessage>> {
    match name {
        "craftui_new_screen" => {
            let screen_name = args
                .get("screen_name")
                .and_then(|v| v.as_str())
                .unwrap_or("MyScreen");
            let desc = args
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("A modern mobile and desktop screen");
            Some(vec![PromptMessage {
                role: "user".to_string(),
                content: ContentItem {
                    kind: "text".to_string(),
                    text: format!(
                        "You are an expert CraftUI engineer. Generate 100% valid .cui DSL code. Never emit HTML/JSX tags. Use 'export component Name {{ ... }}' and '<=>' for two-way state binding.\n\nPlease build the screen '{screen_name}'. Requirements: {desc}. Include both the .cui DSL file and the corresponding Rust controller pattern."
                    ),
                },
            }])
        }
        "craftui_debug_component" => {
            let errors = args
                .get("error_log")
                .and_then(|v| v.as_str())
                .unwrap_or("Syntax error");
            let code = args.get("code").and_then(|v| v.as_str()).unwrap_or("");
            Some(vec![PromptMessage {
                role: "user".to_string(),
                content: ContentItem {
                    kind: "text".to_string(),
                    text: format!(
                        "You are a debugging assistant for CraftUI. Fix the errors in the provided .cui file without inventing properties or syntax.\n\nThe following .cui component failed validation:\n```cui\n{code}\n```\n\nCompiler errors:\n{errors}\n\nPlease identify the exact issue and output the corrected .cui component."
                    ),
                },
            }])
        }
        _ => None,
    }
}
