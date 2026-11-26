use super::*;
use craftui_core::{
    BatteryState, ClipboardBackend, CursorIcon, DeviceCapabilitiesBackend, FileDialogRequest,
    NetworkStatus, Permission, PermissionBackend, PermissionStatus, PointerButton, SharePayload,
};
use std::path::PathBuf;
use winit::event::{MouseButton, MouseScrollDelta};
use winit::keyboard::ModifiersState;
use winit::window::CursorIcon as WinitCursorIcon;

#[test]
fn translates_mouse_buttons_cursors_scroll_deltas_and_clipboard() {
    assert_eq!(
        translate_mouse_button(MouseButton::Left),
        PointerButton::Primary
    );
    assert_eq!(
        translate_mouse_button(MouseButton::Right),
        PointerButton::Secondary
    );

    assert_eq!(
        translate_winit_cursor(CursorIcon::Default),
        WinitCursorIcon::Default
    );
    assert_eq!(
        translate_winit_cursor(CursorIcon::Pointer),
        WinitCursorIcon::Pointer
    );
    assert_eq!(
        translate_winit_cursor(CursorIcon::Text),
        WinitCursorIcon::Text
    );
    assert_eq!(
        translate_winit_cursor(CursorIcon::Grab),
        WinitCursorIcon::Grab
    );
    assert_eq!(
        translate_winit_cursor(CursorIcon::Grabbing),
        WinitCursorIcon::Grabbing
    );
    assert_eq!(
        translate_winit_cursor(CursorIcon::ResizeHorizontal),
        WinitCursorIcon::EwResize
    );
    assert_eq!(
        translate_winit_cursor(CursorIcon::ResizeVertical),
        WinitCursorIcon::NsResize
    );
    assert_eq!(
        translate_winit_cursor(CursorIcon::NotAllowed),
        WinitCursorIcon::NotAllowed
    );
    assert_eq!(
        translate_winit_cursor(CursorIcon::Crosshair),
        WinitCursorIcon::Crosshair
    );

    let (dx, dy) = translate_scroll_delta(MouseScrollDelta::LineDelta(0.0, -2.0), 1.0);
    assert_eq!(dx, 0.0);
    assert_eq!(dy, -72.0);

    let mods = translate_modifiers(ModifiersState::CONTROL | ModifiersState::SHIFT);
    assert!(mods.ctrl && mods.shift && !mods.alt);

    let req = FileDialogRequest::open("Select Asset")
        .with_filter(craftui_core::FileFilter::new("Images", ["png", "webp"]))
        .with_multiple(true);
    let _dialog = build_rfd_dialog(&req);

    let mut clip = DesktopClipboard::default();
    assert!(clip.set_text("CraftUI native clipboard"));
    assert!(clip.get_text().is_some());

    let rgba = [0x12u8, 0x34, 0x56, 0xFF, 0xAB, 0xCD, 0xEF, 0x80];
    let mut packed = [0u32; 2];
    pack_rgba8_to_softbuffer_0rgb(&rgba, &mut packed);
    assert_eq!(packed, [0x0012_3456, 0x00AB_CDEF]);
}

#[cfg(feature = "hot-reload")]
#[test]
fn filters_hot_reload_notify_events_to_cui_mutations() {
    use notify::Event as NotifyEvent;

    let access_ev = NotifyEvent::new(notify::EventKind::Access(notify::event::AccessKind::Open(
        notify::event::AccessMode::Read,
    )))
    .add_path(PathBuf::from("ui/main.cui"));
    assert!(!should_trigger_hot_reload(&access_ev));

    let modify_cui = NotifyEvent::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
        notify::event::DataChange::Content,
    )))
    .add_path(PathBuf::from("ui/main.cui"));
    assert!(should_trigger_hot_reload(&modify_cui));

    let modify_other = NotifyEvent::new(notify::EventKind::Modify(
        notify::event::ModifyKind::Data(notify::event::DataChange::Content),
    ))
    .add_path(PathBuf::from("assets/icon.png"));
    assert!(!should_trigger_hot_reload(&modify_other));
}

