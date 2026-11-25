use craftui_core::{
    AppLifecycleEvent, BatteryState, DeviceCapabilitiesBackend, DeviceInfo, HapticStyle,
    HapticsBackend, NetworkStatus, Permission, PermissionBackend, PermissionOutcome,
    PermissionStatus, SecureStorageBackend, ShareBackend, SharePayload,
};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub fn permission_status_to_code(status: PermissionStatus) -> u8 {
    match status {
        PermissionStatus::NotDetermined => 0,
        PermissionStatus::Granted => 1,
        PermissionStatus::Denied => 2,
        PermissionStatus::PermanentlyDenied => 3,
        PermissionStatus::Restricted => 4,
        PermissionStatus::Limited => 5,
        PermissionStatus::Provisional => 6,
        PermissionStatus::Unsupported => 7,
    }
}

pub fn permission_status_from_code(code: u8) -> PermissionStatus {
    match code {
        0 => PermissionStatus::NotDetermined,
        1 => PermissionStatus::Granted,
        2 => PermissionStatus::Denied,
        3 => PermissionStatus::PermanentlyDenied,
        4 => PermissionStatus::Restricted,
        5 => PermissionStatus::Limited,
        6 => PermissionStatus::Provisional,
        _ => PermissionStatus::Unsupported,
    }
}

pub fn lifecycle_event_from_code(code: u8) -> Option<AppLifecycleEvent> {
    match code {
        0 => Some(AppLifecycleEvent::Resumed),
        1 => Some(AppLifecycleEvent::Paused),
        2 => Some(AppLifecycleEvent::Inactive),
        3 => Some(AppLifecycleEvent::Detached),
        4 => Some(AppLifecycleEvent::MemoryWarning),
        _ => None,
    }
}

