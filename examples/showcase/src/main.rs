use craftui::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;

pub mod aot_ui {
    include!(concat!(env!("OUT_DIR"), "/showcase.rs"));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowcaseRoute {
    Gallery,
    TokenInspector,
}

pub struct AccentSwatchWidget {
    label: &'static str,
    accent: Color,
    clicks: Signal<usize>,
    on_select: Rc<dyn Fn(Color)>,
}

impl AccentSwatchWidget {
    pub fn new(
        label: &'static str,
        accent: Color,
        clicks: Signal<usize>,
        on_select: impl Fn(Color) + 'static,
    ) -> Self {
        Self {
            label,
            accent,
            clicks,
            on_select: Rc::new(on_select),
        }
    }
}

impl CustomWidget for AccentSwatchWidget {
    fn debug_name(&self) -> &'static str {
        "AccentSwatchWidget"
    }

    fn measure(
        &mut self,
        _constraints: Constraints,
        _text_engine: &mut craftui::text::TextEngine,
        _theme: &ThemeTokens,
    ) -> Size {
        Size::new(112.0, 32.0)
    }

    fn paint(
        &mut self,
        rect: Rect,
        scene: &mut PaintScene,
        _text_engine: &mut craftui::text::TextEngine,
        theme: &ThemeTokens,
        focused: bool,
    ) {
        scene.fill_rect(rect, theme.surface_variant, CornerRadii::all(8.0));
        scene.stroke_rect(
            rect,
            if focused { theme.primary } else { theme.border },
            if focused { 2.0 } else { 1.0 },
            CornerRadii::all(8.0),
        );
        let dot = Rect::new(rect.x + 8.0, rect.y + 8.0, 16.0, 16.0);
        scene.fill_rect(dot, self.accent, CornerRadii::all(8.0));
        scene.draw_text(
            Point::new(rect.x + 30.0, rect.y + 8.0),
            self.label,
            TextStyle::default()
                .with_size(12.5)
                .with_weight(600.0)
                .with_color(theme.text_primary),
            Some(rect.width - 36.0),
        );
    }

    fn handle_event(&mut self, rect: Rect, event: &UiEvent) -> bool {
        if let UiEvent::Pointer(pe) = event
            && matches!(pe.phase, PointerPhase::Up)
            && rect.contains(pe.position)
        {
            self.clicks.update(|c| *c += 1);
            (self.on_select)(self.accent);
            return true;
        }
        false
    }

    fn a11y(&self) -> A11yMetadata {
        A11yMetadata {
            role: A11yRole::Button,
            label: Some(format!("Apply {} swatch", self.label)),
            focusable: true,
            ..Default::default()
        }
    }
}

const MODES: [ThemeMode; 4] = [
    ThemeMode::Dark,
    ThemeMode::Light,
    ThemeMode::Amoled,
    ThemeMode::HighContrast,
];

fn apply_picked_file(
    picked: Option<PathBuf>,
    screen: &aot_ui::ShowcaseScreen,
    toast_mgr: ToastManager,
) {
    match picked {
        Some(path) => {
            let label = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| path.display().to_string());
            screen.last_action.set(format!("Selected file: {label}"));
            toast_mgr.push(
                "File Selected",
                format!("Picked {label}"),
                ToastSeverity::Info,
                4200,
            );
        }
        None => {
            screen.last_action.set("File dialog cancelled".to_string());
            toast_mgr.push(
                "File Dialog",
                "No file selected.",
                ToastSeverity::Warning,
                3200,
            );
        }
    }
}

