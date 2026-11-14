use super::*;
use craftui_a11y::A11yRole;
use craftui_core::*;
use craftui_widgets::*;
use craftui_layout::LayoutStyle;
use craftui_reactive::reset_runtime;

    #[test]
    fn web_host_dpr_ime_overlay_hash_router_dom_input_and_2d_fallback_render() {
        reset_runtime();
        assert_eq!(BrowserRouteSync::route_to_hash("/settings"), "#/settings");
        assert_eq!(BrowserRouteSync::hash_to_route("#/settings"), "/settings");

        let mut host = WebAppHost::new(UiDispatcher::new(), 400.0, 300.0, 2.0);
        let router = Router::new(host.scope, "/".to_string());
        BrowserRouteSync::connect_string_router(host.scope, router);
        assert_eq!(host.take_pending_hash_update().as_deref(), Some("#/"));
        assert!(host.apply_url_hash("#/dashboard"));
        assert_eq!(router.current(), "/dashboard");
        router.push("/profile".to_string());
        assert_eq!(
            host.take_pending_hash_update().as_deref(),
            Some("#/profile")
        );

        let (pw, ph) = host.resize_css(400.0, 300.0, 2.0);
        assert_eq!((pw, ph), (800, 600));

        let sig = host.scope.signal("hello".to_string());
        let inp = host.tree.text_input(host.scope, "", "Search", Some(sig));
        host.tree.set_layout(
            inp,
            LayoutStyle::row().with_size(Length::Dp(240.0), Length::Dp(40.0)),
        );
        host.tree.set_root(inp);
        let _ = host.step_frame(0.016);

        let c = host.tree.rect(inp).center();
        let down = dom_pointer_event(
            1,
            PointerPhase::Down,
            c.x,
            c.y,
            0.0,
            0.0,
            0,
            false,
            Modifiers::default(),
            0,
        );
        let up = dom_pointer_event(
            1,
            PointerPhase::Up,
            c.x,
            c.y,
            0.0,
            0.0,
            0,
            false,
            Modifiers::default(),
            20,
        );
        host.tree.handle_event(&down);
        host.tree.handle_event(&up);

        let ime = host.hidden_ime_state();
        assert!(ime.visible);
        assert_eq!(ime.value, "hello");

        // CSS cursor tracking reflects hovered TextInput -> "text"
        let move_ev = dom_pointer_event(
            1,
            PointerPhase::Move,
            c.x,
            c.y,
            0.0,
            0.0,
            -1,
            false,
            Modifiers::default(),
            25,
        );
        host.handle_ui_event(&move_ev);
        assert_eq!(host.hovered_cursor_css(), "text");

        // Hydrated clipboard supports synchronous Ctrl+V paste into focused TextInput
        host.hydrate_clipboard(" world");
        host.tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::KeyV,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
        }));
        assert_eq!(sig.get(), "hello world");

        // DOM ARIA extraction exposes textbox role, focus state, and value
        let aria = host.extract_aria_tree();
        assert!(aria.iter().any(|n| n.role == "textbox" && n.focused));

        // Mobile virtual keyboard DOM `input` insert & backspace events
        host.tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::KeyA,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
        }));
        host.tree
            .handle_event(&UiEvent::Ime(ImeEvent::Commit("hello".into())));
        for ev in dom_input_to_events("hello", "hello!", "insertText", Some("!")) {
            host.tree.handle_event(&ev);
        }
        assert_eq!(sig.get(), "hello!");
        for ev in dom_input_to_events("hello!", "hello", "deleteContentBackward", None) {
            host.tree.handle_event(&ev);
        }
        assert_eq!(sig.get(), "hello");

        let (rw, rh, rgba) = host.render_rgba_2d();
        assert_eq!((rw, rh), (800, 600));
        assert_eq!(rgba.len(), 800 * 600 * 4);

        // Demand-driven Web frame check: idle focused TextInput does not animate continuously,
        // and only triggers redraw at blink interval boundaries.
        host.tree.reset_caret_blink(1000);
        let (anim0, _painted0) = host.step_frame_with_redraw(0.016, 1000);
        assert!(!anim0);
        let (anim1, painted1) = host.step_frame_with_redraw(0.016, 1100);
        assert!(!anim1 && !painted1);
        let (anim2, painted2) = host.step_frame_with_redraw(0.016, 1600);
        assert!(!anim2 && painted2);

        let ky = dom_key_to_craft(
            "y",
            true,
            false,
            Modifiers {
                ctrl: true,
                ..Default::default()
            },
        );
        assert_eq!(ky.code, KeyCode::KeyY);
    }

    #[test]
    fn web_css_cursors_and_aria_roles_map_cleanly() {
        assert_eq!(craft_cursor_to_css(CursorIcon::Default), "default");
        assert_eq!(craft_cursor_to_css(CursorIcon::Pointer), "pointer");
        assert_eq!(craft_cursor_to_css(CursorIcon::Text), "text");
        assert_eq!(craft_cursor_to_css(CursorIcon::Grab), "grab");
        assert_eq!(craft_cursor_to_css(CursorIcon::Grabbing), "grabbing");
        assert_eq!(
            craft_cursor_to_css(CursorIcon::ResizeHorizontal),
            "ew-resize"
        );
        assert_eq!(craft_cursor_to_css(CursorIcon::ResizeVertical), "ns-resize");
        assert_eq!(craft_cursor_to_css(CursorIcon::NotAllowed), "not-allowed");
        assert_eq!(craft_cursor_to_css(CursorIcon::Crosshair), "crosshair");

        assert_eq!(craft_role_to_aria(A11yRole::Button), "button");
        assert_eq!(craft_role_to_aria(A11yRole::TextInput), "textbox");
        assert_eq!(craft_role_to_aria(A11yRole::CheckBox), "checkbox");
        assert_eq!(craft_role_to_aria(A11yRole::Switch), "switch");
        assert_eq!(craft_role_to_aria(A11yRole::Slider), "slider");
        assert_eq!(craft_role_to_aria(A11yRole::Alert), "alert");
    }

    #[test]
    fn web_host_supports_permissions_haptics_share_secure_storage_and_telemetry() {
        reset_runtime();
        let mut host = WebAppHost::new(UiDispatcher::new(), 390.0, 844.0, 3.0);
        assert_eq!(
            web_permission_descriptor(&Permission::Camera),
            Some("camera")
        );
        assert_eq!(
            web_permission_descriptor(&Permission::Notifications),
            Some("notifications")
        );
        assert_eq!(web_permission_descriptor(&Permission::SmsSend), None);
        assert_eq!(
            parse_web_permission_state("granted"),
            PermissionStatus::Granted
        );
        assert_eq!(
            parse_web_permission_state("denied"),
            PermissionStatus::Denied
        );
        assert_eq!(
            parse_web_permission_state("default"),
            PermissionStatus::NotDetermined
        );

        let services = host.tree.services.clone();
        assert_eq!(
            services.check_permission(&Permission::Camera),
            PermissionStatus::NotDetermined
        );
        assert_eq!(
            services.check_permission(&Permission::SmsSend),
            PermissionStatus::Unsupported
        );
        host.hydrate_permission_status(Permission::Camera, PermissionStatus::Granted);
        assert_eq!(
            services.check_permission(&Permission::Camera),
            PermissionStatus::Granted
        );
        assert!(!services.open_app_settings());

        // Host fallback share writes formatted string into WebClipboard cache
        assert!(!services.share(&SharePayload::new()));
        assert!(
            services.share(
                &SharePayload::url("https://craftui.dev")
                    .with_title("CraftUI")
                    .with_text("Check this out")
            )
        );
        let clip_text = services.read_clipboard().unwrap_or_default();
        assert!(clip_text.contains("https://craftui.dev"));

        // Secure storage + device telemetry + lifecycle tracking
        assert!(services.set_secret("session_jwt", "jwt_abc"));
        assert_eq!(
            services.get_secret("session_jwt").as_deref(),
            Some("jwt_abc")
        );
        assert!(services.delete_secret("session_jwt"));
        assert_eq!(services.get_secret("session_jwt"), None);

        host.bridge().set_network_status(NetworkStatus::Wifi);
        host.bridge().set_battery_state(BatteryState {
            level_pct: Some(74),
            charging: true,
        });
        assert_eq!(services.device_info().os, "Web");
        assert_eq!(services.network_status(), NetworkStatus::Wifi);
        assert_eq!(services.battery_state().level_pct, Some(74));

        host.push_lifecycle_event(AppLifecycleEvent::Paused);
        host.push_lifecycle_event(AppLifecycleEvent::Resumed);
        assert_eq!(
            host.bridge().lifecycle_history(),
            vec![AppLifecycleEvent::Paused, AppLifecycleEvent::Resumed]
        );
        assert_eq!(WebHapticsBackend::duration_ms(HapticStyle::Medium), 25.0);
    }
