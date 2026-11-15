//! Desktop platform runner (`winit` + `wgpu`/`vello`/`gpu-sdf` + `accesskit_winit` + `notify` hot-reload)
//! supporting Linux (Wayland/X11), macOS, and Windows with multi-window support, in-process native OS
//! clipboard (`x11-clipboard`/`wl-clipboard-rs`/`objc2-app-kit`/`windows-sys`), `rfd` native file
//! pickers (`xdg-portal` + `wayland`, AppKit, Win32 IFileDialog), cursor synchronization, and timer scheduling.

use accesskit_winit::Adapter as AccessKitAdapter;
use craftui_core::{
    BatteryState, ClipboardBackend, Color, CraftError, CursorIcon, DeviceCapabilitiesBackend,
    DeviceInfo, FileDialogBackend, FileDialogRequest, ImeEvent, KeyCode, KeyEvent,
    MemorySecureStorage, Modifiers, NetworkStatus, NodeId, Permission, PermissionBackend,
    PermissionStatus, PlatformServices, Point, PointerButton, PointerEvent, PointerId,
    PointerPhase, Result, ScrollDelta, SecureStorageBackend, ShareBackend, SharePayload, Size,
    UiDispatcher, UiEvent, WindowMetrics, monotonic_ms,
};
#[cfg(feature = "hot-reload")]
use craftui_core::{BoxShadow, Brush, CornerRadii, Rect, ThemeMode, ThemeTokens};
use craftui_reactive::Scope;
#[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
use craftui_render::GpuSurfacePipeline;
#[cfg(feature = "hot-reload")]
use craftui_render::PaintCommand;
use craftui_render::{PaintScene, SoftwareRenderer, TextureRegistry};
#[cfg(feature = "hot-reload")]
use craftui_runtime::HotReloadSession;
use craftui_text::TextEngine;
#[cfg(feature = "hot-reload")]
use craftui_text::TextStyle;
use craftui_widgets::WidgetTree;
#[cfg(feature = "hot-reload")]
use notify::{Event as NotifyEvent, RecursiveMode, Watcher};
use std::cell::RefCell;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{
    ElementState, Ime as WinitIme, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::event_loop::{
    ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle,
};
use winit::keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{CursorIcon as WinitCursorIcon, Window, WindowAttributes, WindowId};

/// Desktop window configuration.
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub force_cpu: bool,
    pub resizable: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "CraftUI Application".to_string(),
            width: 1024.0,
            height: 768.0,
            force_cpu: false,
            resizable: true,
        }
    }
}

#[derive(Default)]
struct DesktopClipboardState {
    fallback: String,
    #[cfg(all(unix, not(target_os = "macos")))]
    x11: Option<x11_clipboard::Clipboard>,
}

/// In-process permissive OS clipboard bridge (`wl-clipboard-rs` + `x11-clipboard` on Linux/BSD,
/// `objc2-app-kit` `NSPasteboard` on macOS, `windows-sys` Win32 Clipboard on Windows) with an
/// in-memory fallback cache for headless CI environments without a display server.
#[derive(Clone, Default)]
pub struct DesktopClipboard {
    state: Arc<Mutex<DesktopClipboardState>>,
}

impl ClipboardBackend for DesktopClipboard {
    fn get_text(&mut self) -> Option<String> {
        let Ok(mut guard) = self.state.lock() else {
            return None;
        };
        if let Some(sys) = read_os_clipboard(&mut guard) {
            guard.fallback = sys.clone();
            return Some(sys);
        }
        Some(guard.fallback.clone())
    }

    fn set_text(&mut self, text: &str) -> bool {
        let Ok(mut guard) = self.state.lock() else {
            return false;
        };
        guard.fallback.clear();
        guard.fallback.push_str(text);
        let _ = write_os_clipboard(&mut guard, text);
        true
    }
}

fn read_os_clipboard(
    #[allow(unused_variables)] state: &mut DesktopClipboardState,
) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
        let pb = NSPasteboard::generalPasteboard();
        // SAFETY: `NSPasteboardTypeString` is a static immutable AppKit UTI constant.
        let pb_type = unsafe { NSPasteboardTypeString };
        pb.stringForType(pb_type).map(|ns| ns.to_string())
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::DataExchange::{
            CloseClipboard, GetClipboardData, OpenClipboard,
        };
        use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

        const CF_UNICODETEXT: u32 = 13;
        struct ClipboardCloseGuard;
        impl Drop for ClipboardCloseGuard {
            fn drop(&mut self) {
                // SAFETY: Paired with a successful `OpenClipboard` before guard construction.
                unsafe {
                    CloseClipboard();
                }
            }
        }

        // SAFETY: Opening and reading global clipboard memory handle within balanced Open/Close.
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return None;
            }
            let _guard = ClipboardCloseGuard;
            let handle: HANDLE = GetClipboardData(CF_UNICODETEXT);
            if handle.is_null() {
                return None;
            }
            let ptr = GlobalLock(handle) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let byte_len = GlobalSize(handle);
            let max_u16 = byte_len / std::mem::size_of::<u16>();
            let slice = std::slice::from_raw_parts(ptr, max_u16);
            let nul_pos = slice.iter().position(|&u| u == 0).unwrap_or(slice.len());
            let decoded = String::from_utf16_lossy(&slice[..nul_pos]);
            GlobalUnlock(handle);
            Some(decoded)
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            && let Ok((mut pipe, _mime)) = wl_clipboard_rs::paste::get_contents(
                wl_clipboard_rs::paste::ClipboardType::Regular,
                wl_clipboard_rs::paste::Seat::Unspecified,
                wl_clipboard_rs::paste::MimeType::Text,
            )
        {
            let mut buf = String::new();
            if std::io::Read::read_to_string(&mut pipe, &mut buf).is_ok() {
                return Some(buf);
            }
        }
        if std::env::var_os("DISPLAY").is_some() {
            if state.x11.is_none() {
                state.x11 = x11_clipboard::Clipboard::new().ok();
            }
            if let Some(x11) = state.x11.as_ref()
                && let Ok(bytes) = x11.load(
                    x11.getter.atoms.clipboard,
                    x11.getter.atoms.utf8_string,
                    x11.getter.atoms.property,
                    Some(Duration::from_millis(250)),
                )
                && let Ok(text) = String::from_utf8(bytes)
            {
                return Some(text);
            }
        }
        None
    }
}

