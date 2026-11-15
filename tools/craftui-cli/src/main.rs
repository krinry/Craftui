mod manifest;
mod remote_server;

use clap::{Parser, Subcommand, ValueEnum};
use craftui_core::{CraftError, Permission, Result, Size, WindowMetrics};
use craftui_dsl::{Severity, generate_rust, resolve_file};
use craftui_reactive::Scope;
use craftui_render::{
    BackendKind, PaintScene, TextureRegistry, render_scene_to_rgba, write_rgba_png,
};
use craftui_runtime::HotReloadSession;
use craftui_widgets::WidgetTree;
use manifest::{
    AppManifest, ManifestDiagnostic, default_usage_description, find_project_manifest,
    web_permission_name, xml_escape,
};
use notify::{Event, RecursiveMode, Watcher};
use std::io::{Read, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, mpsc};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "craftui",
    version,
    about = "CraftUI CLI — scaffolding, dev hot-reload, .cui validation, permission manifests, and cross-platform builds"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum PlatformTarget {
    Desktop,
    Web,
    Android,
    Ios,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum RunPlatform {
    Desktop,
    Web,
    Android,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum ProjectTemplate {
    Counter,
    Dashboard,
    Mobile,
}

#[derive(Subcommand, Debug)]
enum PermissionCommands {
    /// List all 35 built-in cross-platform permission keys and native Android/iOS/Web mappings
    List {
        #[arg(long)]
        json: bool,
    },
    /// Validate a craftui.toml manifest and audit Android/iOS/Web permission readiness
    Check {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scaffold a new CraftUI project with build.rs, craftui.toml, and a reactive template
    New {
        name: String,
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ProjectTemplate::Counter)]
        template: ProjectTemplate,
    },
    /// Initialize craftui.toml in an existing project without overwriting existing source files
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Install/verify Rust cross-compilation targets (wasm32, android, macOS ios) and inspect SDK tools
    Setup {
        #[arg(long)]
        check_only: bool,
        #[arg(long)]
        skip_ios: bool,
    },
    /// Build and run a CraftUI application directly on desktop, web (local server), or android
    Run {
        #[arg(long, value_enum, default_value_t = RunPlatform::Desktop)]
        platform: RunPlatform,
        #[arg(long)]
        release: bool,
        #[arg(short, long)]
        package: Option<String>,
        #[arg(long, default_value_t = 8080)]
        port: u16,
        #[arg(long)]
        skip_cargo: bool,
    },
    /// Watch .cui files and print fast sub-100ms hot-reload validation feedback
    Dev {
        #[arg(long, default_value = "ui/main.cui")]
        entry: PathBuf,
        #[arg(long, default_value = "ui")]
        dir: PathBuf,
    },
    /// Open an interactive live preview canvas for a .cui file with telemetry HUD, device frame, and inspector
    Preview {
        file: PathBuf,
        #[arg(long)]
        component: Option<String>,
        #[arg(long)]
        cpu: bool,
        #[arg(long, help = "Render inside mobile device frame (Android 412x915)")]
        device: bool,
        #[arg(long, help = "Enable element inspection overlay")]
        inspector: bool,
        #[arg(
            long,
            help = "Broadcast live preview to connected Android devices over TCP/ADB"
        )]
        remote: bool,
        #[arg(
            long,
            default_value_t = 9876,
            help = "TCP port for remote live preview server (default: 9876)"
        )]
        remote_port: u16,
        #[arg(
            long,
            help = "Run remote live preview server headlessly without opening desktop window"
        )]
        no_desktop: bool,
    },
    /// Parse, validate, and optionally emit AOT Rust code for .cui files
    Check {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        emit_rust: bool,
    },
    /// Headlessly render a .cui file to an RGBA PNG snapshot via vello_cpu
    Render {
        file: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        #[arg(long)]
        component: Option<String>,
        #[arg(long, default_value_t = 800)]
        width: u32,
        #[arg(long, default_value_t = 600)]
        height: u32,
    },
    /// Inspect cross-platform toolchain readiness (Rust targets, NDK, wasm-bindgen, Xcode, .cui, craftui.toml)
    Doctor {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect all mobile/desktop/web permission keys or audit a project's craftui.toml manifest
    Permissions {
        #[command(subcommand)]
        command: PermissionCommands,
    },
    /// Build & package a CraftUI application for desktop, web, android, or ios
    Build {
        #[arg(long, value_enum, default_value_t = PlatformTarget::Desktop)]
        platform: PlatformTarget,
        #[arg(long)]
        release: bool,
        #[arg(short, long)]
        package: Option<String>,
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Generate platform packaging templates/bundles without invoking cargo cross-compilation
        #[arg(long)]
        skip_cargo: bool,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::New {
            name,
            path,
            template,
        } => {
            let target_dir = path.unwrap_or_else(|| PathBuf::from(&name));
            scaffold_project_with_template(&name, &target_dir, template)?;
            println!(
                "Created CraftUI ({}) project '{}' at {}",
                template_label(template),
                name,
                target_dir.display()
            );
            println!(
                "Next steps:\n  cd {}\n  craftui permissions check craftui.toml\n  craftui check ui/main.cui\n  craftui run --platform desktop",
                target_dir.display()
            );
            Ok(())
        }
        Commands::Init {
            path,
            name,
            id,
            force,
        } => {
            let out_path = init_manifest_in_dir(&path, name.as_deref(), id.as_deref(), force)?;
            println!("Initialized CraftUI manifest at {}", out_path.display());
            Ok(())
        }
        Commands::Setup {
            check_only,
            skip_ios,
        } => run_setup(check_only, skip_ios),
        Commands::Run {
            platform,
            release,
            package,
            port,
            skip_cargo,
        } => run_app_target(platform, release, package.as_deref(), port, skip_cargo),
        Commands::Dev { entry, dir } => run_dev_watcher(&entry, &dir),
        Commands::Preview {
            file,
            component,
            cpu,
            device,
            inspector,
            remote,
            remote_port,
            no_desktop,
        } => {
            let _remote_srv = if remote {
                let srv = Arc::new(remote_server::RemotePreviewServer::start(remote_port));
                srv.broadcast_file(&file, component.as_deref());

                let watch_target = file
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));

                let s_clone = srv.clone();
                let f_clone = file.clone();
                let c_clone = component.clone();

                std::thread::Builder::new()
                    .name("craftui-remote-watcher".into())
                    .spawn(move || {
                        let (tx, rx) = mpsc::channel();
                        if let Ok(mut watcher) = notify::recommended_watcher(tx) {
                            let _ = watcher.watch(&watch_target, RecursiveMode::Recursive);
                            for event in rx.into_iter().flatten() {
                                let touches = event
                                    .paths
                                    .iter()
                                    .any(|p| p.extension().and_then(|e| e.to_str()) == Some("cui"));
                                if touches {
                                    s_clone.broadcast_file(&f_clone, c_clone.as_deref());
                                }
                            }
                        }
                    })
                    .ok();

                Some(srv)
            } else {
                None
            };

            if no_desktop {
                if !remote {
                    return Err(CraftError::Platform(
                        "--no-desktop flag requires --remote to be passed".into(),
                    ));
                }
                println!("[craftui-remote] Running headlessly. Press Ctrl+C to stop.");
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            } else {
                craftui_platform_desktop::run_live_preview_cui(
                    craftui_platform_desktop::WindowConfig {
                        title: format!("CraftUI Live Preview — {}", file.display()),
                        force_cpu: cpu,
                        width: if device { 460.0 } else { 800.0 },
                        height: if device { 960.0 } else { 600.0 },
                        ..Default::default()
                    },
                    file,
                    component,
                    device,
                    inspector,
                )
            }
        }
        Commands::Check { path, emit_rust } => {
            let stats = check_cui_paths(&path, emit_rust)?;
            println!(
                "Checked {} .cui file(s): {} warning(s), {} error(s)",
                stats.files, stats.warnings, stats.errors
            );
            if stats.errors > 0 {
                return Err(CraftError::Dsl(format!(
                    "{} .cui validation error(s) found",
                    stats.errors
                )));
            }
            Ok(())
        }
        Commands::Render {
            file,
            out,
            component,
            width,
            height,
        } => {
            render_cui_file_to_png(&file, component.as_deref(), width, height, &out)?;
            println!(
                "Rendered {}x{} snapshot to {}",
                width,
                height,
                out.display()
            );
            Ok(())
        }
        Commands::Doctor { path, json } => run_doctor(&path, json),
        Commands::Permissions { command } => match command {
            PermissionCommands::List { json } => run_permissions_list(json),
            PermissionCommands::Check { path, json } => run_permissions_check(&path, json),
        },
        Commands::Build {
            platform,
            release,
            package,
            out,
            skip_cargo,
        } => run_build(
            platform,
            release,
            package.as_deref(),
            out.as_deref(),
            skip_cargo,
        ),
    }
}

fn template_label(template: ProjectTemplate) -> &'static str {
    match template {
        ProjectTemplate::Counter => "counter",
        ProjectTemplate::Dashboard => "dashboard",
        ProjectTemplate::Mobile => "mobile",
    }
}

fn sanitize_pkg_name(name: &str) -> String {
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
    };

    std::fs::write(root.join("Cargo.toml"), cargo_toml)
        .and_then(|_| std::fs::write(root.join("build.rs"), build_rs))
        .and_then(|_| std::fs::write(root.join("craftui.toml"), manifest.to_toml()))
        .and_then(|_| std::fs::write(ui_dir.join("main.cui"), main_cui))
        .and_then(|_| std::fs::write(src_dir.join("main.rs"), main_rs))
        .map_err(|e| CraftError::Platform(format!("failed writing template files: {e}")))?;
    Ok(())
}

