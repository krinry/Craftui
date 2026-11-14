mod dev;
mod doctor;
mod manifest;
mod permissions;
mod platform;
mod remote_server;
mod templates;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use clap::{Parser, Subcommand, ValueEnum};
use craftui_core::{CraftError, Result};
use notify::{RecursiveMode, Watcher};
use crate::dev::{check_cui_paths, render_cui_file_to_png, run_dev_watcher};
use crate::doctor::{run_doctor, run_setup};
use crate::permissions::{run_permissions_check, run_permissions_list};
use crate::platform::build::{run_app_target, run_build};
use crate::templates::{init_manifest_in_dir, scaffold_project_with_template, template_label};

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

