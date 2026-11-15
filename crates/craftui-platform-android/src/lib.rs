//! Android platform adapter (`android-activity` `GameActivity` + Vulkan/GLES `wgpu` + JNI/IME/Surface bridges).

use craftui_core::{
    AndroidPermissionSpec, AppLifecycleEvent, BatteryState, DeviceCapabilitiesBackend, DeviceInfo,
    DirtyFlags, FileDialogBackend, FileDialogRequest, HapticStyle, HapticsBackend, ImeEvent,
    Insets, KeyCode, KeyEvent, Modifiers, NetworkStatus, Permission, PermissionBackend,
    PermissionOutcome, PermissionStatus, PlatformServices, PlatformViewId, Point, PointerButton,
    PointerEvent, PointerId, PointerPhase, SecureStorageBackend, ShareBackend, SharePayload, Size,
    TextureId, UiDispatcher, UiEvent, WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{PaintScene, TextureHandle, TextureRegistry};
use craftui_widgets::{Router, WidgetKind, WidgetTree};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "android")]
pub use android_activity;

pub mod remote_preview;

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
    device_info: Mutex<DeviceInfo>,
    network_status: Mutex<NetworkStatus>,
    battery_state: Mutex<BatteryState>,
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

/// Embedding strategy for native Android Views (e.g., CameraX / ExoPlayer / WebView)
/// hosted inside CraftUI [`WidgetKind::PlatformView`](craftui_widgets::WidgetKind::PlatformView).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidPlatformViewMode {
    /// Zero-copy Vulkan/GLES `HardwareBuffer` / `SurfaceTexture` external texture composited directly inside Vello.
    ExternalSurfaceTexture { surface_id: u64 },
    /// Transparent `PaintCommand::PlatformViewHole` punch composited over an Android `SurfaceView`.
    HolePunch { view_id: PlatformViewId },
}

/// Bridge trait for importing Android `AHardwareBuffer` / `SurfaceTexture` RGBA frames into CraftUI textures.
pub trait ExternalSurfaceTextureBridge: Send + Sync {
    fn surface_id(&self) -> u64;
    /// Copy or acquire the latest frame dimensions and straight RGBA8 bytes when dirty.
    fn acquire_latest_rgba(&mut self) -> Option<(u32, u32, Vec<u8>)>;
}

/// Platform-neutral Android keycodes translated by [`AndroidHost::handle_android_key_event`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidKeyCode {
    Back,
    Enter,
    NumpadEnter,
    Tab,
    Del,
    ForwardDel,
    Escape,
    DpadLeft,
    DpadRight,
    DpadUp,
    DpadDown,
    Home,
    MoveEnd,
    PageUp,
    PageDown,
    Space,
    Cut,
    Copy,
    Paste,
    Character(char),
    Other,
}

/// Hardware / virtual punctuation & numpad keys mapped across US QWERTY shift layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidPunctuationKey {
    Comma,
    Period,
    Minus,
    Equals,
    LeftBracket,
    RightBracket,
    Backslash,
    Semicolon,
    Apostrophe,
    Slash,
    Grave,
    At,
    Plus,
    Star,
    Pound,
    NumpadDivide,
    NumpadMultiply,
    NumpadSubtract,
    NumpadAdd,
    NumpadDot,
    NumpadComma,
    NumpadEquals,
    NumpadLeftParen,
    NumpadRightParen,
}

/// Translate a punctuation/numpad key with active `shift` modifier into a printable character.
pub fn map_android_punctuation(key: AndroidPunctuationKey, shift: bool) -> char {
    match key {
        AndroidPunctuationKey::Comma => {
            if shift {
                '<'
            } else {
                ','
            }
        }
        AndroidPunctuationKey::Period => {
            if shift {
                '>'
            } else {
                '.'
            }
        }
        AndroidPunctuationKey::Minus => {
            if shift {
                '_'
            } else {
                '-'
            }
        }
        AndroidPunctuationKey::Equals => {
            if shift {
                '+'
            } else {
                '='
            }
        }
        AndroidPunctuationKey::LeftBracket => {
            if shift {
                '{'
            } else {
                '['
            }
        }
        AndroidPunctuationKey::RightBracket => {
            if shift {
                '}'
            } else {
                ']'
            }
        }
        AndroidPunctuationKey::Backslash => {
            if shift {
                '|'
            } else {
                '\\'
            }
        }
        AndroidPunctuationKey::Semicolon => {
            if shift {
                ':'
            } else {
                ';'
            }
        }
        AndroidPunctuationKey::Apostrophe => {
            if shift {
                '"'
            } else {
                '\''
            }
        }
        AndroidPunctuationKey::Slash => {
            if shift {
                '?'
            } else {
                '/'
            }
        }
        AndroidPunctuationKey::Grave => {
            if shift {
                '~'
            } else {
                '`'
            }
        }
        AndroidPunctuationKey::At => '@',
        AndroidPunctuationKey::Plus => '+',
        AndroidPunctuationKey::Star => '*',
        AndroidPunctuationKey::Pound => '#',
        AndroidPunctuationKey::NumpadDivide => '/',
        AndroidPunctuationKey::NumpadMultiply => '*',
        AndroidPunctuationKey::NumpadSubtract => '-',
        AndroidPunctuationKey::NumpadAdd => '+',
        AndroidPunctuationKey::NumpadDot => '.',
        AndroidPunctuationKey::NumpadComma => ',',
        AndroidPunctuationKey::NumpadEquals => '=',
        AndroidPunctuationKey::NumpadLeftParen => '(',
        AndroidPunctuationKey::NumpadRightParen => ')',
    }
}

