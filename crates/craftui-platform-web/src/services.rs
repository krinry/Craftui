use crate::dom::WebClipboard;
#[cfg(target_arch = "wasm32")]
use crate::runner::wasm_runner;
use craftui_core::{
    AppLifecycleEvent, BatteryState, ClipboardBackend, DeviceCapabilitiesBackend, DeviceInfo,
    HapticStyle, HapticsBackend, NetworkStatus, Permission, PermissionBackend, PermissionStatus,
    SecureStorageBackend, ShareBackend, SharePayload,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub fn web_permission_descriptor(permission: &Permission) -> Option<&'static str> {
    match permission {
        Permission::Camera => Some("camera"),
        Permission::Microphone => Some("microphone"),
        Permission::LocationWhenInUse | Permission::LocationCoarse | Permission::LocationAlways => {
            Some("geolocation")
        }
        Permission::Notifications => Some("notifications"),
        Permission::ClipboardRead => Some("clipboard-read"),
        Permission::Bluetooth | Permission::BluetoothScan | Permission::BluetoothConnect => {
            Some("bluetooth")
        }
        Permission::Nfc => Some("nfc"),
        Permission::Sensors | Permission::MotionActivity => Some("accelerometer"),
        Permission::MediaProjection => Some("display-capture"),
        Permission::BackgroundRefresh => Some("periodic-background-sync"),
        _ => None,
    }
}

/// Parse a DOM Permissions API / Notification API status string (`"granted"`, `"denied"`, `"prompt"`, `"default"`).
pub fn parse_web_permission_state(state: &str) -> PermissionStatus {
    match state.trim().to_ascii_lowercase().as_str() {
        "granted" => PermissionStatus::Granted,
        "denied" => PermissionStatus::Denied,
        "prompt" | "default" => PermissionStatus::NotDetermined,
        _ => PermissionStatus::Unsupported,
    }
}

/// Shared Web platform bridge holding browser permission cache, session secrets, device info,
/// network/battery telemetry, and tab visibility lifecycle history.
#[derive(Debug)]
pub struct WebBridgeState {
    statuses: Mutex<HashMap<Permission, PermissionStatus>>,
    secure_entries: Mutex<HashMap<String, String>>,
    device_info: Mutex<DeviceInfo>,
    network_status: Mutex<NetworkStatus>,
    battery_state: Mutex<BatteryState>,
    lifecycle_history: Mutex<Vec<AppLifecycleEvent>>,
}

impl Default for WebBridgeState {
    fn default() -> Self {
        Self::new()
    }
}

