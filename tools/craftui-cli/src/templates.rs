use crate::ProjectTemplate;
use crate::manifest::{self, AppManifest};
use craftui_core::{CraftError, Permission, Result};
use std::path::{Path, PathBuf};

pub fn template_label(template: ProjectTemplate) -> &'static str {
    match template {
        ProjectTemplate::Counter => "counter",
        ProjectTemplate::Dashboard => "dashboard",
        ProjectTemplate::Mobile => "mobile",
        ProjectTemplate::Plugin => "plugin",
    }
}

pub fn sanitize_pkg_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[allow(dead_code)]
pub fn scaffold_project(name: &str, root: &Path) -> Result<()> {
    scaffold_project_with_template(name, root, ProjectTemplate::Counter)
}

pub fn scaffold_project_with_template(
    name: &str,
    root: &Path,
    template: ProjectTemplate,
) -> Result<()> {
    let src_dir = root.join("src");
    let ui_dir = root.join("ui");
    std::fs::create_dir_all(&src_dir)
        .and_then(|_| std::fs::create_dir_all(&ui_dir))
        .map_err(|e| CraftError::Platform(format!("failed creating directories: {e}")))?;

    let pkg_name = sanitize_pkg_name(name);
    let manifest = manifest_for_template(name, template);

    if template == ProjectTemplate::Plugin {
        let android_dir = root.join("android");
        let ios_dir = root.join("ios");
        std::fs::create_dir_all(&android_dir)
            .and_then(|_| std::fs::create_dir_all(&ios_dir))
            .map_err(|e| CraftError::Platform(format!("failed creating plugin dirs: {e}")))?;

        let channel_name = format!("dev.craftui.plugin.{pkg_name}");
        let plugin_cargo = format!(
            r#"[package]
name = "{pkg_name}"
version = "0.1.0"
edition = "2024"
license = "MIT"
description = "CraftUI MethodChannel plugin crate for {name}"

[dependencies]
craftui-core = "0.1.0"
serde_json = "1.0"
"#
        );
        let plugin_lib_rs = format!(
            r#"use craftui_core::{{CraftError, CraftPlugin, MethodCall, PluginRegistry, Result}};
use serde_json::{{Value, json}};

pub const CHANNEL_NAME: &str = "{channel_name}";

/// First/third-party `MethodChannel` plugin implementation for `{pkg_name}`.
pub struct Plugin;

impl CraftPlugin for Plugin {{
    fn name(&self) -> &str {{
        CHANNEL_NAME
    }}

    fn handle(&self, call: &MethodCall) -> Result<Value> {{
        match call.method.as_str() {{
            "ping" => Ok(json!({{ "pong": true, "channel": CHANNEL_NAME }})),
            other => Err(CraftError::Platform(format!(
                "unknown method `{{other}}` on channel `{{CHANNEL_NAME}}`"
            ))),
        }}
    }}
}}

pub fn register(registry: &PluginRegistry) {{
    registry.register(Plugin);
}}

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn ping_roundtrip_works() {{
        let reg = PluginRegistry::new();
        register(&reg);
        let out = reg.invoke(CHANNEL_NAME, "ping", json!({{}})).unwrap();
        assert_eq!(out["pong"], true);
    }}
}}
"#
        );
        let kotlin_pkg: String = pkg_name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        let plugin_kotlin = format!(
            r#"package dev.craftui.plugins.{kotlin_pkg}

import org.json.JSONObject

object PluginHandler {{
    const val CHANNEL = "{channel_name}"

    fun handle(method: String, argsJson: String): JSONObject {{
        return when (method) {{
            "ping" -> JSONObject().put("pong", true).put("channel", CHANNEL)
            else -> JSONObject().put("error", "unhandled method: $method")
        }}
    }}
}}
"#
        );
        let plugin_swift = format!(
            r#"import Foundation

enum PluginHandler {{
    static let channel = "{channel_name}"

    static func handle(method: String, args: [String: Any]) -> [String: Any] {{
        switch method {{
        case "ping":
            return ["pong": true, "channel": channel]
        default:
            return ["error": "unhandled method: \(method)"]
        }}
    }}
}}
"#
        );
        let demo_cui = "export component PluginPreview {\n    Column {\n        padding: 16;\n        Text { text: \"CraftUI Plugin Package\"; }\n    }\n}\n";
        std::fs::write(root.join("Cargo.toml"), plugin_cargo)
            .and_then(|_| std::fs::write(root.join("craftui.toml"), manifest.to_toml()))
            .and_then(|_| std::fs::write(src_dir.join("lib.rs"), plugin_lib_rs))
            .and_then(|_| std::fs::write(ui_dir.join("main.cui"), demo_cui))
            .and_then(|_| std::fs::write(android_dir.join("PluginHandler.kt"), plugin_kotlin))
            .and_then(|_| std::fs::write(ios_dir.join("PluginHandler.swift"), plugin_swift))
            .map_err(|e| CraftError::Platform(format!("failed writing plugin template: {e}")))?;
        return Ok(());
    }

    let cargo_toml = format!(
        r#"[package]
name = "{pkg_name}"
version = "0.1.0"
edition = "2024"

[dependencies]
craftui = "0.1.0"

[build-dependencies]
craftui-dsl = "0.1.0"
"#
    );

    let build_rs = r#"fn main() {
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=craftui.toml");
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    craftui_dsl::compile_cui_dir("ui", &out_dir).expect("failed compiling .cui templates");
}
"#;

    let (main_cui, main_rs) = match template {
        ProjectTemplate::Counter => (counter_template_cui(), counter_template_rs()),
        ProjectTemplate::Dashboard => (dashboard_template_cui(), dashboard_template_rs()),
        ProjectTemplate::Mobile => (mobile_template_cui(), mobile_template_rs()),
        ProjectTemplate::Plugin => unreachable!("handled cleanly by early return above"),
    };

    std::fs::write(root.join("Cargo.toml"), cargo_toml)
        .and_then(|_| std::fs::write(root.join("build.rs"), build_rs))
        .and_then(|_| std::fs::write(root.join("craftui.toml"), manifest.to_toml()))
        .and_then(|_| std::fs::write(ui_dir.join("main.cui"), main_cui))
        .and_then(|_| std::fs::write(src_dir.join("main.rs"), main_rs))
        .map_err(|e| CraftError::Platform(format!("failed writing template files: {e}")))?;
    Ok(())
}

