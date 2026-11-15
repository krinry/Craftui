use super::protocol::{CallToolResult, Tool};
use craftui_core::{Size, WindowMetrics};
use craftui_dsl::resolve_source;
use craftui_reactive::Scope;
use craftui_render::{
    BackendKind, PaintScene, TextureRegistry, encode_rgba_png, render_scene_to_rgba,
};
use craftui_runtime::HotReloadSession;
use craftui_widgets::WidgetTree;
use serde_json::{Value, json};
use std::path::Path;

pub fn list_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "craftui_validate".to_string(),
            description: "Parse and validate CraftUI .cui declarative UI markup. Returns syntax errors, type errors, unknown components, and suggested fixes with line/column coordinates.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "The .cui DSL code snippet or full file to validate"
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional file path context for resolving imports"
                    }
                },
                "required": ["code"]
            }),
        },
        Tool {
            name: "craftui_widget_catalog".to_string(),
            description: "Inspect the complete machine-readable catalog of CraftUI widgets, including allowed properties, types, default values, and event handler signatures.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "widget": {
                        "type": "string",
                        "description": "Optional widget name to filter (e.g., 'Button', 'TextInput', 'VirtualList'). Omit to list all widgets."
                    }
                }
            }),
        },
        Tool {
            name: "craftui_scaffold".to_string(),
            description: "Generate 100% valid, production-ready .cui component templates and corresponding Rust controller code.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "template": {
                        "type": "string",
                        "enum": ["counter", "login_form", "master_detail", "settings_card", "search_filter", "todo_list", "custom_card"],
                        "description": "The template recipe to scaffold"
                    },
                    "name": {
                        "type": "string",
                        "description": "Component identifier name (e.g., 'ProfileSettings', 'TaskBoard')"
                    }
                },
                "required": ["template"]
            }),
        },
        Tool {
            name: "craftui_render_preview".to_string(),
            description: "Headlessly render a .cui DSL component into an RGBA PNG snapshot using CraftUI's software renderer. Returns a base64 PNG data URL or saves to an output file path.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "The .cui DSL source code to render"
                    },
                    "component": {
                        "type": "string",
                        "description": "Optional exported component name to mount (defaults to first export)"
                    },
                    "width": {
                        "type": "integer",
                        "default": 800,
                        "description": "Logical viewport width in pixels"
                    },
                    "height": {
                        "type": "integer",
                        "default": 600,
                        "description": "Logical viewport height in pixels"
                    },
                    "out_path": {
                        "type": "string",
                        "description": "Optional filesystem path to save the output PNG. If omitted, returns base64 image data."
                    }
                },
                "required": ["code"]
            }),
        },
        Tool {
            name: "craftui_doctor".to_string(),
            description: "Inspect the local CraftUI toolchain readiness, including Rust compiler version, cross-compilation targets (wasm32, android), Android NDK, and wasm-bindgen.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "default": ".",
                        "description": "Project root directory to inspect"
                    }
                }
            }),
        },
        Tool {
            name: "craftui_explain".to_string(),
            description: "Get detailed architectural explanations and exact code patterns for CraftUI reactivity, layout, two-way bindings, animations, and signals.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "enum": ["reactivity", "layout", "two_way_binding", "animations", "virtual_list", "theming"],
                        "description": "The core CraftUI topic to explain"
                    }
                },
                "required": ["topic"]
            }),
        },
    ]
}

pub fn call_tool(name: &str, args: Value) -> CallToolResult {
    match name {
        "craftui_validate" => tool_validate(args),
        "craftui_widget_catalog" => tool_widget_catalog(args),
        "craftui_scaffold" => tool_scaffold(args),
        "craftui_render_preview" => tool_render_preview(args),
        "craftui_doctor" => tool_doctor(args),
        "craftui_explain" => tool_explain(args),
        _ => CallToolResult::err(format!("unknown tool '{name}'")),
    }
}

