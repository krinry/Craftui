use super::protocol::{Resource, ResourceContent};

pub fn list_resources() -> Vec<Resource> {
    vec![
        Resource {
            uri: "craftui://docs/cheatsheet".to_string(),
            name: "CraftUI .cui Syntax & Widget Cheatsheet".to_string(),
            description: Some(
                "One-page cheat sheet of all keywords, types, and properties".to_string(),
            ),
            mime_type: Some("text/markdown".to_string()),
        },
        Resource {
            uri: "craftui://docs/llms-full".to_string(),
            name: "CraftUI Full LLM Specification".to_string(),
            description: Some(
                "Token-optimized single-file architecture & anti-hallucination spec".to_string(),
            ),
            mime_type: Some("text/plain".to_string()),
        },
        Resource {
            uri: "craftui://templates/counter".to_string(),
            name: "Counter Example Template".to_string(),
            description: Some("Minimal reactive counter example in .cui".to_string()),
            mime_type: Some("text/plain".to_string()),
        },
        Resource {
            uri: "craftui://templates/login".to_string(),
            name: "Login Form Template".to_string(),
            description: Some("Production login form with input validation in .cui".to_string()),
            mime_type: Some("text/plain".to_string()),
        },
    ]
}

pub fn read_resource(uri: &str) -> Option<ResourceContent> {
    match uri {
        "craftui://docs/cheatsheet" => Some(ResourceContent {
            uri: uri.to_string(),
            mime_type: Some("text/markdown".to_string()),
            text: include_str!("../../../../docs/src/dsl/cheatsheet.md").to_string(),
        }),
        "craftui://docs/llms-full" => Some(ResourceContent {
            uri: uri.to_string(),
            mime_type: Some("text/plain".to_string()),
            text: include_str!("../../../../docs/llms-full.txt").to_string(),
        }),
        "craftui://templates/counter" => Some(ResourceContent {
            uri: uri.to_string(),
            mime_type: Some("text/plain".to_string()),
            text: r#"export component Counter {
    in-out property <int> count: 0;

    Container {
        padding: 24px;
        Column {
            gap: 16px;
            Text { text: "Count: " + count; font_size: 24px; }
            Row {
                gap: 8px;
                Button { text: "-"; on_tap => { count -= 1; } }
                Button { text: "+"; on_tap => { count += 1; } }
            }
        }
    }
}"#
            .to_string(),
        }),
        "craftui://templates/login" => Some(ResourceContent {
            uri: uri.to_string(),
            mime_type: Some("text/plain".to_string()),
            text: r#"export component LoginForm {
    in-out property <string> email: "";
    in-out property <string> password: "";
    callback on_submit(string, string);

    Column {
        gap: 12px;
        width: 320px;
        TextInput { placeholder: "Email"; value <=> email; }
        TextInput { placeholder: "Password"; value <=> password; }
        Button { text: "Sign In"; on_tap => { on_submit(email, password); } }
    }
}"#
            .to_string(),
        }),
        _ => None,
    }
}