pub fn manifest_for_template(name: &str, template: ProjectTemplate) -> AppManifest {
    let app_id = AppManifest::sanitize_id(name);
    match template {
        ProjectTemplate::Counter => AppManifest::with_app(name, app_id),
        ProjectTemplate::Dashboard => {
            let mut m = AppManifest::with_app(name, app_id);
            for (perm, reason) in [
                (
                    Permission::Notifications,
                    "Deliver operational alert notifications for live dashboard thresholds.",
                ),
                (
                    Permission::ClipboardRead,
                    "Allow one-click paste for filtering exported table queries.",
                ),
            ] {
                let key = perm.key().into_owned();
                m.permissions.insert(
                    key.clone(),
                    manifest::PermissionDeclaration {
                        key,
                        permission: perm,
                        enabled: true,
                        reason: Some(reason.to_string()),
                    },
                );
            }
            m
        }
        ProjectTemplate::Mobile => {
            let mut m = AppManifest::with_app(name, app_id);
            for (perm, reason) in [
                (
                    Permission::Camera,
                    "Scan QR codes and capture attachments inside mobile workflows.",
                ),
                (
                    Permission::LocationWhenInUse,
                    "Show nearby field locations while the application is active.",
                ),
                (
                    Permission::Notifications,
                    "Deliver instant status reminders and background task updates.",
                ),
                (
                    Permission::Photos,
                    "Pick saved images from the photo library for uploads.",
                ),
                (
                    Permission::Biometrics,
                    "Protect sensitive credentials using Face ID, Touch ID, or device biometrics.",
                ),
                (
                    Permission::Vibrate,
                    "Provide subtle haptic confirmation when toggling permissions or submitting actions.",
                ),
            ] {
                let key = perm.key().into_owned();
                m.permissions.insert(
                    key.clone(),
                    manifest::PermissionDeclaration {
                        key,
                        permission: perm,
                        enabled: true,
                        reason: Some(reason.to_string()),
                    },
                );
            }
            m
        }
        ProjectTemplate::Plugin => {
            let mut m = AppManifest::with_app(name, app_id);
            m.plugins.insert(
                sanitize_pkg_name(name),
                format!("dev.craftui.plugin.{}", sanitize_pkg_name(name)),
            );
            m
        }
    }
}