fn write_os_clipboard(
    #[allow(unused_variables)] state: &mut DesktopClipboardState,
    text: &str,
) -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
        use objc2_foundation::NSString;
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        let ns_text = NSString::from_str(text);
        // SAFETY: `NSPasteboardTypeString` is a static immutable AppKit UTI constant.
        let pb_type = unsafe { NSPasteboardTypeString };
        pb.setString_forType(&ns_text, pb_type)
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
        use windows_sys::Win32::System::DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
        };
        use windows_sys::Win32::System::Memory::{
            GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
        };

        const CF_UNICODETEXT: u32 = 13;
        struct ClipboardCloseGuard;
        impl Drop for ClipboardCloseGuard {
            fn drop(&mut self) {
                // SAFETY: Paired with a successful `OpenClipboard` before guard construction.
                unsafe {
                    CloseClipboard();
                }
            }
        }

        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let byte_len = wide.len() * std::mem::size_of::<u16>();
        // SAFETY: Allocates movable HGLOBAL, fills UTF-16 payload, transfers ownership to clipboard on success.
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return false;
            }
            let _guard = ClipboardCloseGuard;
            if EmptyClipboard() == 0 {
                return false;
            }
            let hmem: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, byte_len);
            if hmem.is_null() {
                return false;
            }
            let ptr = GlobalLock(hmem) as *mut u16;
            if ptr.is_null() {
                GlobalFree(hmem);
                return false;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            GlobalUnlock(hmem);
            if SetClipboardData(CF_UNICODETEXT, hmem as HANDLE).is_null() {
                GlobalFree(hmem);
                return false;
            }
            true
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            && wl_clipboard_rs::copy::Options::new()
                .copy(
                    wl_clipboard_rs::copy::Source::Bytes(text.as_bytes().into()),
                    wl_clipboard_rs::copy::MimeType::Text,
                )
                .is_ok()
        {
            return true;
        }
        if std::env::var_os("DISPLAY").is_some() {
            if state.x11.is_none() {
                state.x11 = x11_clipboard::Clipboard::new().ok();
            }
            if let Some(x11) = state.x11.as_ref()
                && x11
                    .store(
                        x11.setter.atoms.clipboard,
                        x11.setter.atoms.utf8_string,
                        text.as_bytes().to_vec(),
                    )
                    .is_ok()
            {
                return true;
            }
        }
        false
    }
}

/// Pure permissive native OS file open/save picker powered in-process by `rfd` (`xdg-portal` + `wayland`
/// on Linux/BSD, AppKit `NSOpenPanel`/`NSSavePanel` on macOS, Win32 `IFileDialog` on Windows).
///
/// Use [`PlatformServices::pick_open_file_async`], [`PlatformServices::pick_open_files_async`], or
/// [`PlatformServices::pick_save_file_async`] from UI handlers so dialogs run off the UI thread.
#[derive(Debug, Default, Clone, Copy)]
pub struct DesktopFileDialog;

/// Build an `rfd::FileDialog` configured from a platform-neutral [`FileDialogRequest`].
pub fn build_rfd_dialog(request: &FileDialogRequest) -> rfd::FileDialog {
    let mut dialog = rfd::FileDialog::new();
    if !request.title.is_empty() {
        dialog = dialog.set_title(&request.title);
    }
    if let Some(dir) = &request.default_directory {
        dialog = dialog.set_directory(dir);
    }
    if let Some(name) = &request.default_name {
        dialog = dialog.set_file_name(name);
    }
    for filter in &request.filters {
        if !filter.extensions.is_empty() {
            let exts: Vec<&str> = filter.extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(&filter.name, &exts);
        }
    }
    dialog
}

#[cfg(all(unix, not(target_os = "macos")))]
fn has_unix_display() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn has_unix_display() -> bool {
    true
}

impl FileDialogBackend for DesktopFileDialog {
    fn open_files(&self, request: &FileDialogRequest) -> Vec<PathBuf> {
        if !has_unix_display() {
            return Vec::new();
        }
        let dialog = build_rfd_dialog(request);
        match (request.directory, request.multiple) {
            (true, true) => dialog.pick_folders().unwrap_or_default(),
            (true, false) => dialog.pick_folder().into_iter().collect(),
            (false, true) => dialog.pick_files().unwrap_or_default(),
            (false, false) => dialog.pick_file().into_iter().collect(),
        }
    }

    fn save_file(&self, request: &FileDialogRequest) -> Option<PathBuf> {
        if !has_unix_display() {
            return None;
        }
        build_rfd_dialog(request).save_file()
    }
}

/// Desktop host permission backend reporting desktop-accessible capabilities as [`PermissionStatus::Granted`],
/// mobile/telephony-only capabilities as [`PermissionStatus::Unsupported`], and honoring explicit test/host overrides.
#[derive(Debug, Default)]
pub struct DesktopPermissionBackend {
    overrides: Mutex<HashMap<Permission, PermissionStatus>>,
    launch_os_settings: bool,
}

impl DesktopPermissionBackend {
    pub fn new() -> Self {
        Self {
            overrides: Mutex::new(HashMap::new()),
            launch_os_settings: true,
        }
    }

    pub fn with_status(self, permission: Permission, status: PermissionStatus) -> Self {
        self.set_status(permission, status);
        self
    }

    pub fn with_settings_launcher(mut self, enabled: bool) -> Self {
        self.launch_os_settings = enabled;
        self
    }

    pub fn set_status(&self, permission: Permission, status: PermissionStatus) {
        if let Ok(mut guard) = self.overrides.lock() {
            guard.insert(permission, status);
        }
    }

    pub fn default_desktop_status(permission: &Permission) -> PermissionStatus {
        match permission {
            Permission::Camera
            | Permission::Microphone
            | Permission::LocationWhenInUse
            | Permission::LocationCoarse
            | Permission::Photos
            | Permission::PhotosAddOnly
            | Permission::Videos
            | Permission::AudioLibrary
            | Permission::Notifications
            | Permission::Bluetooth
            | Permission::BluetoothScan
            | Permission::BluetoothConnect
            | Permission::BluetoothAdvertise
            | Permission::Biometrics
            | Permission::Contacts
            | Permission::CalendarRead
            | Permission::CalendarWrite
            | Permission::StorageRead
            | Permission::StorageWrite
            | Permission::BackgroundRefresh
            | Permission::MediaProjection
            | Permission::ClipboardRead => PermissionStatus::Granted,
            Permission::LocationAlways
            | Permission::Sensors
            | Permission::MotionActivity
            | Permission::ManageExternalStorage
            | Permission::Nfc
            | Permission::PhoneState
            | Permission::CallPhone
            | Permission::SmsRead
            | Permission::SmsSend
            | Permission::SystemAlertWindow
            | Permission::ScheduleExactAlarm
            | Permission::InstallPackages
            | Permission::Vibrate
            | Permission::DeviceAdmin
            | Permission::WriteSettings
            | Permission::PackageUsageStats
            | Permission::IgnoreBatteryOptimizations
            | Permission::NotificationListener
            | Permission::VpnService
            | Permission::Custom(_) => PermissionStatus::Unsupported,
            Permission::AccessibilityService => PermissionStatus::Granted,
            Permission::AdminPrivileges => {
                #[cfg(target_os = "windows")]
                {
                    PermissionStatus::Denied
                }
                #[cfg(unix)]
                {
                    if std::env::var("USER").map(|u| u == "root").unwrap_or(false)
                        || std::env::var("SUDO_USER").is_ok()
                    {
                        PermissionStatus::Granted
                    } else {
                        PermissionStatus::Denied
                    }
                }
                #[cfg(not(any(target_os = "windows", unix)))]
                {
                    PermissionStatus::Denied
                }
            }
        }
    }
}

impl PermissionBackend for DesktopPermissionBackend {
    fn check(&self, permission: &Permission) -> PermissionStatus {
        if let Ok(guard) = self.overrides.lock()
            && let Some(status) = guard.get(permission)
        {
            return *status;
        }
        Self::default_desktop_status(permission)
    }

    fn request(&self, permission: &Permission) -> PermissionStatus {
        self.check(permission)
    }

    fn should_show_rationale(&self, _permission: &Permission) -> bool {
        false
    }