fn manifest_for_template(name: &str, template: ProjectTemplate) -> AppManifest {
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

fn detect_cargo_package_name(cargo_toml: &Path) -> Option<String> {
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
            let cleaned = v.trim().trim_matches('"').trim_matches('\'');
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

fn run_setup(check_only: bool, skip_ios: bool) -> Result<()> {
    let report = collect_doctor_report(Path::new("."))?;
    println!("CraftUI Toolchain Setup");
    println!("-----------------------");
    let mut needed: Vec<&str> = Vec::new();
    if !report.wasm_target {
        needed.push("wasm32-unknown-unknown");
    }
    if !report.android_target {
        needed.push("aarch64-linux-android");
    }
    if cfg!(target_os = "macos") && !skip_ios && !report.ios_target {
        needed.push("aarch64-apple-ios");
    }

    if needed.is_empty() {
        println!("[ok] Required Rust compilation targets are already installed.");
    } else if check_only {
        println!(
            "[info] Missing Rust targets (--check-only): {}",
            needed.join(", ")
        );
        println!(
            "       Run `craftui setup` or `rustup target add {}`",
            needed.join(" ")
        );
    } else if find_in_path("rustup").is_some() {
        println!("Installing missing Rust targets: {}", needed.join(", "));
        let mut cmd = Command::new("rustup");
        cmd.arg("target").arg("add");
        for target in &needed {
            cmd.arg(target);
        }
        let status = cmd
            .status()
            .map_err(|e| CraftError::Platform(format!("failed running rustup: {e}")))?;
        if !status.success() {
            return Err(CraftError::Platform(format!(
                "rustup target add exited with {status}"
            )));
        }
        println!("[ok] Installed Rust targets successfully.");
    } else {
        println!("[warn] `rustup` was not found on PATH; install rustup from https://rustup.rs/");
    }

    println!();
    println!("Native SDK Hints:");
    println!(
        "  Web:     {} (`cargo install wasm-bindgen-cli` for JS glue)",
        if report.wasm_bindgen {
            "wasm-bindgen detected"
        } else {
            "wasm-bindgen not installed"
        }
    );
    println!(
        "  Android: {} (set ANDROID_NDK_HOME or install via Android Studio SDK Manager)",
        report.android_ndk.as_ref().map_or_else(
            || "NDK not detected".to_string(),
            |p| p.display().to_string()
        )
    );
    println!(
        "  ADB:     {}",
        report
            .adb
            .as_ref()
            .map_or_else(|| "not found".to_string(), |p| p.display().to_string())
    );
    if cfg!(target_os = "macos") {
        println!(
            "  iOS:     xcodebuild = {}",
            if report.xcodebuild {
                "ready"
            } else {
                "missing (install Xcode Command Line Tools)"
            }
        );
    } else {
        println!(
            "  iOS:     macOS + Xcode required for final .ipa linking (`--platform ios --skip-cargo` emits Swift/Metal template anywhere)"
        );
    }
    Ok(())
}

fn run_app_target(
    platform: RunPlatform,
    release: bool,
    package: Option<&str>,
    port: u16,
    skip_cargo: bool,
) -> Result<()> {
    match platform {
        RunPlatform::Desktop => {
            let mut cmd = Command::new("cargo");
            cmd.arg("run");
            if let Some(pkg) = package {
                cmd.args(["-p", pkg]);
            }
            if release {
                cmd.arg("--release");
            }
            let status = cmd
                .status()
                .map_err(|e| CraftError::Platform(format!("failed spawning cargo run: {e}")))?;
            if !status.success() {
                return Err(CraftError::Platform(format!(
                    "cargo run exited with status {status}"
                )));
            }
            Ok(())
        }
        RunPlatform::Web => {
            let out_dir = PathBuf::from("dist/web");
            run_build(
                PlatformTarget::Web,
                release,
                package,
                Some(&out_dir),
                skip_cargo,
            )?;
            serve_web_directory(&out_dir, port)
        }
        RunPlatform::Android => {
            let out_dir = PathBuf::from("dist/android");
            run_build(
                PlatformTarget::Android,
                release,
                package,
                Some(&out_dir),
                skip_cargo,
            )?;
            let abs_out_dir = std::fs::canonicalize(&out_dir).unwrap_or_else(|_| out_dir.clone());
            let gradlew = abs_out_dir.join(if cfg!(windows) {
                "gradlew.bat"
            } else {
                "gradlew"
            });
            let gradle_bin = if gradlew.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = std::fs::metadata(&gradlew) {
                        let mut perms = meta.permissions();
                        perms.set_mode(0o755);
                        let _ = std::fs::set_permissions(&gradlew, perms);
                    }
                }
                Some(gradlew)
            } else if let Some(sys_gradle) = find_in_path("gradle") {
                // Generate gradle wrapper if not present
                let _ = Command::new(&sys_gradle)
                    .arg("wrapper")
                    .current_dir(&abs_out_dir)
                    .status();
                if gradlew.is_file() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(meta) = std::fs::metadata(&gradlew) {
                            let mut perms = meta.permissions();
                            perms.set_mode(0o755);
                            let _ = std::fs::set_permissions(&gradlew, perms);
                        }
                    }
                    Some(gradlew)
                } else {
                    Some(sys_gradle)
                }
            } else {
                None
            };

            if let Some(gbin) = gradle_bin {
                let task = if release {
                    "assembleRelease"
                } else {
                    "assembleDebug"
                };
                println!("Running gradle {task} in {}...", out_dir.display());
                let status = Command::new(&gbin)
                    .arg(task)
                    .current_dir(&out_dir)
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning gradle: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "gradle {task} exited with {status}"
                    )));
                }

                // Locate generated APK
                let default_apk = out_dir.join(if release {
                    "app/build/outputs/apk/release/app-release-unsigned.apk"
                } else {
                    "app/build/outputs/apk/debug/app-debug.apk"
                });
                let apk = if default_apk.is_file() {
                    Some(default_apk)
                } else {
                    let apk_search_dir = out_dir.join("app/build/outputs/apk");
                    let mut found: Option<PathBuf> = None;
                    if let Ok(entries) = std::fs::read_dir(&apk_search_dir) {
                        for sub in entries.flatten() {
                            if sub.path().is_dir()
                                && let Ok(files) = std::fs::read_dir(sub.path())
                            {
                                for f in files.flatten() {
                                    if f.path().extension().and_then(|e| e.to_str()) == Some("apk")
                                    {
                                        found = Some(f.path());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    found
                };

                if let Some(apk_path) = apk {
                    println!("Built Android APK at: {}", apk_path.display());
                    if let Some(adb) = discover_adb() {
                        let has_devices = Command::new(&adb)
                            .arg("devices")
                            .output()
                            .ok()
                            .map(|out| {
                                let stdout = String::from_utf8_lossy(&out.stdout);
                                stdout.lines().skip(1).any(|l| l.contains("\tdevice"))
                            })
                            .unwrap_or(false);

                        if has_devices {
                            println!("Installing APK on device via {}...", adb.display());
                            let install = Command::new(&adb)
                                .args(["install", "-r"])
                                .arg(&apk_path)
                                .status()
                                .map_err(|e| {
                                    CraftError::Platform(format!("adb install failed: {e}"))
                                })?;
                            if !install.success() {
                                return Err(CraftError::Platform(format!(
                                    "adb install exited with {install}"
                                )));
                            }
                            let app_id =
                                std::fs::read_to_string(out_dir.join("app/build.gradle.kts"))
                                    .ok()
                                    .and_then(|content| {
                                        content.lines().find_map(|line| {
                                            let trimmed = line.trim();
                                            if trimmed.starts_with("applicationId =") {
                                                trimmed.split('"').nth(1).map(str::to_string)
                                            } else {
                                                None
                                            }
                                        })
                                    })
                                    .unwrap_or_else(|| {
                                        load_active_manifest(Path::new("."))
                                            .ok()
                                            .flatten()
                                            .map_or_else(
                                                || "dev.craftui.app".to_string(),
                                                |m| m.app.id,
                                            )
                                    });

                            println!(
                                "Starting Android app: {app_id}/dev.craftui.app.MainActivity..."
                            );
                            let _ = Command::new(&adb)
                                .args([
                                    "shell",
                                    "am",
                                    "start",
                                    "-n",
                                    &format!("{app_id}/dev.craftui.app.MainActivity"),
                                ])
                                .status();
                            println!(
                                "\x1b[32m[craftui]\x1b[0m App successfully deployed and launched on Android!"
                            );
                        } else {
                            println!(
                                "\x1b[33m[craftui]\x1b[0m No Android device or emulator currently connected. Connect a device with USB debugging or launch an emulator, then run `adb install -r {}`.",
                                apk_path.display()
                            );
                        }
                    } else {
                        println!(
                            "`adb` not found on PATH. Install Android platform-tools or run `adb install -r {}` manually.",
                            apk_path.display()
                        );
                    }
                }
            } else {
                println!(
                    "Android Gradle project updated at {}.\n`gradle`/`gradlew` was not found on PATH; open {} in Android Studio or run `./gradlew installDebug` with a connected device.",
                    out_dir.display(),
                    out_dir.display()
                );
            }
            Ok(())
        }
    }
}

pub fn mime_type_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "application/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}

pub fn resolve_static_asset_path(root: &Path, request_target: &str) -> Option<PathBuf> {
    let clean_target = request_target.split('?').next().unwrap_or("/");
    let trimmed = clean_target.trim_start_matches('/');
    let rel = if trimmed.is_empty() {
        PathBuf::from("index.html")
    } else {
        PathBuf::from(trimmed)
    };
    if rel.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    let joined = root.join(&rel);
    if joined.is_dir() {
        Some(joined.join("index.html"))
    } else {
        Some(joined)
    }
}

pub fn build_http_response(root: &Path, request_target: &str) -> Vec<u8> {
    let Some(asset_path) = resolve_static_asset_path(root, request_target) else {
        let body = b"403 Forbidden";
        return format!(
            "HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            std::str::from_utf8(body).unwrap_or_default()
        )
        .into_bytes();
    };
    match std::fs::read(&asset_path) {
        Ok(bytes) => {
            let mime = mime_type_for_path(&asset_path);
            let mut resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .into_bytes();
            resp.extend_from_slice(&bytes);
            resp
        }
        Err(_) => {
            let body = b"404 Not Found";
            format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n404 Not Found",
                body.len()
            )
            .into_bytes()
        }
    }
}

fn handle_http_connection(mut stream: TcpStream, root: &Path) {
    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };
    let req = String::from_utf8_lossy(&buf[..n]);
    let target = req
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let resp = build_http_response(root, target);
    let _ = stream.write_all(&resp);
    let _ = stream.flush();
}

fn serve_web_directory(root: &Path, port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr)
        .map_err(|e| CraftError::Platform(format!("failed binding http server on {addr}: {e}")))?;
    println!(
        "Serving CraftUI Web bundle ({}) at http://{} (Ctrl+C to stop)",
        root.display(),
        addr
    );
    for stream in listener.incoming().flatten() {
        let dir = root.to_path_buf();
        std::thread::spawn(move || handle_http_connection(stream, &dir));
    }
    Ok(())
}

pub fn format_permissions_list_json() -> String {
    let items: Vec<String> = Permission::all_standard()
        .iter()
        .map(|perm| {
            let key = json_escape(&perm.key());
            let android: Vec<String> = perm
                .android_permissions()
                .into_iter()
                .map(|s| json_escape(&s.name))
                .collect();
            let ios: Vec<String> = perm
                .ios_usage_keys()
                .iter()
                .map(|k| json_escape(k))
                .collect();
            let web = web_permission_name(perm).map_or_else(|| "null".to_string(), json_escape);
            format!(
                "  {{\"key\": {key}, \"android\": [{}], \"ios\": [{}], \"web\": {web}}}",
                android.join(", "),
                ios.join(", ")
            )
        })
        .collect();
    format!("[\n{}\n]", items.join(",\n"))
}

fn run_permissions_list(json: bool) -> Result<()> {
    if json {
        println!("{}", format_permissions_list_json());
        return Ok(());
    }
    println!(
        "CraftUI Standard Permissions ({} built-in keys)",
        Permission::all_standard().len()
    );
    println!("-------------------------------------------------");
    for perm in Permission::all_standard() {
        let android: Vec<String> = perm
            .android_permissions()
            .into_iter()
            .map(|s| match (s.min_sdk, s.max_sdk) {
                (Some(min), Some(max)) => format!("{} (SDK {min}..={max})", s.name),
                (Some(min), None) => format!("{} (SDK >= {min})", s.name),
                (None, Some(max)) => format!("{} (SDK <= {max})", s.name),
                (None, None) => s.name.into_owned(),
            })
            .collect();
        let ios = if perm.ios_usage_keys().is_empty() {
            "-".to_string()
        } else {
            perm.ios_usage_keys().join(", ")
        };
        let web = web_permission_name(perm).unwrap_or("-");
        println!(
            "  {:<24} | Android: {:<50} | iOS: {:<38} | Web: {}",
            perm.key(),
            if android.is_empty() {
                "-".to_string()
            } else {
                android.join(", ")
            },
            ios,
            web
        );
    }
    Ok(())
}

