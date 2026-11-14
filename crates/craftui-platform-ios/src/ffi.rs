use crate::host::{IosAppHost, IosMetalLayerDescriptor};
use crate::services::*;
use crate::types::*;
use craftui_core::{BatteryState, Permission, PermissionStatus, PointerPhase, UiDispatcher};
use std::ffi::{c_char, c_void};

// === C-ABI / UIKit Entrypoints callable directly from Swift/ObjC `UIViewController` ===

/// Allocate and return a new heap-boxed [`IosAppHost`] for UIKit lifecycle ownership.
#[unsafe(no_mangle)]
pub extern "C" fn craftui_ios_app_create(
    width_pt: f32,
    height_pt: f32,
    scale_factor: f32,
) -> *mut IosAppHost {
    let host = IosAppHost::new(UiDispatcher::new(), width_pt, height_pt, scale_factor);
    Box::into_raw(Box::new(host))
}

/// Launch a CraftUI iOS host attached to a `CAMetalLayer` descriptor and optional C builder callback.
///
/// # Safety
/// `layer.ca_metal_layer` (on iOS) must be valid or null if running offscreen; `user_data` is passed
/// verbatim to `build` if provided.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_launch(
    layer: IosMetalLayerDescriptor,
    build: Option<IosBuildUiCallback>,
    user_data: *mut c_void,
) -> *mut IosAppHost {
    let raw = craftui_ios_app_create(layer.width_pt, layer.height_pt, layer.contents_scale);
    if let Some(build_fn) = build {
        build_fn(raw, user_data);
    }
    if !layer.ca_metal_layer.is_null() {
        // SAFETY: `raw` was just allocated by `craftui_ios_app_create` and `layer.ca_metal_layer` invariant is upheld by caller.
        let host = unsafe { &mut *raw };
        let _ = unsafe { host.attach_metal(layer) };
    }
    raw
}

/// Attach or reattach a `CAMetalLayer` after creation. Returns `0` on success or `-1` on error
/// (inspect the message with [`craftui_ios_app_last_error`]).
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer and `layer.ca_metal_layer` (on iOS) must be
/// a valid retained `CAMetalLayer` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_attach_metal(
    app: *mut IosAppHost,
    layer: IosMetalLayerDescriptor,
) -> i32 {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    match unsafe { host.attach_metal(layer) } {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Returns a NUL-terminated UTF-8 pointer to the most recent error message, or NULL when clear.
///
/// # Safety
/// `app` must be null or a valid [`IosAppHost`] pointer. The returned pointer remains valid only
/// until the next call that mutates or destroys `app`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_last_error(app: *const IosAppHost) -> *const c_char {
    let Some(host) = (unsafe { app.as_ref() }) else {
        return std::ptr::null();
    };
    host.last_error_cstr
        .as_ref()
        .map_or(std::ptr::null(), |s| s.as_ptr())
}

/// Clear the stored error message on `app`.
///
/// # Safety
/// `app` must be null or a valid [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_clear_error(app: *mut IosAppHost) {
    if let Some(host) = unsafe { app.as_mut() } {
        host.clear_error();
    }
}

/// Dispose and free an [`IosAppHost`] created by [`craftui_ios_app_create`].
///
/// # Safety
/// `app` must be null or a valid pointer previously returned by [`craftui_ios_app_create`] and not yet destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_destroy(app: *mut IosAppHost) {
    if !app.is_null() {
        // SAFETY: Caller transfers unique ownership of `app` back for deallocation.
        drop(unsafe { Box::from_raw(app) });
    }
}

/// Notify UIKit view size / screen scale changes.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_resize(
    app: *mut IosAppHost,
    width_pt: f32,
    height_pt: f32,
    scale_factor: f32,
) {
    if let Some(host) = unsafe { app.as_mut() } {
        host.resize_pt(width_pt, height_pt, scale_factor);
    }
}

/// Update iOS notch / Dynamic Island / Home Indicator safe-area insets in points.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_set_safe_area(
    app: *mut IosAppHost,
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
) {
    if let Some(host) = unsafe { app.as_mut() } {
        host.set_safe_area(IosSafeAreaInsets {
            top,
            right,
            bottom,
            left,
        });
    }
}