    fn open_app_settings(&self) -> bool {
        if !self.launch_os_settings || cfg!(test) || !has_unix_display() {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security")
                .spawn()
                .is_ok()
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", "ms-settings:privacy"])
                .spawn()
                .is_ok()
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            std::process::Command::new("xdg-open")
                .arg("settings://privacy")
                .spawn()
                .is_ok()
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            all(unix, not(target_os = "macos"))
        )))]
        {
            false
        }
    }

    fn open_accessibility_settings(&self) -> bool {
        if !self.launch_os_settings || cfg!(test) || !has_unix_display() {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
                )
                .spawn()
                .is_ok()
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", "ms-settings:easeofaccess"])
                .spawn()
                .is_ok()
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            std::process::Command::new("gnome-control-center")
                .arg("universal-access")
                .spawn()
                .or_else(|_| {
                    std::process::Command::new("xdg-open")
                        .arg("settings://accessibility")
                        .spawn()
                })
                .is_ok()
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            all(unix, not(target_os = "macos"))
        )))]
        {
            false
        }
    }

    fn open_admin_settings(&self) -> bool {
        if !self.launch_os_settings || cfg!(test) || !has_unix_display() {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security")
                .spawn()
                .is_ok()
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", "ms-settings:privacy"])
                .spawn()
                .is_ok()
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            std::process::Command::new("gnome-control-center")
                .arg("user-accounts")
                .spawn()
                .is_ok()
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            all(unix, not(target_os = "macos"))
        )))]
        {
            false
        }
    }

    fn is_accessibility_enabled(&self) -> bool {
        self.check(&Permission::AccessibilityService) == PermissionStatus::Granted
    }

    fn is_admin_active(&self) -> bool {
        self.check(&Permission::AdminPrivileges) == PermissionStatus::Granted
    }

    // On Windows the platform branch below is a constant, which clippy would
    // ask to fold into the check above; the other platforms need both parts.
    #[cfg_attr(target_os = "windows", allow(clippy::needless_bool))]
    fn request_admin_privileges(&self) -> bool {
        if self.is_admin_active() {
            return true;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("osascript")
                .args([
                    "-e",
                    "do shell script \"echo admin\" with administrator privileges",
                ])
                .output()
                .map(|out| out.status.success())
                .unwrap_or(false)
        }
        #[cfg(target_os = "windows")]
        {
            false
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            std::process::Command::new("pkexec")
                .args(["echo", "admin"])
                .output()
                .map(|out| out.status.success())
                .unwrap_or(false)
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            all(unix, not(target_os = "macos"))
        )))]
        {
            false
        }
    }
}

/// Desktop share adapter formatting shared text/URL/file summaries into the OS clipboard (`DesktopClipboard`).
#[derive(Clone, Default)]
pub struct DesktopShareBackend {
    clipboard: DesktopClipboard,
}

impl DesktopShareBackend {
    pub fn new(clipboard: DesktopClipboard) -> Self {
        Self { clipboard }
    }

    pub fn format_payload(payload: &SharePayload) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(title) = payload.title.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push(title.trim().to_string());
        }
        if let Some(text) = payload.text.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push(text.trim().to_string());
        }
        if let Some(url) = payload.url.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push(url.trim().to_string());
        }
        if !payload.files.is_empty() {
            let paths: Vec<String> = payload
                .files
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            parts.push(paths.join(", "));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }
}

impl ShareBackend for DesktopShareBackend {
    fn share(&self, payload: &SharePayload) -> bool {
        let Some(formatted) = Self::format_payload(payload) else {
            return false;
        };
        let mut cb = self.clipboard.clone();
        cb.set_text(&formatted)
    }
}

/// Desktop session credential store backed by [`MemorySecureStorage`].
#[derive(Debug, Default)]
pub struct DesktopSecureStorage {
    inner: MemorySecureStorage,
}

impl DesktopSecureStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecureStorageBackend for DesktopSecureStorage {
    fn get_secret(&self, key: &str) -> Option<String> {
        self.inner.get_secret(key)
    }

    fn set_secret(&self, key: &str, value: &str) -> bool {
        self.inner.set_secret(key, value)
    }

    fn delete_secret(&self, key: &str) -> bool {
        self.inner.delete_secret(key)
    }
}

/// Desktop hardware/OS telemetry provider reading `/etc/os-release`, sysfs network interfaces, and
/// battery sysfs on Linux with deterministic fallbacks on macOS/Windows/CI hosts.
#[derive(Debug, Default)]
pub struct DesktopDeviceCapabilities {
    network_override: Mutex<Option<NetworkStatus>>,
    battery_override: Mutex<Option<BatteryState>>,
}

impl DesktopDeviceCapabilities {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_network_override(&self, status: Option<NetworkStatus>) {
        if let Ok(mut guard) = self.network_override.lock() {
            *guard = status;
        }
    }

    pub fn set_battery_override(&self, state: Option<BatteryState>) {
        if let Ok(mut guard) = self.battery_override.lock() {
            *guard = state;
        }
    }
}

fn read_desktop_os_version() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
                    return rest.trim().trim_matches('"').to_string();
                }
            }
        }
    }
    std::env::consts::FAMILY.to_string()
}

fn probe_desktop_network() -> NetworkStatus {
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
            let mut saw_any = false;
            let mut has_wifi = false;
            let mut has_ethernet = false;
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str == "lo" {
                    continue;
                }
                saw_any = true;
                let path = entry.path();
                if let Ok(state) = std::fs::read_to_string(path.join("operstate"))
                    && state.trim() == "up"
                {
                    if path.join("wireless").exists() || name_str.starts_with("wl") {
                        has_wifi = true;
                    } else {
                        has_ethernet = true;
                    }
                }
            }
            if has_wifi {
                return NetworkStatus::Wifi;
            }
            if has_ethernet {
                return NetworkStatus::Ethernet;
            }
            if saw_any {
                return NetworkStatus::Offline;
            }
        }
    }
    NetworkStatus::Unknown
}

fn probe_desktop_battery() -> BatteryState {
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if !name.to_string_lossy().starts_with("BAT") {
                    continue;
                }
                let path = entry.path();
                let pct = std::fs::read_to_string(path.join("capacity"))
                    .ok()
                    .and_then(|s| s.trim().parse::<u8>().ok())
                    .map(|v| v.min(100));
                let charging = std::fs::read_to_string(path.join("status"))
                    .map(|s| matches!(s.trim(), "Charging" | "Full"))
                    .unwrap_or(false);
                if pct.is_some() {
                    return BatteryState {
                        level_pct: pct,
                        charging,
                    };
                }
            }
        }
    }
    BatteryState::default()
}

