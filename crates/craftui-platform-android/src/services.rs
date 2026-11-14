use craftui_core::{
    AndroidPermissionSpec, AppLifecycleEvent, BatteryState, DeviceCapabilitiesBackend, DeviceInfo,
    FileDialogBackend, FileDialogRequest, HapticStyle, HapticsBackend,
    NetworkStatus, Permission, PermissionBackend,
    PermissionOutcome, PermissionStatus, SecureStorageBackend, ShareBackend, SharePayload,
};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Android manifest permission entry (`0 = PERMISSION_GRANTED`, `-1 = PERMISSION_DENIED`).
pub const ANDROID_PERMISSION_GRANTED: i32 = 0;

/// Platform commands emitted from CraftUI Rust widgets/services for Android `MainActivity` execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AndroidPlatformCommand {
    RequestPermissions {
        request_id: u64,
        permissions: Vec<Permission>,
        manifest_permissions: Vec<String>,
    },
    OpenAppSettings,
    TriggerHaptic(HapticStyle),
    Share(SharePayload),
    SyncSecureKey {
        key: String,
        value: String,
    },
    DeleteSecureKey {
        key: String,
    },
    OpenFileDialog {
        request_id: u64,
        request: FileDialogRequest,
    },
    SaveFileDialog {
        request_id: u64,
        request: FileDialogRequest,
    },
}

#[derive(Debug, Default)]
struct PendingPermissionRequest {
    permissions: Vec<Permission>,
    resolved: Option<Vec<PermissionOutcome>>,
}

#[derive(Debug, Default)]
struct PendingFileDialogRequest {
    resolved: Option<Vec<PathBuf>>,
}

/// Shared thread-safe Android OS bridge coordinating ActivityResult permissions, SAF pickers,
/// haptics, share intents, keystore secrets, and device telemetry.
#[derive(Debug)]
pub struct AndroidBridgeState {
    next_request_id: AtomicU64,
    statuses: Mutex<HashMap<Permission, PermissionStatus>>,
    rationales: Mutex<HashMap<Permission, bool>>,
    commands: Mutex<VecDeque<AndroidPlatformCommand>>,
    pending_perms: Mutex<HashMap<u64, PendingPermissionRequest>>,
    perm_cv: Condvar,
    pending_files: Mutex<HashMap<u64, PendingFileDialogRequest>>,
    file_cv: Condvar,
    request_timeout: Mutex<Duration>,
    secure_entries: Mutex<HashMap<String, String>>,
    pub(crate) device_info: Mutex<DeviceInfo>,
    pub(crate) network_status: Mutex<NetworkStatus>,
    pub(crate) battery_state: Mutex<BatteryState>,
    lifecycle_history: Mutex<Vec<AppLifecycleEvent>>,
}

impl Default for AndroidBridgeState {
    fn default() -> Self {
        Self::new()
    }
}

