//! Unified public facade crate for the CraftUI pure-Rust declarative UI framework.
//!
//! Typical applications import `use craftui::prelude::*;` and use either `cui!` + `#[component]`
//! in pure Rust or `.cui` templates compiled AOT via `build.rs` and hot-reloaded in dev.

#[cfg(test)]
extern crate self as craftui;

pub use craftui_a11y as a11y;
pub use craftui_core as core;
#[cfg(feature = "dsl")]
pub use craftui_dsl as dsl;
pub use craftui_layout as layout;
pub use craftui_macros::{component, cui};
pub use craftui_reactive as reactive;
pub use craftui_render as render;
#[cfg(feature = "hot-reload")]
pub use craftui_runtime as runtime;
pub use craftui_text as text;
pub use craftui_widgets as widgets;

#[cfg(feature = "desktop")]
pub use craftui_platform_desktop as platform_desktop;

#[cfg(feature = "android")]
pub use craftui_platform_android as platform_android;

#[cfg(feature = "ios")]
pub use craftui_platform_ios as platform_ios;

#[cfg(feature = "web")]
pub use craftui_platform_web as platform_web;

/// Numeric conversion helper enabling ergonomic `pct(100)`, `dp(16)`, `px(8)` in `cui!` calls.
pub trait Scalar: Copy {
    fn into_f32(self) -> f32;
}

impl Scalar for f32 {
    #[inline]
    fn into_f32(self) -> f32 {
        self
    }
}
impl Scalar for f64 {
    #[inline]
    fn into_f32(self) -> f32 {
        self as f32
    }
}
impl Scalar for i32 {
    #[inline]
    fn into_f32(self) -> f32 {
        self as f32
    }
}
impl Scalar for u32 {
    #[inline]
    fn into_f32(self) -> f32 {
        self as f32
    }
}
impl Scalar for usize {
    #[inline]
    fn into_f32(self) -> f32 {
        self as f32
    }
}

#[inline]
pub fn dp(v: impl Scalar) -> craftui_core::Length {
    craftui_core::Length::Dp(v.into_f32())
}

#[inline]
pub fn px(v: impl Scalar) -> craftui_core::Length {
    craftui_core::Length::Px(v.into_f32())
}

#[inline]
pub fn pct(v: impl Scalar) -> craftui_core::Length {
    craftui_core::Length::Pct(v.into_f32())
}

#[inline]
pub const fn auto() -> craftui_core::Length {
    craftui_core::Length::Auto
}

pub mod prelude {
    pub use crate::{auto, dp, pct, px};
    pub use craftui_a11y::{A11yMetadata, A11yRole};
    pub use craftui_core::{
        Alignment, AndroidPermissionSpec, AppLifecycleEvent, Axis, BatteryState, BlendMode, Border,
        BoxShadow, Breakpoint, Brush, CRAFTUI_TAURI_IPC_SCRIPT, ClipboardBackend, Color,
        Constraints, CornerRadii, CursorIcon, DeviceCapabilitiesBackend, DeviceInfo, Direction,
        DirtyFlags, FileDialogBackend, FileDialogRequest, FileFilter, Fit, HapticStyle,
        HapticsBackend, ImeEvent, Insets, IpcMessage, IpcResponse, IpcRouter, Justify, KeyCode,
        KeyEvent, Length, M3Elevation, Material3Tokens, MemorySecureStorage,
        MockDeviceCapabilities, MockHaptics, MockPermissionBackend, MockShare, Modifiers,
        NetworkStatus, NodeId, NoopHaptics, NoopPermissionBackend, NoopShare, Permission,
        PermissionBackend, PermissionOutcome, PermissionStatus, PlatformServices, PlatformViewId,
        Point, PointerButton, PointerEvent, PointerId, PointerPhase, Rect, ScreenKind, ScrollDelta,
        SecureStorageBackend, ShareBackend, SharePayload, Size, TailwindTokens, TextureId,
        ThemeMode, ThemeOverride, ThemeTokens, Transform2D, UiDispatcher, UiEvent, WebViewSource,
        WindowMetrics, monotonic_ms,
    };
    pub use craftui_layout::{LayoutMode, LayoutStyle};
    pub use craftui_macros::{component, cui};
    pub use craftui_reactive::{
        Computed, Effect, ReadSignal, Scope, Signal, TaskHandle, batch, next_timer_delay_ms,
        tick_timers, tick_timers_at, untrack,
    };
    pub use craftui_render::{
        BackendKind, PaintCommand, PaintScene, PixelBufferHandle, TextureHandle, TextureRegistry,
        kurbo, render_scene_to_rgba,
    };
    pub use craftui_text::{
        RichText, TextAlign, TextEditorState, TextOverflow, TextSpan, TextStyle,
    };
    pub use craftui_widgets::{
        AnimatableProp, AnimatedColor, AnimatedFloat, BoolCallback, BoxStyle, CanvasPaintFn,
        CanvasPointerFn, CustomWidget, Easing, FloatCallback, GestureArena, HoverCallback,
        IndexCallback, InspectorState, PinchRotateCallback, ReactiveValue, RecognizedGesture,
        Router, RouterTransition, ScrollPhysics, Spring, TapHandler, TextCallback,
        ToastDismissCallback, ToastItem, ToastManager, ToastSeverity, TransitionCurve, Tween, View,
        ViewFn, VirtualItemFactory, WidgetKind, WidgetTree,
    };

