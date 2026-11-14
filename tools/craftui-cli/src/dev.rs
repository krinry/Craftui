use std::path::{Path, PathBuf};
use std::time::Instant;
use std::sync::mpsc;
use notify::{Event, RecursiveMode, Watcher};

use craftui_core::{CraftError, Result, Size, WindowMetrics};
use craftui_dsl::{Severity, generate_rust, resolve_file};
use craftui_reactive::Scope;
use craftui_render::{BackendKind, PaintScene, TextureRegistry, render_scene_to_rgba, write_rgba_png};
use craftui_runtime::HotReloadSession;
use craftui_widgets::WidgetTree;

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

#[allow(dead_code)]
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

pub fn run_dev_watcher(entry: &Path, dir: &Path) -> Result<()> {
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