impl DeviceCapabilitiesBackend for DesktopDeviceCapabilities {
    fn device_info(&self) -> DeviceInfo {
        DeviceInfo {
            os: std::env::consts::OS.to_string(),
            model: std::env::consts::ARCH.to_string(),
            os_version: read_desktop_os_version(),
            sdk_int: None,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    fn network_status(&self) -> NetworkStatus {
        if let Ok(guard) = self.network_override.lock()
            && let Some(status) = *guard
        {
            return status;
        }
        probe_desktop_network()
    }

    fn battery_state(&self) -> BatteryState {
        if let Ok(guard) = self.battery_override.lock()
            && let Some(state) = *guard
        {
            return state;
        }
        probe_desktop_battery()
    }
}

pub fn default_desktop_services() -> PlatformServices {
    let clipboard = DesktopClipboard::default();
    PlatformServices::new(clipboard.clone(), DesktopFileDialog)
        .with_permissions(DesktopPermissionBackend::new())
        .with_share(DesktopShareBackend::new(clipboard))
        .with_secure_storage(DesktopSecureStorage::new())
        .with_device_capabilities(DesktopDeviceCapabilities::new())
}

/// Custom event loop messages delivered from background worker threads, AccessKit, `.cui` watcher, or window opening.
#[derive(Debug)]
pub enum DesktopUserEvent {
    WakeUi,
    AccessKit(accesskit_winit::Event),
    #[cfg(feature = "hot-reload")]
    FileChanged,
    OpenWindow(u64),
}

impl From<accesskit_winit::Event> for DesktopUserEvent {
    fn from(ev: accesskit_winit::Event) -> Self {
        Self::AccessKit(ev)
    }
}

type BuildUiFn = Box<dyn FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId>;

struct PendingWindowRequest {
    config: WindowConfig,
    builder: BuildUiFn,
}

thread_local! {
    static PENDING_WINDOW_QUEUE: RefCell<(u64, HashMap<u64, PendingWindowRequest>)> =
        RefCell::new((1, HashMap::new()));
}

/// Cloneable handle enabling button click handlers and effects to spawn additional desktop windows.
#[derive(Clone)]
pub struct WindowLauncher {
    proxy: EventLoopProxy<DesktopUserEvent>,
}

impl WindowLauncher {
    /// Request opening a new secondary desktop window with its own independent [`WidgetTree`] & GPU surface.
    pub fn open_window<F>(&self, config: WindowConfig, build_ui: F) -> Result<()>
    where
        F: FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId + 'static,
    {
        let id = PENDING_WINDOW_QUEUE.with(|cell| {
            let mut guard = cell.borrow_mut();
            let id = guard.0;
            guard.0 = guard.0.wrapping_add(1).max(1);
            guard.1.insert(
                id,
                PendingWindowRequest {
                    config,
                    builder: Box::new(build_ui),
                },
            );
            id
        });
        self.proxy
            .send_event(DesktopUserEvent::OpenWindow(id))
            .map_err(|e| CraftError::Platform(format!("failed queuing open_window: {e}")))
    }
}

/// Platform services provided during app setup.
pub struct AppContext {
    pub dispatcher: UiDispatcher,
    pub textures: TextureRegistry,
    pub services: PlatformServices,
    launcher: WindowLauncher,
}

impl AppContext {
    pub fn window_launcher(&self) -> WindowLauncher {
        self.launcher.clone()
    }

    pub fn open_window<F>(&self, config: WindowConfig, build_ui: F) -> Result<()>
    where
        F: FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId + 'static,
    {
        self.launcher.open_window(config, build_ui)
    }
}

/// Run a CraftUI application in a desktop `winit` window (supporting multi-window spawning via `AppContext::open_window`).
pub fn run_app<F>(config: WindowConfig, build_ui: F) -> Result<()>
where
    F: FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId + 'static,
{
    let event_loop = EventLoop::<DesktopUserEvent>::with_user_event()
        .build()
        .map_err(|e| CraftError::Platform(format!("failed to create winit event loop: {e}")))?;
    let proxy = event_loop.create_proxy();

    let proxy_wake = proxy.clone();
    let dispatcher = UiDispatcher::with_waker(move || {
        let _ = proxy_wake.send_event(DesktopUserEvent::WakeUi);
    });

    let mut runner = DesktopRunner::new(config, proxy, dispatcher, Some(Box::new(build_ui)));
    event_loop
        .run_app(&mut runner)
        .map_err(|e| CraftError::Platform(format!("event loop exited with error: {e}")))
}

#[cfg(feature = "hot-reload")]
struct HotReloadSpec {
    path: PathBuf,
    component: Option<String>,
    session: HotReloadSession,
    show_hud: bool,
    show_inspector: bool,
    device_mode: bool,
    last_reload_error: Option<String>,
    frame_count: u32,
    fps_sample_time: Instant,
    current_fps: f32,
    _watcher: Option<notify::RecommendedWatcher>,
}

/// Determine whether a `notify` filesystem event should trigger `.cui` hot-reload.
///
/// Ignores read/open `Access` events emitted when CraftUI reads `.cui` files on Linux `inotify`.
#[cfg(feature = "hot-reload")]
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

/// Run an interactive live preview canvas for `.cui` files with device framing,
/// live telemetry HUD, syntax error banners, element inspector, and theme switching.
#[cfg(feature = "hot-reload")]
pub fn run_live_preview_cui(
    config: WindowConfig,
    cui_path: PathBuf,
    component: Option<String>,
    device_mode: bool,
    show_inspector: bool,
) -> Result<()> {
    let event_loop = EventLoop::<DesktopUserEvent>::with_user_event()
        .build()
        .map_err(|e| CraftError::Platform(format!("failed to create winit event loop: {e}")))?;
    let proxy = event_loop.create_proxy();

    let proxy_wake = proxy.clone();
    let dispatcher = UiDispatcher::with_waker(move || {
        let _ = proxy_wake.send_event(DesktopUserEvent::WakeUi);
    });

    let watch_proxy = proxy.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<NotifyEvent>| {
        if let Ok(ev) = res
            && should_trigger_hot_reload(&ev)
        {
            let _ = watch_proxy.send_event(DesktopUserEvent::FileChanged);
        }
    })
    .map_err(|e| CraftError::Platform(format!("file watcher init failed: {e}")))?;

    let watch_target = cui_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    watcher
        .watch(watch_target, RecursiveMode::Recursive)
        .map_err(|e| CraftError::Platform(format!("watch failed: {e}")))?;

    let root_scope = Scope::root();
    let mut tree = WidgetTree::new(root_scope);
    tree.set_platform_services(default_desktop_services());
    let mut session = HotReloadSession::new(root_scope);
    let mut initial_error = None;
    if let Err(e) = session.reload_file(&cui_path, component.as_deref(), &mut tree) {
        initial_error = Some(e.to_string());
    }

    let spec = HotReloadSpec {
        path: cui_path,
        component,
        session,
        show_hud: true,
        show_inspector,
        device_mode,
        last_reload_error: initial_error,
        frame_count: 0,
        fps_sample_time: Instant::now(),
        current_fps: 60.0,
        _watcher: Some(watcher),
    };

    let mut runner =
        DesktopRunner::new_with_tree(config, proxy, dispatcher, root_scope, tree, None);
    runner.set_primary_hot_reload(spec);
    event_loop
        .run_app(&mut runner)
        .map_err(|e| CraftError::Platform(format!("event loop exited with error: {e}")))
}

/// Run a live hot-reloading `.cui` preview window using `notify` + `craftui-runtime` (with full multi-file import support).
#[cfg(feature = "hot-reload")]
pub fn run_hot_reload_cui(
    config: WindowConfig,
    cui_path: PathBuf,
    component: Option<String>,
) -> Result<()> {
    run_live_preview_cui(config, cui_path, component, false, false)
}

struct InitialWindowSetup {
    config: WindowConfig,
    root_scope: Scope,
    tree: WidgetTree,
    builder: Option<BuildUiFn>,
    #[cfg(feature = "hot-reload")]
    hot_reload: Option<HotReloadSpec>,
}

struct SoftbufferPresenter {
    surface: softbuffer::Surface<OwnedDisplayHandle, Arc<Window>>,
    cpu: SoftwareRenderer,
    width: u32,
    height: u32,
}

impl SoftbufferPresenter {
    fn new(
        context: &softbuffer::Context<OwnedDisplayHandle>,
        window: Arc<Window>,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        let mut surface = softbuffer::Surface::new(context, window)
            .map_err(|e| CraftError::Render(format!("softbuffer surface creation failed: {e}")))?;
        let w = width.max(1);
        let h = height.max(1);
        if let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) {
            surface
                .resize(nw, nh)
                .map_err(|e| CraftError::Render(format!("softbuffer resize failed: {e}")))?;
        }
        Ok(Self {
            surface,
            cpu: SoftwareRenderer::new(w, h),
            width: w,
            height: h,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if self.width == w && self.height == h {
            return;
        }
        if let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h))
            && self.surface.resize(nw, nh).is_ok()
        {
            self.width = w;
            self.height = h;
        }
    }