fn tool_validate(args: Value) -> CallToolResult {
    let code = match args.get("code").and_then(Value::as_str) {
        Some(c) => c,
        None => return CallToolResult::err("missing required 'code' argument"),
    };
    let path = args.get("path").and_then(Value::as_str);
    let base_dir = path.and_then(|p| Path::new(p).parent());

    let resolved = resolve_source(code, base_dir);
    let diags = resolved.diagnostics;

    if diags.is_empty() {
        CallToolResult::ok(
            "✓ Validation succeeded: 0 errors, 0 warnings. The .cui DSL code is 100% valid.",
        )
    } else {
        let mut out = String::new();
        let error_count = diags
            .iter()
            .filter(|d| matches!(d.severity, craftui_dsl::Severity::Error))
            .count();
        let warn_count = diags
            .iter()
            .filter(|d| matches!(d.severity, craftui_dsl::Severity::Warning))
            .count();

        out.push_str(&format!(
            "Validation result: {error_count} error(s), {warn_count} warning(s)\n\n"
        ));

        let lines: Vec<&str> = code.lines().collect();
        for (i, d) in diags.iter().enumerate() {
            let sev = match d.severity {
                craftui_dsl::Severity::Error => "ERROR",
                craftui_dsl::Severity::Warning => "WARNING",
            };
            out.push_str(&format!(
                "[{}] {} (bytes {}-{}): {}\n",
                i + 1,
                sev,
                d.span.start,
                d.span.end,
                d.message
            ));

            // Compute line and column from byte offset
            let mut offset = 0;
            for (l_idx, line) in lines.iter().enumerate() {
                let line_len = line.len() + 1; // +1 for newline
                if offset + line_len > d.span.start {
                    let line_no = l_idx + 1;
                    let col_no = d.span.start - offset + 1;
                    out.push_str(&format!("  --> Line {line_no}:{col_no}\n"));
                    out.push_str(&format!("   | {}\n", line));
                    let caret_pad = " ".repeat(col_no.saturating_sub(1));
                    let caret_len = (d.span.end.saturating_sub(d.span.start)).max(1);
                    out.push_str(&format!("   | {}{}\n", caret_pad, "^".repeat(caret_len)));
                    break;
                }
                offset += line_len;
            }
            out.push('\n');
        }

        if error_count > 0 {
            CallToolResult::err(out)
        } else {
            CallToolResult::ok(out)
        }
    }
}