fn mount_showcase_app_inner(
    cx: Scope,
    tree: &mut WidgetTree,
    launcher: Option<WindowLauncher>,
    services: PlatformServices,
    dispatcher: Option<UiDispatcher>,
) -> (
    NodeId,
    Router<ShowcaseRoute>,
    Signal<usize>,
    aot_ui::ShowcaseScreen,
    ToastManager,
) {
    tree.set_platform_services(services.clone());

    let theme_idx = cx.signal(0usize);
    let preset_idx = cx.signal(0usize);
    let is_rtl = cx.signal(false);
    let swatch_hits = cx.signal(0usize);
    let notify_count = cx.signal(0usize);
    let router = Router::new(cx, ShowcaseRoute::Gallery);
    let toast_mgr = ToastManager::new(cx);
    let theme_sig = tree.theme_signal;
    let ov_sig = tree.theme_override_signal;
    let dir_sig = tree.direction_signal;

    let screen = aot_ui::ShowcaseScreen::new(cx);

    cx.effect({
        let scr = screen.clone();
        move || {
            scr.toasts.set(toast_mgr.items());
        }
    });

    {
        let scr_theme = screen.clone();
        screen.set_on_theme_cycle(move || {
            let next = (theme_idx.get_untracked() + 1) % MODES.len();
            theme_idx.set(next);
            let mode = MODES[next];
            theme_sig.set(ThemeTokens::from_mode(mode).applied(&ov_sig.get_untracked()));
            scr_theme
                .last_action
                .set(format!("Theme switched to {mode:?}"));
        });
    }

    {
        let scr_notify = screen.clone();
        let th_notify = theme_sig;
        screen.set_on_notify_toast(move || {
            let mut idx = 0usize;
            notify_count.update(|n| {
                *n += 1;
                idx = *n;
            });
            let severity = match idx % 4 {
                1 => ToastSeverity::Info,
                2 => ToastSeverity::Success,
                3 => ToastSeverity::Warning,
                _ => ToastSeverity::Error,
            };
            let mode = th_notify.get_untracked().mode;
            let split_pct = (scr_notify.split_ratio.get_untracked() * 100.0).round();
            toast_mgr.push(
                format!("Showcase Alert #{idx}"),
                format!("Theme: {mode:?} | Split: {split_pct:.0}%"),
                severity,
                4500,
            );
            scr_notify
                .last_action
                .set(format!("Queued toast #{idx} ({severity:?})"));
        });
    }

    {
        let svc_copy = services.clone();
        let scr_copy = screen.clone();
        let th_copy = theme_sig;
        screen.set_on_copy_summary(move || {
            let tokens = th_copy.get_untracked();
            let summary = format!(
                "CraftUI Showcase | Theme={:?} | Primary={} | Tab={} | Split={:.0}% | Note={}",
                tokens.mode,
                tokens.primary.to_hex(),
                scr_copy.active_tab.get_untracked(),
                scr_copy.split_ratio.get_untracked() * 100.0,
                scr_copy.draft_note.get_untracked(),
            );
            if svc_copy.write_clipboard(&summary) {
                toast_mgr.push(
                    "Clipboard Updated",
                    "Copied showcase summary to clipboard.",
                    ToastSeverity::Success,
                    3600,
                );
                scr_copy
                    .last_action
                    .set("Copied summary to clipboard".to_string());
            } else {
                toast_mgr.push(
                    "Clipboard Unavailable",
                    "Clipboard backend rejected write.",
                    ToastSeverity::Warning,
                    4000,
                );
                scr_copy
                    .last_action
                    .set("Clipboard unavailable".to_string());
            }
        });
    }

    {
        let svc_file = services.clone();
        let scr_file = screen.clone();
        screen.set_on_open_file(move || {
            let req = FileDialogRequest::open("Open CraftUI Asset or .cui Screen").with_filter(
                FileFilter::new("CraftUI DSL & Assets", ["cui", "toml", "png", "json"]),
            );
            if let Some(ref disp) = dispatcher {
                let svc_bg = svc_file.clone();
                let scr_done = scr_file.clone();
                scr_file
                    .last_action
                    .set("Waiting for file selection...".to_string());
                cx.spawn_with_dispatcher(
                    disp,
                    move || svc_bg.pick_open_file(&req),
                    move |picked| apply_picked_file(picked, &scr_done, toast_mgr),
                );
            } else {
                let picked = svc_file.pick_open_file(&req);
                apply_picked_file(picked, &scr_file, toast_mgr);
            }
        });
    }

    {
        let scr_dismiss = screen.clone();
        screen.set_on_toast_dismiss(move |id| {
            if id > 0 {
                toast_mgr.dismiss(id as u64);
                scr_dismiss
                    .last_action
                    .set(format!("Dismissed toast #{id}"));
            }
        });
    }

    let mobile_perm_list = [
        Permission::Camera,
        Permission::LocationWhenInUse,
        Permission::Notifications,
        Permission::Biometrics,
        Permission::StorageRead,
        Permission::Vibrate,
    ];

    let update_perm_table = {
        let svc = services.clone();
        let scr = screen.clone();
        let perms = mobile_perm_list.clone();
        move || {
            let rows: Vec<Vec<String>> = perms
                .iter()
                .map(|p| {
                    let cap = match p {
                        Permission::Camera => "Camera",
                        Permission::LocationWhenInUse => "Location",
                        Permission::Notifications => "Notifications",
                        Permission::Biometrics => "Biometrics",
                        Permission::StorageRead => "Storage Access",
                        Permission::Vibrate => "Haptics & Vibration",
                        _ => "Custom",
                    };
                    let st = svc.check_permission(p);
                    vec![cap.to_string(), p.key().into_owned(), format!("{st:?}")]
                })
                .collect();
            scr.mobile_perm_rows.set(rows);
        }
    };
    update_perm_table();

    {
        let svc = services.clone();
        let scr = screen.clone();
        let update_fn = update_perm_table.clone();
        let perms = mobile_perm_list;
        screen.set_on_request_permission(move || {
            let sel = scr.perm_table_selected.get_untracked();
            let idx = (sel.max(0) as usize).min(perms.len().saturating_sub(1));
            let target = &perms[idx];
            let outcome = svc.request_permission(target);
            let key = target.key();
            scr.last_action
                .set(format!("Permission {key} -> {outcome:?}"));
            toast_mgr.push(
                "Permission Updated",
                format!("{key}: {outcome:?}"),
                if outcome.is_granted() {
                    ToastSeverity::Success
                } else {
                    ToastSeverity::Warning
                },
                3500,
            );
            update_fn();
        });
    }

    {
        let svc = services.clone();
        let scr = screen.clone();
        screen.set_on_trigger_haptic(move || {
            let ok = svc.trigger_haptic(HapticStyle::Medium);
            scr.last_action.set(format!("Haptic triggered: {ok}"));
            toast_mgr.push(
                "Haptic Feedback",
                if ok {
                    "Tactile impulse dispatched"
                } else {
                    "Haptics unsupported on this host"
                },
                ToastSeverity::Info,
                2500,
            );
        });
    }

    {
        let svc = services.clone();
        let scr = screen.clone();
        screen.set_on_share_payload(move || {
            let payload =
                SharePayload::text("CraftUI: Lightweight, beautiful cross-platform UI framework")
                    .with_title("CraftUI Framework")
                    .with_url("https://github.com/craftui/craftui");
            let ok = svc.share(&payload);
            scr.last_action.set(format!("Native share invoked: {ok}"));
            toast_mgr.push(
                "Native Share",
                if ok {
                    "Opened OS share sheet"
                } else {
                    "Share unavailable on this host"
                },
                ToastSeverity::Info,
                3000,
            );
        });
    }

    {
        let scr_preset = screen.clone();
        screen.set_on_cycle_preset(move || {
            let presets = [
                (craftui::core::DesignSystem::Material3, "Material3"),
                (craftui::core::DesignSystem::Cupertino, "Cupertino"),
                (craftui::core::DesignSystem::WinUi, "WinUI"),
            ];
            let next = (preset_idx.get_untracked() + 1) % presets.len();
            preset_idx.set(next);
            let (preset, label) = presets[next];
            let mode = MODES[theme_idx.get_untracked()];
            theme_sig.set(ThemeTokens::from_preset(preset, mode).applied(&ov_sig.get_untracked()));
            scr_preset.design_preset_label.set(label.to_string());
            scr_preset
                .last_action
                .set(format!("Design preset: {label}"));
        });
    }

    {
        let scr_rtl = screen.clone();
        screen.set_on_toggle_locale(move || {
            let next_rtl = !is_rtl.get_untracked();
            is_rtl.set(next_rtl);
            let locale = if next_rtl { "ar-SA" } else { "en-US" };
            let dir = craftui::core::locale_direction(locale);
            dir_sig.set(dir);
            scr_rtl.locale_label.set(if next_rtl {
                "ar-SA (RTL)".to_string()
            } else {
                "en-US (LTR)".to_string()
            });
            scr_rtl
                .last_action
                .set(format!("Locale switched to {locale} ({dir:?})"));
        });
    }

    {
        let scr_form = screen.clone();
        screen.set_on_validate_form(move || {
            let note_validator = craftui::widgets::form::FieldValidator::new(
                cx,
                scr_form.draft_note,
                vec![
                    craftui::widgets::form::ValidationRule::Required("Headline required".into()),
                    craftui::widgets::form::ValidationRule::MinLength(
                        4,
                        "Headline too short".into(),
                    ),
                ],
            );
            let date_validator = craftui::widgets::form::FieldValidator::new(
                cx,
                scr_form.selected_date,
                vec![craftui::widgets::form::ValidationRule::Contains(
                    "-".into(),
                    "Use YYYY-MM-DD".into(),
                )],
            );
            let group = craftui::widgets::form::FormGroup::new(vec![
                note_validator.clone(),
                date_validator.clone(),
            ]);
            let valid = group.validate_all();
            let status = if valid {
                "Form Valid".to_string()
            } else {
                note_validator
                    .error
                    .get_untracked()
                    .or_else(|| date_validator.error.get_untracked())
                    .unwrap_or_else(|| "Invalid".into())
            };
            scr_form.form_status.set(status.clone());
            scr_form.last_action.set(format!("Form check: {status}"));
        });
    }

    cx.effect({
        let scr = screen.clone();
        let svc = services.clone();
        move || {
            let seg = scr.mobile_segment.get();
            match seg {
                1 => {
                    let info = svc.device_info();
                    let net = svc.network_status();
                    let bat = svc.battery_state();
                    let bat_str = bat
                        .level_pct
                        .map(|p| format!("{p}%"))
                        .unwrap_or_else(|| "N/A".into());
                    scr.last_action.set(format!(
                        "Device: {} {} | Network: {:?} | Battery: {} (charging: {})",
                        info.os, info.model, net, bat_str, bat.charging
                    ));
                }
                2 => {
                    scr.last_action.set(
                        "OS Capabilities: Safe-area insets, SAF pickers, Keystore secrets"
                            .to_string(),
                    );
                }
                _ => {}
            }
        }
    });

    let top_bar = {
        let r_nav = router;
        let l_opt = launcher;
        let ov_ocean = ov_sig;
        let th_ocean = theme_sig;
        let ov_amber = ov_sig;
        let th_amber = theme_sig;
        let _ = theme_idx;

        cui! {
            Column(width: pct(100), height: pct(100), bg: theme.background) {
                Row(
                    width: pct(100),
                    bg: theme.surface,
                    padding_x: 20,
                    padding_y: 10,
                    gap: 10,
                    align: Alignment::Center,
                    justify: Justify::SpaceBetween,
                    border_width: 1,
                    border_color: theme.border
                ) {
                    Row(gap: 8, align: Alignment::Center) {
                        Button(
                            label: move || match r_nav.current() {
                                ShowcaseRoute::Gallery => "Inspect Theme Tokens".to_string(),
                                ShowcaseRoute::TokenInspector => "< Back to Gallery".to_string(),
                            },
                            bg: theme.surface_variant,
                            fg: theme.text_primary,
                            on_tap: move || match r_nav.current() {
                                ShowcaseRoute::Gallery => r_nav.push(ShowcaseRoute::TokenInspector),
                                ShowcaseRoute::TokenInspector => {
                                    r_nav.pop();
                                }
                            }
                        )
                    }
                    Row(gap: 8, align: Alignment::Center) {
                        Custom(widget: AccentSwatchWidget::new(
                            "Ocean",
                            Color::rgb(14, 165, 233),
                            swatch_hits,
                            move |accent| {
                                let ov = ThemeOverride::new().with_primary(accent, Color::WHITE);
                                ov_ocean.set(ov.clone());
                                let mode = th_ocean.get_untracked().mode;
                                th_ocean.set(ThemeTokens::from_mode(mode).applied(&ov));
                            },
                        ))
                        Custom(widget: AccentSwatchWidget::new(
                            "Amber",
                            Color::rgb(217, 119, 6),
                            swatch_hits,
                            move |accent| {
                                let ov = ThemeOverride::new().with_primary(accent, Color::WHITE);
                                ov_amber.set(ov.clone());
                                let mode = th_amber.get_untracked().mode;
                                th_amber.set(ThemeTokens::from_mode(mode).applied(&ov));
                            },
                        ))
                        Button(
                            label: "New Window",
                            bg: theme.surface_variant,
                            fg: theme.text_primary,
                            on_tap: move || {
                                if let Some(ref win) = l_opt {
                                    let _ = win.open_window(
                                        WindowConfig {
                                            title: "CraftUI Theme Tokens".to_string(),
                                            width: 440.0,
                                            height: 320.0,
                                            ..Default::default()
                                        },
                                        |w_cx, _w_tree, _ctx| {
                                            cui! {
                                                Column(width: pct(100), height: pct(100), bg: theme.background, padding: 24, gap: 12) {
                                                    Text(text: "Secondary CraftUI Window", size: 20, weight: 700.0, color: theme.text_primary)
                                                    Text(text: "Multi-window DesktopRunner with isolated child Scope.", size: 14, color: theme.text_secondary)
                                                }
                                            }.build(w_cx, _w_tree)
                                        },
                                    );
                                }
                            }
                        )
                    }
                }
            }
        }
        .build(cx, tree)
    };

    let screen_slot = screen.clone();
    let content_slot = tree.dynamic_slot(
        cx,
        LayoutStyle::column()
            .with_size(pct(100), pct(100))
            .with_flex_grow(1.0),
        {
            let r_c = router;
            move || r_c.current()
        },
        move |slot_cx, route, slot_tree| match route {
            ShowcaseRoute::Gallery => screen_slot.build(slot_cx, slot_tree),
            ShowcaseRoute::TokenInspector => cui! {
                Column(width: pct(100), height: pct(100), padding: 24, gap: 14, bg: theme.background) {
                    Text(text: "Live Theme Tokens Inspector", size: 22, weight: 700.0, color: theme.text_primary)
                    Text(
                        text: move || format!("Active Theme Mode: {:?} | Primary: {}", theme_sig.get().mode, theme_sig.get().primary.to_hex()),
                        size: 14,
                        color: theme.text_secondary
                    )
                    Row(gap: 12) {
                        Container(width: dp(120), height: dp(64), bg: theme.primary, radius: 10)
                        Container(width: dp(120), height: dp(64), bg: theme.surface, radius: 10, border_width: 1, border_color: theme.border)
                        Container(width: dp(120), height: dp(64), bg: theme.danger, radius: 10)
                    }
                }
            }
            .build(slot_cx, slot_tree),
        },
    );
    tree.append_child(top_bar, content_slot);

    (top_bar, router, swatch_hits, screen, toast_mgr)
}