    #[cfg(all(feature = "desktop", feature = "hot-reload"))]
    pub use craftui_platform_desktop::run_hot_reload_cui;
    #[cfg(feature = "desktop")]
    pub use craftui_platform_desktop::{
        AppContext, DesktopClipboard, DesktopFileDialog, WindowConfig, WindowLauncher, run_app,
    };
}

#[cfg(test)]
mod tests {
    use super::prelude::*;
    use craftui_reactive::reset_runtime;
    use std::cell::Cell;
    use std::rc::Rc;

    #[component]
    fn sample_panel(cx: Scope, count: Signal<i32>) -> impl View {
        let name = cx.signal("World".to_string());
        let active_tab = cx.signal(0usize);
        let agreed = cx.signal(true);
        let volume = cx.signal(0.4f32);
        let choice = cx.signal(1usize);
        let notes = cx.signal("Line 1".to_string());
        let modal_open = cx.signal(false);

        cui! {
            Column(
                width: pct(100),
                height: pct(100),
                bg: theme.background,
                border_width: 1,
                border_color: theme.border,
                padding: 16,
                gap: 10
            ) {
                Row(width: pct(100), align: Alignment::Center, gap: 8) {
                    Icon(svg: "check", size: 20, color: theme.primary)
                    Text(text: move || format!("Count = {}", count.get()), size: 20, color: theme.text_primary)
                    Tooltip(text: "Interactive count badge")
                }
                Divider(axis: Axis::Horizontal, thickness: 1, color: theme.border)
                Tabs(tabs: vec!["General".into(), "Preview".into()], active: active_tab)
                Row(width: pct(100), gap: 8) {
                    TextInput(placeholder: "Name", value: name)
                    Button(
                        label: "Add",
                        bg: theme.primary,
                        fg: theme.on_primary,
                        on_tap: move || count.update(|v| *v += 1)
                    )
                }
                TextArea(placeholder: "Notes", value: notes)
                Row(width: pct(100), gap: 12) {
                    Checkbox(checked: agreed, label: "Agree")
                    Switch(checked: agreed, label: "Notify")
                    Select(options: vec!["Alpha".into(), "Beta".into()], selected: choice)
                }
                RadioGroup(options: vec!["Compact".into(), "Comfortable".into()], selected: choice)
                Slider(value: volume, min: 0.0, max: 1.0, label: "Volume")
                ProgressIndicator(progress: Some(0.65))
                Modal(open: modal_open, on_close: move || modal_open.set(false)) {
                    Text(text: "Inside Modal", color: theme.text_primary)
                }
            }
        }
    }