#[test]
fn desktop_services_support_permissions_share_secure_storage_and_device_telemetry() {
    let services = default_desktop_services();
    assert_eq!(
        services.check_permission(&Permission::Camera),
        PermissionStatus::Granted
    );
    assert_eq!(
        services.request_permission(&Permission::StorageRead),
        PermissionStatus::Granted
    );
    assert_eq!(
        services.check_permission(&Permission::SmsSend),
        PermissionStatus::Unsupported
    );
    assert!(!services.should_show_permission_rationale(&Permission::Camera));
    assert!(!services.open_app_settings());

    let perms = DesktopPermissionBackend::new()
        .with_status(Permission::Notifications, PermissionStatus::Denied)
        .with_settings_launcher(false);
    assert_eq!(
        perms.check(&Permission::Notifications),
        PermissionStatus::Denied
    );
    assert!(!perms.open_app_settings());

    assert!(services.set_secret("desktop_token", "abc_123"));
    assert_eq!(
        services.get_secret("desktop_token").as_deref(),
        Some("abc_123")
    );
    assert!(services.delete_secret("desktop_token"));
    assert_eq!(services.get_secret("desktop_token"), None);

    assert!(!services.share(&SharePayload::new()));
    assert!(
        services.share(
            &SharePayload::url("https://craftui.dev")
                .with_title("CraftUI")
                .with_text("Lightweight GPU UI")
        )
    );

    let info = services.device_info();
    assert_eq!(info.os, std::env::consts::OS);
    assert!(!info.model.is_empty());
    assert!(!info.app_version.is_empty());

    let caps = DesktopDeviceCapabilities::new();
    caps.set_network_override(Some(NetworkStatus::Ethernet));
    caps.set_battery_override(Some(BatteryState {
        level_pct: Some(95),
        charging: true,
    }));
    assert_eq!(caps.network_status(), NetworkStatus::Ethernet);
    assert_eq!(
        caps.battery_state(),
        BatteryState {
            level_pct: Some(95),
            charging: true
        }
    );
}

#[test]
fn system_tray_menu_and_global_shortcuts_dispatch_correctly() {
    use std::cell::Cell;
    use std::rc::Rc;

    let tray = crate::tray::SystemTrayHandle::in_memory();
    tray.set_tooltip("CraftUI Tray");
    tray.set_visible(true);
    assert_eq!(tray.tooltip(), "CraftUI Tray");
    assert!(tray.is_visible());

    tray.set_menu(vec![
        crate::tray::TrayMenuItem::new("show", "Show Window"),
        crate::tray::TrayMenuItem::separator(),
        crate::tray::TrayMenuItem::new("disabled_item", "Disabled").with_enabled(false),
    ]);
    assert_eq!(tray.menu().len(), 3);

    let shown = Rc::new(Cell::new(0usize));
    let shown_c = shown.clone();
    tray.on_menu_select("show", move || {
        shown_c.set(shown_c.get() + 1);
    });
    assert!(tray.trigger_menu_item("show"));
    assert_eq!(shown.get(), 1);
    assert!(!tray.trigger_menu_item("disabled_item"));
    assert!(!tray.trigger_menu_item("missing"));

    let shortcut_hits = Rc::new(Cell::new(0usize));
    let sc_c = shortcut_hits.clone();
    let mods = craftui_core::Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    };
    tray.register_shortcut(
        crate::tray::GlobalShortcut::new(
            "toggle_win",
            mods,
            craftui_core::KeyCode::Space,
            "Ctrl+Shift+Space",
        ),
        move || {
            sc_c.set(sc_c.get() + 1);
        },
    );
    assert_eq!(tray.shortcuts().len(), 1);

    let matching = craftui_core::KeyEvent {
        code: craftui_core::KeyCode::Space,
        text: None,
        pressed: true,
        repeat: false,
        modifiers: mods,
    };
    assert!(tray.dispatch_key_event(&matching));
    assert_eq!(shortcut_hits.get(), 1);

    let repeat_matching = craftui_core::KeyEvent {
        repeat: true,
        ..matching.clone()
    };
    assert!(tray.dispatch_key_event(&repeat_matching));
    assert_eq!(shortcut_hits.get(), 1);

    let non_matching = craftui_core::KeyEvent {
        modifiers: craftui_core::Modifiers::default(),
        ..matching.clone()
    };
    assert!(!tray.dispatch_key_event(&non_matching));
    assert!(tray.unregister_shortcut("toggle_win"));
    assert!(tray.shortcuts().is_empty());

    // Verify self-unregistering inside a shortcut callback is preserved.
    let self_tray = tray.clone();
    tray.register_shortcut(
        crate::tray::GlobalShortcut::new("one_shot", mods, craftui_core::KeyCode::Space, "OneShot"),
        move || {
            self_tray.unregister_shortcut("one_shot");
        },
    );
    assert!(tray.dispatch_key_event(&matching));
    assert!(tray.shortcuts().is_empty());

    let dummy_id = winit::window::WindowId::dummy();
    assert!(!tray.is_window_hidden(dummy_id));
    tray.hide_window(dummy_id).unwrap();
    assert!(tray.is_window_hidden(dummy_id));
    tray.toggle_window(dummy_id).unwrap();
    assert!(!tray.is_window_hidden(dummy_id));
}