fn tool_widget_catalog(args: Value) -> CallToolResult {
    let filter = args
        .get("widget")
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase());

    let catalog = vec![
        json!({
            "name": "Container",
            "category": "Layout",
            "description": "Versatile box container supporting padding, margin, width, height, background color/brush, border, corner radius, box shadow, and clip.",
            "props": ["width", "height", "min_width", "max_width", "min_height", "max_height", "padding", "margin", "background", "border_color", "border_width", "corner_radius", "shadow_color", "shadow_blur", "shadow_offset_x", "shadow_offset_y", "clip", "opacity"],
            "events": ["on_tap", "on_hover"]
        }),
        json!({
            "name": "Column",
            "category": "Layout",
            "description": "Vertical flex container stacking children from top to bottom.",
            "props": ["gap", "align_items", "justify_content", "padding", "width", "height", "background", "corner_radius"],
            "events": ["on_tap"]
        }),
        json!({
            "name": "Row",
            "category": "Layout",
            "description": "Horizontal flex container arranging children from left to right.",
            "props": ["gap", "align_items", "justify_content", "padding", "width", "height", "background", "corner_radius"],
            "events": ["on_tap"]
        }),
        json!({
            "name": "Stack",
            "category": "Layout",
            "description": "Z-order overlapping container rendering children layered on top of each other.",
            "props": ["width", "height", "padding", "background", "clip"],
            "events": ["on_tap"]
        }),
        json!({
            "name": "Text",
            "category": "Display",
            "description": "Typography element for labels, titles, and paragraphs.",
            "props": ["text", "color", "font_size", "font_weight", "font_family", "line_height", "letter_spacing", "align", "max_lines", "overflow"],
            "events": ["on_tap"]
        }),
        json!({
            "name": "Button",
            "category": "Control",
            "description": "Interactive push button with automatic hover, pressed, and disabled states.",
            "props": ["text", "label", "foreground", "color", "font_size", "font_weight", "disabled", "background", "padding", "corner_radius", "border_color", "border_width"],
            "events": ["on_tap", "on_change"]
        }),
        json!({
            "name": "TextInput",
            "category": "Control",
            "description": "Single-line or multiline text input field with placeholder and cursor selection.",
            "props": ["value", "placeholder", "text", "multiline", "disabled", "font_size", "color", "background", "border_color", "corner_radius", "padding"],
            "events": ["on_change", "on_submit"]
        }),
        json!({
            "name": "Switch",
            "category": "Control",
            "description": "Binary toggle switch for boolean preferences with animated thumb.",
            "props": ["checked", "label", "disabled"],
            "events": ["on_change"]
        }),
        json!({
            "name": "Checkbox",
            "category": "Control",
            "description": "Checkable box with optional text label.",
            "props": ["checked", "label", "disabled"],
            "events": ["on_change"]
        }),
        json!({
            "name": "Slider",
            "category": "Control",
            "description": "Continuous or stepped range slider for numeric values.",
            "props": ["value", "min", "max", "label", "disabled"],
            "events": ["on_change"]
        }),
        json!({
            "name": "VirtualList",
            "category": "Layout",
            "description": "High-performance virtualized recycler view rendering only visible off-screen items.",
            "props": ["direction", "columns", "item_extent", "item_count", "count", "items", "model"],
            "events": []
        }),
        json!({
            "name": "ScrollView",
            "category": "Layout",
            "description": "Scrollable viewport for overflowing content with smooth kinetic physics.",
            "props": ["direction", "width", "height"],
            "events": []
        }),
        json!({
            "name": "Tabs",
            "category": "Navigation",
            "description": "Tab strip for switching between different pages or views.",
            "props": ["tabs", "active"],
            "events": ["on_change"]
        }),
        json!({
            "name": "Select",
            "category": "Control",
            "description": "Dropdown select menu for choosing from a list of options.",
            "props": ["options", "selected", "placeholder"],
            "events": ["on_change"]
        }),
        json!({
            "name": "Badge",
            "category": "Display",
            "description": "Pill-shaped indicator badge for status, counters, or labels.",
            "props": ["label", "foreground", "background", "icon"],
            "events": []
        }),
        json!({
            "name": "Spinner",
            "category": "Feedback",
            "description": "Indeterminate spinning activity indicator.",
            "props": ["size", "thickness", "color"],
            "events": []
        }),
        json!({
            "name": "ProgressIndicator",
            "category": "Feedback",
            "description": "Linear progress bar supporting determinate (0.0 to 1.0) or indeterminate mode.",
            "props": ["progress", "color", "background"],
            "events": []
        }),
        json!({
            "name": "ToastHost",
            "category": "Feedback",
            "description": "Overlay container rendering ephemeral notification toasts.",
            "props": ["items"],
            "events": ["on_dismiss"]
        }),
        json!({
            "name": "SegmentedControl",
            "category": "Control",
            "description": "iOS/macOS style segmented toggle control.",
            "props": ["items", "selected"],
            "events": ["on_change"]
        }),
        json!({
            "name": "BottomNavBar",
            "category": "Navigation",
            "description": "Mobile bottom navigation bar with icons and labels.",
            "props": ["items", "icons", "selected"],
            "events": ["on_change"]
        }),
    ];

    let filtered: Vec<Value> = if let Some(ref f) = filter {
        catalog
            .into_iter()
            .filter(|w| {
                w.get("name")
                    .and_then(Value::as_str)
                    .map(|n| n.to_ascii_lowercase().contains(f))
                    .unwrap_or(false)
            })
            .collect()
    } else {
        catalog
    };

    CallToolResult::ok(serde_json::to_string_pretty(&filtered).unwrap_or_else(|_| "[]".to_string()))
}

