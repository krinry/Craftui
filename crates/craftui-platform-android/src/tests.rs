use super::*;
use craftui_core::*;
use craftui_layout::LayoutStyle;
use craftui_reactive::reset_runtime;
use craftui_widgets::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct DummyExternalSurface {
    dirty: bool,
}

impl ExternalSurfaceTextureBridge for DummyExternalSurface {
    fn surface_id(&self) -> u64 {
        42
    }

    fn acquire_latest_rgba(&mut self) -> Option<(u32, u32, Vec<u8>)> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        Some((
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        ))
    }
}

#[test]
fn android_host_safe_area_keyboard_insets_and_ime_commands() {
    reset_runtime();
    let disp = UiDispatcher::new();
    let mut host = AndroidHost::new(disp, 1080, 2400, 3.0);
    // Status bar = 90px top, IME keyboard occupies bottom 750px
    host.update_safe_area_and_ime(1080, 2400, 0, 90, 1080, 1650);
    let eff = host.tree.metrics.effective_safe_area();
    assert!((eff.top - 30.0).abs() < 0.01);
    assert!((eff.bottom - 250.0).abs() < 0.01);

    let inp = host.tree.text_input(host.scope, "", "Search", None);
    let mut ls = LayoutStyle::row().with_size(Length::Dp(300.0), Length::Dp(44.0));
    ls.margin = Insets::ZERO;
    host.tree.set_layout(inp, ls);
    host.tree.set_root(inp);
    let _ = host.step_frame(0.016);

    // Touch down+up focuses the TextInput
    host.push_touch(0, PointerPhase::Down, 150.0, 60.0, 0);
    host.push_touch(0, PointerPhase::Up, 150.0, 60.0, 20);
    let (_, cmds) = host.step_frame(0.016);
    assert!(cmds.contains(&AndroidImeCommand::ShowSoftInput));

    host.push_ime_commit("invoice");
    let (_, cmds2) = host.step_frame(0.016);
    assert!(cmds2.iter().any(|c| matches!(
        c,
        AndroidImeCommand::SyncTextState {
            text,
            selection_start: 7,
            selection_end: 7,
            compose_region: None,
        } if text == "invoice"
    )));

    // IME preedit emits non-empty compose_region, then commit clears it back to None
    assert!(host.push_ime_preedit("東京", Some((0, 6))));
    let (_, cmds_comp) = host.step_frame(0.016);
    assert!(cmds_comp.iter().any(|c| matches!(
        c,
        AndroidImeCommand::SyncTextState {
            text,
            compose_region: Some((7, 13)),
            ..
        } if text == "invoice東京"
    )));

    // Full Android GameTextInput TextEvent buffer replacement preserves bound editor + preedit without echoing
    assert!(host.push_ime_text_state("hello-world", 11, 11, Some((6, 11))));
    let (_, cmds_echo) = host.step_frame(0.016);
    assert!(cmds_echo.is_empty());
    let WidgetKind::TextInput { editor, .. } = &host.tree.nodes.get(inp).unwrap().kind else {
        panic!("expected TextInput");
    };
    assert_eq!(editor.text, "hello-");
    assert_eq!(editor.preedit.as_deref(), Some("world"));
}

#[test]
fn android_punctuation_keys_clipboard_shortcuts_and_error_state() {
    reset_runtime();
    assert_eq!(
        map_android_punctuation(AndroidPunctuationKey::Period, false),
        '.'
    );
    assert_eq!(
        map_android_punctuation(AndroidPunctuationKey::Period, true),
        '>'
    );
    assert_eq!(
        map_android_punctuation(AndroidPunctuationKey::Minus, true),
        '_'
    );
    assert_eq!(
        map_android_punctuation(AndroidPunctuationKey::Slash, true),
        '?'
    );
    assert_eq!(
        map_android_punctuation(AndroidPunctuationKey::At, false),
        '@'
    );
    assert_eq!(
        map_android_punctuation(AndroidPunctuationKey::NumpadAdd, false),
        '+'
    );
    assert_eq!(map_android_digit('2', true), '@');
    assert_eq!(map_android_digit('8', false), '8');

    let mut host = AndroidHost::new(UiDispatcher::new(), 1080, 1920, 2.0);
    assert!(host.last_error().is_none());
    host.record_error("vulkan init failed");
    assert_eq!(host.last_error(), Some("vulkan init failed"));
    host.clear_error();
    assert!(host.last_error().is_none());

    let text_sig = host.scope.signal(String::new());
    let inp = host
        .tree
        .text_input(host.scope, "", "Email", Some(text_sig));
    host.tree.set_root(inp);
    let _ = host.step_frame(0.016);
    host.tree.focused = Some(inp);

    for ch in [
        map_android_digit('1', false),
        map_android_punctuation(AndroidPunctuationKey::At, false),
        map_android_punctuation(AndroidPunctuationKey::Slash, true),
    ] {
        assert!(host.handle_android_key_event(
            AndroidKeyCode::Character(ch),
            true,
            false,
            Modifiers::default()
        ));
    }
    assert_eq!(text_sig.get(), "1@?");

    // Select all via Ctrl+A then hardware Cut removes selected text
    assert!(host.handle_android_key_event(
        AndroidKeyCode::Character('a'),
        true,
        false,
        Modifiers {
            ctrl: true,
            ..Default::default()
        }
    ));
    assert!(host.handle_android_key_event(AndroidKeyCode::Cut, true, false, Modifiers::default()));
    assert_eq!(text_sig.get(), "");
}