fn run_permissions_check(path: &Path, json: bool) -> Result<()> {
    let manifest_path = find_project_manifest(path).ok_or_else(|| {
        CraftError::Platform(format!(
            "no craftui.toml found at {} (run `craftui init` to create one)",
            path.display()
        ))
    })?;
    let manifest = AppManifest::load_file(&manifest_path)?;
    let diags = manifest.validate();
    let error_count = diags.iter().filter(|d| d.is_error).count();
    let warning_count = diags.len() - error_count;

    if json {
        let android_list: Vec<String> = manifest
            .android_manifest_permissions()
            .into_iter()
            .map(|s| json_escape(&s.name))
            .collect();
        let ios_list: Vec<String> = manifest
            .ios_info_plist_usage_entries()
            .into_iter()
            .map(|(k, _)| json_escape(&k))
            .collect();
        let diag_json: Vec<String> = diags
            .iter()
            .map(|d| {
                format!(
                    "    {{\"severity\": {}, \"message\": {}}}",
                    if d.is_error {
                        "\"error\""
                    } else {
                        "\"warning\""
                    },
                    json_escape(&d.message)
                )
            })
            .collect();
        println!(
            "{{\n  \"manifest\": {},\n  \"app_id\": {},\n  \"enabled_permissions\": {},\n  \"android_uses_permissions\": [{}],\n  \"ios_usage_keys\": [{}],\n  \"errors\": {},\n  \"warnings\": {},\n  \"diagnostics\": [\n{}\n  ]\n}}",
            json_escape(&manifest_path.display().to_string()),
            json_escape(&manifest.app.id),
            manifest.enabled_permissions().len(),
            android_list.join(", "),
            ios_list.join(", "),
            error_count,
            warning_count,
            diag_json.join(",\n")
        );
    } else {
        println!(
            "CraftUI Permission Audit — {} ({})",
            manifest.app.name, manifest.app.id
        );
        println!("Manifest: {}", manifest_path.display());
        println!(
            "Enabled Permissions: {} | Android entries: {} | iOS Info.plist keys: {}",
            manifest.enabled_permissions().len(),
            manifest.android_manifest_permissions().len(),
            manifest.ios_info_plist_usage_entries().len()
        );
        for d in &diags {
            let level = if d.is_error { "error" } else { "warning" };
            println!("  [{level}] {}", d.message);
        }
        println!("Result: {warning_count} warning(s), {error_count} error(s)");
    }

    if error_count > 0 {
        return Err(CraftError::Platform(format!(
            "{error_count} craftui.toml manifest error(s) found"
        )));
    }
    Ok(())
}

fn load_active_manifest(dir: &Path) -> Result<Option<AppManifest>> {
    if let Some(p) = find_project_manifest(dir) {
        Ok(Some(AppManifest::load_file(&p)?))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CheckStats {
    pub files: usize,
    pub warnings: usize,
    pub errors: usize,
}

pub fn check_cui_paths(path: &Path, emit_rust: bool) -> Result<CheckStats> {
    let mut files = Vec::new();
    collect_cui_files(path, &mut files)?;
    let mut stats = CheckStats::default();

    for f in files {
        stats.files += 1;
        let resolved = resolve_file(&f);
        for d in &resolved.diagnostics {
            match d.severity {
                Severity::Warning => {
                    stats.warnings += 1;
                    eprintln!(
                        "warning [{} @ {}..{}]: {}",
                        f.display(),
                        d.span.start,
                        d.span.end,
                        d.message
                    );
                }
                Severity::Error => {
                    stats.errors += 1;
                    eprintln!(
                        "error [{} @ {}..{}]: {}",
                        f.display(),
                        d.span.start,
                        d.span.end,
                        d.message
                    );
                }
            }
        }
        if emit_rust {
            let code = generate_rust(&resolved.merged);
            println!("// === {} ===\n{}", f.display(), code);
        }
    }
    Ok(stats)
}

fn collect_cui_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("cui") {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }
    if path.is_dir() {
        for entry in std::fs::read_dir(path)
            .map_err(|e| CraftError::Platform(format!("read_dir {} failed: {e}", path.display())))?
        {
            let entry = entry.map_err(|e| CraftError::Platform(format!("dir entry error: {e}")))?;
            let p = entry.path();
            if p.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            if p.is_dir() {
                collect_cui_files(&p, out)?;
            } else if p.extension().and_then(|e| e.to_str()) == Some("cui") {
                out.push(p);
            }
        }
    }
    Ok(())
}

pub fn render_cui_to_png(
    source: &str,
    component: Option<&str>,
    width: u32,
    height: u32,
    out_png: &Path,
) -> Result<()> {
    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);
    let mut session = HotReloadSession::new(cx);
    session.reload(source, component, &mut tree)?;
    render_loaded_tree(&mut tree, width, height, out_png)
}

pub fn render_cui_file_to_png(
    file: &Path,
    component: Option<&str>,
    width: u32,
    height: u32,
    out_png: &Path,
) -> Result<()> {
    let cx = Scope::root();
    let mut tree = WidgetTree::new(cx);
    let mut session = HotReloadSession::new(cx);
    session.reload_file(file, component, &mut tree)?;
    render_loaded_tree(&mut tree, width, height, out_png)
}

fn render_loaded_tree(
    tree: &mut WidgetTree,
    width: u32,
    height: u32,
    out_png: &Path,
) -> Result<()> {
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
    write_rgba_png(out_png, width.max(1), height.max(1), &rgba)?;
    Ok(())
}

