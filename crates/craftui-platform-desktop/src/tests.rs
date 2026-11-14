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

    let access_ev = NotifyEvent::new(notify::EventKind::Access(
        notify::event::AccessKind::Open(notify::event::AccessMode::Read),
    ))
    .add_path(PathBuf::from("ui/main.cui"));
    assert!(!should_trigger_hot_reload(&access_ev));

    let modify_cui = NotifyEvent::new(notify::EventKind::Modify(
        notify::event::ModifyKind::Data(notify::event::DataChange::Content),
    ))
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