#[test]
fn android_back_router_pop_enter_dpad_and_external_texture_work() {
    reset_runtime();
    let mut host = AndroidHost::new(UiDispatcher::new(), 1080, 1920, 2.0);
    let router = Router::new(host.scope, "home".to_string());
    host.bind_back_router(router);
    router.push("detail".to_string());

    let modal_open = host.scope.signal(true);
    let root = host.tree.column(host.scope);
    let tab_idx = host.scope.signal(0usize);
    let tabs = host
        .tree
        .tabs(host.scope, vec!["One".into(), "Two".into()], tab_idx);
    let pv_id = PlatformViewId::default();
    let pv = host.tree.platform_view(
        host.scope,
        pv_id,
        LayoutStyle::row().with_size(Length::Dp(120.0), Length::Dp(80.0)),
    );
    let modal = host.tree.overlay(
        host.scope,
        modal_open,
        false,
        Some(Rc::new(RefCell::new(move || modal_open.set(false)))),
    );
    host.tree.append_child(root, tabs);
    host.tree.append_child(root, pv);
    host.tree.append_child(root, modal);
    host.tree.set_root(root);
    let _ = host.step_frame(0.016);

    // Back key first dismisses active modal/overlay
    assert!(host.handle_android_key_event(AndroidKeyCode::Back, true, false, Modifiers::default()));
    assert!(!modal_open.get());
    assert_eq!(router.current(), "detail");

    // Second Back key pops the Router stack
    assert!(host.handle_android_key_event(AndroidKeyCode::Back, true, false, Modifiers::default()));
    assert_eq!(router.current(), "home");

    // Third Back key at root returns false (unhandled so Android finishes Activity)
    assert!(!host.handle_android_key_event(
        AndroidKeyCode::Back,
        true,
        false,
        Modifiers::default()
    ));

    // Tab -> focus Tabs, DpadRight switches active tab
    host.tree.focused = Some(tabs);
    assert!(host.handle_android_key_event(
        AndroidKeyCode::DpadRight,
        true,
        false,
        Modifiers::default()
    ));
    assert_eq!(tab_idx.get(), 1);

    // Spring animations run on tab switch & overlay close, then settle to demand-driven idle (`None`)
    let (anim_active, painted_now, _) = host.step_frame_with_redraw(0.016);
    assert!(anim_active && painted_now);
    assert!(host.next_poll_timeout(anim_active, true).is_some());

    for _ in 0..240 {
        let (still_anim, _, _) = host.step_frame_with_redraw(0.016);
        if !still_anim {
            break;
        }
    }
    let (anim_idle, painted_idle, _) = host.step_frame_with_redraw(0.016);
    assert!(!anim_idle && !painted_idle);
    assert_eq!(host.next_poll_timeout(false, true), None);

    // External SurfaceTexture upload + PlatformViewHole emitted in scene
    let mut bridge = DummyExternalSurface { dirty: true };
    let mut tex_slot = None;
    let tid = host
        .sync_external_texture(&mut bridge, &mut tex_slot)
        .expect("external texture uploaded");
    assert!(host.textures.get(tid).is_some());
    assert!(host.scene.commands.iter().any(|c| matches!(
        c,
        craftui_render::PaintCommand::PlatformViewHole { id, .. } if *id == pv_id
    )));
}

