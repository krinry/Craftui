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
            align: Center;

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
            align: Stretch;

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
            align: Stretch;

            Text {
                text: "Settings";
                font_size: 22px;
                color: #ffffff;
            }

            Row {
                justify: SpaceBetween;
                align: Center;
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

pub const SLINT_TEMPLATE: &str = r#"export component Demo {
    Container {
        width: 320px;
        background: #18181b;
        border_radius: 6px;
        padding: 24px;

        Column {
            gap: 16px;
            align: Center;

            Text {
                text: "Hello World!";
                font_size: 24px;
                color: #ffffff;
            }

            Container {
                background: #ffffff;
                border_radius: 30px;
                padding: 12px;
                Text {
                    text: "⚡ Slint";
                    color: #000000;
                    font_weight: 700.0;
                }
            }

            Button {
                text: "OK!";
                border_radius: 4px;
                background: #a855f7;
                foreground: #ffffff;
                padding: 8px;
            }
        }
    }
}"#;

#[derive(Clone)]
pub struct CraftpadController {
    pub selected_template: Signal<String>,
    pub source_code: Signal<String>,
    pub status_text: Signal<String>,
    pub demo_count: Signal<i32>,
}

impl CraftpadController {
    pub fn new(cx: Scope) -> Self {
        Self {
            selected_template: cx.signal("Flutter Counter".to_string()),
            source_code: cx.signal(COUNTER_TEMPLATE.to_string()),
            status_text: cx.signal("Ready — not yet validated".to_string()),
            demo_count: cx.signal(0i32),
        }
    }

    pub fn select_template(&self, template: &str) {
        let (label, code) = match template.to_ascii_lowercase().as_str() {
            "slint" => ("Slint Dialog", SLINT_TEMPLATE),
            "settings" => ("Settings Screen", SETTINGS_TEMPLATE),
            "login" => ("Login Form", LOGIN_TEMPLATE),
            _ => ("Flutter Counter", COUNTER_TEMPLATE),
        };
        self.selected_template.set(label.to_string());
        self.source_code.set(code.to_string());
        self.status_text.set(format!("✓ Loaded {label} template"));
    }

    pub fn run_validation(&self) {
        let code = self.source_code.get();
        let resolved = resolve_source(&code, None);
        if resolved.diagnostics.is_empty() {
            self.status_text.set(
                "✓ Validation succeeded: 0 problems detected (preview is a mockup)".to_string(),
            );
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
    studio.demo_count.set(ctrl.demo_count.get());

    let ctrl_run = ctrl.clone();
    let studio_run = studio.clone();
    studio.set_on_run(move || {
        ctrl_run.source_code.set(studio_run.source_code.get());
        ctrl_run.run_validation();
        studio_run.status_text.set(ctrl_run.status_text.get());
    });

    let ctrl_reload = ctrl.clone();
    let studio_reload = studio.clone();
    studio.set_on_reload(move || {
        ctrl_reload.source_code.set(studio_reload.source_code.get());
        ctrl_reload.run_validation();
        studio_reload.status_text.set(ctrl_reload.status_text.get());
    });

    let ctrl_tmpl = ctrl.clone();
    let studio_tmpl = studio.clone();
    studio.set_on_select_template(move |tmpl: String| {
        ctrl_tmpl.select_template(&tmpl);
        studio_tmpl
            .selected_template
            .set(ctrl_tmpl.selected_template.get());
        studio_tmpl.source_code.set(ctrl_tmpl.source_code.get());
        studio_tmpl.status_text.set(ctrl_tmpl.status_text.get());
    });

    let root = studio.build(cx, tree);
    (root, ctrl)
}

#[cfg(target_arch = "wasm32")]
mod web_preview {
    use std::cell::RefCell;
    use std::rc::Rc;

    use craftui::prelude::Scope;
    use craftui_platform_web::{WebBridgeConfig, WebRunnerHandle, mount_web_app};
    use craftui_runtime::HotReloadSession;
    use wasm_bindgen::prelude::*;

    struct Preview {
        handle: WebRunnerHandle,
        session: HotReloadSession,
    }

    thread_local! {
        static PREVIEW: RefCell<Option<Preview>> = const { RefCell::new(None) };
    }

    /// Mount a canvas host once. Source is subsequently applied with `preview_source`.
    #[wasm_bindgen]
    pub fn init_preview() -> Result<(), JsValue> {
        if PREVIEW.with(|preview| preview.borrow().is_some()) {
            return Ok(());
        }
        let session = Rc::new(RefCell::new(None));
        let session_out = session.clone();
        let config = WebBridgeConfig {
            sync_url_hash: false,
            force_2d_fallback: true,
            ..Default::default()
        };
        let handle = match mount_web_app(config, move |cx, tree, _, _| {
            *session_out.borrow_mut() = Some(HotReloadSession::new(cx));
            tree.column(cx)
        }) {
            Ok(handle) => handle,
            Err(error) => {
                if let Some(session) = session.borrow_mut().take() {
                    session.state_scope.dispose();
                }
                return Err(error);
            }
        };
        let session = session
            .borrow_mut()
            .take()
            .ok_or_else(|| JsValue::from_str("Preview session was not created"))?;
        PREVIEW.with(|preview| *preview.borrow_mut() = Some(Preview { handle, session }));
        Ok(())
    }

    /// Rebuild the live UI while keeping component property signals on ordinary reloads.
    #[wasm_bindgen]
    pub fn preview_source(source: &str, reset_state: bool) -> Result<(), JsValue> {
        PREVIEW.with(|preview| {
            let mut preview = preview.borrow_mut();
            let Preview { handle, session } = preview
                .as_mut()
                .ok_or_else(|| JsValue::from_str("Preview is not initialized"))?;
            if reset_state {
                let mut next = HotReloadSession::new(Scope::root());
                let result = handle.with_tree(|tree| next.reload(source, None, tree));
                match result {
                    Ok(_) => {
                        let old = std::mem::replace(session, next);
                        old.state_scope.dispose();
                        Ok(())
                    }
                    Err(error) => {
                        next.state_scope.dispose();
                        Err(JsValue::from_str(&error.to_string()))
                    }
                }
            } else {
                handle
                    .with_tree(|tree| session.reload(source, None, tree))
                    .map(|_| ())
                    .map_err(|error| JsValue::from_str(&error.to_string()))
            }
        })
    }
}