/// Translate top-row digits `'0'..='9'` with Shift modifier cleanly.
pub fn map_android_digit(digit: char, shift: bool) -> char {
    if !shift {
        return digit;
    }
    match digit {
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        other => other,
    }
}

/// Commands emitted toward Android `InputMethodManager` via `GameActivity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AndroidImeCommand {
    ShowSoftInput,
    HideSoftInput,
    SyncTextState {
        text: String,
        selection_start: usize,
        selection_end: usize,
        compose_region: Option<(usize, usize)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyncedImeSnapshot {
    text: String,
    selection_start: usize,
    selection_end: usize,
    compose_region: Option<(usize, usize)>,
}

fn clamp_char_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Platform-neutral Android lifecycle, safe-area/IME inset, back navigation, and event translator host.
///
/// Used both on-device inside [`android_runtime::run_android_app`] and in deterministic desktop/CI integration tests.
pub struct AndroidHost {
    pub scope: Scope,
    pub tree: WidgetTree,
    pub scene: PaintScene,
    pub textures: TextureRegistry,
    pub dispatcher: UiDispatcher,
    pub bridge: Arc<AndroidBridgeState>,
    back_handler: Option<Box<dyn FnMut() -> bool>>,
    last_ime_visible: bool,
    last_synced_ime_state: Option<SyncedImeSnapshot>,
    needs_redraw: bool,
    last_error: Option<String>,
    pub remote_preview: Option<remote_preview::RemotePreviewClient>,
    #[cfg(feature = "hot-reload")]
    pub hot_reload_session: Option<craftui_runtime::HotReloadSession>,
}

impl AndroidHost {
    pub fn new(dispatcher: UiDispatcher, width_px: u32, height_px: u32, scale_factor: f32) -> Self {
        let scope = Scope::root();
        let mut tree = WidgetTree::new(scope);
        let bridge = Arc::new(AndroidBridgeState::new());
        tree.set_platform_services(
            PlatformServices::in_memory()
                .with_permissions(AndroidPermissionBackend::new(bridge.clone()))
                .with_haptics(AndroidHapticsBackend::new(bridge.clone()))
                .with_share(AndroidShareBackend::new(bridge.clone()))
                .with_secure_storage(AndroidSecureStorage::new(bridge.clone()))
                .with_device_capabilities(AndroidDeviceCapabilities::new(bridge.clone()))
                .with_file_dialog(AndroidFileDialogBridge::new(bridge.clone())),
        );
        let sf = scale_factor.max(1.0);
        tree.metrics = WindowMetrics {
            logical_size: Size::new(width_px as f32 / sf, height_px as f32 / sf),
            scale_factor: sf,
            safe_area: Insets::new(44.0, 0.0, 40.0, 0.0),
            keyboard_inset: 0.0,
        };
        Self {
            scope,
            tree,
            scene: PaintScene::new(),
            textures: TextureRegistry::new(),
            dispatcher,
            bridge,
            back_handler: None,
            last_ime_visible: false,
            last_synced_ime_state: None,
            needs_redraw: true,
            last_error: None,
            remote_preview: None,
            #[cfg(feature = "hot-reload")]
            hot_reload_session: None,
        }
    }

    pub fn bridge(&self) -> Arc<AndroidBridgeState> {
        self.bridge.clone()
    }

    /// Enable background TCP remote live preview client connecting to host via ADB reverse tunnel.
    pub fn enable_remote_preview(&mut self, port: u16) {
        self.remote_preview = Some(remote_preview::RemotePreviewClient::start(
            "127.0.0.1",
            port,
            self.dispatcher.clone(),
        ));
    }

    /// Apply an incoming in-memory `.cui` source payload directly into the live widget tree.
    pub fn apply_hot_reload_source(
        &mut self,
        source: &str,
        component: Option<&str>,
    ) -> craftui_core::Result<craftui_core::NodeId> {
        #[cfg(feature = "hot-reload")]
        {
            let session = self
                .hot_reload_session
                .get_or_insert_with(|| craftui_runtime::HotReloadSession::new(self.scope));
            let root = session.reload(source, component, &mut self.tree)?;
            self.tree.set_root(root);
            self.needs_redraw = true;
            Ok(root)
        }
        #[cfg(not(feature = "hot-reload"))]
        {
            let _ = (source, component);
            Err(craftui_core::CraftError::Runtime(
                "craftui-platform-android compiled without hot-reload feature".into(),
            ))
        }
    }

    pub fn set_permission_wait_timeout(&self, timeout: Duration) {
        self.bridge.set_request_timeout(timeout);
    }

    pub fn drain_platform_commands(&self) -> Vec<AndroidPlatformCommand> {
        self.bridge.drain_commands()
    }

    pub fn sync_permission_statuses(&mut self, entries: Vec<(Permission, PermissionStatus, bool)>) {
        self.bridge.sync_permission_statuses(entries);
        self.needs_redraw = true;
    }

    pub fn push_permission_results(&mut self, request_id: u64, results: &[(String, i32, bool)]) {
        self.bridge.push_permission_results(request_id, results);
        self.needs_redraw = true;
    }

    pub fn push_file_dialog_result(&mut self, request_id: u64, paths: Vec<PathBuf>) {
        self.bridge.push_file_dialog_result(request_id, paths);
        self.needs_redraw = true;
    }

    pub fn push_lifecycle_event(&mut self, ev: AppLifecycleEvent) {
        self.bridge.push_lifecycle_event(ev);
    }

    pub fn set_device_info(&mut self, info: DeviceInfo) {
        if let Ok(mut guard) = self.bridge.device_info.lock() {
            *guard = info;
        }
    }

    pub fn set_network_status(&mut self, status: NetworkStatus) {
        if let Ok(mut guard) = self.bridge.network_status.lock() {
            *guard = status;
        }
    }

    pub fn set_battery_state(&mut self, state: BatteryState) {
        if let Ok(mut guard) = self.bridge.battery_state.lock() {
            *guard = state;
        }
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn clear_error(&mut self) {
        self.last_error = None;
    }

    pub fn record_error(&mut self, msg: impl Into<String>) {
        self.last_error = Some(msg.into());
    }

    pub fn request_redraw(&mut self) {
        self.needs_redraw = true;
    }

    /// Suggested poll timeout for demand-driven Android event loops.
    pub fn next_poll_timeout(&self, animating: bool, has_gpu: bool) -> Option<std::time::Duration> {
        if animating || self.needs_redraw {
            return Some(if has_gpu {
                std::time::Duration::ZERO
            } else {
                std::time::Duration::from_millis(16)
            });
        }
        let now_ms = craftui_core::monotonic_ms();
        let mut min_delay = craftui_reactive::next_timer_delay_ms(now_ms);
        if let Some(caret_delay) = self.tree.next_internal_delay_ms(now_ms) {
            min_delay = Some(min_delay.map_or(caret_delay, |cur| cur.min(caret_delay)));
        }
        min_delay.map(|ms| std::time::Duration::from_millis(ms.max(1)))
    }

    /// Register a hardware Back button handler (e.g. `move || router.pop()`).
    pub fn set_back_handler(&mut self, handler: impl FnMut() -> bool + 'static) {
        self.back_handler = Some(Box::new(handler));
    }

    /// Convenience helper binding a [`Router<R>`] to Android's hardware/gesture Back key.
    pub fn bind_back_router<R: Clone + PartialEq + 'static>(&mut self, router: Router<R>) {
        self.set_back_handler(move || router.pop());
    }

    pub fn update_safe_area_and_ime(
        &mut self,
        window_width_px: i32,
        window_height_px: i32,
        content_left_px: i32,
        content_top_px: i32,
        content_right_px: i32,
        content_bottom_px: i32,
    ) {
        let sf = self.tree.metrics.scale_factor.max(1.0);
        let top = if content_top_px > 0 {
            (content_top_px as f32) / sf
        } else {
            44.0
        };
        let left = (content_left_px.max(0) as f32) / sf;
        let right = ((window_width_px - content_right_px).max(0) as f32) / sf;
        let bottom_px = (window_height_px - content_bottom_px).max(0) as f32;
        let bottom = bottom_px / sf;
        let eff_bottom = if bottom > 0.0 { bottom } else { 40.0 };
        self.tree.metrics.safe_area = Insets::new(top, right, eff_bottom, left);
        self.tree.metrics.keyboard_inset = bottom;
        self.needs_redraw = true;
    }

    pub fn push_touch(
        &mut self,
        pointer_id: i32,
        phase: PointerPhase,
        raw_x_px: f32,
        raw_y_px: f32,
        timestamp_ms: u64,
    ) -> bool {
        let sf = self.tree.metrics.scale_factor.max(1.0);
        let pos = Point::new(raw_x_px / sf, raw_y_px / sf);
        let handled = self.tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(pointer_id as u64),
            phase,
            position: pos,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms,
        }));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    /// Translate and dispatch Android key events, including `Back`, `Enter`, directional D-pad keys, and text input keys.
    pub fn handle_android_key_event(
        &mut self,
        code: AndroidKeyCode,
        pressed: bool,
        repeat: bool,
        modifiers: Modifiers,
    ) -> bool {
        if code == AndroidKeyCode::Back {
            if !pressed {
                return false;
            }
            // 1. Dismiss open overlay / modal / popup menu if active.
            let esc = KeyEvent {
                code: KeyCode::Escape,
                text: None,
                pressed: true,
                repeat: false,
                modifiers,
            };
            if self.tree.handle_event(&UiEvent::Key(esc)) {
                self.needs_redraw = true;
                return true;
            }
            // 2. Pop custom registered Back handler / Router.
            if let Some(ref mut cb) = self.back_handler
                && cb()
            {
                self.needs_redraw = true;
                return true;
            }
            // 3. Fallback to Router<String> in root context if present.
            if let Some(router) = self.scope.use_context::<Router<String>>()
                && router.pop()
            {
                self.needs_redraw = true;
                return true;
            }
            return false;
        }

        let mut dispatch_mods = modifiers;
        let (craft_code, text) = match code {
            AndroidKeyCode::Enter | AndroidKeyCode::NumpadEnter => (
                KeyCode::Enter,
                (!modifiers.command()).then(|| "\n".to_string()),
            ),
            AndroidKeyCode::Tab => (KeyCode::Tab, None),
            AndroidKeyCode::Del => (KeyCode::Backspace, None),
            AndroidKeyCode::ForwardDel => (KeyCode::Delete, None),
            AndroidKeyCode::Escape => (KeyCode::Escape, None),
            AndroidKeyCode::DpadLeft => (KeyCode::ArrowLeft, None),
            AndroidKeyCode::DpadRight => (KeyCode::ArrowRight, None),
            AndroidKeyCode::DpadUp => (KeyCode::ArrowUp, None),
            AndroidKeyCode::DpadDown => (KeyCode::ArrowDown, None),
            AndroidKeyCode::Home => (KeyCode::Home, None),
            AndroidKeyCode::MoveEnd => (KeyCode::End, None),
            AndroidKeyCode::PageUp => (KeyCode::PageUp, None),
            AndroidKeyCode::PageDown => (KeyCode::PageDown, None),
            AndroidKeyCode::Space => (
                KeyCode::Space,
                (!modifiers.command()).then(|| " ".to_string()),
            ),
            AndroidKeyCode::Cut => {
                dispatch_mods.ctrl = true;
                (KeyCode::KeyX, None)
            }
            AndroidKeyCode::Copy => {
                dispatch_mods.ctrl = true;
                (KeyCode::KeyC, None)
            }
            AndroidKeyCode::Paste => {
                dispatch_mods.ctrl = true;
                (KeyCode::KeyV, None)
            }
            AndroidKeyCode::Character(ch) => {
                let s = ch.to_string();
                let txt = (!modifiers.command() && !ch.is_control()).then(|| s.clone());
                (KeyCode::Character(s), txt)
            }
            AndroidKeyCode::Back | AndroidKeyCode::Other => (KeyCode::Unknown, None),
        };

        let handled = self.tree.handle_event(&UiEvent::Key(KeyEvent {
            code: craft_code,
            text,
            pressed,
            repeat,
            modifiers: dispatch_mods,
        }));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    pub fn push_ime_commit(&mut self, text: impl Into<String>) -> bool {
        let handled = self
            .tree
            .handle_event(&UiEvent::Ime(ImeEvent::Commit(text.into())));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    pub fn push_ime_preedit(
        &mut self,
        text: impl Into<String>,
        cursor: Option<(usize, usize)>,
    ) -> bool {
        let handled = self
            .tree
            .handle_event(&UiEvent::Ime(ImeEvent::Preedit(text.into(), cursor)));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    /// Apply a full Android `GameTextInput` buffer update (`TextEvent`) to the focused text input,
    /// separating active `compose_region` preedit from committed text and avoiding JNI echo loops.
    pub fn push_ime_text_state(
        &mut self,
        text: &str,
        selection_start: usize,
        selection_end: usize,
        compose_region: Option<(usize, usize)>,
    ) -> bool {
        let Some(focused) = self.tree.focused else {
            return false;
        };
        let sel_s = clamp_char_boundary(text, selection_start.min(selection_end));
        let sel_e = clamp_char_boundary(text, selection_start.max(selection_end));
        let compose = compose_region.and_then(|(s, e)| {
            let cs = clamp_char_boundary(text, s.min(e));
            let ce = clamp_char_boundary(text, s.max(e));
            (cs < ce).then_some((cs, ce))
        });

        let mut changed_cb: Option<(craftui_widgets::TextCallback, String)> = None;
        {
            let Some(node) = self.tree.nodes.get_mut(focused) else {
                return false;
            };
            let WidgetKind::TextInput {
                editor,
                bound_signal,
                on_change,
                ..
            } = &mut node.kind
            else {
                return false;
            };

            let (committed, next_cursor, next_anchor, next_preedit, next_pe_cur) =
                if let Some((cs, ce)) = compose {
                    let pre = text[cs..ce].to_string();
                    let pe_len = pre.len();
                    let pe_cur = (
                        sel_s.saturating_sub(cs).min(pe_len),
                        sel_e.saturating_sub(cs).min(pe_len),
                    );
                    (
                        format!("{}{}", &text[..cs], &text[ce..]),
                        cs,
                        cs,
                        Some(pre),
                        Some(pe_cur),
                    )
                } else {
                    (text.to_string(), sel_e, sel_s, None, None)
                };

            let text_changed = editor.text != committed;
            editor.text = committed;
            editor.cursor = next_cursor;
            editor.anchor = next_anchor;
            editor.preedit = next_preedit;
            editor.preedit_cursor = next_pe_cur;

            if text_changed {
                node.a11y.value = Some(editor.text.clone());
                if let Some(sig) = bound_signal
                    && sig.get_untracked() != editor.text
                {
                    sig.set(editor.text.clone());
                }
                if let Some(cb) = on_change.clone() {
                    changed_cb = Some((cb, editor.text.clone()));
                }
            }
            node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
        }

        if let Some((cb, txt)) = changed_cb
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f(txt);
        }
        self.tree.reset_caret_blink(craftui_core::monotonic_ms());
        self.last_synced_ime_state = Some(SyncedImeSnapshot {
            text: text.to_string(),
            selection_start: sel_s,
            selection_end: sel_e,
            compose_region: compose,
        });
        self.needs_redraw = true;
        true
    }

    /// Update or allocate a CraftUI [`TextureHandle`] from an external Android [`ExternalSurfaceTextureBridge`].
    pub fn sync_external_texture(
        &mut self,
        bridge: &mut dyn ExternalSurfaceTextureBridge,
        slot: &mut Option<TextureHandle>,
    ) -> Option<TextureId> {
        let (w, h, rgba) = bridge.acquire_latest_rgba()?;
        if w == 0 || h == 0 || rgba.len() != (w as usize) * (h as usize) * 4 {
            return None;
        }
        self.needs_redraw = true;
        if let Some(existing) = slot.as_ref() {
            existing.buffer.replace_rgba(w, h, rgba);
            Some(existing.id)
        } else {
            let handle = self.textures.create_from_rgba(w, h, rgba);
            let id = handle.id;
            *slot = Some(handle);
            Some(id)
        }
    }

    /// Advance reactive timers, animations, layout, and paint when dirty or animating.
    /// Returns `(animating, did_paint, cmds)`.
    pub fn step_frame_with_redraw(&mut self, dt_sec: f32) -> (bool, bool, Vec<AndroidImeCommand>) {
        let now_ms = craftui_core::monotonic_ms();
        let timers = craftui_reactive::tick_timers();
        let jobs = self.dispatcher.drain();
        let pending_reload = self
            .remote_preview
            .as_ref()
            .and_then(|c| c.take_pending_reload());
        if let Some(reload) = pending_reload {
            let t0 = Instant::now();
            let res = self.apply_hot_reload_source(&reload.source, reload.component.as_deref());
            if let Some(client) = self.remote_preview.as_ref() {
                match res {
                    Ok(_) => {
                        let dur = t0.elapsed().as_secs_f64() * 1000.0;
                        tracing::info!(
                            "Remote live preview reloaded {} in {:.2}ms",
                            reload.path,
                            dur
                        );
                        client.send_ack(reload.path, dur);
                    }
                    Err(err) => {
                        tracing::warn!("Remote live preview reload error: {err}");
                        client.send_error(reload.path, err.to_string());
                    }
                }
            }
        }
        let blink = self.tree.tick_blink_timers(now_ms);
        let anim_tick = self.tree.tick_animations(dt_sec);
        let dirty = self.tree.layout(self.tree.metrics);
        let animating = anim_tick || self.tree.has_active_animations();
        let should_paint =
            self.needs_redraw || timers > 0 || jobs > 0 || blink || anim_tick || dirty > 0;
        if should_paint {
            self.tree.paint(&mut self.scene);
            self.needs_redraw = false;
        }

        let mut cmds = Vec::new();
        let focused_input = self.tree.focused.and_then(|id| {
            self.tree.nodes.get(id).and_then(|n| match &n.kind {
                WidgetKind::TextInput { editor, .. } => {
                    if let Some(pre) = editor.preedit.as_deref().filter(|p| !p.is_empty()) {
                        let cur = editor.cursor.min(editor.text.len());
                        let full = editor.display_text().into_owned();
                        let compose = Some((cur, cur + pre.len()));
                        let sel = match editor.preedit_cursor {
                            Some((ps, pe)) => {
                                ((cur + ps).min(full.len()), (cur + pe).min(full.len()))
                            }
                            None => (cur + pre.len(), cur + pre.len()),
                        };
                        Some(SyncedImeSnapshot {
                            text: full,
                            selection_start: sel.0,
                            selection_end: sel.1,
                            compose_region: compose,
                        })
                    } else {
                        let (s, e) = editor
                            .selection_range()
                            .unwrap_or((editor.cursor, editor.cursor));
                        Some(SyncedImeSnapshot {
                            text: editor.text.clone(),
                            selection_start: s,
                            selection_end: e,
                            compose_region: None,
                        })
                    }
                }
                _ => None,
            })
        });

        let ime_needed = focused_input.is_some();
        if ime_needed != self.last_ime_visible {
            self.last_ime_visible = ime_needed;
            if ime_needed {
                cmds.push(AndroidImeCommand::ShowSoftInput);
            } else {
                self.last_synced_ime_state = None;
                cmds.push(AndroidImeCommand::HideSoftInput);
            }
        }
        if let Some(next_state) = focused_input
            && self.last_synced_ime_state.as_ref() != Some(&next_state)
        {
            self.last_synced_ime_state = Some(next_state.clone());
            cmds.push(AndroidImeCommand::SyncTextState {
                text: next_state.text,
                selection_start: next_state.selection_start,
                selection_end: next_state.selection_end,
                compose_region: next_state.compose_region,
            });
        }

        (animating, should_paint, cmds)
    }

    pub fn step_frame(&mut self, dt_sec: f32) -> (bool, Vec<AndroidImeCommand>) {
        let (animating, _painted, cmds) = self.step_frame_with_redraw(dt_sec);
        (animating, cmds)
    }
}

#[cfg(target_os = "android")]
pub mod android_runtime {
    use super::*;
    use android_activity::input::{
        InputEvent, KeyAction, Keycode, MotionAction, TextInputState, TextSpan,
    };
    use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
    use craftui_core::NodeId;
    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
    use craftui_render::GpuSurfacePipeline;
    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
    use raw_window_handle::{
        AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
    };
    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
    use std::ptr::NonNull;
    use std::time::Instant;

    #[cfg(target_os = "android")]
    pub fn android_log(msg: &str) {
        unsafe extern "C" {
            fn __android_log_write(
                prio: std::ffi::c_int,
                tag: *const std::ffi::c_char,
                text: *const std::ffi::c_char,
            ) -> std::ffi::c_int;
        }
        use std::ffi::CString;
        if let (Ok(tag), Ok(text)) = (CString::new("CraftUI_Native"), CString::new(msg)) {
            unsafe {
                __android_log_write(4, tag.as_ptr(), text.as_ptr());
            }
        }
    }

    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
    struct AndroidSurfaceTarget {
        window: RawWindowHandle,
        display: RawDisplayHandle,
    }

    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
    impl AndroidSurfaceTarget {
        fn new(ptr: NonNull<std::ffi::c_void>) -> Self {
            Self {
                window: RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(ptr)),
                display: RawDisplayHandle::Android(AndroidDisplayHandle::new()),
            }
        }
    }

    fn map_activity_keycode(
        kc: Keycode,
        meta: android_activity::input::MetaState,
    ) -> AndroidKeyCode {
        let shift = meta.shift_on();
        let letter_upper = shift ^ meta.caps_lock_on();
        let ch = |lower: char, upper: char| {
            AndroidKeyCode::Character(if letter_upper { upper } else { lower })
        };
        let punct =
            |p: AndroidPunctuationKey| AndroidKeyCode::Character(map_android_punctuation(p, shift));
        let digit = |d: char| AndroidKeyCode::Character(map_android_digit(d, shift));

        match kc {
            Keycode::Back => AndroidKeyCode::Back,
            Keycode::Enter | Keycode::DpadCenter => AndroidKeyCode::Enter,
            Keycode::NumpadEnter => AndroidKeyCode::NumpadEnter,
            Keycode::Tab => AndroidKeyCode::Tab,
            Keycode::Del => AndroidKeyCode::Del,
            Keycode::ForwardDel => AndroidKeyCode::ForwardDel,
            Keycode::Escape => AndroidKeyCode::Escape,
            Keycode::DpadLeft => AndroidKeyCode::DpadLeft,
            Keycode::DpadRight => AndroidKeyCode::DpadRight,
            Keycode::DpadUp => AndroidKeyCode::DpadUp,
            Keycode::DpadDown => AndroidKeyCode::DpadDown,
            Keycode::MoveHome | Keycode::Home => AndroidKeyCode::Home,
            Keycode::MoveEnd => AndroidKeyCode::MoveEnd,
            Keycode::PageUp => AndroidKeyCode::PageUp,
            Keycode::PageDown => AndroidKeyCode::PageDown,
            Keycode::Space => AndroidKeyCode::Space,
            Keycode::Cut => AndroidKeyCode::Cut,
            Keycode::Copy => AndroidKeyCode::Copy,
            Keycode::Paste => AndroidKeyCode::Paste,
            Keycode::A => ch('a', 'A'),
            Keycode::B => ch('b', 'B'),
            Keycode::C => ch('c', 'C'),
            Keycode::D => ch('d', 'D'),
            Keycode::E => ch('e', 'E'),
            Keycode::F => ch('f', 'F'),
            Keycode::G => ch('g', 'G'),
            Keycode::H => ch('h', 'H'),
            Keycode::I => ch('i', 'I'),
            Keycode::J => ch('j', 'J'),
            Keycode::K => ch('k', 'K'),
            Keycode::L => ch('l', 'L'),
            Keycode::M => ch('m', 'M'),
            Keycode::N => ch('n', 'N'),
            Keycode::O => ch('o', 'O'),
            Keycode::P => ch('p', 'P'),
            Keycode::Q => ch('q', 'Q'),
            Keycode::R => ch('r', 'R'),
            Keycode::S => ch('s', 'S'),
            Keycode::T => ch('t', 'T'),
            Keycode::U => ch('u', 'U'),
            Keycode::V => ch('v', 'V'),
            Keycode::W => ch('w', 'W'),
            Keycode::X => ch('x', 'X'),
            Keycode::Y => ch('y', 'Y'),
            Keycode::Z => ch('z', 'Z'),
            Keycode::Keycode0 => digit('0'),
            Keycode::Keycode1 => digit('1'),
            Keycode::Keycode2 => digit('2'),
            Keycode::Keycode3 => digit('3'),
            Keycode::Keycode4 => digit('4'),
            Keycode::Keycode5 => digit('5'),
            Keycode::Keycode6 => digit('6'),
            Keycode::Keycode7 => digit('7'),
            Keycode::Keycode8 => digit('8'),
            Keycode::Keycode9 => digit('9'),
            Keycode::Numpad0 => AndroidKeyCode::Character('0'),
            Keycode::Numpad1 => AndroidKeyCode::Character('1'),
            Keycode::Numpad2 => AndroidKeyCode::Character('2'),
            Keycode::Numpad3 => AndroidKeyCode::Character('3'),
            Keycode::Numpad4 => AndroidKeyCode::Character('4'),
            Keycode::Numpad5 => AndroidKeyCode::Character('5'),
            Keycode::Numpad6 => AndroidKeyCode::Character('6'),
            Keycode::Numpad7 => AndroidKeyCode::Character('7'),
            Keycode::Numpad8 => AndroidKeyCode::Character('8'),
            Keycode::Numpad9 => AndroidKeyCode::Character('9'),
            Keycode::Comma => punct(AndroidPunctuationKey::Comma),
            Keycode::Period => punct(AndroidPunctuationKey::Period),
            Keycode::Minus => punct(AndroidPunctuationKey::Minus),
            Keycode::Equals => punct(AndroidPunctuationKey::Equals),
            Keycode::LeftBracket => punct(AndroidPunctuationKey::LeftBracket),
            Keycode::RightBracket => punct(AndroidPunctuationKey::RightBracket),
            Keycode::Backslash => punct(AndroidPunctuationKey::Backslash),
            Keycode::Semicolon => punct(AndroidPunctuationKey::Semicolon),
            Keycode::Apostrophe => punct(AndroidPunctuationKey::Apostrophe),
            Keycode::Slash => punct(AndroidPunctuationKey::Slash),
            Keycode::Grave => punct(AndroidPunctuationKey::Grave),
            Keycode::At => punct(AndroidPunctuationKey::At),
            Keycode::Plus => punct(AndroidPunctuationKey::Plus),
            Keycode::Star => punct(AndroidPunctuationKey::Star),
            Keycode::Pound => punct(AndroidPunctuationKey::Pound),
            Keycode::NumpadDivide => punct(AndroidPunctuationKey::NumpadDivide),
            Keycode::NumpadMultiply => punct(AndroidPunctuationKey::NumpadMultiply),
            Keycode::NumpadSubtract => punct(AndroidPunctuationKey::NumpadSubtract),
            Keycode::NumpadAdd => punct(AndroidPunctuationKey::NumpadAdd),
            Keycode::NumpadDot => punct(AndroidPunctuationKey::NumpadDot),
            Keycode::NumpadComma => punct(AndroidPunctuationKey::NumpadComma),
            Keycode::NumpadEquals => punct(AndroidPunctuationKey::NumpadEquals),
            Keycode::NumpadLeftParen => punct(AndroidPunctuationKey::NumpadLeftParen),
            Keycode::NumpadRightParen => punct(AndroidPunctuationKey::NumpadRightParen),
            _ => AndroidKeyCode::Other,
        }
    }

    pub fn run_android_app<F>(app: AndroidApp, mut build_ui: F)
    where
        F: FnMut(Scope, &mut WidgetTree, &mut TextureRegistry, &UiDispatcher) -> NodeId,
    {
        let waker = app.create_waker();
        let dispatcher = UiDispatcher::with_waker(move || {
            waker.wake();
        });

        let density = app
            .config()
            .density()
            .map(|d| (d as f32 / 160.0).max(1.0))
            .unwrap_or(2.0);

        let mut host = AndroidHost::new(dispatcher.clone(), 1080, 1920, density);
        #[cfg(feature = "hot-reload")]
        host.enable_remote_preview(craftui_core::DEFAULT_REMOTE_PREVIEW_PORT);
        let root = build_ui(host.scope, &mut host.tree, &mut host.textures, &dispatcher);
        host.tree.set_root(root);

        #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
        let mut gpu: Option<GpuSurfacePipeline> = None;
        let mut last_tick = Instant::now();
        let start = Instant::now();
        let mut running = true;
        let mut animating = false;

        while running {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            let has_gpu = gpu.is_some();
            #[cfg(not(any(feature = "gpu-sdf", feature = "gpu-vello")))]
            let has_gpu = false;
            let timeout = host.next_poll_timeout(animating, has_gpu);
            app.poll_events(timeout, |event| match event {
                PollEvent::Wake | PollEvent::Timeout => {
                    host.request_redraw();
                }
                PollEvent::Main(main_ev) => match main_ev {
                    MainEvent::InitWindow { .. } => {
                        android_log("run_android_app: MainEvent::InitWindow received");
                        if let Some(nw) = app.native_window() {
                            let w = nw.width().max(1) as u32;
                            let h = nw.height().max(1) as u32;
                            android_log(&format!("run_android_app: native_window dimensions: {w}x{h}"));
                            host.tree.handle_event(&UiEvent::Resize {
                                width: w as f32 / density,
                                height: h as f32 / density,
                                scale_factor: density,
                            });
                            host.request_redraw();
                            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
                            if let Some(nn) = NonNull::new(nw.ptr().as_ptr().cast()) {
                                let tgt = AndroidSurfaceTarget::new(nn);
                                let raw_target = wgpu::SurfaceTargetUnsafe::RawHandle {
                                    raw_display_handle: Some(tgt.display),
                                    raw_window_handle: tgt.window,
                                };
                                android_log("run_android_app: calling GpuSurfacePipeline::new_unsafe...");
                                // SAFETY: `nw` is a live ANativeWindow until `TerminateWindow` drops `gpu`.
                                match unsafe {
                                    GpuSurfacePipeline::new_unsafe(raw_target, w, h, false)
                                } {
                                    Ok(pipeline) => {
                                        android_log("run_android_app: GpuSurfacePipeline created successfully!");
                                        host.clear_error();
                                        gpu = Some(pipeline);
                                    }
                                    Err(err) => {
                                        let msg = format!(
                                            "Android Vulkan/GLES surface init failed: {err}"
                                        );
                                        android_log(&format!("run_android_app ERROR: {msg}"));
                                        tracing::warn!("{msg}");
                                        host.record_error(msg);
                                        gpu = None;
                                    }
                                }
                            }
                        }
                    }
                    MainEvent::TerminateWindow { .. } => {
                        android_log("run_android_app: MainEvent::TerminateWindow received");
                        #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
                        {
                            gpu = None;
                        }
                    }
                    MainEvent::WindowResized { .. } | MainEvent::ContentRectChanged { .. } => {
                        if let Some(nw) = app.native_window() {
                            let w = nw.width().max(1) as u32;
                            let h = nw.height().max(1) as u32;
                            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
                            if let Some(g) = gpu.as_mut() {
                                g.resize(w, h);
                            }
                            let cr = app.content_rect();
                            host.update_safe_area_and_ime(
                                w as i32, h as i32, cr.left, cr.top, cr.right, cr.bottom,
                            );
                            host.tree.handle_event(&UiEvent::Resize {
                                width: w as f32 / density,
                                height: h as f32 / density,
                                scale_factor: density,
                            });
                            host.request_redraw();
                        }
                    }
                    MainEvent::InputAvailable => {
                        if let Ok(mut iter) = app.input_events_iter() {
                            while iter.next(|ev| match ev {
                                InputEvent::MotionEvent(me) => {
                                    let action = me.action();
                                    let ts = start.elapsed().as_millis() as u64;
                                    let ptr_idx = me.pointer_index();
                                    for p in me.pointers() {
                                        let phase = match action {
                                            MotionAction::Down => PointerPhase::Down,
                                            MotionAction::PointerDown
                                                if p.pointer_index() == ptr_idx =>
                                            {
                                                PointerPhase::Down
                                            }
                                            MotionAction::Up => PointerPhase::Up,
                                            MotionAction::PointerUp
                                                if p.pointer_index() == ptr_idx =>
                                            {
                                                PointerPhase::Up
                                            }
                                            MotionAction::Cancel => PointerPhase::Cancel,
                                            _ => PointerPhase::Move,
                                        };
                                        host.push_touch(p.pointer_id(), phase, p.x(), p.y(), ts);
                                    }
                                    InputStatus::Handled
                                }
                                InputEvent::KeyEvent(ke) => {
                                    let pressed = ke.action() == KeyAction::Down;
                                    let repeat = ke.repeat_count() > 0;
                                    let meta = ke.meta_state();
                                    let mods = Modifiers {
                                        shift: meta.shift_on(),
                                        ctrl: meta.ctrl_on(),
                                        alt: meta.alt_on(),
                                        super_key: meta.meta_on(),
                                    };
                                    let code = map_activity_keycode(ke.key_code(), meta);
                                    if host.handle_android_key_event(code, pressed, repeat, mods) {
                                        InputStatus::Handled
                                    } else if code == AndroidKeyCode::Back {
                                        InputStatus::Unhandled
                                    } else {
                                        InputStatus::Handled
                                    }
                                }
                                InputEvent::TextEvent(ts) => {
                                    let state: TextInputState = ts.clone();
                                    let compose = state.compose_region.map(|r| (r.start, r.end));
                                    if host.push_ime_text_state(
                                        &state.text,
                                        state.selection.start,
                                        state.selection.end,
                                        compose,
                                    ) {
                                        host.request_redraw();
                                    }
                                    InputStatus::Handled
                                }
                                _ => InputStatus::Unhandled,
                            }) {}
                        }
                    }
                    MainEvent::Resume { .. } => {
                        host.push_lifecycle_event(AppLifecycleEvent::Resumed);
                        host.request_redraw();
                    }
                    MainEvent::Pause => {
                        host.push_lifecycle_event(AppLifecycleEvent::Paused);
                    }
                    MainEvent::LowMemory => {
                        host.push_lifecycle_event(AppLifecycleEvent::MemoryWarning);
                    }
                    MainEvent::Destroy => {
                        android_log("run_android_app: MainEvent::Destroy received!");
                        host.push_lifecycle_event(AppLifecycleEvent::Detached);
                        running = false;
                    }
                    _ => {}
                },
                _ => {}
            });

            if !running {
                break;
            }
            let now = Instant::now();
            let dt = (now - last_tick).as_secs_f32().clamp(0.001, 0.05);
            last_tick = now;

            #[cfg_attr(
                not(any(feature = "gpu-sdf", feature = "gpu-vello")),
                allow(unused_variables)
            )]
            let (still_anim, did_paint, cmds) = host.step_frame_with_redraw(dt);
            animating = still_anim;
            for cmd in cmds {
                match cmd {
                    AndroidImeCommand::ShowSoftInput => app.show_soft_input(true),
                    AndroidImeCommand::HideSoftInput => app.hide_soft_input(false),
                    AndroidImeCommand::SyncTextState {
                        text,
                        selection_start,
                        selection_end,
                        compose_region,
                    } => {
                        app.set_text_input_state(TextInputState {
                            text,
                            selection: TextSpan {
                                start: selection_start,
                                end: selection_end,
                            },
                            compose_region: compose_region
                                .map(|(start, end)| TextSpan { start, end }),
                        });
                    }
                }
            }
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            if did_paint && let Some(g) = gpu.as_mut() {
                let bg = host.tree.theme().background;
                if let Err(err) = g.render_frame(
                    &host.scene,
                    density,
                    bg,
                    &host.textures,
                    &mut host.tree.text_engine,
                ) {
                    let msg = format!("Android GPU render_frame failed: {err}");
                    android_log(&format!("run_android_app: {msg}"));
                    tracing::warn!("{msg}");
                    host.record_error(msg);
                } else {
                    static ONCE: std::sync::atomic::AtomicBool =
                        std::sync::atomic::AtomicBool::new(false);
                    if !ONCE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        android_log(
                            "run_android_app: first frame rendered successfully to ANativeWindow!",
                        );
                    }
                }
            }
        }
    }
}