pub fn mount_showcase_app_with_services(
    cx: Scope,
    tree: &mut WidgetTree,
    launcher: Option<WindowLauncher>,
    services: PlatformServices,
    dispatcher: Option<UiDispatcher>,
) -> (NodeId, Router<ShowcaseRoute>, Signal<usize>) {
    let (root, router, hits, _, _) =
        mount_showcase_app_inner(cx, tree, launcher, services, dispatcher);
    (root, router, hits)
}

pub fn mount_showcase_app(
    cx: Scope,
    tree: &mut WidgetTree,
    launcher: Option<WindowLauncher>,
) -> (NodeId, Router<ShowcaseRoute>, Signal<usize>) {
    mount_showcase_app_with_services(cx, tree, launcher, PlatformServices::in_memory(), None)
}

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Flagship Showcase".to_string(),
            width: 960.0,
            height: 700.0,
            ..Default::default()
        },
        |cx, tree, ctx| {
            let services = PlatformServices::new(DesktopClipboard::default(), DesktopFileDialog);
            let (root, _, _) = mount_showcase_app_with_services(
                cx,
                tree,
                Some(ctx.window_launcher()),
                services,
                Some(ctx.dispatcher.clone()),
            );
            root
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui::core::{MemoryClipboard, MockFileDialog};
    use craftui::reactive::reset_runtime;

    fn click_at(tree: &mut WidgetTree, pos: Point) {
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: pos,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: pos,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 40,
        }));
    }

    fn move_to(tree: &mut WidgetTree, pos: Point) {
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Move,
            position: pos,
            button: None,
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 50,
        }));
    }

    fn render_tree_rgba(tree: &mut WidgetTree, width: u32, height: u32) -> Vec<u8> {
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);
        let textures = TextureRegistry::new();
        render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            width,
            height,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        )
    }

    #[test]
    fn aot_cui_multifile_showcase_compiles_interacts_and_switches_theme() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let screen = aot_ui::ShowcaseScreen::new(cx);
        let root = screen.build(cx, &mut tree);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(960.0, 700.0),
            ..Default::default()
        };
        tree.layout(metrics);

        let dark_rgba = render_tree_rgba(&mut tree, 960, 700);
        assert_eq!(dark_rgba.len(), 960 * 700 * 4);
        assert!(dark_rgba.iter().any(|&b| b > 40));

        // Switch to DataList tab -> renders VirtualList cleanly
        screen.active_tab.set(1);
        tree.layout(metrics);

        // Open Modal & verify Escape closes via reactive on_close / open<=>show_modal
        screen.show_modal.set(true);
        tree.layout(metrics);
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Escape,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
        assert!(!screen.show_modal.get());

        // Live theme switch updates reactive Theme.* bindings
        tree.set_theme_mode(ThemeMode::Light);
        for _ in 0..120 {
            if !tree.tick_animations(0.016) {
                break;
            }
        }
        tree.layout(metrics);
        let light_rgba = render_tree_rgba(&mut tree, 960, 700);
        assert_ne!(dark_rgba, light_rgba);
    }

    #[test]
    fn custom_widget_theme_override_and_router_work() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let (root, router, hits) = mount_showcase_app(cx, &mut tree, None);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(960.0, 700.0),
            ..Default::default()
        };
        tree.layout(metrics);

        // Find CustomWidget node ("AccentSwatchWidget") and click it
        let swatch_id = tree
            .nodes
            .iter()
            .find_map(|(id, node)| (node.debug_name == "AccentSwatchWidget").then_some(id))
            .expect("swatch widget missing");
        let rect = tree.rect(swatch_id);
        click_at(&mut tree, rect.center());
        assert_eq!(hits.get(), 1);
        assert_eq!(tree.theme().primary, Color::rgb(14, 165, 233));

        // Navigate with Router
        assert_eq!(router.current(), ShowcaseRoute::Gallery);
        router.push(ShowcaseRoute::TokenInspector);
        tree.layout(metrics);
        assert_eq!(router.current(), ShowcaseRoute::TokenInspector);
        assert!(router.pop());
        assert_eq!(router.current(), ShowcaseRoute::Gallery);
    }

    #[test]
    fn split_pane_spinner_and_badge_layout_interact_and_render() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let (root, _, _, screen, _) =
            mount_showcase_app_inner(cx, &mut tree, None, PlatformServices::in_memory(), None);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(960.0, 700.0),
            ..Default::default()
        };
        tree.layout(metrics);

        // Header Badge exists with configured label/icon and non-zero layout rect
        let header_badge_id = tree
            .node_by_id("header_badge")
            .expect("header_badge missing");
        let header_badge_rect = tree.rect(header_badge_id);
        assert!(header_badge_rect.width > 40.0 && header_badge_rect.height > 14.0);
        if let WidgetKind::Badge { label, icon, .. } =
            &tree.nodes.get(header_badge_id).unwrap().kind
        {
            assert_eq!(label.get(), "Production");
            assert_eq!(icon.as_ref().map(|s| s.get()).as_deref(), Some("check"));
        } else {
            panic!("header_badge should be a Badge widget");
        }

        // SplitPane lays out both panes side-by-side with >= min_first / min_second width
        let split_id = tree
            .node_by_id("controls_split")
            .expect("controls_split missing");
        let left_id = tree
            .node_by_id("controls_left")
            .expect("controls_left missing");
        let right_id = tree
            .node_by_id("controls_right")
            .expect("controls_right missing");
        let split_rect = tree.rect(split_id);
        let left_rect = tree.rect(left_id);
        let right_rect = tree.rect(right_id);
        assert!(split_rect.width > 800.0 && split_rect.height > 300.0);
        assert!(left_rect.width >= 220.0);
        assert!(right_rect.width >= 220.0);
        assert!(right_rect.x > left_rect.max_x());

        // Hovering SplitPane divider reports ResizeHorizontal cursor and dragging updates ratio
        let div_mid = Point::new(
            (left_rect.max_x() + right_rect.x) * 0.5,
            split_rect.y + split_rect.height * 0.5,
        );
        assert_eq!(tree.cursor_at(div_mid), CursorIcon::ResizeHorizontal);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: div_mid,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 100,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Move,
            position: Point::new(split_rect.x + split_rect.width - 120.0, div_mid.y),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 120,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: Point::new(split_rect.x + split_rect.width - 120.0, div_mid.y),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 140,
        }));
        tree.layout(metrics);
        let new_left = tree.rect(left_id);
        let new_right = tree.rect(right_id);
        assert!(screen.split_ratio.get() > 0.65);
        assert!(new_left.width > left_rect.width + 100.0);
        assert!((new_right.width - 220.0).abs() <= 1.0);

        // Tab 0 Spinner advances phase cleanly while active and stops ticking when gated off
        let spinner_id = tree
            .node_by_id("controls_spinner")
            .expect("controls_spinner missing");
        let spinner_rect = tree.rect(spinner_id);
        assert!(spinner_rect.width >= 18.0 && spinner_rect.height >= 18.0);
        let phase0 = match &tree.nodes.get(spinner_id).unwrap().kind {
            WidgetKind::Spinner { phase, .. } => *phase,
            _ => panic!("controls_spinner should be WidgetKind::Spinner"),
        };
        assert!(tree.has_active_animations());
        assert!(tree.tick_animations(0.05));
        let phase1 = match &tree.nodes.get(spinner_id).unwrap().kind {
            WidgetKind::Spinner { phase, .. } => *phase,
            _ => unreachable!(),
        };
        assert!(phase1 > phase0);

        // Turning Live Preview off hides the spinner so active animations settle (and phase freezes)
        screen.feature_enabled.set(false);
        tree.layout(metrics);
        for _ in 0..120 {
            if !tree.tick_animations(0.016) {
                break;
            }
        }
        let phase_frozen = match &tree.nodes.get(spinner_id).unwrap().kind {
            WidgetKind::Spinner { phase, .. } => *phase,
            _ => unreachable!(),
        };
        assert!((phase_frozen - phase1).abs() < 1e-6);
        assert!(!tree.has_active_animations());
        screen.feature_enabled.set(true);
        tree.layout(metrics);

        // Switch to Tab 1 (Virtual Data) and verify VirtualList + Badge status pills render
        screen.active_tab.set(1);
        tree.layout(metrics);
        let vlist_id = tree
            .node_by_id("virtual_data_list")
            .expect("virtual_data_list missing");
        let vlist_rect = tree.rect(vlist_id);
        assert!(vlist_rect.width > 800.0 && vlist_rect.height >= 280.0);
        let badge_labels: Vec<String> = tree
            .nodes
            .values()
            .filter_map(|n| match &n.kind {
                WidgetKind::Badge { label, .. } if !n.layout.hidden => Some(label.get()),
                _ => None,
            })
            .collect();
        assert!(badge_labels.iter().any(|l| l == "Synced"));
        assert!(badge_labels.iter().any(|l| l == "Queued"));
        let tab1_rgba = render_tree_rgba(&mut tree, 960, 700);
        assert!(tab1_rgba.iter().any(|&b| b > 40));
    }

    #[test]
    fn glass_stat_card_toast_host_overlays_and_platform_services_work() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let services = PlatformServices::new(
            MemoryClipboard::default(),
            MockFileDialog::new(vec![PathBuf::from("/tmp/design_tokens.cui")], None),
        );
        let disp = UiDispatcher::new();
        let (root, _, _, screen, toast_mgr) =
            mount_showcase_app_inner(cx, &mut tree, None, services.clone(), Some(disp.clone()));
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(960.0, 700.0),
            ..Default::default()
        };
        tree.layout(metrics);

        // Click Tab 2 ("Glass & Split") directly via Tabs widget
        let tabs_id = tree
            .node_by_id("showcase_tabs")
            .expect("showcase_tabs missing");
        let tabs_rect = tree.rect(tabs_id);
        click_at(
            &mut tree,
            Point::new(
                tabs_rect.x + tabs_rect.width * (2.5 / 6.0),
                tabs_rect.center().y,
            ),
        );
        tree.layout(metrics);
        assert_eq!(screen.active_tab.get(), 2);

        // GlassStatCard verifies blur, shadows, pointer cursor, spring hover scale, and tap callback
        let card_id = tree
            .node_by_id("glass_card_engine")
            .expect("glass_card_engine missing");
        let card_rect = tree.rect(card_id);
        assert!(card_rect.width > 200.0 && card_rect.height > 70.0);
        {
            let card_node = tree.nodes.get(card_id).unwrap();
            assert!((card_node.box_style.backdrop_blur - 14.0).abs() < 1e-3);
            assert_eq!(card_node.box_style.shadows.len(), 2);
            assert_eq!(card_node.box_style.cursor, Some(CursorIcon::Pointer));
            assert_eq!(
                card_node.box_style.border_color.as_ref().map(|c| c.get()),
                Some(tree.theme().border)
            );
        }
        assert_eq!(tree.cursor_at(card_rect.center()), CursorIcon::Pointer);
        move_to(&mut tree, card_rect.center());
        tree.layout(metrics);
        assert!(tree.tick_animations(0.016));
        assert_eq!(
            tree.nodes
                .get(card_id)
                .unwrap()
                .box_style
                .border_color
                .as_ref()
                .map(|c| c.get()),
            Some(tree.theme().primary)
        );
        click_at(&mut tree, card_rect.center());
        tree.layout(metrics);
        assert!(screen.last_action.get().contains("Render Engine"));

        // Tab 2 Spinner advances smoothly
        let glass_spin = tree
            .node_by_id("glass_spinner")
            .expect("glass_spinner missing");
        let gphase0 = match &tree.nodes.get(glass_spin).unwrap().kind {
            WidgetKind::Spinner { phase, .. } => *phase,
            _ => panic!("glass_spinner should be Spinner"),
        };
        assert!(tree.tick_animations(0.05));
        let gphase1 = match &tree.nodes.get(glass_spin).unwrap().kind {
            WidgetKind::Spinner { phase, .. } => *phase,
            _ => unreachable!(),
        };
        assert!(gphase1 > gphase0);

        let before_toast_rgba = render_tree_rgba(&mut tree, 960, 700);

        // Notify Toast button pushes a ToastItem onto ToastHost & visibly renders
        let notify_id = tree
            .node_by_id("notify_toast_btn")
            .expect("notify_toast_btn missing");
        let notify_pt = tree.rect(notify_id).center();
        assert_eq!(tree.cursor_at(notify_pt), CursorIcon::Pointer);
        click_at(&mut tree, notify_pt);
        tree.layout(metrics);
        assert_eq!(toast_mgr.len(), 1);
        assert_eq!(screen.toasts.get().len(), 1);
        let after_toast_rgba = render_tree_rgba(&mut tree, 960, 700);
        assert_ne!(before_toast_rgba, after_toast_rgba);

        // Copy Summary button writes clipboard via PlatformServices and adds a toast
        let copy_id = tree
            .node_by_id("copy_summary_btn")
            .expect("copy_summary_btn missing");
        let copy_pt = tree.rect(copy_id).center();
        click_at(&mut tree, copy_pt);
        tree.layout(metrics);
        let clip = services.read_clipboard().unwrap_or_default();
        assert!(clip.contains("CraftUI Showcase"));
        assert!(clip.contains("Primary="));
        assert_eq!(toast_mgr.len(), 2);

        // Open File Dialog button runs worker off UI thread and delivers result onto dispatcher
        let open_id = tree
            .node_by_id("open_file_btn")
            .expect("open_file_btn missing");
        let open_pt = tree.rect(open_id).center();
        click_at(&mut tree, open_pt);
        for _ in 0..100 {
            let _ = disp.drain();
            if toast_mgr.len() >= 3 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        tree.layout(metrics);
        assert_eq!(screen.last_action.get(), "Selected file: design_tokens.cui");
        assert_eq!(toast_mgr.len(), 3);

        // Clicking ToastHost close button fires on_dismiss and removes toast
        let toast_host_id = tree
            .node_by_id("showcase_toasts")
            .expect("showcase_toasts missing");
        let th_rect = tree.rect(toast_host_id);
        let card_w = (th_rect.width - 32.0).clamp(180.0, 340.0);
        let card_x = th_rect.max_x() - 16.0 - card_w;
        let card_y = th_rect.max_y() - 16.0 - 64.0;
        let close_center = Point::new(card_x + card_w - 19.0, card_y + 19.0);
        assert_eq!(tree.cursor_at(close_center), CursorIcon::Pointer);
        click_at(&mut tree, close_center);
        tree.layout(metrics);
        assert_eq!(toast_mgr.len(), 2);
        assert!(screen.last_dismissed_toast.get() > 0);

        // Switch to Tab 3 ("Overlays") & verify Modal (blur 12), BottomSheet (blur 14), PopupMenu
        click_at(
            &mut tree,
            Point::new(
                tabs_rect.x + tabs_rect.width * (3.5 / 6.0),
                tabs_rect.center().y,
            ),
        );
        tree.layout(metrics);
        assert_eq!(screen.active_tab.get(), 3);

        let modal_id = tree
            .node_by_id("showcase_modal")
            .expect("showcase_modal missing");
        let sheet_id = tree
            .node_by_id("showcase_sheet")
            .expect("showcase_sheet missing");
        assert!((tree.nodes.get(modal_id).unwrap().box_style.backdrop_blur - 12.0).abs() < 1e-3);
        assert!((tree.nodes.get(sheet_id).unwrap().box_style.backdrop_blur - 14.0).abs() < 1e-3);

        let open_modal_btn = tree
            .node_by_id("open_modal_btn")
            .expect("open_modal_btn missing");
        let open_modal_pt = tree.rect(open_modal_btn).center();
        click_at(&mut tree, open_modal_pt);
        tree.layout(metrics);
        assert!(screen.show_modal.get());
        let modal_rgba = render_tree_rgba(&mut tree, 960, 700);
        assert!(modal_rgba.iter().any(|&b| b > 40));

        let close_modal_btn = tree
            .node_by_id("close_modal_btn")
            .expect("close_modal_btn missing");
        let close_modal_pt = tree.rect(close_modal_btn).center();
        click_at(&mut tree, close_modal_pt);
        tree.layout(metrics);
        assert!(!screen.show_modal.get());

        let open_sheet_btn = tree
            .node_by_id("open_sheet_btn")
            .expect("open_sheet_btn missing");
        let open_sheet_pt = tree.rect(open_sheet_btn).center();
        click_at(&mut tree, open_sheet_pt);
        tree.layout(metrics);
        assert!(screen.show_sheet.get());
        let dismiss_sheet_btn = tree
            .node_by_id("dismiss_sheet_btn")
            .expect("dismiss_sheet_btn missing");
        let dismiss_sheet_pt = tree.rect(dismiss_sheet_btn).center();
        click_at(&mut tree, dismiss_sheet_pt);
        tree.layout(metrics);
        assert!(!screen.show_sheet.get());

        let menu_btn = tree
            .node_by_id("overlay_popup_menu_btn")
            .expect("overlay_popup_menu_btn missing");
        let menu_pt = tree.rect(menu_btn).center();
        click_at(&mut tree, menu_pt);
        tree.layout(metrics);
        assert!(screen.show_menu.get());
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Escape,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
        tree.layout(metrics);
        assert!(!screen.show_menu.get());

        // Switch to Tab 4 ("Mobile & Native") & verify SafeArea, SegmentedControl, Accordion, DataTable
        click_at(
            &mut tree,
            Point::new(
                tabs_rect.x + tabs_rect.width * (4.5 / 6.0),
                tabs_rect.center().y,
            ),
        );
        tree.layout(metrics);
        assert_eq!(screen.active_tab.get(), 4);

        let mobile_panel = tree
            .node_by_id("mobile_tab_panel")
            .expect("mobile_tab_panel missing");
        assert!(tree.rect(mobile_panel).width > 400.0);

        // Click Request Selected button -> triggers on_request_permission -> updates permission table & pushes toast
        let req_btn = tree
            .node_by_id("req_perm_btn")
            .expect("req_perm_btn missing");
        let req_pt = tree.rect(req_btn).center();
        click_at(&mut tree, req_pt);
        tree.layout(metrics);
        assert!(screen.last_action.get().contains("Permission"));

        // Click Trigger Haptic button -> dispatches haptic feedback
        let haptic_btn = tree.node_by_id("haptic_btn").expect("haptic_btn missing");
        let haptic_pt = tree.rect(haptic_btn).center();
        click_at(&mut tree, haptic_pt);
        tree.layout(metrics);
        assert!(screen.last_action.get().contains("Haptic triggered"));

        // Click Share Link button -> invokes native OS share sheet
        let share_btn = tree.node_by_id("share_btn").expect("share_btn missing");
        let share_pt = tree.rect(share_btn).center();
        click_at(&mut tree, share_pt);
        tree.layout(metrics);
        assert!(screen.last_action.get().contains("Native share invoked"));

        // Switch to Tab 5 ("Parity & Rich") & exercise MenuBar, Presets, RTL, FormGroup, DnD, Pickers, Media, RichTextEditor
        click_at(
            &mut tree,
            Point::new(
                tabs_rect.x + tabs_rect.width * (5.5 / 6.0),
                tabs_rect.center().y,
            ),
        );
        tree.layout(metrics);
        assert_eq!(screen.active_tab.get(), 5);

        for id in [
            "parity_tab_panel",
            "parity_menubar",
            "parity_draggable",
            "parity_drop_target",
            "parity_date_picker",
            "parity_time_picker",
            "parity_media_player",
            "parity_rich_editor",
        ] {
            assert!(tree.node_by_id(id).is_some(), "missing {id}");
        }

        let preset_btn = tree.node_by_id("preset_btn").expect("preset_btn missing");
        let preset_pt = tree.rect(preset_btn).center();
        click_at(&mut tree, preset_pt);
        tree.layout(metrics);
        assert_eq!(screen.design_preset_label.get(), "Cupertino");

        let rtl_btn = tree
            .node_by_id("rtl_toggle_btn")
            .expect("rtl_toggle_btn missing");
        let rtl_pt = tree.rect(rtl_btn).center();
        click_at(&mut tree, rtl_pt);
        tree.layout(metrics);
        assert_eq!(tree.direction(), Direction::Rtl);
        assert_eq!(screen.locale_label.get(), "ar-SA (RTL)");

        let form_btn = tree
            .node_by_id("validate_form_btn")
            .expect("validate_form_btn missing");
        let form_pt = tree.rect(form_btn).center();
        click_at(&mut tree, form_pt);
        tree.layout(metrics);
        assert_eq!(screen.form_status.get(), "Form Valid");
    }
}
