#![cfg(feature = "hot-reload")]

use craftui_core::{BoxShadow, Brush, Color, CornerRadii, Point, Rect, ThemeMode};
use craftui_render::{PaintCommand, PaintScene};
use craftui_runtime::HotReloadSession;
use craftui_text::TextStyle;
use craftui_widgets::WidgetTree;
use notify::Event as NotifyEvent;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(crate) struct HotReloadSpec {
    pub(crate) path: PathBuf,
    pub(crate) component: Option<String>,
    pub(crate) session: HotReloadSession,
    pub(crate) show_hud: bool,
    pub(crate) show_inspector: bool,
    pub(crate) device_mode: bool,
    pub(crate) last_reload_error: Option<String>,
    pub(crate) frame_count: u32,
    pub(crate) fps_sample_time: Instant,
    pub(crate) current_fps: f32,
    pub(crate) _watcher: Option<notify::RecommendedWatcher>,
}

/// Determine whether a `notify` filesystem event should trigger `.cui` hot-reload.
///
/// Ignores read/open `Access` events emitted when CraftUI reads `.cui` files on Linux `inotify`.
pub fn should_trigger_hot_reload(event: &NotifyEvent) -> bool {
    let is_mutation = matches!(
        event.kind,
        notify::EventKind::Modify(_) | notify::EventKind::Create(_) | notify::EventKind::Remove(_)
    );
    if !is_mutation {
        return false;
    }
    event.paths.is_empty()
        || event
            .paths
            .iter()
            .any(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cui")))
}

pub(crate) fn paint_live_preview_overlays(
    scene: &mut PaintScene,
    tree: &WidgetTree,
    cursor_pos: Point,
    spec: &mut HotReloadSpec,
) {
    // 1. Frame counter & FPS sampling
    spec.frame_count += 1;
    let elapsed = spec.fps_sample_time.elapsed();
    if elapsed >= Duration::from_millis(500) {
        spec.current_fps = spec.frame_count as f32 / elapsed.as_secs_f32();
        spec.frame_count = 0;
        spec.fps_sample_time = Instant::now();
    }

    let win_w = tree.metrics.logical_size.width;
    let win_h = tree.metrics.logical_size.height;

    // 2. Device frame (Android phone bezel)
    if spec.device_mode && win_w >= 412.0 {
        let frame_x = (win_w - 412.0) / 2.0;
        let frame_y = ((win_h - 915.0).max(0.0)) / 2.0;
        let chassis_rect = Rect::new(frame_x - 8.0, frame_y - 8.0, 412.0 + 16.0, 915.0 + 16.0);
        scene.stroke_rect(
            chassis_rect,
            Color::rgb(71, 85, 105),
            4.0,
            CornerRadii::all(36.0),
        );
        // Punch hole camera
        scene.fill_rect(
            Rect::new(frame_x + 206.0 - 5.0, frame_y + 12.0, 10.0, 10.0),
            Brush::Solid(Color::BLACK),
            CornerRadii::all(5.0),
        );
        // Gesture pill at bottom
        scene.fill_rect(
            Rect::new(frame_x + 206.0 - 40.0, frame_y + 915.0 - 12.0, 80.0, 4.0),
            Brush::Solid(Color::rgba(255, 255, 255, 102)),
            CornerRadii::all(2.0),
        );
    }

    // 3. Live Element Inspector
    if spec.show_inspector
        && let Some(nid) = tree.hit_test(cursor_pos)
    {
        let rect = tree.rect(nid);
        if rect.width > 0.0 && rect.height > 0.0 {
            scene.stroke_rect(rect, Color::rgb(6, 182, 212), 2.0, CornerRadii::all(4.0));
            if let Some(node) = tree.nodes.get(nid) {
                let tag_text = format!("{}  {:.0}×{:.0}", node.debug_name, rect.width, rect.height);
                let tag_w = (tag_text.len() as f32) * 7.0 + 14.0;
                let tag_y = (rect.y - 22.0).max(4.0);
                let tag_rect = Rect::new(rect.x, tag_y, tag_w, 20.0);
                scene.fill_rect(
                    tag_rect,
                    Brush::Solid(Color::rgba(15, 23, 42, 242)),
                    CornerRadii::all(4.0),
                );
                scene.stroke_rect(
                    tag_rect,
                    Color::rgb(6, 182, 212),
                    1.0,
                    CornerRadii::all(4.0),
                );
                scene.commands.push(PaintCommand::DrawText {
                    origin: Point::new(rect.x + 6.0, tag_y + 4.0),
                    text: tag_text,
                    style: TextStyle {
                        font_size: 11.0,
                        color: Color::WHITE,
                        ..Default::default()
                    },
                    max_width: None,
                });
            }
        }
    }

    // 4. Live Diagnostic Banner (bottom of window)
    if let Some(ref err) = spec.last_reload_error {
        let banner_rect = Rect::new(
            16.0,
            (win_h - 56.0).max(10.0),
            (win_w - 32.0).max(100.0),
            44.0,
        );
        scene.drop_shadow(
            banner_rect,
            CornerRadii::all(8.0),
            BoxShadow {
                blur_radius: 12.0,
                color: Color::rgba(0, 0, 0, 153),
                offset_y: 4.0,
                ..Default::default()
            },
        );
        scene.fill_rect(
            banner_rect,
            Brush::Solid(Color::rgba(127, 29, 29, 245)),
            CornerRadii::all(8.0),
        );
        scene.stroke_rect(
            banner_rect,
            Color::rgb(239, 68, 68),
            1.5,
            CornerRadii::all(8.0),
        );
        let err_short = if err.chars().count() > 100 {
            format!("{}...", err.chars().take(100).collect::<String>())
        } else {
            err.clone()
        };
        let err_text = format!("⚠ Live Error: {err_short}");
        scene.commands.push(PaintCommand::DrawText {
            origin: Point::new(banner_rect.x + 12.0, banner_rect.y + 14.0),
            text: err_text,
            style: TextStyle {
                font_size: 12.0,
                color: Color::WHITE,
                ..Default::default()
            },
            max_width: Some(banner_rect.width - 24.0),
        });
    }

    // 5. Telemetry HUD Pill (top-right of window)
    if spec.show_hud {
        let hud_w = 340.0f32;
        let hud_x = (win_w - hud_w - 16.0).max(10.0);
        let hud_rect = Rect::new(hud_x, 12.0, hud_w, 28.0);
        scene.drop_shadow(
            hud_rect,
            CornerRadii::all(14.0),
            BoxShadow {
                blur_radius: 8.0,
                color: Color::rgba(0, 0, 0, 102),
                offset_y: 2.0,
                ..Default::default()
            },
        );
        scene.fill_rect(
            hud_rect,
            Brush::Solid(Color::rgba(15, 23, 42, 224)),
            CornerRadii::all(14.0),
        );
        scene.stroke_rect(
            hud_rect,
            Color::rgba(255, 255, 255, 38),
            1.0,
            CornerRadii::all(14.0),
        );

        let dot_color = if spec.last_reload_error.is_some() {
            Color::rgb(239, 68, 68)
        } else {
            Color::rgb(34, 197, 94)
        };
        scene.fill_rect(
            Rect::new(hud_x + 10.0, 22.0, 8.0, 8.0),
            Brush::Solid(dot_color),
            CornerRadii::all(4.0),
        );

        let reload_ms = spec.session.last_reload_duration.as_secs_f32() * 1000.0;
        let theme_str = match tree.theme().mode {
            ThemeMode::Amoled => "AMOLED",
            ThemeMode::Dark => "Dark",
            ThemeMode::Light => "Light",
            _ => "Custom",
        };
        let hud_text = format!(
            "LIVE {:.1}ms | {:.0} FPS | {} | [F1-F3]",
            reload_ms, spec.current_fps, theme_str
        );
        scene.commands.push(PaintCommand::DrawText {
            origin: Point::new(hud_x + 24.0, 18.0),
            text: hud_text,
            style: TextStyle {
                font_size: 11.0,
                color: Color::rgb(241, 245, 249),
                ..Default::default()
            },
            max_width: None,
        });
    }
}