impl WebBridgeState {
    pub fn new() -> Self {
        Self {
            statuses: Mutex::new(HashMap::new()),
            secure_entries: Mutex::new(HashMap::new()),
            device_info: Mutex::new(DeviceInfo {
                os: "Web".to_string(),
                model: "Browser".to_string(),
                os_version: String::new(),
                sdk_int: None,
                app_version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            network_status: Mutex::new(NetworkStatus::Unknown),
            battery_state: Mutex::new(BatteryState::default()),
            lifecycle_history: Mutex::new(Vec::new()),
        }
    }

    pub fn set_permission_status(&self, permission: Permission, status: PermissionStatus) {
        if let Ok(mut guard) = self.statuses.lock() {
            guard.insert(permission, status);
        }
    }

    pub fn permission_status(&self, permission: &Permission) -> Option<PermissionStatus> {
        self.statuses
            .lock()
            .ok()
            .and_then(|g| g.get(permission).copied())
    }

    pub fn set_device_info(&self, info: DeviceInfo) {
        if let Ok(mut guard) = self.device_info.lock() {
            *guard = info;
        }
    }

    pub fn device_info(&self) -> DeviceInfo {
        self.device_info
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn set_network_status(&self, status: NetworkStatus) {
        if let Ok(mut guard) = self.network_status.lock() {
            *guard = status;
        }
    }

    pub fn network_status(&self) -> NetworkStatus {
        self.network_status.lock().map(|g| *g).unwrap_or_default()
    }

    pub fn set_battery_state(&self, state: BatteryState) {
        if let Ok(mut guard) = self.battery_state.lock() {
            *guard = state;
        }
    }

    pub fn battery_state(&self) -> BatteryState {
        self.battery_state.lock().map(|g| *g).unwrap_or_default()
    }

    pub fn push_lifecycle_event(&self, event: AppLifecycleEvent) {
        if let Ok(mut guard) = self.lifecycle_history.lock() {
            guard.push(event);
        }
    }

    pub fn lifecycle_history(&self) -> Vec<AppLifecycleEvent> {
        self.lifecycle_history
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    #[cfg(target_arch = "wasm32")]
    pub fn hydrate_from_browser(self: &Arc<Self>, on_update: impl Fn() + Clone + 'static) {
        use wasm_bindgen::JsCast;
        let Some(window) = web_sys::window() else {
            return;
        };
        let nav = window.navigator();

        self.refresh_online_from_navigator(&nav);

        let platform = js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("platform"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_else(|| "Browser".to_string());
        let ua = js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("userAgent"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        self.set_device_info(DeviceInfo {
            os: "Web".to_string(),
            model: platform,
            os_version: ua,
            sdk_int: None,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        });

        if let Some(notif_status) = read_browser_notification_permission(&window) {
            self.set_permission_status(Permission::Notifications, notif_status);
        }

        for perm in [
            Permission::Camera,
            Permission::Microphone,
            Permission::LocationWhenInUse,
            Permission::Notifications,
            Permission::ClipboardRead,
            Permission::MediaProjection,
        ] {
            query_browser_permission_async(self.clone(), perm, on_update.clone());
        }

        if let Ok(get_bat) =
            js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("getBattery"))
            && let Some(func) = get_bat.dyn_ref::<js_sys::Function>()
            && let Ok(prom_val) = func.call0(&nav)
            && let Ok(promise) = prom_val.dyn_into::<js_sys::Promise>()
        {
            let bridge = self.clone();
            let wake = on_update;
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(mgr) = wasm_bindgen_futures::JsFuture::from(promise).await {
                    let level =
                        js_sys::Reflect::get(&mgr, &wasm_bindgen::JsValue::from_str("level"))
                            .ok()
                            .and_then(|v| v.as_f64())
                            .map(|f| (f * 100.0).round().clamp(0.0, 100.0) as u8);
                    let charging =
                        js_sys::Reflect::get(&mgr, &wasm_bindgen::JsValue::from_str("charging"))
                            .ok()
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                    bridge.set_battery_state(BatteryState {
                        level_pct: level,
                        charging,
                    });
                    wake();
                }
            });
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn refresh_online_from_navigator(&self, nav: &web_sys::Navigator) {
        let online = js_sys::Reflect::get(nav, &wasm_bindgen::JsValue::from_str("onLine"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if !online {
            self.set_network_status(NetworkStatus::Offline);
            return;
        }
        let conn_type = js_sys::Reflect::get(nav, &wasm_bindgen::JsValue::from_str("connection"))
            .ok()
            .and_then(|c| {
                js_sys::Reflect::get(&c, &wasm_bindgen::JsValue::from_str("type"))
                    .ok()
                    .and_then(|t| t.as_string())
            })
            .unwrap_or_default();
        let status = match conn_type.to_ascii_lowercase().as_str() {
            "cellular" => NetworkStatus::Cellular,
            "ethernet" => NetworkStatus::Ethernet,
            "wifi" => NetworkStatus::Wifi,
            "none" => NetworkStatus::Offline,
            _ => NetworkStatus::Wifi,
        };
        self.set_network_status(status);
    }
}

#[cfg(target_arch = "wasm32")]
fn read_browser_notification_permission(window: &web_sys::Window) -> Option<PermissionStatus> {
    let notif =
        js_sys::Reflect::get(window, &wasm_bindgen::JsValue::from_str("Notification")).ok()?;
    let perm = js_sys::Reflect::get(&notif, &wasm_bindgen::JsValue::from_str("permission"))
        .ok()?
        .as_string()?;
    Some(parse_web_permission_state(&perm))
}

#[cfg(target_arch = "wasm32")]
fn query_browser_permission_async(
    bridge: Arc<WebBridgeState>,
    permission: Permission,
    on_update: impl Fn() + 'static,
) {
    use wasm_bindgen::JsCast;
    let Some(descriptor) = web_permission_descriptor(&permission) else {
        return;
    };
    let Some(window) = web_sys::window() else {
        return;
    };
    let nav = window.navigator();
    let Ok(perms_obj) = js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("permissions"))
    else {
        return;
    };
    let Ok(query_fn_val) =
        js_sys::Reflect::get(&perms_obj, &wasm_bindgen::JsValue::from_str("query"))
    else {
        return;
    };
    let Ok(query_fn) = query_fn_val.dyn_into::<js_sys::Function>() else {
        return;
    };
    let desc = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &desc,
        &wasm_bindgen::JsValue::from_str("name"),
        &wasm_bindgen::JsValue::from_str(descriptor),
    );
    let Ok(prom_val) = query_fn.call1(&perms_obj, &desc) else {
        return;
    };
    let Ok(promise) = prom_val.dyn_into::<js_sys::Promise>() else {
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(status_obj) = wasm_bindgen_futures::JsFuture::from(promise).await
            && let Ok(state_val) =
                js_sys::Reflect::get(&status_obj, &wasm_bindgen::JsValue::from_str("state"))
            && let Some(state_str) = state_val.as_string()
        {
            bridge.set_permission_status(permission, parse_web_permission_state(&state_str));
            on_update();
        }
    });
}

/// Web browser permission bridge querying `navigator.permissions` & `Notification.permission`
/// without ever blocking the browser main thread.
#[derive(Clone, Debug)]
pub struct WebPermissionBackend {
    bridge: Arc<WebBridgeState>,
}

impl WebPermissionBackend {
    pub fn new(bridge: Arc<WebBridgeState>) -> Self {
        Self { bridge }
    }
}

impl PermissionBackend for WebPermissionBackend {
    fn check(&self, permission: &Permission) -> PermissionStatus {
        if let Some(cached) = self.bridge.permission_status(permission) {
            return cached;
        }
        #[cfg(target_arch = "wasm32")]
        if *permission == Permission::Notifications
            && let Some(win) = web_sys::window()
            && let Some(st) = read_browser_notification_permission(&win)
        {
            self.bridge.set_permission_status(permission.clone(), st);
            return st;
        }
        if web_permission_descriptor(permission).is_some() {
            PermissionStatus::NotDetermined
        } else {
            PermissionStatus::Unsupported
        }
    }

    fn request(&self, permission: &Permission) -> PermissionStatus {
        let current = self.check(permission);
        if current != PermissionStatus::NotDetermined {
            return current;
        }
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if *permission == Permission::Notifications
                && let Some(window) = web_sys::window()
                && let Ok(notif) =
                    js_sys::Reflect::get(&window, &wasm_bindgen::JsValue::from_str("Notification"))
                && let Ok(req_fn_val) = js_sys::Reflect::get(
                    &notif,
                    &wasm_bindgen::JsValue::from_str("requestPermission"),
                )
                && let Ok(req_fn) = req_fn_val.dyn_into::<js_sys::Function>()
                && let Ok(prom_val) = req_fn.call0(&notif)
                && let Ok(promise) = prom_val.dyn_into::<js_sys::Promise>()
            {
                let bridge = self.bridge.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(res) = wasm_bindgen_futures::JsFuture::from(promise).await
                        && let Some(s) = res.as_string()
                    {
                        bridge.set_permission_status(
                            Permission::Notifications,
                            parse_web_permission_state(&s),
                        );
                        wasm_runner::request_all_frames();
                    }
                });
            } else {
                query_browser_permission_async(
                    self.bridge.clone(),
                    permission.clone(),
                    wasm_runner::request_all_frames,
                );
            }
        }
        current
    }

    fn should_show_rationale(&self, _permission: &Permission) -> bool {
        false
    }

    fn open_app_settings(&self) -> bool {
        false
    }
}

/// Web vibration haptics bridge calling `navigator.vibrate(ms)` when supported by mobile browsers.
#[derive(Clone, Copy, Debug, Default)]
pub struct WebHapticsBackend;

impl WebHapticsBackend {
    pub const fn duration_ms(style: HapticStyle) -> f64 {
        match style {
            HapticStyle::Selection => 8.0,
            HapticStyle::Light => 12.0,
            HapticStyle::Success => 18.0,
            HapticStyle::Medium => 25.0,
            HapticStyle::Warning => 35.0,
            HapticStyle::Heavy => 45.0,
            HapticStyle::Error => 60.0,
        }
    }
}

impl HapticsBackend for WebHapticsBackend {
    fn trigger(&self, #[allow(unused_variables)] style: HapticStyle) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if let Some(window) = web_sys::window() {
                let nav = window.navigator();
                if let Ok(vibrate_val) =
                    js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("vibrate"))
                    && let Ok(vibrate_fn) = vibrate_val.dyn_into::<js_sys::Function>()
                    && let Ok(res) = vibrate_fn.call1(
                        &nav,
                        &wasm_bindgen::JsValue::from_f64(Self::duration_ms(style)),
                    )
                {
                    return res.as_bool().unwrap_or(false);
                }
            }
        }
        false
    }
}