    #[test]
    fn facade_cui_macro_supports_all_widgets_and_live_theme_switch() {
        reset_runtime();
        let cx = Scope::root();
        let count = cx.signal(0);
        let mut tree = WidgetTree::new(cx);
        tree.mount(cx, sample_panel(cx, count));

        let metrics = WindowMetrics {
            logical_size: Size::new(480.0, 640.0),
            scale_factor: 1.0,
            ..Default::default()
        };
        tree.layout(metrics);
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);

        let textures = TextureRegistry::new();
        let dark_rgba = render_scene_to_rgba(
            BackendKind::Auto,
            &scene,
            480,
            640,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(dark_rgba.len(), 480 * 640 * 4);
        assert!(dark_rgba.iter().any(|&b| b > 20));

        // Verify reactive `Theme.*` tokens in `cui!` respond live to ThemeMode + ThemeOverride changes
        tree.set_theme_mode(ThemeMode::Light);
        for _ in 0..240 {
            if !tree.tick_animations(0.016) {
                break;
            }
        }
        assert_eq!(tree.theme().mode, ThemeMode::Light);

        let mut light_scene = PaintScene::new();
        tree.paint(&mut light_scene);
        let light_rgba = render_scene_to_rgba(
            BackendKind::Auto,
            &light_scene,
            480,
            640,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_ne!(dark_rgba, light_rgba);

        // Verify Scope timer hooks cleanly fire when ticked
        let ticks = Rc::new(Cell::new(0usize));
        let t_c = ticks.clone();
        let handle = cx.interval_at(100, 50, move || t_c.set(t_c.get() + 1));
        tick_timers_at(150);
        tick_timers_at(200);
        assert_eq!(ticks.get(), 2);
        handle.cancel();
        tick_timers_at(300);
        assert_eq!(ticks.get(), 2);
    }

    #[derive(Clone, PartialEq)]
    struct RowItem {
        id: u64,
        label: String,
    }

    #[test]
    fn cui_macro_supports_phase3_if_else_keyed_for_two_way_transforms_glass_and_animate() {
        reset_runtime();
        let cx = Scope::root();
        let show_alt = cx.signal(false);
        let zoomed = cx.signal(false);
        let text_val = cx.signal("hello".to_string());
        let rows = cx.signal(vec![
            RowItem {
                id: 1,
                label: "Alpha".into(),
            },
            RowItem {
                id: 2,
                label: "Beta".into(),
            },
        ]);

        let view = cui! {
            Column(width: pct(100), height: pct(100), gap: 8) {
                Container(
                    id: "glass_card",
                    width: dp(200),
                    height: dp(80),
                    backdrop_blur: 14.0,
                    backdrop_tint: Color::rgba(15, 23, 42, 160),
                    blend_mode: BlendMode::Overlay,
                    cursor: CursorIcon::Pointer,
                    scale: move || if zoomed.get() { 1.12 } else { 1.0 },
                    opacity: move || if show_alt.get() { 0.85 } else { 1.0 }
                ) {
                    animate scale { curve: TransitionCurve::snappy() }
                    animate opacity { duration: 0.18, easing: EaseOut }
                    Text(text: "Card")
                }

                TextInput(id: "name_box", value <=> text_val, placeholder: "Type")

                if show_alt.get() {
                    Text(id: "branch_then", text: "ThenBranch")
                } else {
                    Text(id: "branch_else", text: "ElseBranch")
                }

                for (item, idx) in rows key: item.id {
                    Text(text: move || format!("{}:{}", idx.get(), item.get().label))
                }
            }
        };

        let mut tree = WidgetTree::new(cx);
        tree.mount(cx, view);
        let metrics = WindowMetrics {
            logical_size: Size::new(640.0, 480.0),
            ..Default::default()
        };
        tree.layout(metrics);

        // Initial snapshot checks: no spurious animations, glassmorphism/cursor/blend set, Else branch visible
        let card = tree
            .node_by_id("glass_card")
            .expect("glass_card registered");
        let card_node = tree.nodes.get(card).unwrap();
        assert!((card_node.box_style.backdrop_blur - 14.0).abs() < 0.01);
        assert_eq!(card_node.box_style.blend_mode, BlendMode::Overlay);
        assert_eq!(card_node.box_style.cursor, Some(CursorIcon::Pointer));
        assert!(tree.node_by_id("branch_else").is_some());
        assert!(tree.node_by_id("branch_then").is_none());

        // Trigger animate scale & if/else switch
        zoomed.set(true);
        show_alt.set(true);
        tree.layout(metrics);
        assert!(
            tree.has_active_animations(),
            "zoom/opacity transitions should activate"
        );
        assert!(tree.node_by_id("branch_then").is_some());
        for _ in 0..120 {
            if !tree.tick_animations(0.016) {
                break;
            }
        }
        assert!(!tree.has_active_animations());

        // Find the rendered key-1 and key-2 Text nodes and verify NodeId reuse across reorder + rename
        let text_nodes_before: Vec<(NodeId, String)> = tree
            .nodes
            .iter()
            .filter_map(|(id, n)| match &n.kind {
                WidgetKind::Text { content, .. } => {
                    let s = content.get();
                    if s.ends_with(":Alpha") || s.ends_with(":Beta") {
                        Some((id, s))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();
        assert_eq!(text_nodes_before.len(), 2);
        let beta_id = text_nodes_before
            .iter()
            .find(|(_, s)| s == "1:Beta")
            .unwrap()
            .0;

        rows.set(vec![
            RowItem {
                id: 2,
                label: "BetaUpdated".into(),
            },
            RowItem {
                id: 1,
                label: "Alpha".into(),
            },
        ]);
        tree.layout(metrics);

        match &tree.nodes.get(beta_id).unwrap().kind {
            WidgetKind::Text { content, .. } => assert_eq!(content.get(), "0:BetaUpdated"),
            _ => panic!("expected Text node reuse"),
        }
    }

    #[test]
    fn cui_macro_supports_phase4_spinner_badge_split_pane_and_toast_host() {
        reset_runtime();
        let cx = Scope::root();
        let ratio = cx.signal(0.35f32);
        let toasts = ToastManager::new(cx);
        let toast_id = toasts.push_at(0, "Saved", "Layout ready", ToastSeverity::Success, 0);

        let view = cui! {
            Stack(width: pct(100), height: pct(100)) {
                SplitPane(
                    id: "split",
                    direction: Axis::Horizontal,
                    ratio <=> ratio,
                    min_first: 80.0,
                    min_second: 80.0,
                    divider_size: 10.0
                ) {
                    Column(id: "pane_a", padding: 12, gap: 8) {
                        Spinner(id: "spin", size: 28, thickness: 3.5, color: theme.primary)
                        Row {
                            Badge(
                                id: "badge",
                                label: "Production",
                                icon: "check",
                                bg: theme.primary,
                                fg: theme.on_primary
                            )
                        }
                    }
                    Column(id: "pane_b", padding: 12) {
                        Text(text: "Details")
                    }
                }
                ToastHost(id: "toasts", manager: toasts)
            }
        };

        let mut tree = WidgetTree::new(cx);
        tree.mount(cx, view);
        tree.layout(WindowMetrics {
            logical_size: Size::new(500.0, 320.0),
            ..Default::default()
        });

        let badge = tree.node_by_id("badge").unwrap();
        assert!(
            tree.nodes
                .get(badge)
                .unwrap()
                .box_style
                .background
                .is_none()
        );
        let pane_a = tree.rect(tree.node_by_id("pane_a").unwrap());
        let pane_b = tree.rect(tree.node_by_id("pane_b").unwrap());
        assert!(pane_a.width < pane_b.width);
        assert_eq!(toasts.len(), 1);

        // Dismiss via click on ToastHost card close button (500x320 -> card y=240..304, close center=(465, 259))
        let close_pt = Point::new(465.0, 259.0);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(0),
            phase: PointerPhase::Down,
            position: close_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 0,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(0),
            phase: PointerPhase::Up,
            position: close_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 15,
        }));
        assert!(toasts.is_empty());
        assert!(!toasts.dismiss(toast_id));
    }

    #[test]
    fn prelude_exposes_mobile_permissions_and_capabilities() {
        let perms = MockPermissionBackend::new()
            .with_status(Permission::Photos, PermissionStatus::Limited)
            .with_rationale(Permission::Camera, true);
        let haptics = MockHaptics::new();
        let share = MockShare::new();
        let storage = MemorySecureStorage::new();
        let device = MockDeviceCapabilities::new()
            .with_network_status(NetworkStatus::Wifi)
            .with_battery_state(BatteryState {
                level_pct: Some(92),
                charging: false,
            })
            .with_device_info(DeviceInfo {
                os: "ios".to_string(),
                model: "iPhone 16".to_string(),
                os_version: "18.0".to_string(),
                sdk_int: None,
                app_version: "1.0.0".to_string(),
            });

        let services = PlatformServices::in_memory()
            .with_permissions(perms)
            .with_haptics(haptics)
            .with_share(share)
            .with_secure_storage(storage)
            .with_device_capabilities(device);

        let outcome = services.request_permissions(&[Permission::Camera, Permission::Photos]);
        assert_eq!(outcome.len(), 2);
        assert!(outcome[0].is_granted());
        assert!(outcome[0].should_show_rationale);
        assert_eq!(outcome[1].status, PermissionStatus::Limited);
        assert!(services.trigger_haptic(HapticStyle::Medium));
        assert!(services.share(&SharePayload::text("CraftUI")));
        assert!(services.set_secret("k", "v"));
        assert_eq!(services.get_secret("k").as_deref(), Some("v"));
        assert_eq!(services.network_status(), NetworkStatus::Wifi);
        assert_eq!(services.battery_state().level_pct, Some(92));
        assert_eq!(services.device_info().os, "ios");
        assert_ne!(AppLifecycleEvent::Resumed, AppLifecycleEvent::MemoryWarning);

        let _noop_perm: NoopPermissionBackend = NoopPermissionBackend;
        let _noop_haptics: NoopHaptics = NoopHaptics;
        let _noop_share: NoopShare = NoopShare;
        let _spec: Vec<AndroidPermissionSpec> = Permission::Camera.android_permissions();
        let _trait_check: (
            &dyn PermissionBackend,
            &dyn HapticsBackend,
            &dyn ShareBackend,
        ) = (&_noop_perm, &_noop_haptics, &_noop_share);
        let _sec: &dyn SecureStorageBackend = &MemorySecureStorage::new();
        let _dev: &dyn DeviceCapabilitiesBackend = &MockDeviceCapabilities::new();
        let _outcome: PermissionOutcome = outcome[0].clone();
    }

    #[test]
    fn cui_macro_supports_phase5_safe_area_segmented_accordion_and_data_table() {
        reset_runtime();
        let cx = Scope::root();
        let seg_idx = cx.signal(0usize);
        let acc_open = cx.signal(false);
        let tbl_sel = cx.signal(0usize);
        let sorted_col = Rc::new(Cell::new(None::<usize>));
        let sorted_c = sorted_col.clone();

        let view = cui! {
            SafeArea(id: "safe", padding: 8) {
                Column(id: "inner", width: pct(100), gap: 10) {
                    SegmentedControl(
                        id: "seg",
                        items: vec!["Permissions".into(), "Sensors".into()],
                        selected <=> seg_idx,
                        on_change: move |_i| {}
                    )
                    Accordion(
                        id: "acc",
                        title: "Manifest Permissions",
                        expanded <=> acc_open,
                        on_toggle: move |_exp| {}
                    ) {
                        Text(text: "CAMERA, LOCATION")
                    }
                    DataTable(
                        id: "tbl",
                        columns: vec!["Permission".into(), "Status".into()],
                        rows: vec![
                            vec!["Camera".into(), "Granted".into()],
                            vec!["Location".into(), "Denied".into()],
                        ],
                        selected <=> tbl_sel,
                        striped: true,
                        row_height: 36.0,
                        on_sort: move |col| sorted_c.set(Some(col))
                    )
                }
            }
        };

        let mut tree = WidgetTree::new(cx);
        tree.mount(cx, view);
        tree.layout(WindowMetrics {
            logical_size: Size::new(420.0, 760.0),
            safe_area: Insets::new(44.0, 0.0, 34.0, 0.0),
            ..Default::default()
        });
        let inner_r1 = tree.rect(tree.node_by_id("inner").unwrap());
        assert!((inner_r1.y - 52.0).abs() < 0.5);

        tree.layout(WindowMetrics {
            logical_size: Size::new(420.0, 760.0),
            safe_area: Insets::new(16.0, 8.0, 16.0, 8.0),
            ..Default::default()
        });
        let inner_r2 = tree.rect(tree.node_by_id("inner").unwrap());
        assert!((inner_r2.y - 24.0).abs() < 0.5);

        assert!(matches!(
            tree.nodes
                .get(tree.node_by_id("seg").unwrap())
                .unwrap()
                .kind,
            WidgetKind::SegmentedControl { .. }
        ));
        assert!(matches!(
            tree.nodes
                .get(tree.node_by_id("acc").unwrap())
                .unwrap()
                .kind,
            WidgetKind::Accordion { .. }
        ));
        let tbl_rect = tree.rect(tree.node_by_id("tbl").unwrap());
        // Click header cell 0 to fire on_sort
        let sort_pt = Point::new(tbl_rect.x + 24.0, tbl_rect.y + 18.0);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(0),
            phase: PointerPhase::Down,
            position: sort_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 0,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(0),
            phase: PointerPhase::Up,
            position: sort_pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        assert_eq!(sorted_col.get(), Some(0));
    }

    #[test]
    fn cui_macro_supports_hybrid_html_and_material3_theming() {
        let cx = Scope::root();

        let view = cui! {
            div(
                id: "hybrid_card",
                class: "flex flex-col p-4 gap-4 bg-slate-900 rounded-xl shadow-md w-full"
            ) {
                h1(
                    id: "main_heading",
                    text: "Hybrid Architecture",
                    class: "text-2xl font-bold text-white"
                )
                p(
                    id: "sub_heading",
                    text: "Tauri 2.0 bridge with pure Rust native rendering"
                )
                HtmlView(
                    id: "webview_layer",
                    src: "https://tauri.app"
                )
            }
        };

        let mut tree = WidgetTree::new(cx);
        tree.mount(cx, view);

        // Apply Material 3 theme generated from seed color
        let seed = Color::rgb(103, 80, 164);
        let m3_theme = ThemeTokens::material3(seed, ThemeMode::Dark);
        tree.set_theme(m3_theme);

        tree.layout(WindowMetrics {
            logical_size: Size::new(480.0, 800.0),
            ..Default::default()
        });

        let card_id = tree.node_by_id("hybrid_card").unwrap();
        let card = tree.nodes.get(card_id).unwrap();
        assert_eq!(card.layout.padding, Insets::all(16.0));
        assert_eq!(card.layout.gap_x, 16.0);
        assert_eq!(card.box_style.radius, CornerRadii::all(16.0));

        let web_id = tree.node_by_id("webview_layer").unwrap();
        let web = tree.nodes.get(web_id).unwrap();
        match &web.kind {
            WidgetKind::HtmlView { src, .. } => {
                assert_eq!(src.get(), "https://tauri.app");
            }
            _ => panic!("expected HtmlView widget"),
        }

        let mut scene = PaintScene::new();
        tree.paint(&mut scene);
        assert!(!scene.commands.is_empty());
    }
}