fn run_dev_watcher(entry: &Path, dir: &Path) -> Result<()> {
    let watch_dir = if dir.exists() {
        dir
    } else if entry.exists() {
        entry.parent().unwrap_or_else(|| Path::new("."))
    } else {
        Path::new(".")
    };

    if entry.exists() {
        let _ = check_cui_paths(entry, false)?;
    }
    println!(
        "Watching {} for .cui hot-reload (Ctrl+C to stop)...",
        watch_dir.display()
    );

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(tx)
        .map_err(|e| CraftError::Platform(format!("watcher init failed: {e}")))?;
    watcher
        .watch(watch_dir, RecursiveMode::Recursive)
        .map_err(|e| CraftError::Platform(format!("watch failed: {e}")))?;

    let mut session = HotReloadSession::new(Scope::root());
    let mut tree = WidgetTree::new(session.state_scope);

    for event in rx.into_iter().flatten() {
        let touches_cui = event
            .paths
            .iter()
            .any(|p| p.extension().and_then(|e| e.to_str()) == Some("cui"));
        if !touches_cui {
            continue;
        }
        let target_path = if entry.exists() {
            entry.to_path_buf()
        } else if let Some(p) = event
            .paths
            .iter()
            .find(|p| p.extension().and_then(|e| e.to_str()) == Some("cui"))
        {
            p.clone()
        } else {
            continue;
        };
        let t0 = Instant::now();
        match session.reload_file(&target_path, None, &mut tree) {
            Ok(_) => println!(
                "[hot-reload] {} synced in {:.2}ms",
                target_path.display(),
                t0.elapsed().as_secs_f64() * 1000.0
            ),
            Err(e) => eprintln!("[hot-reload] {}: {e}", target_path.display()),
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    pub rustc_version: Option<String>,
    pub cargo_version: Option<String>,
    pub installed_targets: Vec<String>,
    pub wasm_target: bool,
    pub android_target: bool,
    pub ios_target: bool,
    pub wasm_bindgen: bool,
    pub wasm_opt: bool,
    pub trunk: bool,
    pub adb: Option<PathBuf>,
    pub android_ndk: Option<PathBuf>,
    pub clang: Option<PathBuf>,
    pub xcodebuild: bool,
    pub cui_stats: CheckStats,
    pub manifest_found: bool,
    pub manifest_permissions: usize,
    pub manifest_diagnostics: Vec<ManifestDiagnostic>,
}

fn command_stdout(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    #[cfg(windows)]
    let candidates = [
        format!("{name}.exe"),
        format!("{name}.cmd"),
        name.to_string(),
    ];
    #[cfg(not(windows))]
    let candidates = [name.to_string()];

    for dir in std::env::split_paths(&path_var) {
        for cand in &candidates {
            let full = dir.join(cand);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

fn is_valid_ndk_root(dir: &Path) -> bool {
    dir.join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .is_dir()
}

pub fn discover_android_ndk() -> Option<PathBuf> {
    for var in ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"] {
        if let Ok(val) = std::env::var(var) {
            let p = PathBuf::from(val);
            if is_valid_ndk_root(&p) {
                return Some(p);
            }
        }
    }

    let mut sdk_roots = Vec::new();
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(val) = std::env::var(var) {
            sdk_roots.push(PathBuf::from(val));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let h = PathBuf::from(home);
        sdk_roots.push(h.join("Android").join("Sdk"));
        sdk_roots.push(h.join("Library").join("Android").join("sdk"));
    }

    let mut ndk_dirs = Vec::new();
    for sdk in sdk_roots {
        let ndk_parent = sdk.join("ndk");
        if let Ok(entries) = std::fs::read_dir(&ndk_parent) {
            for entry in entries.flatten() {
                let p = entry.path();
                if is_valid_ndk_root(&p) {
                    ndk_dirs.push(p);
                }
            }
        }
    }
    ndk_dirs.sort();
    ndk_dirs.pop()
}

fn discover_adb() -> Option<PathBuf> {
    if let Some(p) = find_in_path("adb") {
        return Some(p);
    }
    if let Ok(home) = std::env::var("HOME") {
        let h = PathBuf::from(home);
        for cand in [
            h.join("Android/Sdk/platform-tools/adb"),
            h.join("Library/Android/sdk/platform-tools/adb"),
        ] {
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

fn ndk_toolchain_bin(ndk_root: &Path) -> Option<PathBuf> {
    let prebuilt = ndk_root.join("toolchains").join("llvm").join("prebuilt");
    for host in [
        "linux-x86_64",
        "darwin-arm64",
        "darwin-x86_64",
        "windows-x86_64",
    ] {
        let bin = prebuilt.join(host).join("bin");
        if bin.is_dir() {
            return Some(bin);
        }
    }
    None
}

fn configure_android_ndk_env(cmd: &mut Command, ndk_root: &Path) -> Option<(PathBuf, PathBuf)> {
    let bin = ndk_toolchain_bin(ndk_root)?;
    let mut best_api: Option<(u32, PathBuf, PathBuf)> = None;
    if let Ok(entries) = std::fs::read_dir(&bin) {
        for entry in entries.flatten() {
            let p = entry.path();
            let Some(fname) = p.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(rest) = fname.strip_prefix("aarch64-linux-android") else {
                continue;
            };
            let Some(api_str) = rest.strip_suffix("-clang++") else {
                continue;
            };
            if let Ok(api) = api_str.parse::<u32>() {
                let cc = bin.join(format!("aarch64-linux-android{api}-clang"));
                if cc.is_file()
                    && best_api.as_ref().is_none_or(|(b, _, _)| {
                        api > *b && *b < 24 || (api >= 24 && (*b < 24 || api < *b))
                    })
                {
                    best_api = Some((api, cc, p));
                }
            }
        }
    }
    let (_, cc, cxx) = best_api?;
    let ar = bin.join("llvm-ar");
    if std::env::var_os("CC_aarch64_linux_android").is_none() {
        cmd.env("CC_aarch64_linux_android", &cc);
        cmd.env("CC_aarch64-linux-android", &cc);
    }
    if std::env::var_os("CXX_aarch64_linux_android").is_none() {
        cmd.env("CXX_aarch64_linux_android", &cxx);
        cmd.env("CXX_aarch64-linux-android", &cxx);
    }
    if ar.is_file() && std::env::var_os("AR_aarch64_linux_android").is_none() {
        cmd.env("AR_aarch64_linux_android", &ar);
        cmd.env("AR_aarch64-linux-android", &ar);
    }
    if std::env::var_os("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER").is_none() {
        cmd.env("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", &cc);
    }
    Some((cc, cxx))
}

pub fn collect_doctor_report(project_dir: &Path) -> Result<DoctorReport> {
    let rustc_version = command_stdout("rustc", &["--version"]);
    let cargo_version = command_stdout("cargo", &["--version"]);
    let installed_targets: Vec<String> =
        command_stdout("rustup", &["target", "list", "--installed"])
            .map(|s| {
                s.lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default();

    let has_target = |t: &str| installed_targets.iter().any(|x| x == t);
    let cui_stats = if project_dir.exists() {
        check_cui_paths(project_dir, false)?
    } else {
        CheckStats::default()
    };

    let (manifest_found, manifest_permissions, manifest_diagnostics) =
        if let Some(mpath) = find_project_manifest(project_dir) {
            match AppManifest::load_file(&mpath) {
                Ok(m) => {
                    let perms = m.enabled_permissions().len();
                    let diags = m.validate();
                    (true, perms, diags)
                }
                Err(err) => (
                    true,
                    0,
                    vec![ManifestDiagnostic {
                        is_error: true,
                        message: err.to_string(),
                    }],
                ),
            }
        } else {
            (false, 0, Vec::new())
        };

    Ok(DoctorReport {
        rustc_version,
        cargo_version,
        wasm_target: has_target("wasm32-unknown-unknown"),
        android_target: has_target("aarch64-linux-android"),
        ios_target: has_target("aarch64-apple-ios"),
        installed_targets,
        wasm_bindgen: find_in_path("wasm-bindgen").is_some(),
        wasm_opt: find_in_path("wasm-opt").is_some(),
        trunk: find_in_path("trunk").is_some(),
        adb: discover_adb(),
        android_ndk: discover_android_ndk(),
        clang: find_in_path("clang"),
        xcodebuild: cfg!(target_os = "macos") && find_in_path("xcodebuild").is_some(),
        cui_stats,
        manifest_found,
        manifest_permissions,
        manifest_diagnostics,
    })
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn format_doctor_json(r: &DoctorReport) -> String {
    let opt_str = |o: &Option<String>| {
        o.as_ref()
            .map_or_else(|| "null".to_string(), |s| json_escape(s))
    };
    let opt_path = |o: &Option<PathBuf>| {
        o.as_ref().map_or_else(
            || "null".to_string(),
            |p| json_escape(&p.display().to_string()),
        )
    };
    let manifest_errors = r.manifest_diagnostics.iter().filter(|d| d.is_error).count();
    format!(
        "{{\n  \"rustc\": {},\n  \"cargo\": {},\n  \"targets\": {{\n    \"wasm32_unknown_unknown\": {},\n    \"aarch64_linux_android\": {},\n    \"aarch64_apple_ios\": {}\n  }},\n  \"tools\": {{\n    \"wasm_bindgen\": {},\n    \"wasm_opt\": {},\n    \"trunk\": {},\n    \"adb\": {},\n    \"android_ndk\": {},\n    \"clang\": {},\n    \"xcodebuild\": {}\n  }},\n  \"cui\": {{\n    \"files\": {},\n    \"warnings\": {},\n    \"errors\": {}\n  }},\n  \"manifest\": {{\n    \"found\": {},\n    \"permissions\": {},\n    \"errors\": {}\n  }}\n}}",
        opt_str(&r.rustc_version),
        opt_str(&r.cargo_version),
        r.wasm_target,
        r.android_target,
        r.ios_target,
        r.wasm_bindgen,
        r.wasm_opt,
        r.trunk,
        opt_path(&r.adb),
        opt_path(&r.android_ndk),
        opt_path(&r.clang),
        r.xcodebuild,
        r.cui_stats.files,
        r.cui_stats.warnings,
        r.cui_stats.errors,
        r.manifest_found,
        r.manifest_permissions,
        manifest_errors,
    )
}

fn run_doctor(path: &Path, json: bool) -> Result<()> {
    let report = collect_doctor_report(path)?;
    if json {
        println!("{}", format_doctor_json(&report));
    } else {
        println!("CraftUI Environment Doctor");
        println!("--------------------------");
        println!(
            "[{}] rustc: {}",
            if report.rustc_version.is_some() {
                "ok"
            } else {
                "missing"
            },
            report.rustc_version.as_deref().unwrap_or("not found")
        );
        println!(
            "[{}] cargo: {}",
            if report.cargo_version.is_some() {
                "ok"
            } else {
                "missing"
            },
            report.cargo_version.as_deref().unwrap_or("not found")
        );
        println!(
            "[{}] Desktop target (host): ready{}",
            if report.rustc_version.is_some() {
                "ok"
            } else {
                "missing"
            },
            if report.clang.is_some() {
                ""
            } else {
                " (clang missing)"
            }
        );
        println!(
            "[{}] Web target (wasm32-unknown-unknown): {} (wasm-bindgen: {}, wasm-opt: {}, trunk: {})",
            if report.wasm_target { "ok" } else { "missing" },
            if report.wasm_target {
                "installed"
            } else {
                "run `craftui setup`"
            },
            if report.wasm_bindgen { "yes" } else { "no" },
            if report.wasm_opt { "yes" } else { "no" },
            if report.trunk { "yes" } else { "no" },
        );
        println!(
            "[{}] Android target (aarch64-linux-android): {} (NDK: {}, adb: {})",
            if report.android_target && report.android_ndk.is_some() {
                "ok"
            } else {
                "warn"
            },
            if report.android_target {
                "installed"
            } else {
                "run `craftui setup`"
            },
            report
                .android_ndk
                .as_ref()
                .map_or_else(|| "not detected".into(), |p| p.display().to_string()),
            report
                .adb
                .as_ref()
                .map_or_else(|| "not found".into(), |p| p.display().to_string()),
        );
        println!(
            "[{}] iOS target (aarch64-apple-ios): {} (xcodebuild: {})",
            if report.ios_target && report.xcodebuild {
                "ok"
            } else {
                "info"
            },
            if report.ios_target {
                "installed"
            } else if cfg!(target_os = "macos") {
                "run `craftui setup`"
            } else {
                "macOS/Xcode required for linking (`craftui build --platform ios --skip-cargo` emits Swift/Metal template)"
            },
            if report.xcodebuild { "yes" } else { "no" },
        );
        println!(
            "[{}] Templates (.cui): {} file(s), {} warning(s), {} error(s)",
            if report.cui_stats.errors == 0 {
                "ok"
            } else {
                "error"
            },
            report.cui_stats.files,
            report.cui_stats.warnings,
            report.cui_stats.errors,
        );
        let m_errors = report
            .manifest_diagnostics
            .iter()
            .filter(|d| d.is_error)
            .count();
        println!(
            "[{}] Manifest (craftui.toml): {}",
            if !report.manifest_found {
                "info"
            } else if m_errors == 0 {
                "ok"
            } else {
                "error"
            },
            if report.manifest_found {
                format!(
                    "{} permission(s) declared, {} diagnostic(s)",
                    report.manifest_permissions,
                    report.manifest_diagnostics.len()
                )
            } else {
                "not present (`craftui init` to add)".to_string()
            }
        );
    }

    if report.rustc_version.is_none() || report.cargo_version.is_none() {
        return Err(CraftError::Platform(
            "Rust toolchain (rustc/cargo) is required".to_string(),
        ));
    }
    if report.cui_stats.errors > 0 {
        return Err(CraftError::Dsl(format!(
            "{} .cui validation error(s) found",
            report.cui_stats.errors
        )));
    }
    let manifest_errors = report
        .manifest_diagnostics
        .iter()
        .filter(|d| d.is_error)
        .count();
    if manifest_errors > 0 {
        return Err(CraftError::Platform(format!(
            "{manifest_errors} craftui.toml manifest error(s) found"
        )));
    }
    Ok(())
}

pub fn emit_web_bundle(out_dir: &Path, app_name: &str, built_wasm: Option<&Path>) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| CraftError::Platform(format!("failed creating {}: {e}", out_dir.display())))?;

    let mut wasm_bindgen_ran = false;
    if let Some(wasm_path) = built_wasm
        && wasm_path.is_file()
    {
        let raw_dest = out_dir.join(
            wasm_path
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("craftui_app.wasm")),
        );
        std::fs::copy(wasm_path, &raw_dest).map_err(|e| {
            CraftError::Platform(format!("failed copying wasm {}: {e}", wasm_path.display()))
        })?;

        if find_in_path("wasm-bindgen").is_some() {
            let status = Command::new("wasm-bindgen")
                .arg(wasm_path)
                .args(["--target", "web", "--out-name", "craftui_app", "--out-dir"])
                .arg(out_dir)
                .status()
                .map_err(|e| CraftError::Platform(format!("wasm-bindgen failed: {e}")))?;
            if !status.success() {
                return Err(CraftError::Platform(format!(
                    "wasm-bindgen exited with status {status}"
                )));
            }
            wasm_bindgen_ran = true;

            let bg_wasm = out_dir.join("craftui_app_bg.wasm");
            if bg_wasm.is_file() && find_in_path("wasm-opt").is_some() {
                let _ = Command::new("wasm-opt")
                    .arg("-O3")
                    .arg(&bg_wasm)
                    .arg("-o")
                    .arg(&bg_wasm)
                    .status();
            }
        }
    }

    let safe_title = xml_escape(app_name);
    let index_html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover" />
  <title>{safe_title} — CraftUI</title>
  <style>
    html, body {{
      margin: 0;
      padding: 0;
      width: 100%;
      height: 100%;
      overflow: hidden;
      background: #0b0e14;
      color: #f8fafc;
      font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }}
    #craftui-canvas {{
      width: 100vw;
      height: 100vh;
      display: block;
      outline: none;
    }}
    #craftui-status {{
      position: fixed;
      inset: 0;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 14px;
      pointer-events: none;
      opacity: 0.78;
    }}
  </style>
</head>
<body>
  <canvas id="craftui-canvas" tabindex="0"></canvas>
  <div id="craftui-status">Loading {safe_title}...</div>
  <script type="module">
    const status = document.getElementById("craftui-status");
    try {{
      const mod = await import("./craftui_app.js");
      await mod.default();
      status.remove();
    }} catch (err) {{
      status.textContent = "CraftUI WASM bundle pending wasm-bindgen output (./craftui_app.js): " + err;
    }}
  </script>
</body>
</html>
"#
    );

    let readme = format!(
        "# {app_name} (Web / WASM Bundle)\n\nGenerated by `craftui build --platform web`.\n{}\nServe locally with built-in HTTP/WASM MIME server:\n```sh\ncraftui run --platform web --skip-cargo\n```\n",
        if wasm_bindgen_ran {
            "JS glue (`craftui_app.js`) and `craftui_app_bg.wasm` generated via `wasm-bindgen`.\n"
        } else {
            "Note: `wasm-bindgen` was not detected on PATH, so only raw HTML + `.wasm` (if built) were written.\nInstall `cargo install wasm-bindgen-cli` and run `wasm-bindgen <target.wasm> --target web --out-name craftui_app --out-dir .` inside this directory.\n"
        }
    );

    std::fs::write(out_dir.join("index.html"), index_html)
        .and_then(|_| std::fs::write(out_dir.join("README.md"), readme))
        .map_err(|e| CraftError::Platform(format!("failed writing web bundle: {e}")))?;
    Ok(())
}

pub fn emit_android_template(
    out_dir: &Path,
    app_name: &str,
    rust_lib_name: &str,
    built_so: Option<&Path>,
) -> Result<()> {
    emit_android_template_with_manifest(out_dir, app_name, rust_lib_name, built_so, None)
}

pub fn emit_android_template_with_manifest(
    out_dir: &Path,
    app_name: &str,
    rust_lib_name: &str,
    built_so: Option<&Path>,
    manifest_opt: Option<&AppManifest>,
) -> Result<()> {
    let effective_manifest = manifest_opt
        .cloned()
        .unwrap_or_else(|| AppManifest::with_app(app_name, AppManifest::sanitize_id(app_name)));

    let app_display_name = xml_escape(&effective_manifest.app.name);
    let app_id = effective_manifest.app.id.clone();
    let app_version = effective_manifest.app.version.clone();
    let min_sdk = effective_manifest.android.min_sdk;
    let target_sdk = effective_manifest.android.target_sdk;
    let version_code = effective_manifest.android.version_code;
    let permission_xml = effective_manifest.render_android_manifest_permissions_xml();

    let java_dir = out_dir.join("app/src/main/kotlin/dev/craftui/app");
    let jni_dir = out_dir.join("app/src/main/jniLibs/arm64-v8a");
    let res_values_dir = out_dir.join("app/src/main/res/values");
    std::fs::create_dir_all(&java_dir)
        .and_then(|_| std::fs::create_dir_all(&jni_dir))
        .and_then(|_| std::fs::create_dir_all(&res_values_dir))
        .map_err(|e| CraftError::Platform(format!("failed creating android dirs: {e}")))?;

    let settings_gradle = format!(
        r#"pluginManagement {{
    repositories {{
        google()
        mavenCentral()
        gradlePluginPortal()
    }}
}}
dependencyResolutionManagement {{
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {{
        google()
        mavenCentral()
    }}
}}
rootProject.name = "{}"
include(":app")
"#,
        effective_manifest.app.name.replace('"', "\\\"")
    );
    let root_gradle = r#"plugins {
    id("com.android.application") version "8.6.1" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
}
"#;
    let app_gradle = format!(
        r#"plugins {{
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}}