fn counter_template_cui() -> &'static str {
    r#"export component MainView {
    in-out property <int> count: 0;

    SafeArea {
        Column {
            width: 100%;
            height: 100%;
            background: Theme.background;
            padding: 24dp;
            gap: 16dp;

            Row {
                align: Center;
                gap: 10dp;
                Text {
                    text: "CraftUI App";
                    font_size: 24;
                    color: Theme.text_primary;
                }
                Badge {
                    label: "Ready";
                    icon: "check";
                    background: Theme.primary;
                    color: Theme.on_primary;
                }
            }
            Text {
                text: count;
                font_size: 20;
                color: Theme.primary;
            }
            Row {
                gap: 12dp;
                Button {
                    text: "Increment";
                    background: Theme.primary;
                    cursor: Pointer;
                    on_tap => { count = count + 1; }
                }
                Button {
                    text: "Reset";
                    cursor: Pointer;
                    on_tap => { count = 0; }
                }
            }
        }
    }
}
"#
}

fn counter_template_rs() -> &'static str {
    r#"use craftui::prelude::*;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/main.rs"));
}

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Starter".to_string(),
            ..Default::default()
        },
        |cx, tree, _ctx| {
            let ui = generated::MainView::new(cx);
            ui.build(cx, tree)
        },
    )
}
"#
}

fn dashboard_template_cui() -> &'static str {
    r#"export component MainView {
    in-out property <int> section: 0;
    in-out property <int> selected_row: 0;
    in-out property <int> sort_col: 0;
    in-out property <bool> show_details: true;
    in property <[[string]]> rows: [
        ["API Gateway", "Healthy", "12 ms"],
        ["WASM Worker", "Healthy", "18 ms"],
        ["Mobile Push", "Degraded", "49 ms"]
    ];

    Column {
        width: 100%;
        height: 100%;
        background: Theme.background;
        padding: 20dp;
        gap: 14dp;

        Row {
            align: Center;
            gap: 12dp;
            Text {
                text: "Operations Console";
                font_size: 22;
                color: Theme.text_primary;
            }
            Badge {
                label: "Live";
                background: Theme.primary;
                color: Theme.on_primary;
            }
        }

        SegmentedControl {
            items: ["Overview", "Services", "Telemetry"];
            selected <=> section;
        }

        SplitPane {
            ratio: 0.65;
            Column {
                gap: 10dp;
                DataTable {
                    columns: ["Service", "Status", "Latency"];
                    rows: rows;
                    selected <=> selected_row;
                    striped: true;
                    row_height: 36;
                    on_sort => { sort_col = column; }
                }
            }
            Column {
                padding: 12dp;
                gap: 10dp;
                Accordion {
                    title: "Inspector Details";
                    expanded <=> show_details;
                    Text {
                        text: "Select a service row to inspect live telemetry.";
                        font_size: 13;
                        color: Theme.text_secondary;
                    }
                    ProgressIndicator {
                        progress: 0.82;
                    }
                }
            }
        }
    }
}
"#
}

fn dashboard_template_rs() -> &'static str {
    r#"use craftui::prelude::*;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/main.rs"));
}

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Dashboard".to_string(),
            ..Default::default()
        },
        |cx, tree, _ctx| {
            let ui = generated::MainView::new(cx);
            ui.build(cx, tree)
        },
    )
}
"#
}

fn mobile_template_cui() -> &'static str {
    r#"export component MainView {
    in-out property <int> active_tab: 0;
    in-out property <bool> rationale_open: true;
    in-out property <int> selected_perm: 0;
    in-out property <string> status_banner: "Tap Request to query native OS capabilities";
    in property <[[string]]> perm_rows: [
        ["camera", "Camera", "NotDetermined"],
        ["location_when_in_use", "Location", "NotDetermined"],
        ["notifications", "Notifications", "NotDetermined"],
        ["biometrics", "Biometrics", "NotDetermined"]
    ];
    callback request_selected();
    callback open_settings();

    SafeArea {
        Column {
            width: 100%;
            height: 100%;
            background: Theme.background;
            padding: 16dp;
            gap: 12dp;

            Row {
                align: Center;
                gap: 8dp;
                Text {
                    text: "Mobile Studio";
                    font_size: 22;
                    color: Theme.text_primary;
                }
                Badge {
                    label: "SafeArea";
                    background: Theme.primary;
                    color: Theme.on_primary;
                }
            }

            SegmentedControl {
                items: ["Permissions", "Device", "Storage"];
                selected <=> active_tab;
            }

            Text {
                text: status_banner;
                font_size: 13;
                color: Theme.text_secondary;
            }

            Accordion {
                title: "Why we ask for access";
                expanded <=> rationale_open;
                Text {
                    text: "Declared usage strings in craftui.toml automatically populate AndroidManifest.xml and iOS Info.plist.";
                    font_size: 12;
                    color: Theme.text_secondary;
                }
            }

            DataTable {
                columns: ["Key", "Capability", "Status"];
                rows: perm_rows;
                selected <=> selected_perm;
                striped: true;
                row_height: 40;
            }

            Row {
                gap: 10dp;
                Button {
                    text: "Request Selected";
                    background: Theme.primary;
                    cursor: Pointer;
                    on_tap => { request_selected(); }
                }
                Button {
                    text: "Open App Settings";
                    cursor: Pointer;
                    on_tap => { open_settings(); }
                }
            }
        }
    }
}
"#
}