impl AndroidBridgeState {
    pub fn new() -> Self {
        Self {
            next_request_id: AtomicU64::new(1),
            statuses: Mutex::new(HashMap::new()),
            rationales: Mutex::new(HashMap::new()),
            commands: Mutex::new(VecDeque::new()),
            pending_perms: Mutex::new(HashMap::new()),
            perm_cv: Condvar::new(),
            pending_files: Mutex::new(HashMap::new()),
            file_cv: Condvar::new(),
            request_timeout: Mutex::new(Duration::from_millis(if cfg!(target_os = "android") {
                30_000
            } else {
                2_000
            })),
            secure_entries: Mutex::new(HashMap::new()),
            device_info: Mutex::new(DeviceInfo {
                os: "Android".to_string(),
                model: String::new(),
                os_version: String::new(),
                sdk_int: Some(35),
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

    fn timeout(&self) -> Duration {
        self.request_timeout
            .lock()
            .map(|g| *g)
            .unwrap_or(Duration::from_millis(250))
    }

    fn push_command(&self, cmd: AndroidPlatformCommand) {
        if let Ok(mut guard) = self.commands.lock() {
            guard.push_back(cmd);
        }
    }

    pub fn drain_commands(&self) -> Vec<AndroidPlatformCommand> {
        self.commands
            .lock()
            .map(|mut guard| guard.drain(..).collect())
            .unwrap_or_default()
    }

    pub fn set_permission_status(
        &self,
        permission: Permission,
        status: PermissionStatus,
        rationale: bool,
    ) {
        if let Ok(mut guard) = self.statuses.lock() {
            guard.insert(permission.clone(), status);
        }
        if let Ok(mut guard) = self.rationales.lock() {
            guard.insert(permission, rationale);
        }
    }

    pub fn sync_permission_statuses(&self, entries: Vec<(Permission, PermissionStatus, bool)>) {
        for (perm, status, rationale) in entries {
            self.set_permission_status(perm, status, rationale);
        }
    }

    fn filter_manifest_names(&self, perm: &Permission) -> Vec<String> {
        let sdk = self
            .device_info
            .lock()
            .ok()
            .and_then(|d| d.sdk_int)
            .unwrap_or(35);
        perm.android_permissions()
            .into_iter()
            .filter(|spec: &AndroidPermissionSpec| {
                spec.min_sdk.is_none_or(|min| sdk >= min)
                    && spec.max_sdk.is_none_or(|max| sdk <= max)
            })
            .map(|spec| spec.name.into_owned())
            .collect()
    }

    /// Resolve a queued permission request from Android `ActivityResultContracts.RequestMultiplePermissions`
    /// or `onRequestPermissionsResult` (`grant_result == 0` means granted).
    pub fn push_permission_results(&self, request_id: u64, results: &[(String, i32, bool)]) {
        let requested_perms = self
            .pending_perms
            .lock()
            .ok()
            .and_then(|g| g.get(&request_id).map(|p| p.permissions.clone()))
            .unwrap_or_default();

        let result_map: HashMap<&str, (i32, bool)> = results
            .iter()
            .map(|(name, grant, rat)| (name.as_str(), (*grant, *rat)))
            .collect();

        let mut outcomes = Vec::with_capacity(requested_perms.len());
        if requested_perms.is_empty() {
            for (manifest_name, grant, rationale) in results {
                let perm = Permission::from_key(manifest_name);
                let prev = self
                    .statuses
                    .lock()
                    .ok()
                    .and_then(|g| g.get(&perm).copied());
                let status = classify_android_grant_result(*grant, *rationale, prev);
                self.set_permission_status(perm.clone(), status, *rationale);
                outcomes.push(PermissionOutcome {
                    permission: perm,
                    status,
                    should_show_rationale: *rationale,
                });
            }
        } else {
            for perm in requested_perms {
                let specs = self.filter_manifest_names(&perm);
                let mut any_matched = false;
                let mut all_granted = !specs.is_empty();
                let mut any_rationale = false;
                for name in &specs {
                    if let Some((grant, rat)) = result_map.get(name.as_str()).copied() {
                        any_matched = true;
                        if grant != ANDROID_PERMISSION_GRANTED {
                            all_granted = false;
                        }
                        if rat {
                            any_rationale = true;
                        }
                    } else if let Some((grant, rat)) = result_map.get(perm.key().as_ref()).copied()
                    {
                        any_matched = true;
                        if grant != ANDROID_PERMISSION_GRANTED {
                            all_granted = false;
                        }
                        if rat {
                            any_rationale = true;
                        }
                    }
                }
                let prev = self
                    .statuses
                    .lock()
                    .ok()
                    .and_then(|g| g.get(&perm).copied());
                let status = if specs.is_empty() && !any_matched {
                    // Android does not require a runtime prompt for this permission at the current SDK level.
                    PermissionStatus::Granted
                } else if !any_matched {
                    prev.unwrap_or(PermissionStatus::NotDetermined)
                } else if all_granted {
                    PermissionStatus::Granted
                } else {
                    classify_android_grant_result(-1, any_rationale, prev)
                };
                self.set_permission_status(perm.clone(), status, any_rationale);
                outcomes.push(PermissionOutcome {
                    permission: perm,
                    status,
                    should_show_rationale: any_rationale,
                });
            }
        }

        if let Ok(mut pending) = self.pending_perms.lock()
            && let Some(entry) = pending.get_mut(&request_id)
        {
            entry.resolved = Some(outcomes);
        }
        self.perm_cv.notify_all();
    }

    pub fn push_file_dialog_result(&self, request_id: u64, paths: Vec<PathBuf>) {
        if let Ok(mut pending) = self.pending_files.lock()
            && let Some(entry) = pending.get_mut(&request_id)
        {
            entry.resolved = Some(paths);
        }
        self.file_cv.notify_all();
    }

    pub fn push_lifecycle_event(&self, ev: AppLifecycleEvent) {
        if let Ok(mut guard) = self.lifecycle_history.lock() {
            guard.push(ev);
        }
    }

    pub fn lifecycle_history(&self) -> Vec<AppLifecycleEvent> {
        self.lifecycle_history
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

fn classify_android_grant_result(
    grant_result: i32,
    should_show_rationale: bool,
    previous: Option<PermissionStatus>,
) -> PermissionStatus {
    if grant_result == ANDROID_PERMISSION_GRANTED {
        PermissionStatus::Granted
    } else if should_show_rationale {
        PermissionStatus::Denied
    } else if matches!(
        previous,
        Some(PermissionStatus::Denied | PermissionStatus::PermanentlyDenied)
    ) {
        PermissionStatus::PermanentlyDenied
    } else {
        PermissionStatus::Denied
    }
}

/// Android runtime permission backend backed by [`AndroidBridgeState`] (`MainActivity` `ActivityResult`).
#[derive(Clone)]
pub struct AndroidPermissionBackend {
    state: Arc<AndroidBridgeState>,
}

impl AndroidPermissionBackend {
    pub fn new(state: Arc<AndroidBridgeState>) -> Self {
        Self { state }
    }
}

impl PermissionBackend for AndroidPermissionBackend {
    fn check(&self, permission: &Permission) -> PermissionStatus {
        self.state
            .statuses
            .lock()
            .ok()
            .and_then(|g| g.get(permission).copied())
            .unwrap_or_else(|| {
                if self.state.filter_manifest_names(permission).is_empty() {
                    PermissionStatus::Granted
                } else {
                    PermissionStatus::NotDetermined
                }
            })
    }

    fn request(&self, permission: &Permission) -> PermissionStatus {
        self.request_many(std::slice::from_ref(permission))
            .into_iter()
            .next()
            .map_or(PermissionStatus::Unsupported, |o| o.status)
    }

    fn request_many(&self, permissions: &[Permission]) -> Vec<PermissionOutcome> {
        if permissions.is_empty() {
            return Vec::new();
        }
        let mut all_granted = true;
        let mut cached = Vec::with_capacity(permissions.len());
        for perm in permissions {
            let st = self.check(perm);
            let rat = self.should_show_rationale(perm);
            if !st.is_granted() {
                all_granted = false;
            }
            cached.push(PermissionOutcome {
                permission: perm.clone(),
                status: st,
                should_show_rationale: rat,
            });
        }
        if all_granted {
            return cached;
        }

        let mut manifest_names = Vec::new();
        for perm in permissions {
            for name in self.state.filter_manifest_names(perm) {
                if !manifest_names.contains(&name) {
                    manifest_names.push(name);
                }
            }
        }
        if manifest_names.is_empty() {
            for perm in permissions {
                self.state
                    .set_permission_status(perm.clone(), PermissionStatus::Granted, false);
            }
            return permissions
                .iter()
                .map(|p| PermissionOutcome {
                    permission: p.clone(),
                    status: PermissionStatus::Granted,
                    should_show_rationale: false,
                })
                .collect();
        }

        let request_id = self.state.next_request_id.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut pending) = self.state.pending_perms.lock() {
            pending.insert(
                request_id,
                PendingPermissionRequest {
                    permissions: permissions.to_vec(),
                    resolved: None,
                },
            );
        }
        self.state
            .push_command(AndroidPlatformCommand::RequestPermissions {
                request_id,
                permissions: permissions.to_vec(),
                manifest_permissions: manifest_names,
            });

        let timeout = self.state.timeout();
        if !timeout.is_zero()
            && let Ok(mut pending) = self.state.pending_perms.lock()
        {
            let deadline = Instant::now() + timeout;
            while pending
                .get(&request_id)
                .is_some_and(|entry| entry.resolved.is_none())
            {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let Ok((next_guard, wait_res)) =
                    self.state.perm_cv.wait_timeout(pending, remaining)
                else {
                    return cached;
                };
                pending = next_guard;
                if wait_res.timed_out() {
                    break;
                }
            }
            if let Some(entry) = pending.remove(&request_id)
                && let Some(outcomes) = entry.resolved
            {
                return outcomes;
            }
        }
        if let Ok(mut pending) = self.state.pending_perms.lock() {
            pending.remove(&request_id);
        }
        cached
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
        self.state
            .push_command(AndroidPlatformCommand::OpenAppSettings);
        true
    }
}

/// Android haptics bridge forwarding tactile patterns to `VibrationEffect`.
#[derive(Clone)]
pub struct AndroidHapticsBackend {
    state: Arc<AndroidBridgeState>,
}

impl AndroidHapticsBackend {
    pub fn new(state: Arc<AndroidBridgeState>) -> Self {
        Self { state }
    }
}

impl HapticsBackend for AndroidHapticsBackend {
    fn trigger(&self, style: HapticStyle) -> bool {
        self.state
            .push_command(AndroidPlatformCommand::TriggerHaptic(style));
        true
    }
}

/// Android `Intent.ACTION_SEND` share sheet bridge.
#[derive(Clone)]
pub struct AndroidShareBackend {
    state: Arc<AndroidBridgeState>,
}

impl AndroidShareBackend {
    pub fn new(state: Arc<AndroidBridgeState>) -> Self {
        Self { state }
    }
}

impl ShareBackend for AndroidShareBackend {
    fn share(&self, payload: &SharePayload) -> bool {
        self.state
            .push_command(AndroidPlatformCommand::Share(payload.clone()));
        true
    }
}

/// Android Keystore / EncryptedSharedPreferences synchronized secure storage backend.
#[derive(Clone)]
pub struct AndroidSecureStorage {
    state: Arc<AndroidBridgeState>,
}

impl AndroidSecureStorage {
    pub fn new(state: Arc<AndroidBridgeState>) -> Self {
        Self { state }
    }
}

impl SecureStorageBackend for AndroidSecureStorage {
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
        self.state
            .push_command(AndroidPlatformCommand::SyncSecureKey {
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
            .push_command(AndroidPlatformCommand::DeleteSecureKey {
                key: key.to_string(),
            });
        existed
    }
}

/// Android device info, network connectivity, and battery state provider.
#[derive(Clone)]
pub struct AndroidDeviceCapabilities {
    state: Arc<AndroidBridgeState>,
}

impl AndroidDeviceCapabilities {
    pub fn new(state: Arc<AndroidBridgeState>) -> Self {
        Self { state }
    }
}

impl DeviceCapabilitiesBackend for AndroidDeviceCapabilities {
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

/// Android Storage Access Framework (`ActivityResultContracts.OpenDocument` / `CreateDocument`) bridge.
#[derive(Clone)]
pub struct AndroidFileDialogBridge {
    state: Arc<AndroidBridgeState>,
}

impl AndroidFileDialogBridge {
    pub fn new(state: Arc<AndroidBridgeState>) -> Self {
        Self { state }
    }

    fn wait_result(&self, request_id: u64) -> Vec<PathBuf> {
        let timeout = self.state.timeout();
        if !timeout.is_zero()
            && let Ok(mut pending) = self.state.pending_files.lock()
        {
            let deadline = Instant::now() + timeout;
            while pending
                .get(&request_id)
                .is_some_and(|entry| entry.resolved.is_none())
            {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let Ok((next_guard, wait_res)) =
                    self.state.file_cv.wait_timeout(pending, remaining)
                else {
                    return Vec::new();
                };
                pending = next_guard;
                if wait_res.timed_out() {
                    break;
                }
            }
            if let Some(entry) = pending.remove(&request_id) {
                return entry.resolved.unwrap_or_default();
            }
        }
        if let Ok(mut pending) = self.state.pending_files.lock() {
            pending.remove(&request_id);
        }
        Vec::new()
    }
}

impl FileDialogBackend for AndroidFileDialogBridge {
    fn open_files(&self, request: &FileDialogRequest) -> Vec<PathBuf> {
        let request_id = self.state.next_request_id.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut pending) = self.state.pending_files.lock() {
            pending.insert(request_id, PendingFileDialogRequest::default());
        }
        self.state
            .push_command(AndroidPlatformCommand::OpenFileDialog {
                request_id,
                request: request.clone(),
            });
        self.wait_result(request_id)
    }

    fn save_file(&self, request: &FileDialogRequest) -> Option<PathBuf> {
        let request_id = self.state.next_request_id.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut pending) = self.state.pending_files.lock() {
            pending.insert(request_id, PendingFileDialogRequest::default());
        }
        self.state
            .push_command(AndroidPlatformCommand::SaveFileDialog {
                request_id,
                request: request.clone(),
            });
        self.wait_result(request_id).into_iter().next()
    }
}

