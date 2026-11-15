pub mod aot_ui {
    include!(concat!(env!("OUT_DIR"), "/craftpad.rs"));
}

use craftui::prelude::*;
use craftui_dsl::resolve_source;

pub const COUNTER_TEMPLATE: &str = r#"export component Counter {
    in-out property <int> count: 0;

    Container {
        padding: 24px;
        background: #0f172a;
        border_radius: 8px;

        Column {
            gap: 16px;
            align: "center";

            Text {
                text: "Reactive Counter";
                font_size: 24px;
                color: #f8fafc;
            }

            Text {
                text: "Count: " + count;
                font_size: 36px;
                color: #38bdf8;
            }

            Row {
                gap: 12px;
                Button {
                    text: "- Decrement";
                    background: #334155;
                    on_tap => { count -= 1; }
                }
                Button {
                    text: "+ Increment";
                    background: #0284c7;
                    on_tap => { count += 1; }
                }
            }
        }
    }
}"#;

pub const LOGIN_TEMPLATE: &str = r#"export component LoginForm {
    in-out property <string> email: "";
    in-out property <string> password: "";
    callback on_submit(string, string);

    Container {
        padding: 24px;
        background: #0f172a;
        border_radius: 8px;

        Column {
            gap: 12px;
            width: 320px;
            align: "stretch";

            Text {
                text: "Sign In";
                font_size: 22px;
                color: #ffffff;
            }

            TextInput {
                placeholder: "Email address";
                value <=> email;
            }

            TextInput {
                placeholder: "Password";
                value <=> password;
            }

            Button {
                text: "Continue";
                background: #2563eb;
                on_tap => { on_submit(email, password); }
            }
        }
    }
}"#;

pub const SETTINGS_TEMPLATE: &str = r#"export component SettingsView {
    in-out property <bool> dark_mode: true;
    in-out property <float> volume: 80.0;

    Container {
        padding: 24px;
        background: #0f172a;
        border_radius: 8px;

        Column {
            gap: 16px;
            align: "stretch";

            Text {
                text: "Settings";
                font_size: 22px;
                color: #ffffff;
            }

            Row {
                justify: "space_between";
                align: "center";
                Text { text: "Dark Mode"; color: #cbd5e1; }
                Switch { checked <=> dark_mode; }
            }

            Column {
                gap: 8px;
                Text { text: "Volume: " + volume + "%"; color: #cbd5e1; }
                Slider { value <=> volume; min: 0.0; max: 100.0; }
            }
        }
    }
}"#;

#[derive(Clone)]
pub struct CraftpadController {
    pub selected_template: Signal<String>,
    pub source_code: Signal<String>,
    pub status_text: Signal<String>,
}

impl CraftpadController {
    pub fn new(cx: Scope) -> Self {
        Self {
            selected_template: cx.signal("Counter".to_string()),
            source_code: cx.signal(COUNTER_TEMPLATE.to_string()),
            status_text: cx.signal("Ready — 0 errors".to_string()),
        }
    }

    pub fn select_template(&self, template: &str) {
        self.selected_template.set(template.to_string());
        let code = match template {
            "Login" => LOGIN_TEMPLATE,
            "Settings" => SETTINGS_TEMPLATE,
            _ => COUNTER_TEMPLATE,
        };
        self.source_code.set(code.to_string());
        self.status_text.set(format!("Loaded {template} template"));
    }

    pub fn run_validation(&self) {
        let code = self.source_code.get();
        let resolved = resolve_source(&code, None);
        if resolved.diagnostics.is_empty() {
            self.status_text
                .set("✓ Validated successfully: 0 errors".to_string());
        } else {
            let errors = resolved
                .diagnostics
                .iter()
                .filter(|d| matches!(d.severity, craftui_dsl::Severity::Error))
                .count();
            let warns = resolved.diagnostics.len() - errors;
            self.status_text
                .set(format!("Validation: {errors} error(s), {warns} warning(s)"));
        }
    }
}

pub fn mount_craftpad_app(cx: Scope, tree: &mut WidgetTree) -> (NodeId, CraftpadController) {
    let ctrl = CraftpadController::new(cx);
    let studio = aot_ui::CraftpadStudio::new(cx);

    studio.selected_template.set(ctrl.selected_template.get());
    studio.source_code.set(ctrl.source_code.get());
    studio.status_text.set(ctrl.status_text.get());

    let ctrl_run = ctrl.clone();
    let studio_run = studio.clone();
    studio.set_on_run(move || {
        ctrl_run.run_validation();
        studio_run.status_text.set(ctrl_run.status_text.get());
    });

    let ctrl_fmt = ctrl.clone();
    let studio_fmt = studio.clone();
    studio.set_on_format(move || {
        ctrl_fmt.run_validation();
        studio_fmt.status_text.set(ctrl_fmt.status_text.get());
    });

    let ctrl_tmpl = ctrl.clone();
    let studio_tmpl = studio.clone();
    studio.set_on_select_template(move |tmpl: String| {
        ctrl_tmpl.select_template(&tmpl);
        studio_tmpl.selected_template.set(tmpl);
        studio_tmpl.source_code.set(ctrl_tmpl.source_code.get());
        studio_tmpl.status_text.set(ctrl_tmpl.status_text.get());
    });

    let root = studio.build(cx, tree);
    (root, ctrl)
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() -> std::result::Result<(), JsValue> {
    craftui_platform_web::mount_canvas("craftui-canvas", |cx, tree| mount_craftpad_app(cx, tree).0)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}