android {{
    namespace = "dev.craftui.app"
    compileSdk = {target_sdk}

    defaultConfig {{
        applicationId = "{app_id}"
        minSdk = {min_sdk}
        targetSdk = {target_sdk}
        versionCode = {version_code}
        versionName = "{app_version}"
        ndk {{
            abiFilters += listOf("arm64-v8a")
        }}
    }}

    buildFeatures {{
        prefab = true
    }}

    compileOptions {{
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }}
    kotlinOptions {{
        jvmTarget = "17"
    }}
}}

dependencies {{
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.activity:activity-ktx:1.9.3")
    implementation("androidx.biometric:biometric:1.1.0")
    implementation("androidx.games:games-activity:3.0.5")
}}
"#
    );

    let android_manifest = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
{permission_xml}    <application
        android:label="{app_display_name}"
        android:allowBackup="true"
        android:supportsRtl="true"
        android:theme="@style/Theme.CraftUI">
        <activity
            android:name=".MainActivity"
            android:exported="true"
            android:launchMode="singleTask"
            android:windowSoftInputMode="adjustResize"
            android:configChanges="orientation|keyboardHidden|keyboard|screenSize|smallestScreenSize|locale|layoutDirection|fontScale|screenLayout|density|uiMode">
            <meta-data
                android:name="android.app.lib_name"
                android:value="{rust_lib_name}" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#
    );

    let styles_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<resources>
    <style name="Theme.CraftUI" parent="Theme.AppCompat.DayNight.NoActionBar" />
</resources>
"#;

    let main_activity = format!(
        r#"package dev.craftui.app

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.BatteryManager
import android.os.Build
import android.os.Bundle
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.provider.Settings
import android.view.HapticFeedbackConstants
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import com.google.androidgamesdk.GameActivity

class MainActivity : GameActivity() {{
    companion object {{
        init {{
            System.loadLibrary("{rust_lib_name}")
        }}
        var instance: MainActivity? = null

        @JvmStatic
        fun launchCameraFromNative() {{
            instance?.launchCameraWithPermission()
        }}

        @JvmStatic
        fun launchGalleryFromNative() {{
            instance?.launchGalleryWithPermission()
        }}

        @JvmStatic
        fun showTauriWebViewNative() {{
            instance?.showWebView()
        }}

        @JvmStatic
        fun hideTauriWebViewNative() {{
            instance?.hideWebView()
        }}
    }}

    private var pendingPermRequestId: Long = 0L
    private var pendingFileRequestId: Long = 0L
    private var lastReportedInsetsKey: String = ""

    private val cameraPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) {{ isGranted ->
        if (isGranted) {{
            openCameraIntent()
        }}
    }}

    private val galleryPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) {{ isGranted ->
        if (isGranted) {{
            openGalleryIntent()
        }}
    }}

    private val takePictureLauncher = registerForActivityResult(
        ActivityResultContracts.TakePicturePreview()
    ) {{ _ ->
    }}

    private val pickImageLauncher = registerForActivityResult(
        ActivityResultContracts.GetContent()
    ) {{ _ ->
    }}

    fun launchCameraWithPermission() {{
        runOnUiThread {{
            if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {{
                openCameraIntent()
            }} else {{
                cameraPermissionLauncher.launch(android.Manifest.permission.CAMERA)
            }}
        }}
    }}

    fun launchGalleryWithPermission() {{
        runOnUiThread {{
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {{
                openGalleryIntent()
            }} else if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.READ_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED) {{
                openGalleryIntent()
            }} else {{
                galleryPermissionLauncher.launch(android.Manifest.permission.READ_EXTERNAL_STORAGE)
            }}
        }}
    }}

    fun showWebView() {{
        runOnUiThread {{
            // Activate overlay WebView on demand
        }}
    }}

    fun hideWebView() {{
        runOnUiThread {{
            // Deactivate overlay WebView
        }}
    }}

    private fun openCameraIntent() {{
        try {{
            takePictureLauncher.launch(null)
        }} catch (_: Exception) {{
            val intent = Intent(android.provider.MediaStore.ACTION_IMAGE_CAPTURE)
            if (intent.resolveActivity(packageManager) != null) {{
                startActivity(intent)
            }}
        }}
    }}

    private fun openGalleryIntent() {{
        try {{
            pickImageLauncher.launch("image/*")
        }} catch (_: Exception) {{
            val intent = Intent(Intent.ACTION_PICK, android.provider.MediaStore.Images.Media.EXTERNAL_CONTENT_URI)
            startActivity(intent)
        }}
    }}

    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) {{ grants ->
        val rationales = grants.keys.associateWith {{ perm ->
            ActivityCompat.shouldShowRequestPermissionRationale(this, perm)
        }}
        onNativePermissionResults(pendingPermRequestId, grants, rationales)
    }}

    private val openFilesLauncher = registerForActivityResult(
        ActivityResultContracts.OpenMultipleDocuments()
    ) {{ uris: List<Uri> ->
        onNativeFileDialogResults(pendingFileRequestId, uris.map {{ it.toString() }})
    }}

    private val saveFileLauncher = registerForActivityResult(
        ActivityResultContracts.CreateDocument("*/*")
    ) {{ uri: Uri? ->
        onNativeFileDialogResults(pendingFileRequestId, uri?.let {{ listOf(it.toString()) }} ?: emptyList())
    }}

    override fun onCreate(savedInstanceState: Bundle?) {{
        super.onCreate(savedInstanceState)
        instance = this
        WindowCompat.setDecorFitsSystemWindows(window, false)
        ViewCompat.setOnApplyWindowInsetsListener(window.decorView) {{ _, insets ->
            val safe = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
            val key = "${{safe.top}},${{safe.right}},${{safe.bottom}},${{safe.left}},${{ime.bottom}}"
            if (key != lastReportedInsetsKey) {{
                lastReportedInsetsKey = key
            }}
            insets
        }}
    }}

    fun requestCraftUiPermissions(requestId: Long, manifestPermissions: Array<String>) {{
        pendingPermRequestId = requestId
        permissionLauncher.launch(manifestPermissions)
    }}

    fun checkCraftUiPermissionGranted(manifestPermission: String): Boolean {{
        return ContextCompat.checkSelfPermission(this, manifestPermission) == PackageManager.PERMISSION_GRANTED
    }}

    fun openCraftUiAppSettings() {{
        val intent = Intent(
            Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
            Uri.fromParts("package", packageName, null)
        )
        startActivity(intent)
    }}

    fun openCraftUiDocuments(requestId: Long, mimeTypes: Array<String>) {{
        pendingFileRequestId = requestId
        openFilesLauncher.launch(if (mimeTypes.isEmpty()) arrayOf("*/*") else mimeTypes)
    }}

    fun saveCraftUiDocument(requestId: Long, suggestedName: String) {{
        pendingFileRequestId = requestId
        saveFileLauncher.launch(suggestedName)
    }}

    fun triggerCraftUiHaptic(style: String) {{
        val view = window.decorView
        when (style) {{
            "selection" -> view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
            "heavy" -> vibrateOneShot(35L, 220)
            "medium" -> vibrateOneShot(20L, 150)
            "success", "warning", "error" -> vibrateOneShot(28L, 180)
            else -> vibrateOneShot(12L, 90)
        }}
    }}

    private fun vibrateOneShot(durationMs: Long, amplitude: Int) {{
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {{
            val vm = getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager
            vm?.defaultVibrator?.vibrate(VibrationEffect.createOneShot(durationMs, amplitude))
        }} else {{
            @Suppress("DEPRECATION")
            val v = getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {{
                v?.vibrate(VibrationEffect.createOneShot(durationMs, amplitude))
            }}
        }}
    }}

    fun shareCraftUiContent(title: String?, text: String?) {{
        val send = Intent(Intent.ACTION_SEND).apply {{
            type = "text/plain"
            text?.let {{ putExtra(Intent.EXTRA_TEXT, it) }}
            title?.let {{ putExtra(Intent.EXTRA_SUBJECT, it) }}
        }}
        startActivity(Intent.createChooser(send, title ?: "Share"))
    }}

    fun queryNetworkClassification(): Int {{
        val cm = getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return 0
        val network = cm.activeNetwork ?: return 1
        val caps = cm.getNetworkCapabilities(network) ?: return 1
        return when {{
            caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> 2
            caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> 3
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> 4
            else -> 0
        }}
    }}

    fun queryBatteryPercentAndCharging(): Pair<Int, Boolean> {{
        val bm = getSystemService(Context.BATTERY_SERVICE) as? BatteryManager ?: return Pair(-1, false)
        val pct = bm.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)
        val charging = bm.isCharging
        return Pair(pct, charging)
    }}

    fun writeAppSecret(key: String, value: String) {{
        getSharedPreferences("craftui_secure_cache", Context.MODE_PRIVATE)
            .edit()
            .putString(key, value)
            .apply()
    }}

    fun deleteAppSecret(key: String) {{
        getSharedPreferences("craftui_secure_cache", Context.MODE_PRIVATE)
            .edit()
            .remove(key)
            .apply()
    }}

    private fun onNativePermissionResults(
        requestId: Long,
        grants: Map<String, Boolean>,
        rationales: Map<String, Boolean>
    ) {{
        // Dispatched into AndroidBridgeState::push_permission_results via JNI bridge
    }}

    private fun onNativeFileDialogResults(requestId: Long, uris: List<String>) {{
        // Dispatched into AndroidBridgeState::push_file_dialog_result via JNI bridge
    }}
}}
"#
    );

    if let Some(so_path) = built_so
        && so_path.is_file()
    {
        if let Ok(entries) = std::fs::read_dir(&jni_dir) {
            for entry in entries.flatten() {
                if entry.path().extension().is_some_and(|e| e == "so") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let target_so = jni_dir.join(format!("lib{rust_lib_name}.so"));
        std::fs::copy(so_path, &target_so).map_err(|e| {
            CraftError::Platform(format!("failed copying .so {}: {e}", so_path.display()))
        })?;
    }

    std::fs::write(out_dir.join("settings.gradle.kts"), settings_gradle)
        .and_then(|_| std::fs::write(out_dir.join("build.gradle.kts"), root_gradle))
        .and_then(|_| std::fs::write(out_dir.join("app/build.gradle.kts"), app_gradle))
        .and_then(|_| {
            std::fs::write(
                out_dir.join("app/src/main/AndroidManifest.xml"),
                android_manifest,
            )
        })
        .and_then(|_| std::fs::write(res_values_dir.join("styles.xml"), styles_xml))
        .and_then(|_| std::fs::write(java_dir.join("MainActivity.kt"), main_activity))
        .map_err(|e| CraftError::Platform(format!("failed writing android template: {e}")))?;

    // `local.properties` tells Gradle where the Android SDK is. When it cannot
    // be worked out, the file is left out and Gradle falls back to ANDROID_HOME
    // or reports a clear "SDK location not found" error.
    let sdk_dir = std::env::var("ANDROID_HOME")
        .or_else(|_| std::env::var("ANDROID_SDK_ROOT"))
        .or_else(|_| std::env::var("HOME").map(|home| format!("{home}/Android/Sdk")));
    if let Ok(sdk_dir) = sdk_dir {
        let _ = std::fs::write(
            out_dir.join("local.properties"),
            format!("sdk.dir={}\n", sdk_dir.replace('\\', "/")),
        );
    }
    let _ = std::fs::write(
        out_dir.join("gradle.properties"),
        "android.useAndroidX=true\n",
    );

    Ok(())
}

pub fn emit_ios_template(out_dir: &Path, app_name: &str, rust_lib_name: &str) -> Result<()> {
    emit_ios_template_with_manifest(out_dir, app_name, rust_lib_name, None)
}

pub fn emit_ios_template_with_manifest(
    out_dir: &Path,
    app_name: &str,
    rust_lib_name: &str,
    manifest_opt: Option<&AppManifest>,
) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| CraftError::Platform(format!("failed creating ios dirs: {e}")))?;

    let effective_manifest = manifest_opt
        .cloned()
        .unwrap_or_else(|| AppManifest::with_app(app_name, AppManifest::sanitize_id(app_name)));

    let header = r#"#ifndef CRAFTUI_IOS_H