    fn render_frame(
        &mut self,
        paint_scene: &PaintScene,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
        text_engine: &mut TextEngine,
    ) -> Result<()> {
        let w = self.width.max(1);
        let h = self.height.max(1);
        let rgba = self.cpu.render_straight_rgba(
            paint_scene,
            w,
            h,
            scale_factor,
            background,
            textures,
            text_engine,
        );
        let mut buffer = self
            .surface
            .buffer_mut()
            .map_err(|e| CraftError::Render(format!("softbuffer buffer_mut failed: {e}")))?;
        pack_rgba8_to_softbuffer_0rgb(&rgba, &mut buffer);
        buffer
            .present()
            .map_err(|e| CraftError::Render(format!("softbuffer present failed: {e}")))?;
        Ok(())
    }
}

/// Convert straight RGBA8 pixels (`[r, g, b, a]`) into `softbuffer`'s `0x00RRGGBB` layout.
pub fn pack_rgba8_to_softbuffer_0rgb(rgba: &[u8], dst: &mut [u32]) {
    let (chunks, _) = rgba.as_chunks::<4>();
    for (pixel_out, chunk) in dst.iter_mut().zip(chunks.iter()) {
        *pixel_out = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
    }
}

enum DesktopPresenter {
    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
    Wgpu(Box<GpuSurfacePipeline>),
    Softbuffer(Box<SoftbufferPresenter>),
}

impl DesktopPresenter {
    fn resize(&mut self, width: u32, height: u32) {
        match self {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            Self::Wgpu(gpu) => gpu.resize(width, height),
            Self::Softbuffer(sb) => sb.resize(width, height),
        }
    }

    fn render_frame(
        &mut self,
        paint_scene: &PaintScene,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
        text_engine: &mut TextEngine,
    ) -> Result<()> {
        match self {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            Self::Wgpu(gpu) => {
                gpu.render_frame(paint_scene, scale_factor, background, textures, text_engine)
            }
            Self::Softbuffer(sb) => {
                sb.render_frame(paint_scene, scale_factor, background, textures, text_engine)
            }
        }
    }

    fn label(&self) -> &'static str {
        match self {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            Self::Wgpu(gpu) => gpu.active_backend_name(),
            Self::Softbuffer(_) => "softbuffer (CPU)",
        }
    }
}

struct DesktopWindowState {
    window: Arc<Window>,
    root_scope: Scope,
    is_primary: bool,
    tree: WidgetTree,
    textures: TextureRegistry,
    scene: PaintScene,
    a11y: Option<AccessKitAdapter>,
    a11y_active: bool,
    presenter: Option<DesktopPresenter>,
    #[cfg(feature = "hot-reload")]
    hot_reload: Option<HotReloadSpec>,
    cursor_pos: Point,
    modifiers: Modifiers,
    mouse_down: bool,
    last_focused_was_input: bool,
    last_cursor: Option<WinitCursorIcon>,
}

fn sync_window_cursor(state: &mut DesktopWindowState) {
    let desired = translate_winit_cursor(state.tree.hovered_cursor());
    if state.last_cursor != Some(desired) {
        state.last_cursor = Some(desired);
        state.window.set_cursor(desired);
    }
}

struct DesktopRunner {
    proxy: EventLoopProxy<DesktopUserEvent>,
    dispatcher: UiDispatcher,
    services: PlatformServices,
    softbuffer_ctx: Option<softbuffer::Context<OwnedDisplayHandle>>,
    pending_primary: Option<InitialWindowSetup>,
    windows: HashMap<WindowId, DesktopWindowState>,
    start_instant: Instant,
    last_frame_instant: Instant,
}

impl DesktopRunner {
    fn new(
        config: WindowConfig,
        proxy: EventLoopProxy<DesktopUserEvent>,
        dispatcher: UiDispatcher,
        builder: Option<BuildUiFn>,
    ) -> Self {
        let root_scope = Scope::root();
        let mut tree = WidgetTree::new(root_scope);
        let services = default_desktop_services();
        tree.set_platform_services(services.clone());
        Self::new_with_tree(config, proxy, dispatcher, root_scope, tree, builder)
    }

    fn new_with_tree(
        config: WindowConfig,
        proxy: EventLoopProxy<DesktopUserEvent>,
        dispatcher: UiDispatcher,
        root_scope: Scope,
        mut tree: WidgetTree,
        builder: Option<BuildUiFn>,
    ) -> Self {
        let services = default_desktop_services();
        tree.set_platform_services(services.clone());
        tree.metrics = WindowMetrics {
            logical_size: Size::new(config.width, config.height),
            scale_factor: 1.0,
            ..Default::default()
        };
        Self {
            proxy,
            dispatcher,
            services,
            softbuffer_ctx: None,
            pending_primary: Some(InitialWindowSetup {
                config,
                root_scope,
                tree,
                builder,
                #[cfg(feature = "hot-reload")]
                hot_reload: None,
            }),
            windows: HashMap::new(),
            start_instant: Instant::now(),
            last_frame_instant: Instant::now(),
        }
    }

    #[cfg(feature = "hot-reload")]
    fn set_primary_hot_reload(&mut self, spec: HotReloadSpec) {
        if let Some(init) = self.pending_primary.as_mut() {
            init.hot_reload = Some(spec);
        }
    }

    fn create_softbuffer_presenter(
        &mut self,
        display_handle: OwnedDisplayHandle,
        window: Arc<Window>,
        width: u32,
        height: u32,
    ) -> Result<SoftbufferPresenter> {
        if self.softbuffer_ctx.is_none() {
            let ctx = softbuffer::Context::new(display_handle).map_err(|e| {
                CraftError::Render(format!("softbuffer context creation failed: {e}"))
            })?;
            self.softbuffer_ctx = Some(ctx);
        }
        let ctx = self
            .softbuffer_ctx
            .as_ref()
            .ok_or_else(|| CraftError::Render("softbuffer context missing".into()))?;
        SoftbufferPresenter::new(ctx, window, width, height)
    }

    fn timestamp_ms(&self) -> u64 {
        self.start_instant.elapsed().as_millis() as u64
    }

