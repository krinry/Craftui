use craftui_core::{
    BatteryState, ClipboardBackend, DeviceCapabilitiesBackend, DeviceInfo, MemorySecureStorage,
    NetworkStatus, PlatformServices, SecureStorageBackend, ShareBackend, SharePayload,
};
use std::sync::Mutex;

use crate::clipboard::DesktopClipboard;
use crate::file_dialog::DesktopFileDialog;
use crate::permissions::DesktopPermissionBackend;

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