#define CRAFTUI_IOS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct IosAppHost IosAppHost;

typedef struct {
    void *ca_metal_layer;
    float width_pt;
    float height_pt;
    float contents_scale;
} IosMetalLayerDescriptor;

typedef void (*IosBuildUiCallback)(IosAppHost *host, void *user_data);

IosAppHost *craftui_ios_app_create(float width_pt, float height_pt, float scale_factor);
IosAppHost *craftui_ios_app_launch(IosMetalLayerDescriptor layer, IosBuildUiCallback build, void *user_data);
int32_t craftui_ios_app_attach_metal(IosAppHost *app, IosMetalLayerDescriptor layer);
const char *craftui_ios_app_last_error(const IosAppHost *app);
void craftui_ios_app_clear_error(IosAppHost *app);
void craftui_ios_app_destroy(IosAppHost *app);
void craftui_ios_app_resize(IosAppHost *app, float width_pt, float height_pt, float scale_factor);
void craftui_ios_app_set_safe_area(IosAppHost *app, float top, float right, float bottom, float left);
void craftui_ios_app_set_keyboard(IosAppHost *app, float overlap_height_pt, bool visible);
bool craftui_ios_app_touch(IosAppHost *app, uint64_t touch_id, uint8_t phase, float x_pt, float y_pt, uint64_t timestamp_ms);
bool craftui_ios_app_insert_text(IosAppHost *app, const uint8_t *utf8_ptr, size_t utf8_len);
bool craftui_ios_app_set_marked_text(IosAppHost *app, const uint8_t *utf8_ptr, size_t utf8_len, size_t cursor_end);
bool craftui_ios_app_delete_backward(IosAppHost *app);
uint8_t craftui_ios_app_tick(IosAppHost *app, float dt_sec);

uint8_t craftui_ios_app_permission_status_from_key(const IosAppHost *app, const uint8_t *key_ptr, size_t key_len);
void craftui_ios_app_set_permission_status(IosAppHost *app, const uint8_t *key_ptr, size_t key_len, uint8_t status_code);
void craftui_ios_app_deliver_permission_result(IosAppHost *app, uint64_t request_id, const uint8_t *key_ptr, size_t key_len, uint8_t status_code, bool rationale);
int32_t craftui_ios_app_poll_command_json(IosAppHost *app, uint8_t *out_buf, size_t out_cap);
void craftui_ios_app_push_lifecycle(IosAppHost *app, uint8_t event_code);
void craftui_ios_app_set_network_status(IosAppHost *app, uint8_t network_code);
void craftui_ios_app_set_battery(IosAppHost *app, int16_t level_pct, bool charging);

#ifdef __cplusplus
}
#endif

#endif
"#;

    let permissions_swift = r#"import AVFoundation
import Contacts
import CoreBluetooth
import CoreLocation
import CoreMotion
import EventKit
import Foundation
import LocalAuthentication
import Photos
import Security
import UserNotifications

enum CraftUIPermissionCode: UInt8 {
    case notDetermined = 0
    case granted = 1
    case denied = 2
    case permanentlyDenied = 3
    case restricted = 4
    case limited = 5
    case provisional = 6
    case unsupported = 7
}

final class CraftUIPermissionBroker: NSObject, CLLocationManagerDelegate, CBCentralManagerDelegate {
    private var locationManager: CLLocationManager?
    private var pendingLocationCompletion: ((CraftUIPermissionCode) -> Void)?
    private var bluetoothManager: CBCentralManager?

    func pushStatus(_ code: CraftUIPermissionCode, forKey key: String, app: OpaquePointer) {
        var bytes = Array(key.utf8)
        bytes.withUnsafeMutableBufferPointer { buf in
            craftui_ios_app_set_permission_status(app, buf.baseAddress, buf.count, code.rawValue)
        }
    }

    func deliverResult(requestId: UInt64, key: String, code: CraftUIPermissionCode, rationale: Bool = false, app: OpaquePointer) {
        var bytes = Array(key.utf8)
        bytes.withUnsafeMutableBufferPointer { buf in
            craftui_ios_app_deliver_permission_result(app, requestId, buf.baseAddress, buf.count, code.rawValue, rationale)
        }
    }

    func requestPermission(requestId: UInt64, key: String, app: OpaquePointer) {
        switch key {
        case "camera":
            AVCaptureDevice.requestAccess(for: .video) { granted in
                DispatchQueue.main.async {
                    self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                }
            }
        case "microphone":
            AVCaptureDevice.requestAccess(for: .audio) { granted in
                DispatchQueue.main.async {
                    self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                }
            }
        case "notifications":
            UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound]) { granted, _ in
                DispatchQueue.main.async {
                    self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                }
            }
        case "photos", "videos":
            PHPhotoLibrary.requestAuthorization(for: .readWrite) { status in
                let code: CraftUIPermissionCode = switch status {
                case .authorized: .granted
                case .limited: .limited
                case .restricted: .restricted
                case .denied: .permanentlyDenied
                default: .notDetermined
                }
                DispatchQueue.main.async {
                    self.deliverResult(requestId: requestId, key: key, code: code, app: app)
                }
            }
        case "photos_add_only":
            PHPhotoLibrary.requestAuthorization(for: .addOnly) { status in
                let code: CraftUIPermissionCode = status == .authorized ? .granted : .denied
                DispatchQueue.main.async {
                    self.deliverResult(requestId: requestId, key: key, code: code, app: app)
                }
            }
        case "location_when_in_use", "location_coarse":
            let mgr = CLLocationManager()
            mgr.delegate = self
            locationManager = mgr
            pendingLocationCompletion = { code in
                self.deliverResult(requestId: requestId, key: key, code: code, app: app)
            }
            mgr.requestWhenInUseAuthorization()
        case "location_always":
            let mgr = CLLocationManager()
            mgr.delegate = self
            locationManager = mgr
            pendingLocationCompletion = { code in
                self.deliverResult(requestId: requestId, key: key, code: code, app: app)
            }
            mgr.requestAlwaysAuthorization()
        case "contacts":
            CNContactStore().requestAccess(for: .contacts) { granted, _ in
                DispatchQueue.main.async {
                    self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                }
            }
        case "calendar_read", "calendar_write":
            let store = EKEventStore()
            if #available(iOS 17.0, *) {
                store.requestFullAccessToEvents { granted, _ in
                    DispatchQueue.main.async {
                        self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                    }
                }
            } else {
                store.requestAccess(to: .event) { granted, _ in
                    DispatchQueue.main.async {
                        self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                    }
                }
            }
        case "biometrics":
            let ctx = LAContext()
            var err: NSError?
            if ctx.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &err) {
                ctx.evaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, localizedReason: "Authenticate with biometrics") { granted, _ in
                    DispatchQueue.main.async {
                        self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                    }
                }
            } else {
                deliverResult(requestId: requestId, key: key, code: .unsupported, app: app)
            }
        case "bluetooth", "bluetooth_scan", "bluetooth_connect", "bluetooth_advertise":
            bluetoothManager = CBCentralManager(delegate: self, queue: nil)
            deliverResult(requestId: requestId, key: key, code: .granted, app: app)
        case "motion_activity", "sensors":
            let pedometer = CMPedometer()
            pedometer.queryPedometerData(from: Date().addingTimeInterval(-60), to: Date()) { _, error in
                DispatchQueue.main.async {
                    let code: CraftUIPermissionCode = (error == nil) ? .granted : .denied
                    self.deliverResult(requestId: requestId, key: key, code: code, app: app)
                }
            }
        default:
            deliverResult(requestId: requestId, key: key, code: .unsupported, app: app)
        }
    }

    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        let code: CraftUIPermissionCode = switch manager.authorizationStatus {
        case .authorizedAlways, .authorizedWhenInUse: .granted
        case .denied: .permanentlyDenied
        case .restricted: .restricted
        default: .notDetermined
        }
        pendingLocationCompletion?(code)
        pendingLocationCompletion = nil
    }

    func centralManagerDidUpdateState(_ central: CBCentralManager) {
        _ = central.state
    }

    func saveKeychainSecret(key: String, value: String) {
        guard let data = value.data(using: .utf8) else { return }
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
            kSecValueData as String: data
        ]
        SecItemDelete(query as CFDictionary)
        SecItemAdd(query as CFDictionary, nil)
    }

    func deleteKeychainSecret(key: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key
        ]
        SecItemDelete(query as CFDictionary)
    }
}
"#;

    let swift = r#"import Network