/// Update software keyboard overlap from `keyboardWillChangeFrameNotification`.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_set_keyboard(
    app: *mut IosAppHost,
    overlap_height_pt: f32,
    visible: bool,
) {
    if let Some(host) = unsafe { app.as_mut() } {
        host.set_keyboard_frame(IosKeyboardFrame {
            overlap_height_pt,
            visible,
        });
    }
}

/// Dispatch a UIKit `UITouch` phase (`0=Down, 1=Move, 2=Up, 3=Cancel`) in logical points.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_touch(
    app: *mut IosAppHost,
    touch_id: u64,
    phase: u8,
    x_pt: f32,
    y_pt: f32,
    timestamp_ms: u64,
) -> bool {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return false;
    };
    let p = match phase {
        0 => PointerPhase::Down,
        1 => PointerPhase::Move,
        2 => PointerPhase::Up,
        _ => PointerPhase::Cancel,
    };
    host.push_ui_touch(touch_id, p, x_pt, y_pt, timestamp_ms)
}

/// Commit UTF-8 text from UIKit `UIKeyInput::insertText`.
///
/// # Safety
/// `app` must be valid and if `utf8_len > 0`, `utf8_ptr` must point to `utf8_len` valid UTF-8 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_insert_text(
    app: *mut IosAppHost,
    utf8_ptr: *const u8,
    utf8_len: usize,
) -> bool {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return false;
    };
    if utf8_ptr.is_null() || utf8_len == 0 {
        return false;
    }
    // SAFETY: Caller guarantees `utf8_ptr..utf8_ptr+utf8_len` is valid for reading during this call.
    let slice = unsafe { std::slice::from_raw_parts(utf8_ptr, utf8_len) };
    let text = String::from_utf8_lossy(slice).into_owned();
    host.push_insert_text(&text)
}

/// Handle `UIKeyInput::deleteBackward`.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_delete_backward(app: *mut IosAppHost) -> bool {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return false;
    };
    host.push_delete_backward()
}

/// Set marked (IME preedit) UTF-8 text from UIKit `UITextInput::setMarkedText`.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer and if `utf8_len > 0`, `utf8_ptr` must
/// point to `utf8_len` valid UTF-8 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_set_marked_text(
    app: *mut IosAppHost,
    utf8_ptr: *const u8,
    utf8_len: usize,
    cursor_end: usize,
) -> bool {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return false;
    };
    if utf8_ptr.is_null() || utf8_len == 0 {
        return host.push_marked_text("", 0);
    }
    // SAFETY: Caller guarantees `utf8_ptr..utf8_ptr+utf8_len` is valid for reading during this call.
    let slice = unsafe { std::slice::from_raw_parts(utf8_ptr, utf8_len) };
    let text = String::from_utf8_lossy(slice).into_owned();
    host.push_marked_text(&text, cursor_end)
}

/// Advance one `CADisplayLink` frame. Returns a bitmask:
/// - bit 0 (`0b001`): animating
/// - bit 1 (`0b010`): wants keyboard
/// - bit 2 (`0b100`): error recorded (inspect via [`craftui_ios_app_last_error`])
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_tick(app: *mut IosAppHost, dt_sec: f32) -> u8 {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return 0;
    };
    let (anim, cmds) = host.display_link_tick(dt_sec);
    let wants_kb = cmds.iter().any(|c| {
        matches!(
            c,
            IosImeCommand::BecomeFirstResponder | IosImeCommand::SyncText(_)
        )
    });
    let has_err = host.last_error().is_some();
    (u8::from(anim)) | (u8::from(wants_kb) << 1) | (u8::from(has_err) << 2)
}

unsafe fn read_utf8_key<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if ptr.is_null() || len == 0 {
        return None;
    }
    // SAFETY: Caller upholds `ptr..ptr+len` is valid for reading during this C-ABI call.
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(slice).ok()
}