#[cfg(all(target_os = "android", feature = "builtin-main"))]
#[unsafe(no_mangle)]
pub fn android_main(app: android_activity::AndroidApp) {
    android_runtime::run_android_app(app, |cx, tree, _textures, _disp| {
        use craftui_core::{Insets, pct};
        use craftui_layout::LayoutStyle;
        use craftui_text::TextStyle;
        use craftui_widgets::BoxStyle;

        let root = tree.container(
            cx,
            LayoutStyle::column()
                .with_size(pct(100.0), pct(100.0))
                .with_padding(Insets::all(16.0))
                .with_gap(12.0),
            BoxStyle::new(),
        );

        let safe = tree.safe_area(cx);

        let header = tree.text(
            cx,
            "CraftUI Native Android",
            TextStyle::default()
                .with_size(22.0)
                .with_weight(700.0)
                .with_color(tree.theme().text_primary),
        );

        let desc = tree.text(
            cx,
            "Running high-performance GPU SDF / Vulkan on Android!",
            TextStyle::default()
                .with_size(14.0)
                .with_color(tree.theme().text_secondary),
        );

        let btn = tree.button(cx, "Interactive Native Button", move || {
            tracing::info!("Android native button tapped!");
        });

        tree.append_child(safe, header);
        tree.append_child(safe, desc);
        tree.append_child(safe, btn);
        tree.append_child(root, safe);

        root
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::Length;
    use craftui_layout::LayoutStyle;
    use craftui_reactive::reset_runtime;
    use std::cell::RefCell;
    use std::rc::Rc;

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
        assert!(host.handle_android_key_event(
            AndroidKeyCode::Cut,
            true,
            false,
            Modifiers::default()
        ));
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
        assert!(host.handle_android_key_event(
            AndroidKeyCode::Back,
            true,
            false,
            Modifiers::default()
        ));
        assert!(!modal_open.get());
        assert_eq!(router.current(), "detail");

        // Second Back key pops the Router stack
        assert!(host.handle_android_key_event(
            AndroidKeyCode::Back,
            true,
            false,
            Modifiers::default()
        ));
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
                    assert!(
                        manifest_permissions.contains(&"android.permission.CAMERA".to_string())
                    );
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
}