fn mobile_template_rs() -> &'static str {
    r#"use craftui::prelude::*;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/main.rs"));
}

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Mobile Studio".to_string(),
            ..Default::default()
        },
        |cx, tree, ctx| {
            let ui = generated::MainView::new(cx);
            let services = ctx.services.clone();
            let selected_perm = ui.selected_perm;
            let status_banner = ui.status_banner;
            let perm_rows = ui.perm_rows;

            let keys = [
                Permission::Camera,
                Permission::LocationWhenInUse,
                Permission::Notifications,
                Permission::Biometrics,
            ];

            let refresh_rows = {
                let services = services.clone();
                move || {
                    perm_rows.set(vec![
                        vec!["camera".into(), "Camera".into(), format!("{:?}", services.check_permission(&Permission::Camera))],
                        vec!["location_when_in_use".into(), "Location".into(), format!("{:?}", services.check_permission(&Permission::LocationWhenInUse))],
                        vec!["notifications".into(), "Notifications".into(), format!("{:?}", services.check_permission(&Permission::Notifications))],
                        vec!["biometrics".into(), "Biometrics".into(), format!("{:?}", services.check_permission(&Permission::Biometrics))],
                    ]);
                }
            };

            let svc_req = services.clone();
            let refresh_clone = refresh_rows.clone();
            ui.set_request_selected(move || {
                let idx = (selected_perm.get().max(0) as usize).min(keys.len() - 1);
                let perm = keys[idx].clone();
                let st = svc_req.request_permission(&perm);
                let _ = svc_req.trigger_haptic(HapticStyle::Selection);
                status_banner.set(format!("{} -> {:?}", perm.key(), st));
                refresh_clone();
            });

            let svc_set = services;
            ui.set_open_settings(move || {
                let opened = svc_set.open_app_settings();
                status_banner.set(if opened {
                    "Opened OS application settings".into()
                } else {
                    "App settings unavailable on this host".into()
                });
            });

            refresh_rows();
            ui.build(cx, tree)
        },
    )
}
"#
}

pub fn init_manifest_in_dir(
    dir: &Path,
    name_override: Option<&str>,
    id_override: Option<&str>,
    force: bool,
) -> Result<PathBuf> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)
            .map_err(|e| CraftError::Platform(format!("failed creating {}: {e}", dir.display())))?;
    }
    let out_path = dir.join("craftui.toml");
    if out_path.exists() && !force {
        return Err(CraftError::Platform(format!(
            "{} already exists (pass --force to overwrite)",
            out_path.display()
        )));
    }

    let detected_name = name_override
        .map(str::to_string)
        .or_else(|| detect_cargo_package_name(&dir.join("Cargo.toml")))
        .or_else(|| {
            dir.canonicalize()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        })
        .unwrap_or_else(|| "CraftUI App".to_string());

    let app_id = id_override
        .map(str::to_string)
        .unwrap_or_else(|| AppManifest::sanitize_id(&detected_name));
    let manifest = AppManifest::with_app(detected_name, app_id);
    std::fs::write(&out_path, manifest.to_toml())
        .map_err(|e| CraftError::Platform(format!("failed writing {}: {e}", out_path.display())))?;
    Ok(out_path)
}

pub fn detect_cargo_package_name(cargo_toml: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(cargo_toml).ok()?;
    let mut in_pkg = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_pkg = trimmed == "[package]";
            continue;
        }
        if in_pkg
            && let Some((k, v)) = trimmed.split_once('=')
            && k.trim() == "name"
        {
            let val = v.trim();
            let parsed = if let Some(stripped) = val.strip_prefix('"') {
                stripped.split('"').next()
            } else if let Some(stripped) = val.strip_prefix('\'') {
                stripped.split('\'').next()
            } else {
                val.split('#').next().map(str::trim)
            };
            if let Some(cleaned) = parsed
                && !cleaned.is_empty()
            {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}