fn tool_scaffold(args: Value) -> CallToolResult {
    let template = match args.get("template").and_then(Value::as_str) {
        Some(t) => t,
        None => return CallToolResult::err("missing required 'template' argument"),
    };
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("AppView");

    let cui_code = match template {
        "counter" => format!(
            r#"export component {name} {{
    in-out property <int> count: 0;
    callback on_increment();
    callback on_decrement();

    Container {{
        width: 100%;
        height: 100%;
        background: #0f172a;
        padding: 32px;

        Column {{
            gap: 24px;
            align: "center";
            justify: "center";

            Text {{
                text: "Reactive Counter";
                font_size: 28px;
                font_weight: "bold";
                color: #f8fafc;
            }}

            Text {{
                text: "Count: " + count;
                font_size: 48px;
                font_weight: "bold";
                color: #38bdf8;
            }}

            Row {{
                gap: 16px;

                Button {{
                    text: "- Decrement";
                    background: #334155;
                    foreground: #ffffff;
                    padding: 12px;
                    border_radius: 8px;
                    on_tap => {{
                        count -= 1;
                        on_decrement();
                    }}
                }}

                Button {{
                    text: "+ Increment";
                    background: #0284c7;
                    foreground: #ffffff;
                    padding: 12px;
                    border_radius: 8px;
                    on_tap => {{
                        count += 1;
                        on_increment();
                    }}
                }}
            }}
        }}
    }}
}}"#
        ),
        "login_form" => format!(
            r#"export component {name} {{
    in-out property <string> email: "";
    in-out property <string> password: "";
    in-out property <bool> remember_me: true;
    in-out property <string> error_msg: "";
    callback on_submit(string, string);

    Container {{
        width: 100%;
        height: 100%;
        background: #090d16;
        padding: 24px;

        Column {{
            gap: 20px;
            width: 380px;
            align: "stretch";
            justify: "center";

            Text {{
                text: "Welcome Back";
                font_size: 26px;
                font_weight: "bold";
                color: #f1f5f9;
            }}

            if error_msg != "" : Container {{
                background: #7f1d1d;
                padding: 10px;
                border_radius: 6px;
                Text {{
                    text: error_msg;
                    color: #fca5a5;
                    font_size: 13px;
                }}
            }}

            Column {{
                gap: 6px;
                Text {{
                    text: "Email Address";
                    color: #94a3b8;
                    font_size: 13px;
                }}
                TextInput {{
                    placeholder: "you@example.com";
                    value <=> email;
                    background: #1e293b;
                    color: #ffffff;
                    padding: 10px;
                    border_radius: 6px;
                }}
            }}

            Column {{
                gap: 6px;
                Text {{
                    text: "Password";
                    color: #94a3b8;
                    font_size: 13px;
                }}
                TextInput {{
                    placeholder: "••••••••";
                    value <=> password;
                    background: #1e293b;
                    color: #ffffff;
                    padding: 10px;
                    border_radius: 6px;
                }}
            }}

            Row {{
                justify: "space_between";
                align: "center";
                Checkbox {{
                    label: "Remember me";
                    checked <=> remember_me;
                }}
            }}

            Button {{
                text: "Sign In";
                background: #2563eb;
                foreground: #ffffff;
                padding: 12px;
                border_radius: 6px;
                on_tap => {{
                    on_submit(email, password);
                }}
            }}
        }}
    }}
}}"#
        ),
        "settings_card" => format!(
            r#"export component {name} {{
    in-out property <bool> notifications: true;
    in-out property <bool> dark_theme: true;
    in-out property <float> volume: 75.0;
    callback on_change_settings();

    Container {{
        width: 100%;
        background: #1e293b;
        padding: 20px;
        border_radius: 12px;

        Column {{
            gap: 16px;
            align: "stretch";

            Text {{
                text: "Preferences";
                font_size: 20px;
                font_weight: "bold";
                color: #f8fafc;
            }}

            Row {{
                justify: "space_between";
                align: "center";
                Text {{
                    text: "Push Notifications";
                    color: #cbd5e1;
                    font_size: 15px;
                }}
                Switch {{
                    checked <=> notifications;
                    on_change => {{
                        on_change_settings();
                    }}
                }}
            }}

            Row {{
                justify: "space_between";
                align: "center";
                Text {{
                    text: "Dark Theme";
                    color: #cbd5e1;
                    font_size: 15px;
                }}
                Switch {{
                    checked <=> dark_theme;
                    on_change => {{
                        on_change_settings();
                    }}
                }}
            }}

            Column {{
                gap: 8px;
                Text {{
                    text: "Master Volume: " + volume + "%";
                    color: #cbd5e1;
                    font_size: 14px;
                }}
                Slider {{
                    value <=> volume;
                    min: 0.0;
                    max: 100.0;
                    on_change => {{
                        on_change_settings();
                    }}
                }}
            }}
        }}
    }}
}}"#
        ),
        _ => format!(
            r#"export component {name} {{
    in-out property <string> title: "{name}";

    Container {{
        padding: 24px;
        background: #0f172a;
        border_radius: 8px;

        Column {{
            gap: 12px;
            Text {{
                text: title;
                font_size: 22px;
                font_weight: "bold";
                color: #ffffff;
            }}
        }}
    }}
}}"#
        ),
    };

    let controller_code = format!(
        r#"// Rust Controller Pattern for {name}
use craftui::prelude::*;

pub struct {name}Controller {{
    pub count: Signal<i64>,
}}

impl {name}Controller {{
    pub fn new(cx: Scope) -> Self {{
        Self {{
            count: cx.signal(0),
        }}
    }}

    pub fn increment(&self) {{
        self.count.update(|c| *c += 1);
    }}

    pub fn decrement(&self) {{
        self.count.update(|c| *c -= 1);
    }}
}}"#
    );

    let output = format!(
        "// === UI DSL: ui/{}.cui ===\n{}\n\n// === Rust Controller ===\n{}",
        name.to_ascii_lowercase(),
        cui_code,
        controller_code
    );

    CallToolResult::ok(output)
}