    fn spawn_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        setup: InitialWindowSetup,
        is_primary: bool,
    ) {
        let InitialWindowSetup {
            config,
            root_scope,
            mut tree,
            builder,
            #[cfg(feature = "hot-reload")]
            hot_reload,
        } = setup;
        let attrs = WindowAttributes::default()
            .with_title(config.title.clone())
            .with_inner_size(LogicalSize::new(config.width, config.height))
            .with_resizable(config.resizable)
            .with_visible(false);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("create_window failed: {e}");
                eprintln!("[craftui] create_window failed: {e}");
                if self.windows.is_empty() {
                    event_loop.exit();
                }
                return;
            }
        };

        let a11y = AccessKitAdapter::with_event_loop_proxy(event_loop, &window, self.proxy.clone());
        window.set_visible(true);

        let scale = window.scale_factor().max(1.0) as f32;
        let phys = window.inner_size();
        let init_w = if phys.width > 0 {
            phys.width
        } else {
            (config.width.max(1.0) * scale).round().max(1.0) as u32
        };
        let init_h = if phys.height > 0 {
            phys.height
        } else {
            (config.height.max(1.0) * scale).round().max(1.0) as u32
        };

        tree.set_platform_services(self.services.clone());
        tree.metrics = WindowMetrics {
            logical_size: Size::new(init_w as f32 / scale, init_h as f32 / scale),
            scale_factor: scale,
            ..Default::default()
        };

        let force_cpu = config.force_cpu
            || std::env::var("CRAFTUI_FORCE_CPU")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
            || !cfg!(any(feature = "gpu-sdf", feature = "gpu-vello"));

        let display_handle = event_loop.owned_display_handle();
        let presenter = if force_cpu {
            match self.create_softbuffer_presenter(
                display_handle.clone(),
                Arc::clone(&window),
                init_w,
                init_h,
            ) {
                Ok(sb) => Some(DesktopPresenter::Softbuffer(Box::new(sb))),
                Err(sb_err) => {
                    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
                    {
                        tracing::warn!(
                            "Softbuffer init failed ({sb_err}); trying wgpu CPU fallback"
                        );
                        match GpuSurfacePipeline::new_with_display(
                            || Arc::clone(&window),
                            || Box::new(display_handle.clone()),
                            init_w,
                            init_h,
                            true,
                        ) {
                            Ok(gpu) => Some(DesktopPresenter::Wgpu(Box::new(gpu))),
                            Err(gpu_err) => {
                                eprintln!(
                                    "[craftui] window '{}' failed to initialize any CPU presenter (softbuffer: {sb_err}; wgpu: {gpu_err})",
                                    config.title
                                );
                                None
                            }
                        }
                    }
                    #[cfg(not(any(feature = "gpu-sdf", feature = "gpu-vello")))]
                    {
                        eprintln!(
                            "[craftui] window '{}' failed to initialize softbuffer CPU presenter: {sb_err}",
                            config.title
                        );
                        None
                    }
                }
            }
        } else {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            {
                match GpuSurfacePipeline::new_with_display(
                    || Arc::clone(&window),
                    || Box::new(display_handle.clone()),
                    init_w,
                    init_h,
                    false,
                ) {
                    Ok(gpu) => Some(DesktopPresenter::Wgpu(Box::new(gpu))),
                    Err(gpu_err) => {
                        tracing::warn!(
                            "GpuSurfacePipeline initialization failed ({gpu_err}); falling back to softbuffer CPU presenter"
                        );
                        eprintln!(
                            "[craftui] GPU presenter unavailable ({gpu_err}); falling back to CPU softbuffer"
                        );
                        match self.create_softbuffer_presenter(
                            display_handle,
                            Arc::clone(&window),
                            init_w,
                            init_h,
                        ) {
                            Ok(sb) => Some(DesktopPresenter::Softbuffer(Box::new(sb))),
                            Err(sb_err) => {
                                eprintln!(
                                    "[craftui] window '{}' failed to initialize softbuffer fallback: {sb_err}",
                                    config.title
                                );
                                None
                            }
                        }
                    }
                }
            }
            #[cfg(not(any(feature = "gpu-sdf", feature = "gpu-vello")))]
            {
                unreachable!("force_cpu is always true when GPU backends are disabled")
            }
        };

        if let Some(p) = presenter.as_ref() {
            tracing::info!(
                "CraftUI Desktop Window '{}' Renderer initialized ({})",
                config.title,
                p.label()
            );
        }

        let mut app_ctx = AppContext {
            dispatcher: self.dispatcher.clone(),
            textures: TextureRegistry::new(),
            services: self.services.clone(),
            launcher: WindowLauncher {
                proxy: self.proxy.clone(),
            },
        };

        if let Some(build_fn) = builder {
            let root = build_fn(root_scope, &mut tree, &mut app_ctx);
            tree.set_root(root);
        }

        let win_id = window.id();
        window.request_redraw();
        self.windows.insert(
            win_id,
            DesktopWindowState {
                window,
                root_scope,
                is_primary,
                tree,
                textures: app_ctx.textures,
                scene: PaintScene::new(),
                a11y: Some(a11y),
                a11y_active: false,
                presenter,
                #[cfg(feature = "hot-reload")]
                hot_reload,
                cursor_pos: Point::ZERO,
                modifiers: Modifiers::default(),
                mouse_down: false,
                last_focused_was_input: false,
                last_cursor: None,
            },
        );
    }

    fn render_and_present_window(&mut self, id: WindowId) {
        let now = Instant::now();
        let dt = (now - self.last_frame_instant)
            .as_secs_f32()
            .clamp(0.0001, 0.1);
        self.last_frame_instant = now;

        let _ = craftui_reactive::tick_timers();
        let _ = self.dispatcher.drain();

        let Some(state) = self.windows.get_mut(&id) else {
            return;
        };

        let frame_start = Instant::now();
        let _ = state.tree.tick_blink_timers(monotonic_ms());
        let anim_tick = state.tree.tick_animations(dt);

        let layout_start = Instant::now();
        let dirty_count = state.tree.layout(state.tree.metrics);
        let layout_ms = layout_start.elapsed().as_secs_f32() * 1000.0;
        let animating = anim_tick || state.tree.has_active_animations();

        state.tree.paint(&mut state.scene);

        #[cfg(feature = "hot-reload")]
        if let Some(spec) = state.hot_reload.as_mut() {
            paint_live_preview_overlays(&mut state.scene, &state.tree, state.cursor_pos, spec);
        }

        let bg = state.tree.theme().background;
        let scale = state.tree.metrics.scale_factor;
        let tex_reg = &state.textures;
        if let Some(presenter) = state.presenter.as_mut()
            && let Err(e) = presenter.render_frame(
                &state.scene,
                scale,
                bg,
                tex_reg,
                &mut state.tree.text_engine,
            )
        {
            tracing::warn!("desktop frame render error: {e}");
            eprintln!("[craftui] frame render error: {e}");
        }

        if state.a11y_active
            && let Some(a11y) = state.a11y.as_mut()
            && let Some(update) = state.tree.build_a11y_update()
        {
            a11y.update_if_active(|| update);
        }

        let focused_is_input = state.tree.focused_is_text_input();
        if focused_is_input != state.last_focused_was_input {
            state.last_focused_was_input = focused_is_input;
            state.window.set_ime_allowed(focused_is_input);
        }
        sync_window_cursor(state);

        let frame_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        let node_count = state.tree.nodes.len();
        state
            .tree
            .inspector
            .record_frame(frame_ms, layout_ms, node_count, dirty_count);

        if animating {
            state.window.request_redraw();
        }
    }

    #[cfg(feature = "hot-reload")]
    fn reload_cui_from_disk(&mut self) {
        for state in self.windows.values_mut() {
            let Some(spec) = state.hot_reload.as_mut() else {
                continue;
            };
            match spec
                .session
                .reload_file(&spec.path, spec.component.as_deref(), &mut state.tree)
            {
                Ok(_) => {
                    spec.last_reload_error = None;
                    tracing::info!(
                        "Live-reloaded {} in {:.2}ms",
                        spec.path.display(),
                        spec.session.last_reload_duration.as_secs_f64() * 1000.0
                    );
                    state.window.request_redraw();
                }
                Err(e) => {
                    spec.last_reload_error = Some(e.to_string());
                    tracing::warn!("Live-reload diagnostic: {e}");
                    state.window.request_redraw();
                }
            }
        }
    }
}