/// Web Share API (`navigator.share`) bridge with clipboard copy fallback for browsers without Web Share.
#[derive(Clone, Default)]
pub struct WebShareBackend {
    clipboard: WebClipboard,
}

impl WebShareBackend {
    pub fn new(clipboard: WebClipboard) -> Self {
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

impl ShareBackend for WebShareBackend {
    fn share(&self, payload: &SharePayload) -> bool {
        let Some(formatted) = Self::format_payload(payload) else {
            return false;
        };
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if let Some(window) = web_sys::window() {
                let nav = window.navigator();
                if let Ok(share_val) =
                    js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("share"))
                    && let Ok(share_fn) = share_val.dyn_into::<js_sys::Function>()
                {
                    let data = js_sys::Object::new();
                    if let Some(ref title) = payload.title {
                        let _ = js_sys::Reflect::set(
                            &data,
                            &wasm_bindgen::JsValue::from_str("title"),
                            &wasm_bindgen::JsValue::from_str(title),
                        );
                    }
                    if let Some(ref text) = payload.text {
                        let _ = js_sys::Reflect::set(
                            &data,
                            &wasm_bindgen::JsValue::from_str("text"),
                            &wasm_bindgen::JsValue::from_str(text),
                        );
                    }
                    if let Some(ref url) = payload.url {
                        let _ = js_sys::Reflect::set(
                            &data,
                            &wasm_bindgen::JsValue::from_str("url"),
                            &wasm_bindgen::JsValue::from_str(url),
                        );
                    }
                    if let Ok(prom_val) = share_fn.call1(&nav, &data) {
                        if let Ok(promise) = prom_val.dyn_into::<js_sys::Promise>() {
                            wasm_bindgen_futures::spawn_local(async move {
                                let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                            });
                        }
                        return true;
                    }
                }
            }
        }
        let mut cb = self.clipboard.clone();
        cb.set_text(&formatted)
    }
}