#[test]
fn android_permissions_haptics_share_saf_and_telemetry_work() {
    reset_runtime();
    let disp = UiDispatcher::new();
    let mut host = AndroidHost::new(disp.clone(), 1080, 2400, 3.0);

    host.set_device_info(DeviceInfo {
        os: "Android".into(),
        model: "Pixel 9".into(),
        os_version: "15".into(),
        sdk_int: Some(35),
        app_version: "1.2.0".into(),
    });
    host.set_network_status(NetworkStatus::Wifi);
    host.set_battery_state(BatteryState {
        level_pct: Some(84),
        charging: true,
    });
    host.push_lifecycle_event(AppLifecycleEvent::Resumed);
    host.push_lifecycle_event(AppLifecycleEvent::Paused);

    assert_eq!(host.tree.services.device_info().model, "Pixel 9");
    assert_eq!(host.tree.services.network_status(), NetworkStatus::Wifi);
    assert_eq!(host.tree.services.battery_state().level_pct, Some(84));
    assert_eq!(
        host.bridge.lifecycle_history(),
        vec![AppLifecycleEvent::Resumed, AppLifecycleEvent::Paused]
    );

    // Runtime permissions without runtime prompt on API 35 (e.g., Vibrate) are granted cleanly
    assert_eq!(
        host.tree.services.check_permission(&Permission::Camera),
        PermissionStatus::NotDetermined
    );
    assert_eq!(
        host.tree
            .services
            .check_permission(&Permission::BackgroundRefresh),
        PermissionStatus::Granted
    );

    // Haptics, share, app settings, and keystore sync emit typed AndroidPlatformCommands
    assert!(host.tree.services.trigger_haptic(HapticStyle::Medium));
    assert!(
        host.tree
            .services
            .share(&SharePayload::text("Invoice PDF").with_title("Share"))
    );
    assert!(host.tree.services.open_app_settings());
    assert!(host.tree.services.set_secret("jwt", "secret-token"));
    assert_eq!(
        host.tree.services.get_secret("jwt").as_deref(),
        Some("secret-token")
    );
    assert!(host.tree.services.delete_secret("jwt"));
    assert_eq!(host.tree.services.get_secret("jwt"), None);

    let cmds = host.drain_platform_commands();
    assert_eq!(cmds.len(), 5);
    assert!(matches!(
        cmds[0],
        AndroidPlatformCommand::TriggerHaptic(HapticStyle::Medium)
    ));
    assert!(matches!(cmds[1], AndroidPlatformCommand::Share(_)));
    assert_eq!(cmds[2], AndroidPlatformCommand::OpenAppSettings);
    assert!(matches!(
        cmds[3],
        AndroidPlatformCommand::SyncSecureKey { .. }
    ));
    assert!(matches!(
        cmds[4],
        AndroidPlatformCommand::DeleteSecureKey { .. }
    ));

    // Async batch permission request + SAF file open resolved asynchronously on worker thread
    let perm_outcomes = Arc::new(Mutex::new(None));
    let po_c = perm_outcomes.clone();
    host.tree.services.request_permissions_async(
        vec![Permission::Camera, Permission::Notifications],
        disp.clone(),
        move |outcomes| {
            *po_c.lock().unwrap() = Some(outcomes);
        },
    );

    let mut req_id = None;
    for _ in 0..100 {
        for cmd in host.drain_platform_commands() {
            if let AndroidPlatformCommand::RequestPermissions {
                request_id,
                manifest_permissions,
                ..
            } = cmd
            {
                assert!(manifest_permissions.contains(&"android.permission.CAMERA".to_string()));
                assert!(
                    manifest_permissions
                        .contains(&"android.permission.POST_NOTIFICATIONS".to_string())
                );
                req_id = Some(request_id);
            }
        }
        if req_id.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let id = req_id.expect("RequestPermissions command emitted");
    host.push_permission_results(
        id,
        &[
            ("android.permission.CAMERA".to_string(), 0, false),
            (
                "android.permission.POST_NOTIFICATIONS".to_string(),
                -1,
                true,
            ),
        ],
    );

    for _ in 0..100 {
        let _ = disp.drain();
        if perm_outcomes.lock().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let resolved = perm_outcomes
        .lock()
        .unwrap()
        .take()
        .expect("async outcomes delivered");
    assert_eq!(resolved[0].status, PermissionStatus::Granted);
    assert_eq!(resolved[1].status, PermissionStatus::Denied);
    assert!(resolved[1].should_show_rationale);
    assert_eq!(
        host.tree.services.check_permission(&Permission::Camera),
        PermissionStatus::Granted
    );
    assert!(
        host.tree
            .services
            .should_show_permission_rationale(&Permission::Notifications)
    );

    // SAF File Dialog roundtrip
    let picked = Arc::new(Mutex::new(None));
    let pk_c = picked.clone();
    host.tree.services.pick_open_file_async(
        FileDialogRequest::open("Import JSON"),
        disp.clone(),
        move |path| {
            *pk_c.lock().unwrap() = Some(path);
        },
    );
    let mut saf_id = None;
    for _ in 0..100 {
        for cmd in host.drain_platform_commands() {
            if let AndroidPlatformCommand::OpenFileDialog { request_id, .. } = cmd {
                saf_id = Some(request_id);
            }
        }
        if saf_id.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    host.push_file_dialog_result(
        saf_id.expect("OpenFileDialog emitted"),
        vec![PathBuf::from("/sdcard/Download/report.json")],
    );
    for _ in 0..100 {
        let _ = disp.drain();
        if picked.lock().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        picked.lock().unwrap().clone(),
        Some(Some(PathBuf::from("/sdcard/Download/report.json")))
    );
}

#[test]
#[cfg(feature = "hot-reload")]
fn test_android_remote_preview_hot_reload_pipeline() {
    let disp = UiDispatcher::new();
    let mut host = AndroidHost::new(disp, 1080, 2400, 2.625);

    let cui_source = r#"
            import { Card } from "std/material.cui";

            component LiveScreen {
                in-out property <string> header: "CraftUI Remote Preview";
                Column {
                    Text { text: root.header; }
                    Card {
                        Text { text: "Live content rendered on Android!"; }
                    }
                }
            }
        "#;

    let root_id = host
        .apply_hot_reload_source(cui_source, Some("LiveScreen"))
        .expect("remote hot reload source must parse and mount");

    assert_eq!(host.tree.root, Some(root_id));
    assert!(host.needs_redraw);

    let (animating, did_paint, _) = host.step_frame_with_redraw(0.016);
    assert!(!animating);
    assert!(did_paint);
    assert!(!host.needs_redraw);
}

#[test]
fn test_android_method_channel_plugin_dispatch() {
    struct SharedPrefsStub;
    impl craftui_core::CraftPlugin for SharedPrefsStub {
        fn name(&self) -> &str {
            "plugins.flutter.io/shared_preferences"
        }

        fn handle(&self, call: &craftui_core::MethodCall) -> craftui_core::Result<serde_json::Value> {
            match call.method.as_str() {
                "getAll" => Ok(serde_json::json!({ "theme": "amoled" })),
                other => Err(craftui_core::CraftError::Platform(format!(
                    "unsupported method `{other}`"
                ))),
            }
        }
    }

    let mut host = AndroidHost::new(UiDispatcher::new(), 1080, 2400, 2.5);
    host.register_plugin(SharedPrefsStub);
    let out = host
        .invoke_plugin(
            "plugins.flutter.io/shared_preferences",
            "getAll",
            serde_json::json!({}),
        )
        .expect("registered android plugin should respond");
    assert_eq!(out, serde_json::json!({ "theme": "amoled" }));

    // Async Kotlin MethodChannel roundtrip via AndroidBridgeState queue
    let bridge = host.bridge();
    let handle = std::thread::spawn(move || {
        bridge.invoke_method_channel(
            "dev.craftui.camera",
            "takePhoto",
            serde_json::json!({ "quality": 90 }),
        )
    });

    let mut req_id = None;
    for _ in 0..50 {
        for cmd in host.drain_platform_commands() {
            if let crate::AndroidPlatformCommand::InvokeMethodChannel {
                request_id,
                channel,
                method,
                ..
            } = cmd
            {
                assert_eq!(channel, "dev.craftui.camera");
                assert_eq!(method, "takePhoto");
                req_id = Some(request_id);
            }
        }
        if req_id.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let rid = req_id.expect("InvokeMethodChannel command must be queued");
    host.push_method_channel_result(rid, true, r#"{"path":"/sdcard/photo.jpg"}"#);
    let native_res = handle.join().unwrap().expect("async method channel result");
    assert_eq!(native_res, serde_json::json!({ "path": "/sdcard/photo.jpg" }));
}