/// Query cached permission status code (`0=NotDetermined..7=Unsupported`) by permission key (`"camera"`, `"microphone"`, etc.).
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer and `key_ptr..key_ptr+key_len` must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_permission_status_from_key(
    app: *const IosAppHost,
    key_ptr: *const u8,
    key_len: usize,
) -> u8 {
    let Some(host) = (unsafe { app.as_ref() }) else {
        return permission_status_to_code(PermissionStatus::Unsupported);
    };
    let Some(key) = (unsafe { read_utf8_key(key_ptr, key_len) }) else {
        return permission_status_to_code(PermissionStatus::Unsupported);
    };
    let perm = Permission::from_key(key);
    permission_status_to_code(host.tree.services.check_permission(&perm))
}

/// Update cached OS permission authorization state pushed from UIKit / AVFoundation / CoreLocation.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer and `key_ptr..key_ptr+key_len` must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_set_permission_status(
    app: *mut IosAppHost,
    key_ptr: *const u8,
    key_len: usize,
    status_code: u8,
) {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return;
    };
    let Some(key) = (unsafe { read_utf8_key(key_ptr, key_len) }) else {
        return;
    };
    host.bridge.set_permission_status(
        Permission::from_key(key),
        permission_status_from_code(status_code),
        false,
    );
    host.request_redraw();
}

/// Deliver an asynchronous permission request result from Swift back into Rust waiters and `UiDispatcher`.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer and `key_ptr..key_ptr+key_len` must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_deliver_permission_result(
    app: *mut IosAppHost,
    request_id: u64,
    key_ptr: *const u8,
    key_len: usize,
    status_code: u8,
    rationale: bool,
) {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return;
    };
    let Some(key) = (unsafe { read_utf8_key(key_ptr, key_len) }) else {
        return;
    };
    host.bridge.deliver_permission_result(
        request_id,
        key,
        permission_status_from_code(status_code),
        rationale,
    );
    host.request_redraw();
}

/// Poll the next pending [`IosPlatformCommand`] serialized as UTF-8 JSON for Swift `CraftUIViewController`.
/// Returns:
/// - `0` when the command queue is empty
/// - `> 0` byte length written into `out_buf` (command is popped)
/// - `< 0` negated required byte length when `out_buf` is null or `out_cap` is insufficient (command stays queued)
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer. If `out_buf` is non-null, `out_cap` bytes
/// starting at `out_buf` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_poll_command_json(
    app: *mut IosAppHost,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return 0;
    };
    let Ok(mut queue) = host.bridge.commands.lock() else {
        return 0;
    };
    let Some(front) = queue.front() else {
        return 0;
    };
    let json = front.to_json();
    let bytes = json.as_bytes();
    if out_buf.is_null() || out_cap < bytes.len() {
        return -(bytes.len() as i32);
    }
    // SAFETY: Caller guarantees `out_buf` points to at least `out_cap >= bytes.len()` writable bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, bytes.len());
    }
    queue.pop_front();
    bytes.len() as i32
}

/// Forward a UIKit `UIApplication` lifecycle transition (`0=Resumed, 1=Paused, 2=Inactive, 3=Detached, 4=MemoryWarning`).
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_push_lifecycle(app: *mut IosAppHost, event_code: u8) {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return;
    };
    if let Some(ev) = lifecycle_event_from_code(event_code)
        && let Ok(mut hist) = host.bridge.lifecycle_history.lock()
    {
        hist.push(ev);
    }
}

/// Update iOS `NWPathMonitor` network classification (`0=Unknown, 1=Offline, 2=Wifi, 3=Cellular, 4=Ethernet`).
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_set_network_status(
    app: *mut IosAppHost,
    network_code: u8,
) {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return;
    };
    if let Ok(mut guard) = host.bridge.network_status.lock() {
        *guard = network_status_from_code(network_code);
    }
}

/// Update `UIDevice` battery state (`level_pct < 0` indicates unknown level).
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_set_battery(
    app: *mut IosAppHost,
    level_pct: i16,
    charging: bool,
) {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return;
    };
    let level_pct = if (0..=100).contains(&level_pct) {
        Some(level_pct as u8)
    } else {
        None
    };
    if let Ok(mut guard) = host.bridge.battery_state.lock() {
        *guard = BatteryState {
            level_pct,
            charging,
        };
    }
}