/// Web credential store backed by an in-memory cache mirrored to browser `window.sessionStorage`.
#[derive(Clone, Debug)]
pub struct WebSecureStorage {
    bridge: Arc<WebBridgeState>,
}

impl WebSecureStorage {
    pub fn new(bridge: Arc<WebBridgeState>) -> Self {
        Self { bridge }
    }

    #[cfg(target_arch = "wasm32")]
    fn storage_key(key: &str) -> String {
        format!("craftui_secret:{key}")
    }
}

impl SecureStorageBackend for WebSecureStorage {
    fn get_secret(&self, key: &str) -> Option<String> {
        if let Ok(guard) = self.bridge.secure_entries.lock()
            && let Some(val) = guard.get(key)
        {
            return Some(val.clone());
        }
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if let Some(window) = web_sys::window()
                && let Ok(storage) = js_sys::Reflect::get(
                    &window,
                    &wasm_bindgen::JsValue::from_str("sessionStorage"),
                )
                && !storage.is_null()
                && !storage.is_undefined()
                && let Ok(get_fn_val) =
                    js_sys::Reflect::get(&storage, &wasm_bindgen::JsValue::from_str("getItem"))
                && let Ok(get_fn) = get_fn_val.dyn_into::<js_sys::Function>()
                && let Ok(val) = get_fn.call1(
                    &storage,
                    &wasm_bindgen::JsValue::from_str(&Self::storage_key(key)),
                )
                && let Some(s) = val.as_string()
            {
                if let Ok(mut guard) = self.bridge.secure_entries.lock() {
                    guard.insert(key.to_string(), s.clone());
                }
                return Some(s);
            }
        }
        None
    }

    fn set_secret(&self, key: &str, value: &str) -> bool {
        let cached = if let Ok(mut guard) = self.bridge.secure_entries.lock() {
            guard.insert(key.to_string(), value.to_string());
            true
        } else {
            false
        };
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if let Some(window) = web_sys::window()
                && let Ok(storage) = js_sys::Reflect::get(
                    &window,
                    &wasm_bindgen::JsValue::from_str("sessionStorage"),
                )
                && !storage.is_null()
                && !storage.is_undefined()
                && let Ok(set_fn_val) =
                    js_sys::Reflect::get(&storage, &wasm_bindgen::JsValue::from_str("setItem"))
                && let Ok(set_fn) = set_fn_val.dyn_into::<js_sys::Function>()
            {
                let _ = set_fn.call2(
                    &storage,
                    &wasm_bindgen::JsValue::from_str(&Self::storage_key(key)),
                    &wasm_bindgen::JsValue::from_str(value),
                );
            }
        }
        cached
    }

    fn delete_secret(&self, key: &str) -> bool {
        #[allow(unused_mut)]
        let mut removed = self
            .bridge
            .secure_entries
            .lock()
            .map(|mut guard| guard.remove(key).is_some())
            .unwrap_or(false);
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            if let Some(window) = web_sys::window()
                && let Ok(storage) = js_sys::Reflect::get(
                    &window,
                    &wasm_bindgen::JsValue::from_str("sessionStorage"),
                )
                && !storage.is_null()
                && !storage.is_undefined()
                && let Ok(rem_fn_val) =
                    js_sys::Reflect::get(&storage, &wasm_bindgen::JsValue::from_str("removeItem"))
                && let Ok(rem_fn) = rem_fn_val.dyn_into::<js_sys::Function>()
            {
                let _ = rem_fn.call1(
                    &storage,
                    &wasm_bindgen::JsValue::from_str(&Self::storage_key(key)),
                );
                removed = true;
            }
        }
        removed
    }
}

/// Web device telemetry adapter exposing Navigator platform, online/connection status, and battery level.
#[derive(Clone, Debug)]
pub struct WebDeviceCapabilities {
    bridge: Arc<WebBridgeState>,
}

impl WebDeviceCapabilities {
    pub fn new(bridge: Arc<WebBridgeState>) -> Self {
        Self { bridge }
    }
}

impl DeviceCapabilitiesBackend for WebDeviceCapabilities {
    fn device_info(&self) -> DeviceInfo {
        self.bridge.device_info()
    }

    fn network_status(&self) -> NetworkStatus {
        let cur = self.bridge.network_status();
        #[cfg(target_arch = "wasm32")]
        if cur == NetworkStatus::Unknown
            && let Some(window) = web_sys::window()
        {
            self.bridge
                .refresh_online_from_navigator(&window.navigator());
            return self.bridge.network_status();
        }
        cur
    }

    fn battery_state(&self) -> BatteryState {
        self.bridge.battery_state()
    }
}