import QuartzCore
import UIKit

final class CraftUIMetalView: UIView {
    override class var layerClass: AnyClass { CAMetalLayer.self }
    var metalLayer: CAMetalLayer { layer as! CAMetalLayer }
}

final class CraftUIViewController: UIViewController, UIKeyInput {
    private let metalView = CraftUIMetalView()
    private let permissionBroker = CraftUIPermissionBroker()
    private let pathMonitor = NWPathMonitor()
    private var app: OpaquePointer?
    private var displayLink: CADisplayLink?
    private var lastTimestamp: CFTimeInterval = 0
    private var keyboardActive = false

    var hasText: Bool { true }
    override var canBecomeFirstResponder: Bool { true }

    override func loadView() {
        metalView.isMultipleTouchEnabled = true
        metalView.metalLayer.pixelFormat = .bgra8Unorm
        view = metalView
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        let scale = Float(UIScreen.main.nativeScale)
        let layerDesc = IosMetalLayerDescriptor(
            ca_metal_layer: Unmanaged.passUnretained(metalView.metalLayer).toOpaque(),
            width_pt: Float(view.bounds.width),
            height_pt: Float(view.bounds.height),
            contents_scale: scale
        )
        app = craftui_ios_app_launch(layerDesc, nil, nil)
        if let app, let errPtr = craftui_ios_app_last_error(app) {
            NSLog("CraftUI launch error: %@", String(cString: errPtr))
            craftui_ios_app_clear_error(app)
        }

        UIDevice.current.isBatteryMonitoringEnabled = true
        pushBatteryTelemetry()

        pathMonitor.pathUpdateHandler = { [weak self] path in
            DispatchQueue.main.async {
                guard let self, let app = self.app else { return }
                let code: UInt8 = if path.status != .satisfied {
                    1
                } else if path.usesInterfaceType(.wifi) {
                    2
                } else if path.usesInterfaceType(.cellular) {
                    3
                } else if path.usesInterfaceType(.wiredEthernet) {
                    4
                } else {
                    0
                }
                craftui_ios_app_set_network_status(app, code)
            }
        }
        pathMonitor.start(queue: DispatchQueue.global(qos: .utility))

        NotificationCenter.default.addObserver(
            self,
            selector: #selector(handleKeyboard(_:)),
            name: UIResponder.keyboardWillChangeFrameNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(handleBattery(_:)),
            name: UIDevice.batteryLevelDidChangeNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(handleDidBecomeActive),
            name: UIApplication.didBecomeActiveNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(handleDidEnterBackground),
            name: UIApplication.didEnterBackgroundNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(handleMemoryWarning),
            name: UIApplication.didReceiveMemoryWarningNotification,
            object: nil
        )

        let link = CADisplayLink(target: self, selector: #selector(stepFrame(_:)))
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        guard let app else { return }
        let scale = Float(view.window?.screen.nativeScale ?? UIScreen.main.nativeScale)
        metalView.metalLayer.contentsScale = CGFloat(scale)
        craftui_ios_app_resize(app, Float(view.bounds.width), Float(view.bounds.height), scale)
        let sa = view.safeAreaInsets
        craftui_ios_app_set_safe_area(app, Float(sa.top), Float(sa.right), Float(sa.bottom), Float(sa.left))
    }

    @objc private func handleKeyboard(_ note: Notification) {
        guard let app,
              let endFrame = note.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect else { return }
        let overlap = max(0, view.bounds.maxY - view.convert(endFrame, from: nil).minY)
        craftui_ios_app_set_keyboard(app, Float(overlap), overlap > 0)
    }

    @objc private func handleBattery(_ note: Notification) { pushBatteryTelemetry() }
    @objc private func handleDidBecomeActive() { if let app { craftui_ios_app_push_lifecycle(app, 0) } }
    @objc private func handleDidEnterBackground() { if let app { craftui_ios_app_push_lifecycle(app, 1) } }
    @objc private func handleMemoryWarning() { if let app { craftui_ios_app_push_lifecycle(app, 4) } }

    private func pushBatteryTelemetry() {
        guard let app else { return }
        let level = UIDevice.current.batteryLevel
        let pct: Int16 = level < 0 ? -1 : Int16((level * 100.0).rounded())
        let charging = UIDevice.current.batteryState == .charging || UIDevice.current.batteryState == .full
        craftui_ios_app_set_battery(app, pct, charging)
    }

    @objc private func stepFrame(_ link: CADisplayLink) {
        guard let app else { return }
        let dt = lastTimestamp == 0 ? 0.016 : Float(min(0.05, max(0.001, link.timestamp - lastTimestamp)))
        lastTimestamp = link.timestamp
        let flags = craftui_ios_app_tick(app, dt)
        let wantsKeyboard = (flags & 0b010) != 0
        if wantsKeyboard && !keyboardActive {
            keyboardActive = true
            becomeFirstResponder()
        } else if !wantsKeyboard && keyboardActive {
            keyboardActive = false
            resignFirstResponder()
        }
        if (flags & 0b100) != 0, let errPtr = craftui_ios_app_last_error(app) {
            NSLog("CraftUI runtime error: %@", String(cString: errPtr))
            craftui_ios_app_clear_error(app)
        }
        drainPlatformCommands(app)
    }

    private func drainPlatformCommands(_ app: OpaquePointer) {
        var buffer = [UInt8](repeating: 0, count: 4096)
        while true {
            let n = buffer.withUnsafeMutableBufferPointer { ptr in
                craftui_ios_app_poll_command_json(app, ptr.baseAddress, ptr.count)
            }
            if n == 0 {
                break
            }
            if n < 0 {
                buffer = [UInt8](repeating: 0, count: Int(-n) + 1)
                continue
            }
            let data = Data(buffer.prefix(Int(n)))
            guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let type = json["type"] as? String else {
                continue
            }
            handleCommand(type: type, json: json, app: app)
        }
    }

    private func handleCommand(type: String, json: [String: Any], app: OpaquePointer) {
        switch type {
        case "request_permission":
            if let key = json["key"] as? String {
                let reqId = (json["request_id"] as? NSNumber)?.uint64Value ?? 0
                permissionBroker.requestPermission(requestId: reqId, key: key, app: app)
            }
        case "open_app_settings":
            if let url = URL(string: UIApplication.openSettingsURLString) {
                UIApplication.shared.open(url)
            }
        case "haptic":
            switch json["style"] as? String {
            case "selection":
                UISelectionFeedbackGenerator().selectionChanged()
            case "success":
                UINotificationFeedbackGenerator().notificationOccurred(.success)
            case "warning":
                UINotificationFeedbackGenerator().notificationOccurred(.warning)
            case "error":
                UINotificationFeedbackGenerator().notificationOccurred(.error)
            case "heavy":
                UIImpactFeedbackGenerator(style: .heavy).impactOccurred()
            case "medium":
                UIImpactFeedbackGenerator(style: .medium).impactOccurred()
            default:
                UIImpactFeedbackGenerator(style: .light).impactOccurred()
            }
        case "share":
            var items: [Any] = []
            if let txt = json["text"] as? String { items.append(txt) }
            if let urlStr = json["url"] as? String, let url = URL(string: urlStr) { items.append(url) }
            if !items.isEmpty {
                let sheet = UIActivityViewController(activityItems: items, applicationActivities: nil)
                present(sheet, animated: true)
            }
        case "sync_secret":
            if let k = json["key"] as? String, let v = json["value"] as? String {
                permissionBroker.saveKeychainSecret(key: k, value: v)
            }
        case "delete_secret":
            if let k = json["key"] as? String {
                permissionBroker.deleteKeychainSecret(key: k)
            }
        default:
            break
        }
    }

    private func sendTouches(_ touches: Set<UITouch>, phase: UInt8) {
        guard let app else { return }
        for t in touches {
            let pt = t.location(in: view)
            let tid = UInt64(bitPattern: Int64(ObjectIdentifier(t).hashValue))
            _ = craftui_ios_app_touch(app, tid, phase, Float(pt.x), Float(pt.y), UInt64(t.timestamp * 1000.0))
        }
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) { sendTouches(touches, phase: 0) }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { sendTouches(touches, phase: 1) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) { sendTouches(touches, phase: 2) }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) { sendTouches(touches, phase: 3) }

    func insertText(_ text: String) {
        guard let app else { return }
        var bytes = Array(text.utf8)
        bytes.withUnsafeMutableBufferPointer { buf in
            _ = craftui_ios_app_insert_text(app, buf.baseAddress, buf.count)
        }
    }

    func deleteBackward() {
        guard let app else { return }
        _ = craftui_ios_app_delete_backward(app)
    }

    deinit {
        pathMonitor.cancel()
        displayLink?.invalidate()
        NotificationCenter.default.removeObserver(self)
        if let app { craftui_ios_app_destroy(app) }
    }
}
"#;

    let mut extra_plist = String::new();
    for (key, desc) in effective_manifest.ios_info_plist_usage_entries() {
        use std::fmt::Write as _;
        let _ = writeln!(
            extra_plist,
            "  <key>{}</key>\n  <string>{}</string>",
            xml_escape(&key),
            xml_escape(&desc)
        );
    }
    let bg_modes = effective_manifest.ios_background_modes();
    if !bg_modes.is_empty() {
        use std::fmt::Write as _;
        let _ = writeln!(extra_plist, "  <key>UIBackgroundModes</key>\n  <array>");
        for m in bg_modes {
            let _ = writeln!(extra_plist, "    <string>{}</string>", xml_escape(&m));
        }
        let _ = writeln!(extra_plist, "  </array>");
    }

    let info_plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>{}</string>
  <key>CFBundleIdentifier</key>
  <string>{}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>{}</string>
  <key>MinimumOSVersion</key>
  <string>{}</string>
  <key>UIRequiredDeviceCapabilities</key>
  <array><string>arm64</string><string>metal</string></array>
{}</dict>
</plist>
"#,
        xml_escape(&effective_manifest.app.name),
        xml_escape(&effective_manifest.app.id),
        xml_escape(&effective_manifest.app.version),
        xml_escape(&effective_manifest.ios.deployment_target),
        extra_plist
    );

    let readme = format!(
        "# {} (iOS UIKit + Metal + Native Permissions Bridge)\n\n1. On macOS with Xcode installed, build the static library:\n   ```sh\n   cargo build --target aarch64-apple-ios --release -p {rust_lib_name}\n   ```\n2. Add `CraftUI.h` (Bridging Header), `CraftUIPermissions.swift`, `CraftUIViewController.swift`, `Info.plist`, and link `lib{rust_lib_name}.a` + `Metal.framework`, `QuartzCore.framework`, `UIKit.framework`, `Network.framework`.\n",
        effective_manifest.app.name
    );

    std::fs::write(out_dir.join("CraftUI.h"), header)
        .and_then(|_| std::fs::write(out_dir.join("CraftUIPermissions.swift"), permissions_swift))
        .and_then(|_| std::fs::write(out_dir.join("CraftUIViewController.swift"), swift))
        .and_then(|_| std::fs::write(out_dir.join("Info.plist"), info_plist))
        .and_then(|_| std::fs::write(out_dir.join("README.md"), readme))
        .map_err(|e| CraftError::Platform(format!("failed writing ios template: {e}")))?;
    Ok(())
}