#[cfg(feature = "hot-reload")]
fn paint_live_preview_overlays(
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

impl ApplicationHandler<DesktopUserEvent> for DesktopRunner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(init) = self.pending_primary.take() {
            event_loop.set_control_flow(ControlFlow::Wait);
            self.spawn_window(event_loop, init, true);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: DesktopUserEvent) {
        match event {
            DesktopUserEvent::WakeUi => {
                let _ = self.dispatcher.drain();
                for state in self.windows.values() {
                    state.window.request_redraw();
                }
            }
            DesktopUserEvent::AccessKit(ev) => {
                if let Some(state) = self.windows.get_mut(&ev.window_id) {
                    match &ev.window_event {
                        accesskit_winit::WindowEvent::ActionRequested(req) => {
                            if state.tree.handle_a11y_action(req) {
                                state.window.request_redraw();
                            }
                        }
                        accesskit_winit::WindowEvent::InitialTreeRequested => {
                            state.a11y_active = true;
                            state.tree.invalidate_a11y();
                            if let Some(a11y) = state.a11y.as_mut()
                                && let Some(update) = state.tree.build_a11y_update()
                            {
                                a11y.update_if_active(|| update);
                            }
                        }
                        accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                            state.a11y_active = false;
                            state.tree.invalidate_a11y();
                        }
                    }
                }
            }
            #[cfg(feature = "hot-reload")]
            DesktopUserEvent::FileChanged => {
                self.reload_cui_from_disk();
            }
            DesktopUserEvent::OpenWindow(req_id) => {
                let req = PENDING_WINDOW_QUEUE.with(|cell| cell.borrow_mut().1.remove(&req_id));
                if let Some(req) = req {
                    let child_scope = Scope::root().child_scope();
                    let child_tree = WidgetTree::new(child_scope);
                    self.spawn_window(
                        event_loop,
                        InitialWindowSetup {
                            config: req.config,
                            root_scope: child_scope,
                            tree: child_tree,
                            builder: Some(req.builder),
                            #[cfg(feature = "hot-reload")]
                            hot_reload: None,
                        },
                        false,
                    );
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now_ms = monotonic_ms();
        let timers_ran = craftui_reactive::tick_timers();
        let jobs_ran = self.dispatcher.drain();
        let mut min_delay = craftui_reactive::next_timer_delay_ms(now_ms);
        for state in self.windows.values_mut() {
            let blink_due = state.tree.tick_blink_timers(now_ms);
            if timers_ran > 0 || jobs_ran > 0 || blink_due {
                state.window.request_redraw();
            }
            if let Some(win_delay) = state.tree.next_internal_delay_ms(now_ms) {
                min_delay = Some(min_delay.map_or(win_delay, |cur| cur.min(win_delay)));
            }
        }
        if let Some(delay_ms) = min_delay {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(delay_ms.max(1)),
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = self.windows.get_mut(&id) else {
            return;
        };

        if let Some(a11y) = state.a11y.as_mut() {
            a11y.process_event(&state.window, &event);
        }

        match event {
            WindowEvent::CloseRequested => {
                if let Some(removed) = self.windows.remove(&id)
                    && !removed.is_primary
                {
                    removed.root_scope.dispose();
                }
                if self.windows.is_empty() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    if let Some(presenter) = state.presenter.as_mut() {
                        presenter.resize(size.width, size.height);
                    }
                    let sf = state.tree.metrics.scale_factor.max(1.0);
                    state.tree.handle_event(&UiEvent::Resize {
                        width: size.width as f32 / sf,
                        height: size.height as f32 / sf,
                        scale_factor: sf,
                    });
                    state.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let sf = (scale_factor as f32).max(1.0);
                let phys = state.window.inner_size();
                if phys.width > 0 && phys.height > 0 {
                    if let Some(presenter) = state.presenter.as_mut() {
                        presenter.resize(phys.width, phys.height);
                    }
                    state.tree.handle_event(&UiEvent::Resize {
                        width: phys.width as f32 / sf,
                        height: phys.height as f32 / sf,
                        scale_factor: sf,
                    });
                    state.window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let sf = state.tree.metrics.scale_factor.max(1.0);
                state.cursor_pos = Point::new(position.x as f32 / sf, position.y as f32 / sf);
                let ts = self.start_instant.elapsed().as_millis() as u64;
                let handled = state.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                    id: PointerId(1),
                    phase: PointerPhase::Move,
                    position: state.cursor_pos,
                    button: if state.mouse_down {
                        Some(PointerButton::Primary)
                    } else {
                        None
                    },
                    modifiers: state.modifiers,
                    is_touch: false,
                    timestamp_ms: ts,
                }));
                sync_window_cursor(state);
                if handled {
                    state.window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: btn_state,
                button,
                ..
            } => {
                let phase = match btn_state {
                    ElementState::Pressed => {
                        state.mouse_down = true;
                        PointerPhase::Down
                    }
                    ElementState::Released => {
                        state.mouse_down = false;
                        PointerPhase::Up
                    }
                };
                let ts = self.start_instant.elapsed().as_millis() as u64;
                let handled = state.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                    id: PointerId(1),
                    phase,
                    position: state.cursor_pos,
                    button: Some(translate_mouse_button(button)),
                    modifiers: state.modifiers,
                    is_touch: false,
                    timestamp_ms: ts,
                }));
                sync_window_cursor(state);
                if handled {
                    state.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let sf = state.tree.metrics.scale_factor.max(1.0);
                let (dx, dy) = translate_scroll_delta(delta, sf);
                if state.tree.handle_event(&UiEvent::Scroll(ScrollDelta {
                    x: dx,
                    y: dy,
                    position: state.cursor_pos,
                })) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::Touch(touch) => {
                let sf = state.tree.metrics.scale_factor.max(1.0);
                let pos = Point::new(touch.location.x as f32 / sf, touch.location.y as f32 / sf);
                let phase = match touch.phase {
                    TouchPhase::Started => PointerPhase::Down,
                    TouchPhase::Moved => PointerPhase::Move,
                    TouchPhase::Ended => PointerPhase::Up,
                    TouchPhase::Cancelled => PointerPhase::Cancel,
                };
                let ts = self.timestamp_ms();
                if let Some(st) = self.windows.get_mut(&id)
                    && st.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                        id: PointerId(touch.id + 100),
                        phase,
                        position: pos,
                        button: Some(PointerButton::Primary),
                        modifiers: st.modifiers,
                        is_touch: true,
                        timestamp_ms: ts,
                    }))
                {
                    st.window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                state.modifiers = translate_modifiers(mods.state());
            }
            WindowEvent::KeyboardInput { event: key_ev, .. } => {
                let ke = translate_winit_key(&key_ev, state.modifiers);
                #[cfg(feature = "hot-reload")]
                if key_ev.state == ElementState::Pressed
                    && let Some(spec) = state.hot_reload.as_mut()
                {
                    match ke.code {
                        KeyCode::F1 => {
                            spec.show_inspector = !spec.show_inspector;
                            state.window.request_redraw();
                            return;
                        }
                        KeyCode::F2 => {
                            let cur = state.tree.theme().mode;
                            let next = match cur {
                                ThemeMode::Amoled => ThemeTokens::dark(),
                                ThemeMode::Dark => ThemeTokens::light(),
                                _ => ThemeTokens::amoled(),
                            };
                            state.tree.set_theme(next);
                            state.window.request_redraw();
                            return;
                        }
                        KeyCode::F3 => {
                            spec.device_mode = !spec.device_mode;
                            state.window.request_redraw();
                            return;
                        }
                        KeyCode::F5 => {
                            match spec.session.reload_file(
                                &spec.path,
                                spec.component.as_deref(),
                                &mut state.tree,
                            ) {
                                Ok(_) => {
                                    spec.last_reload_error = None;
                                }
                                Err(e) => {
                                    spec.last_reload_error = Some(e.to_string());
                                }
                            }
                            state.window.request_redraw();
                            return;
                        }
                        _ => {}
                    }
                }
                if state.tree.handle_event(&UiEvent::Key(ke)) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::Ime(ime) => {
                let ev = match ime {
                    WinitIme::Enabled => ImeEvent::Enabled,
                    WinitIme::Preedit(s, cur) => ImeEvent::Preedit(s, cur),
                    WinitIme::Commit(s) => ImeEvent::Commit(s),
                    WinitIme::Disabled => ImeEvent::Disabled,
                };
                if state.tree.handle_event(&UiEvent::Ime(ev)) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::PinchGesture { delta, .. } => {
                if state.tree.handle_event(&UiEvent::Pinch {
                    factor: 1.0 + delta as f32,
                    center: state.cursor_pos,
                }) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::Focused(focused) => {
                if state.tree.handle_event(&UiEvent::FocusChanged(focused)) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_and_present_window(id);
            }
            _ => {}
        }
    }
}

pub fn translate_modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        super_key: state.super_key(),
    }
}