pub fn network_status_from_code(code: u8) -> NetworkStatus {
    match code {
        1 => NetworkStatus::Offline,
        2 => NetworkStatus::Wifi,
        3 => NetworkStatus::Cellular,
        4 => NetworkStatus::Ethernet,
        _ => NetworkStatus::Unknown,
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Platform command emitted toward Swift `CraftUIViewController` / `CraftUIPermissions.swift`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IosPlatformCommand {
    RequestPermission { request_id: u64, key: String },
    OpenAppSettings,
    TriggerHaptic(HapticStyle),
    Share(SharePayload),
    SyncSecureKey { key: String, value: String },
    DeleteSecureKey { key: String },
    InvokeMethodChannel {
        request_id: u64,
        channel: String,
        method: String,
        args_json: String,
    },
}

impl IosPlatformCommand {
    pub fn to_json(&self) -> String {
        match self {
            Self::RequestPermission { request_id, key } => format!(
                "{{\"type\":\"request_permission\",\"request_id\":{request_id},\"key\":{}}}",
                json_escape(key)
            ),
            Self::OpenAppSettings => "{\"type\":\"open_app_settings\"}".to_string(),
            Self::TriggerHaptic(style) => {
                let name = match style {
                    HapticStyle::Light => "light",
                    HapticStyle::Medium => "medium",
                    HapticStyle::Heavy => "heavy",
                    HapticStyle::Selection => "selection",
                    HapticStyle::Success => "success",
                    HapticStyle::Warning => "warning",
                    HapticStyle::Error => "error",
                };
                format!("{{\"type\":\"haptic\",\"style\":\"{name}\"}}")
            }
            Self::Share(payload) => {
                let title = payload
                    .title
                    .as_deref()
                    .map(json_escape)
                    .unwrap_or_else(|| "null".to_string());
                let text = payload
                    .text
                    .as_deref()
                    .map(json_escape)
                    .unwrap_or_else(|| "null".to_string());
                let url = payload
                    .url
                    .as_deref()
                    .map(json_escape)
                    .unwrap_or_else(|| "null".to_string());
                let files: Vec<String> = payload
                    .files
                    .iter()
                    .map(|p| json_escape(&p.to_string_lossy()))
                    .collect();
                format!(
                    "{{\"type\":\"share\",\"title\":{title},\"text\":{text},\"url\":{url},\"files\":[{}]}}",
                    files.join(",")
                )
            }
            Self::SyncSecureKey { key, value } => format!(
                "{{\"type\":\"sync_secret\",\"key\":{},\"value\":{}}}",
                json_escape(key),
                json_escape(value)
            ),
            Self::DeleteSecureKey { key } => {
                format!(
                    "{{\"type\":\"delete_secret\",\"key\":{}}}",
                    json_escape(key)
                )
            }
            Self::InvokeMethodChannel {
                request_id,
                channel,
                method,
                args_json,
            } => format!(
                "{{\"type\":\"method_channel\",\"request_id\":{request_id},\"channel\":{},\"method\":{},\"args\":{args_json}}}",
                json_escape(channel),
                json_escape(method)
            ),
        }
    }
}

/// Shared iOS capability, permission, haptics, keychain, and telemetry bridge state.
#[derive(Debug)]
pub struct IosBridgeState {
    next_request_id: AtomicU64,
    statuses: Mutex<HashMap<Permission, PermissionStatus>>,
    rationales: Mutex<HashMap<Permission, bool>>,
    pub(crate) commands: Mutex<VecDeque<IosPlatformCommand>>,
    pending_perms: Mutex<HashMap<u64, Option<PermissionOutcome>>>,
    perm_cv: Condvar,
    pending_plugins: Mutex<HashMap<u64, Option<std::result::Result<serde_json::Value, String>>>>,
    plugin_cv: Condvar,
    request_timeout: Mutex<Duration>,
    secure_entries: Mutex<HashMap<String, String>>,
    pub(crate) device_info: Mutex<DeviceInfo>,
    pub(crate) network_status: Mutex<NetworkStatus>,
    pub(crate) battery_state: Mutex<BatteryState>,
    pub(crate) lifecycle_history: Mutex<Vec<AppLifecycleEvent>>,
}

impl Default for IosBridgeState {
    fn default() -> Self {
        Self::new()
    }
}

impl IosBridgeState {
    pub fn new() -> Self {
        Self {
            next_request_id: AtomicU64::new(1),
            statuses: Mutex::new(HashMap::new()),
            rationales: Mutex::new(HashMap::new()),
            commands: Mutex::new(VecDeque::new()),
            pending_perms: Mutex::new(HashMap::new()),
            perm_cv: Condvar::new(),
            pending_plugins: Mutex::new(HashMap::new()),
            plugin_cv: Condvar::new(),
            request_timeout: Mutex::new(Duration::from_millis(if cfg!(target_os = "ios") {
                30_000
            } else {
                2_000
            })),
            secure_entries: Mutex::new(HashMap::new()),
            device_info: Mutex::new(DeviceInfo {
                os: "iOS".to_string(),
                model: "iPhone".to_string(),
                os_version: String::new(),
                sdk_int: None,
                app_version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            network_status: Mutex::new(NetworkStatus::Unknown),
            battery_state: Mutex::new(BatteryState::default()),
            lifecycle_history: Mutex::new(Vec::new()),
        }
    }

    pub fn set_request_timeout(&self, timeout: Duration) {
        if let Ok(mut guard) = self.request_timeout.lock() {
            *guard = timeout;
        }
    }

    /// Queue an asynchronous native Swift `MethodChannel` invocation and await its JSON response.
    pub fn invoke_method_channel(
        &self,
        channel: &str,
        method: &str,
        args: serde_json::Value,
    ) -> craftui_core::Result<serde_json::Value> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let args_json = args.to_string();
        if let Ok(mut pending) = self.pending_plugins.lock() {
            pending.insert(request_id, None);
        }
        self.push_command(IosPlatformCommand::InvokeMethodChannel {
            request_id,
            channel: channel.to_string(),
            method: method.to_string(),
            args_json,
        });

        let timeout = self
            .request_timeout
            .lock()
            .map(|g| *g)
            .unwrap_or(Duration::from_millis(2_000));
        let deadline = Instant::now() + timeout;
        let mut guard = self.pending_plugins.lock().map_err(|_| {
            craftui_core::CraftError::Platform("ios plugin waiter poisoned".into())
        })?;
        loop {
            if let Some(Some(res)) = guard.get(&request_id).cloned() {
                guard.remove(&request_id);
                return res.map_err(craftui_core::CraftError::Platform);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                guard.remove(&request_id);
                return Err(craftui_core::CraftError::Platform(format!(
                    "ios method channel `{channel}.{method}` timed out"
                )));
            }
            let (next_guard, wait_res) = self.plugin_cv.wait_timeout(guard, remaining).map_err(
                |_| craftui_core::CraftError::Platform("ios plugin condvar poisoned".into()),
            )?;
            guard = next_guard;
            if wait_res.timed_out() {
                if let Some(Some(res)) = guard.remove(&request_id) {
                    return res.map_err(craftui_core::CraftError::Platform);
                }
                return Err(craftui_core::CraftError::Platform(format!(
                    "ios method channel `{channel}.{method}` timed out"
                )));
            }
        }
    }

    /// Resolve a queued `InvokeMethodChannel` request from Swift C-FFI.
    pub fn deliver_method_channel_result(&self, request_id: u64, ok: bool, payload_json: &str) {
        let parsed = if ok {
            serde_json::from_str::<serde_json::Value>(payload_json)
                .map_err(|e| format!("invalid method channel JSON payload: {e}"))
        } else {
            Err(payload_json.to_string())
        };
        if let Ok(mut pending) = self.pending_plugins.lock()
            && let Some(slot) = pending.get_mut(&request_id)
        {
            *slot = Some(parsed);
            self.plugin_cv.notify_all();
        }
    }

    fn timeout(&self) -> Duration {
        self.request_timeout
            .lock()
            .map(|g| *g)
            .unwrap_or(Duration::from_millis(250))
    }

    fn push_command(&self, cmd: IosPlatformCommand) {
        if let Ok(mut guard) = self.commands.lock() {
            guard.push_back(cmd);
        }
    }

    pub fn set_permission_status(
        &self,
        perm: Permission,
        status: PermissionStatus,
        rationale: bool,
    ) {
        if let Ok(mut guard) = self.statuses.lock() {
            guard.insert(perm.clone(), status);
        }
        if let Ok(mut guard) = self.rationales.lock() {
            guard.insert(perm, rationale);
        }
    }

    pub fn deliver_permission_result(
        &self,
        request_id: u64,
        key: &str,
        status: PermissionStatus,
        rationale: bool,
    ) {
        let perm = Permission::from_key(key);
        self.set_permission_status(perm.clone(), status, rationale);
        if let Ok(mut pending) = self.pending_perms.lock()
            && let Some(slot) = pending.get_mut(&request_id)
        {
            *slot = Some(PermissionOutcome {
                permission: perm,
                status,
                should_show_rationale: rationale,
            });
        }
        self.perm_cv.notify_all();
    }

    pub fn lifecycle_history(&self) -> Vec<AppLifecycleEvent> {
        self.lifecycle_history
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

/// iOS runtime permission backend delegating to UIKit / AVFoundation / CoreLocation / UNUserNotificationCenter.
#[derive(Clone)]
pub struct IosPermissionBackend {
    state: Arc<IosBridgeState>,
}

impl IosPermissionBackend {
    pub fn new(state: Arc<IosBridgeState>) -> Self {
        Self { state }
    }
}

impl PermissionBackend for IosPermissionBackend {
    fn check(&self, permission: &Permission) -> PermissionStatus {
        self.state
            .statuses
            .lock()
            .ok()
            .and_then(|g| g.get(permission).copied())
            .unwrap_or(PermissionStatus::NotDetermined)
    }

    fn request(&self, permission: &Permission) -> PermissionStatus {
        let current = self.check(permission);
        if current.is_granted() {
            return current;
        }
        let request_id = self.state.next_request_id.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut pending) = self.state.pending_perms.lock() {
            pending.insert(request_id, None);
        }
        self.state
            .push_command(IosPlatformCommand::RequestPermission {
                request_id,
                key: permission.key().into_owned(),
            });

        let timeout = self.state.timeout();
        if !timeout.is_zero()
            && let Ok(mut pending) = self.state.pending_perms.lock()
        {
            let deadline = Instant::now() + timeout;
            while pending.get(&request_id).is_some_and(|slot| slot.is_none()) {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let Ok((next_guard, wait_res)) =
                    self.state.perm_cv.wait_timeout(pending, remaining)
                else {
                    return current;
                };
                pending = next_guard;
                if wait_res.timed_out() {
                    break;
                }
            }
            if let Some(Some(outcome)) = pending.remove(&request_id) {
                return outcome.status;
            }
        }
        if let Ok(mut pending) = self.state.pending_perms.lock() {
            pending.remove(&request_id);
        }
        self.check(permission)
    }

    fn should_show_rationale(&self, permission: &Permission) -> bool {
        self.state
            .rationales
            .lock()
            .ok()
            .and_then(|g| g.get(permission).copied())
            .unwrap_or(false)
    }

    fn open_app_settings(&self) -> bool {
        self.state.push_command(IosPlatformCommand::OpenAppSettings);
        true
    }
}

/// iOS `UIImpactFeedbackGenerator` / `UINotificationFeedbackGenerator` bridge.
#[derive(Clone)]
pub struct IosHapticsBackend {
    state: Arc<IosBridgeState>,
}

impl IosHapticsBackend {
    pub fn new(state: Arc<IosBridgeState>) -> Self {
        Self { state }
    }
}

impl HapticsBackend for IosHapticsBackend {
    fn trigger(&self, style: HapticStyle) -> bool {
        self.state
            .push_command(IosPlatformCommand::TriggerHaptic(style));
        true
    }
}

/// iOS `UIActivityViewController` native share sheet bridge.
#[derive(Clone)]
pub struct IosShareBackend {
    state: Arc<IosBridgeState>,
}

impl IosShareBackend {
    pub fn new(state: Arc<IosBridgeState>) -> Self {
        Self { state }
    }
}

impl ShareBackend for IosShareBackend {
    fn share(&self, payload: &SharePayload) -> bool {
        self.state
            .push_command(IosPlatformCommand::Share(payload.clone()));
        true
    }
}

/// iOS Keychain synchronized secure storage backend.
#[derive(Clone)]
pub struct IosSecureStorage {
    state: Arc<IosBridgeState>,
}

impl IosSecureStorage {
    pub fn new(state: Arc<IosBridgeState>) -> Self {
        Self { state }
    }
}

impl SecureStorageBackend for IosSecureStorage {
    fn get_secret(&self, key: &str) -> Option<String> {
        self.state
            .secure_entries
            .lock()
            .ok()
            .and_then(|g| g.get(key).cloned())
    }

    fn set_secret(&self, key: &str, value: &str) -> bool {
        let Ok(mut guard) = self.state.secure_entries.lock() else {
            return false;
        };
        guard.insert(key.to_string(), value.to_string());
        self.state.push_command(IosPlatformCommand::SyncSecureKey {
            key: key.to_string(),
            value: value.to_string(),
        });
        true
    }

    fn delete_secret(&self, key: &str) -> bool {
        let Ok(mut guard) = self.state.secure_entries.lock() else {
            return false;
        };
        let existed = guard.remove(key).is_some();
        self.state
            .push_command(IosPlatformCommand::DeleteSecureKey {
                key: key.to_string(),
            });
        existed
    }
}

/// iOS `UIDevice` / `NWPathMonitor` telemetry backend.
#[derive(Clone)]
pub struct IosDeviceCapabilities {
    state: Arc<IosBridgeState>,
}

impl IosDeviceCapabilities {
    pub fn new(state: Arc<IosBridgeState>) -> Self {
        Self { state }
    }
}

impl DeviceCapabilitiesBackend for IosDeviceCapabilities {
    fn device_info(&self) -> DeviceInfo {
        self.state
            .device_info
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    fn network_status(&self) -> NetworkStatus {
        self.state
            .network_status
            .lock()
            .map(|g| *g)
            .unwrap_or_default()
    }

    fn battery_state(&self) -> BatteryState {
        self.state
            .battery_state
            .lock()
            .map(|g| *g)
            .unwrap_or_default()
    }
}