fn find_built_artifact(
    target_triple: &str,
    release: bool,
    pkg: &str,
    ext: &str,
) -> Option<PathBuf> {
    let profile = if release { "release" } else { "debug" };
    let stem = pkg.replace('-', "_");
    let direct = PathBuf::from("target")
        .join(target_triple)
        .join(profile)
        .join(if ext == "so" {
            format!("lib{stem}.so")
        } else {
            format!("{stem}.{ext}")
        });
    if direct.is_file() {
        return Some(direct);
    }
    None
}

fn run_build(
    platform: PlatformTarget,
    release: bool,
    package: Option<&str>,
    out_override: Option<&Path>,
    skip_cargo: bool,
) -> Result<()> {
    let profile_dir = |default_sub: &str| {
        out_override
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("dist").join(default_sub))
    };
    let mut manifest_opt = None;
    if let Some(pkg) = package {
        let candidate1 = Path::new("examples").join(pkg).join("craftui.toml");
        let candidate2 = Path::new("examples")
            .join(pkg.trim_start_matches("craftui-example-"))
            .join("craftui.toml");
        let candidate3 = Path::new("packages").join(pkg).join("craftui.toml");
        for cand in [candidate1, candidate2, candidate3] {
            if cand.is_file() {
                manifest_opt = Some(AppManifest::load_file(&cand)?);
                break;
            }
        }
    }
    if manifest_opt.is_none() {
        manifest_opt = load_active_manifest(Path::new("."))?;
    }
    let app_display = manifest_opt
        .as_ref()
        .map(|m| m.app.name.clone())
        .or_else(|| package.map(str::to_string))
        .unwrap_or_else(|| "craftui_app".to_string());

    match platform {
        PlatformTarget::Desktop => {
            if !skip_cargo {
                let mut cmd = Command::new("cargo");
                cmd.arg("build");
                if let Some(pkg) = package {
                    cmd.args(["-p", pkg]);
                }
                if release {
                    cmd.arg("--release");
                }
                let status = cmd
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "cargo build exited with status {status}"
                    )));
                }
            }
            println!("Desktop build finished.");
        }
        PlatformTarget::Web => {
            let pkg = package.unwrap_or("craftui-platform-web");
            if !skip_cargo {
                let mut cmd = Command::new("cargo");
                cmd.args(["build", "--target", "wasm32-unknown-unknown", "-p", pkg]);
                if release {
                    cmd.arg("--release");
                }
                if pkg != "craftui-platform-web" {
                    cmd.arg("--no-default-features");
                }
                let status = cmd
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "cargo wasm32 build exited with status {status}"
                    )));
                }
            }
            let out_dir = profile_dir("web");
            let wasm = find_built_artifact("wasm32-unknown-unknown", release, pkg, "wasm");
            emit_web_bundle(&out_dir, &app_display, wasm.as_deref())?;
            println!("Packaged CraftUI Web bundle at {}", out_dir.display());
        }
        PlatformTarget::Android => {
            let pkg = package.unwrap_or("craftui-platform-android");
            if !skip_cargo {
                let mut cmd = Command::new("cargo");
                cmd.args(["build", "--target", "aarch64-linux-android", "-p", pkg]);
                if release {
                    cmd.arg("--release");
                }
                if pkg != "craftui-platform-android" {
                    cmd.arg("--no-default-features");
                } else {
                    cmd.args(["--features", "builtin-main"]);
                }
                if let Some(ndk) = discover_android_ndk() {
                    let _ = configure_android_ndk_env(&mut cmd, &ndk);
                }
                let status = cmd
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "cargo android build exited with status {status}"
                    )));
                }
            }
            let out_dir = profile_dir("android");
            let lib_name = pkg.replace('-', "_");
            let so = find_built_artifact("aarch64-linux-android", release, pkg, "so");
            emit_android_template_with_manifest(
                &out_dir,
                &app_display,
                &lib_name,
                so.as_deref(),
                manifest_opt.as_ref(),
            )?;
            println!(
                "Packaged CraftUI Android Gradle project at {}",
                out_dir.display()
            );
        }
        PlatformTarget::Ios => {
            let pkg = package.unwrap_or("craftui-platform-ios");
            if !skip_cargo {
                if !cfg!(target_os = "macos") {
                    return Err(CraftError::Platform(
                        "iOS native compilation (`aarch64-apple-ios`) requires macOS + Xcode; pass `--skip-cargo` to emit the Swift/UIKit/Metal template bundle on Linux/Windows".to_string(),
                    ));
                }
                let mut cmd = Command::new("cargo");
                cmd.args(["build", "--target", "aarch64-apple-ios", "-p", pkg]);
                if release {
                    cmd.arg("--release");
                }
                let status = cmd
                    .status()
                    .map_err(|e| CraftError::Platform(format!("failed spawning cargo: {e}")))?;
                if !status.success() {
                    return Err(CraftError::Platform(format!(
                        "cargo iOS build exited with status {status}"
                    )));
                }
            }
            let out_dir = profile_dir("ios");
            let lib_name = pkg.replace('-', "_");
            emit_ios_template_with_manifest(
                &out_dir,
                &app_display,
                &lib_name,
                manifest_opt.as_ref(),
            )?;
            println!(
                "Packaged CraftUI iOS Xcode bridge bundle at {}",
                out_dir.display()
            );
        }
    }
    let _ = default_usage_description;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_reactive::reset_runtime;

    #[test]
    fn scaffolds_all_templates_init_permissions_and_cross_platform_bundles() {
        reset_runtime();
        let base_tmp = std::env::temp_dir().join(format!(
            "craftui_cli_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        for (tmpl, sub) in [
            (ProjectTemplate::Counter, "counter_demo"),
            (ProjectTemplate::Dashboard, "dash_demo"),
            (ProjectTemplate::Mobile, "mobile_demo"),
        ] {
            let dir = base_tmp.join(sub);
            scaffold_project_with_template(sub, &dir, tmpl).unwrap();
            let cui_path = dir.join("ui/main.cui");
            assert!(cui_path.exists());
            assert!(dir.join("craftui.toml").exists());

            let stats = check_cui_paths(&cui_path, false).unwrap();
            assert_eq!(
                stats,
                CheckStats {
                    files: 1,
                    warnings: 0,
                    errors: 0
                },
                "template {sub} produced .cui diagnostics"
            );

            let loaded_manifest = AppManifest::load_file(&dir.join("craftui.toml")).unwrap();
            assert!(loaded_manifest.validate().iter().all(|d| !d.is_error));
        }

        // Test `init` manifest creation + no-overwrite guard
        let existing_dir = base_tmp.join("existing_crate");
        std::fs::create_dir_all(&existing_dir).unwrap();
        std::fs::write(
            existing_dir.join("Cargo.toml"),
            "[package]\nname = \"acme-wallet\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let inited = init_manifest_in_dir(&existing_dir, None, None, false).unwrap();
        let m_inited = AppManifest::load_file(&inited).unwrap();
        assert_eq!(m_inited.app.name, "acme-wallet");
        assert_eq!(m_inited.app.id, "dev.craftui.acme_wallet");
        assert!(init_manifest_in_dir(&existing_dir, None, None, false).is_err());

        // Test mobile manifest-driven Android + iOS codegen
        let mobile_dir = base_tmp.join("mobile_demo");
        let mobile_manifest = AppManifest::load_file(&mobile_dir.join("craftui.toml")).unwrap();

        let android_out = mobile_dir.join("dist/android");
        emit_android_template_with_manifest(
            &android_out,
            "mobile_demo",
            "mobile_demo",
            None,
            Some(&mobile_manifest),
        )
        .unwrap();
        let manifest_xml =
            std::fs::read_to_string(android_out.join("app/src/main/AndroidManifest.xml")).unwrap();
        assert!(manifest_xml.contains("android.permission.CAMERA"));
        assert!(manifest_xml.contains("android.permission.POST_NOTIFICATIONS"));
        assert!(manifest_xml.contains(
            "<uses-feature android:name=\"android.hardware.camera\" android:required=\"false\" />"
        ));
        let act = std::fs::read_to_string(
            android_out.join("app/src/main/kotlin/dev/craftui/app/MainActivity.kt"),
        )
        .unwrap();
        assert!(act.contains("RequestMultiplePermissions"));
        assert!(act.contains("OpenMultipleDocuments"));
        assert!(act.contains("queryNetworkClassification"));

        let ios_out = mobile_dir.join("dist/ios");
        emit_ios_template_with_manifest(
            &ios_out,
            "mobile_demo",
            "mobile_demo",
            Some(&mobile_manifest),
        )
        .unwrap();
        let hdr = std::fs::read_to_string(ios_out.join("CraftUI.h")).unwrap();
        let vc = std::fs::read_to_string(ios_out.join("CraftUIViewController.swift")).unwrap();
        let perms_swift =
            std::fs::read_to_string(ios_out.join("CraftUIPermissions.swift")).unwrap();
        let plist = std::fs::read_to_string(ios_out.join("Info.plist")).unwrap();
        assert!(hdr.contains("craftui_ios_app_poll_command_json"));
        assert!(hdr.contains("craftui_ios_app_deliver_permission_result"));
        assert!(vc.contains("NWPathMonitor") && vc.contains("UIImpactFeedbackGenerator"));
        assert!(perms_swift.contains("CLLocationManager") && perms_swift.contains("LAContext"));
        assert!(plist.contains("NSCameraUsageDescription"));
        assert!(plist.contains("NSFaceIDUsageDescription"));
        assert!(plist.contains("remote-notification"));

        // Test Web bundle & static MIME server helper (WASM + traversal protection)
        let web_out = mobile_dir.join("dist/web");
        emit_web_bundle(&web_out, "mobile_demo", None).unwrap();
        std::fs::write(web_out.join("app.wasm"), b"\0asm\x01\x00\x00\x00").unwrap();
        let ok_html = String::from_utf8_lossy(&build_http_response(&web_out, "/")).into_owned();
        assert!(ok_html.starts_with("HTTP/1.1 200 OK") && ok_html.contains("text/html"));
        let ok_wasm =
            String::from_utf8_lossy(&build_http_response(&web_out, "/app.wasm?v=1")).into_owned();
        assert!(ok_wasm.contains("application/wasm"));
        let blocked =
            String::from_utf8_lossy(&build_http_response(&web_out, "/../Cargo.toml")).into_owned();
        assert!(blocked.starts_with("HTTP/1.1 403 Forbidden"));

        // Test permissions JSON lists all 35 permissions cleanly
        let perms_json = format_permissions_list_json();
        assert!(perms_json.contains("\"camera\"") && perms_json.contains("\"clipboard_read\""));

        // Test rendering + doctor on mobile project
        let png_out = mobile_dir.join("preview.png");
        let mobile_src = std::fs::read_to_string(mobile_dir.join("ui/main.cui")).unwrap();
        render_cui_to_png(&mobile_src, Some("MainView"), 360, 640, &png_out).unwrap();
        assert!(std::fs::metadata(&png_out).unwrap().len() > 100);

        let doctor = collect_doctor_report(&mobile_dir).unwrap();
        assert!(doctor.manifest_found);
        assert_eq!(doctor.manifest_permissions, 6);
        assert!(format_doctor_json(&doctor).contains("\"manifest\""));

        let _ = std::fs::remove_dir_all(&base_tmp);
    }
}