pub fn translate_winit_cursor(cursor: CursorIcon) -> WinitCursorIcon {
    match cursor {
        CursorIcon::Default => WinitCursorIcon::Default,
        CursorIcon::Pointer => WinitCursorIcon::Pointer,
        CursorIcon::Text => WinitCursorIcon::Text,
        CursorIcon::Grab => WinitCursorIcon::Grab,
        CursorIcon::Grabbing => WinitCursorIcon::Grabbing,
        CursorIcon::ResizeHorizontal => WinitCursorIcon::EwResize,
        CursorIcon::ResizeVertical => WinitCursorIcon::NsResize,
        CursorIcon::NotAllowed => WinitCursorIcon::NotAllowed,
        CursorIcon::Crosshair => WinitCursorIcon::Crosshair,
    }
}

pub fn translate_mouse_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Back => PointerButton::Back,
        MouseButton::Forward => PointerButton::Forward,
        MouseButton::Other(_) => PointerButton::Primary,
    }
}

pub fn translate_scroll_delta(delta: MouseScrollDelta, scale_factor: f32) -> (f32, f32) {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => (x * 36.0, y * 36.0),
        MouseScrollDelta::PixelDelta(pos) => {
            (pos.x as f32 / scale_factor, pos.y as f32 / scale_factor)
        }
    }
}

pub fn translate_winit_key(key_ev: &winit::event::KeyEvent, modifiers: Modifiers) -> KeyEvent {
    let code = match key_ev.physical_key {
        PhysicalKey::Code(WinitKeyCode::KeyI) => KeyCode::KeyI,
        PhysicalKey::Code(WinitKeyCode::KeyA) => KeyCode::KeyA,
        PhysicalKey::Code(WinitKeyCode::KeyC) => KeyCode::KeyC,
        PhysicalKey::Code(WinitKeyCode::KeyV) => KeyCode::KeyV,
        PhysicalKey::Code(WinitKeyCode::KeyX) => KeyCode::KeyX,
        PhysicalKey::Code(WinitKeyCode::KeyY) => KeyCode::KeyY,
        PhysicalKey::Code(WinitKeyCode::KeyZ) => KeyCode::KeyZ,
        _ => match &key_ev.logical_key {
            Key::Named(NamedKey::Enter) => KeyCode::Enter,
            Key::Named(NamedKey::Tab) => KeyCode::Tab,
            Key::Named(NamedKey::Backspace) => KeyCode::Backspace,
            Key::Named(NamedKey::Delete) => KeyCode::Delete,
            Key::Named(NamedKey::Escape) => KeyCode::Escape,
            Key::Named(NamedKey::Space) => KeyCode::Space,
            Key::Named(NamedKey::ArrowLeft) => KeyCode::ArrowLeft,
            Key::Named(NamedKey::ArrowRight) => KeyCode::ArrowRight,
            Key::Named(NamedKey::ArrowUp) => KeyCode::ArrowUp,
            Key::Named(NamedKey::ArrowDown) => KeyCode::ArrowDown,
            Key::Named(NamedKey::Home) => KeyCode::Home,
            Key::Named(NamedKey::End) => KeyCode::End,
            Key::Named(NamedKey::PageUp) => KeyCode::PageUp,
            Key::Named(NamedKey::PageDown) => KeyCode::PageDown,
            Key::Named(NamedKey::F1) => KeyCode::F1,
            Key::Named(NamedKey::F2) => KeyCode::F2,
            Key::Named(NamedKey::F3) => KeyCode::F3,
            Key::Named(NamedKey::F4) => KeyCode::F4,
            Key::Named(NamedKey::F5) => KeyCode::F5,
            Key::Named(NamedKey::F6) => KeyCode::F6,
            Key::Named(NamedKey::F7) => KeyCode::F7,
            Key::Named(NamedKey::F8) => KeyCode::F8,
            Key::Named(NamedKey::F9) => KeyCode::F9,
            Key::Named(NamedKey::F10) => KeyCode::F10,
            Key::Named(NamedKey::F11) => KeyCode::F11,
            Key::Named(NamedKey::F12) => KeyCode::F12,
            Key::Character(s) => KeyCode::Character(s.to_string()),
            _ => KeyCode::Unknown,
        },
    };
    let text = key_ev.text.as_ref().map(|t| t.to_string());
    KeyEvent {
        code,
        text,
        pressed: key_ev.state == ElementState::Pressed,
        repeat: key_ev.repeat,
        modifiers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