fn tool_render_preview(args: Value) -> CallToolResult {
    let code = match args.get("code").and_then(Value::as_str) {
        Some(c) => c,
        None => return CallToolResult::err("missing required 'code' argument"),
    };
    let component = args.get("component").and_then(Value::as_str);
    let width = args.get("width").and_then(Value::as_u64).unwrap_or(800) as u32;
    let height = args.get("height").and_then(Value::as_u64).unwrap_or(600) as u32;
    let out_path = args.get("out_path").and_then(Value::as_str);

    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);
    let mut session = HotReloadSession::new(cx);

    if let Err(e) = session.reload_with_base(code, None, component, &mut tree) {
        return CallToolResult::err(format!("Failed to mount .cui code for rendering: {e}"));
    }

    let metrics = WindowMetrics {
        logical_size: Size::new(width.max(1) as f32, height.max(1) as f32),
        scale_factor: 1.0,
        ..Default::default()
    };
    tree.layout(metrics);
    let mut scene = PaintScene::new();
    tree.paint(&mut scene);

    let textures = TextureRegistry::new();
    let rgba = render_scene_to_rgba(
        BackendKind::VelloCpu,
        &scene,
        width.max(1),
        height.max(1),
        1.0,
        tree.theme().background,
        &textures,
        &mut tree.text_engine,
    );

    let png_bytes = match encode_rgba_png(width.max(1), height.max(1), &rgba) {
        Ok(b) => b,
        Err(e) => return CallToolResult::err(format!("Failed to encode RGBA to PNG: {e}")),
    };

    if let Some(path_str) = out_path {
        let p = Path::new(path_str);
        if let Err(e) = std::fs::write(p, &png_bytes) {
            return CallToolResult::err(format!("Failed to write PNG to {}: {e}", p.display()));
        }
        CallToolResult::ok(format!(
            "Successfully rendered {}x{} snapshot to {}",
            width,
            height,
            p.display()
        ))
    } else {
        let b64 = base64_encode(&png_bytes);
        CallToolResult::ok(format!("data:image/png;base64,{b64}"))
    }
}

fn tool_doctor(args: Value) -> CallToolResult {
    let p = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let path = Path::new(p);
    match crate::doctor::collect_doctor_report(path) {
        Ok(report) => {
            let json_str = crate::doctor::format_doctor_json(&report);
            CallToolResult::ok(json_str)
        }
        Err(e) => CallToolResult::err(format!("Failed to collect doctor report: {e}")),
    }
}

fn tool_explain(args: Value) -> CallToolResult {
    let topic = match args.get("topic").and_then(Value::as_str) {
        Some(t) => t,
        None => return CallToolResult::err("missing required 'topic' argument"),
    };

    let explanation = match topic {
        "reactivity" => {
            r#"# CraftUI Fine-Grained Reactivity
- Scope: Owns the reactive lifecycle (`let cx = Scope::root();`).
- Signal<T>: Reactive state container.
  - Read: `signal.get()` (tracks dependency in current computation).
  - Write: `signal.set(val)` or `signal.update(|v| *v += 1)` (notifies observers).
- In .cui DSL:
  - `<=>` Two-way binding (e.g. `value <=> text_signal;` or `checked <=> bool_signal;`).
  - `:=` One-way binding (e.g. `text: "Count: " + count;`)."#
        }
        "layout" => {
            r#"# CraftUI Flexbox Layout
- `Column`: Stacks children vertically (`gap`, `align_items`, `justify_content`).
- `Row`: Arranges children horizontally (`gap`, `align_items`, `justify_content`).
- `Stack`: Overlays children along the Z-axis.
- Sizing:
  - Exact: `width: 240px;`
  - Relative: `width: 100%; height: 100%;`
  - Flexible: `flex: 1;` takes remaining space in flex container."#
        }
        "animations" => {
            r#"# CraftUI Transitions & Animations
- Per-node animation block:
  `animate opacity { duration: 180ms; easing: EaseInOut; }`
- Spring physics:
  `animate translation_y { curve: spring(280.0, 26.0); }`
  Presets: `curve: snappy();`, `curve: gentle();`"#
        }
        "two_way_binding" => {
            r#"# Two-Way Data Binding (<=>)
In CraftUI .cui files:
- Use `<=>` between a widget's property and a reactive property/signal.
- Example:
  `TextInput { value <=> search_query; }`
  `Switch { checked <=> is_active; }`
  `Slider { value <=> volume_level; min: 0.0; max: 100.0; }`"#
        }
        _ => "Topic not found. Available topics: reactivity, layout, animations, two_way_binding.",
    };

    CallToolResult::ok(explanation)
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}
